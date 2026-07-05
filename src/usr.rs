use crate::arch::registers::csr::Sstatus;
use crate::bits;
use crate::debug;
use crate::elf::{Elf64Ehdr, Elf64Phdr, Endianness, PType};
use crate::mem::PAGE_SIZE;
use crate::mem::frame_allocator::{AllocPage, FRAME_ALLOCATOR};
use core::ptr;

bits! {
    pub type SStatusBits: u64 {
        spp: 8,
        sie: 1
    }
}

/// 解析并加载`ELF`各段
pub fn exec(elf: &[u8]) {
    let ptr = elf.as_ptr();
    let header = unsafe { &*(ptr as *const Elf64Ehdr) };
    
    if header.e_ident.data() != Endianness::LSB {
        panic!("Unsupported endianness")
    }

    let ph_offset = header.e_phoff;
    let ph_size = header.e_phnum;

    debug!("elf machine: {:?}", header.e_machine);
    debug!("elf os abi: {:?}", header.e_ident.os_abi());

    let ph_ptr = unsafe { ptr.add(ph_offset as usize) as *const Elf64Phdr };
    for i in 0..ph_size {
        debug!("============ ph{i} ============");
        let ph = unsafe { &*ph_ptr.add(i as usize) };
        let ty = ph.p_type;
        debug!("type: {:?}", ty);
        let offset = ph.p_offset as usize;
        let file_size = ph.p_filesz as usize;

        debug!("file size: {}, offset: {}", file_size, offset);

        let addr = ph.p_vaddr as usize;
        let mem_size = ph.p_memsz as usize;

        debug!("mem size: {}, addr: {}", mem_size, addr);

        let flags = ph.p_flags;
        let align = ph.p_align as usize;

        debug!("align: {}, flags: {}", align, flags);

        if ty != PType::LOAD {
            continue;
        }

        let alloc_page = FRAME_ALLOCATOR
            .force()
            .lock()
            .alloc_frame(mem_size)
            .expect("out of memory");

        for i in 0..mem_size {
            let current = alloc_page + i;
            debug!("{:#x}: {}", current, unsafe {
                (current as *const u8).read()
            });
        }

        unsafe { ptr::copy(ptr.add(offset), alloc_page as *mut u8, file_size) }

        for i in file_size..mem_size {
            unsafe { (ptr.add(offset).add(i) as *mut u8).write(0) }
        }

        debug!("load ok");

        for i in 0..mem_size {
            let current = alloc_page + i;
            debug!("{:#x}: {:#x}", current, unsafe {
                (current as *const u8).read()
            });
        }
    }
}

/// 设置`sstatus`寄存器`SPP`位为`0`
#[inline]
pub fn set_sstatus_spp() {
    Sstatus::modify(|source| {
        let mut t = SStatusBits::from(source);
        t.set_spp(false);
        t.into()
    })
}

/// 用户栈固定 4KiB
pub fn alloc_user_stack() -> AllocPage {
    let page = FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc(0)
        .expect("out of memory");

    unsafe { AllocPage::from_raw(page, PAGE_SIZE) }
}

// [解析ELF]
//    │
//    ├──> 1. 创建用户根页表，并拷贝内核空间的页表项（确保切到用户态后还能切回内核）
//    ├──> 2. 遍历 ELF Phdr，为各 LOAD 段申请物理页，分配虚拟地址映射（U+R+W/X 权限）
//    ├──> 3. 将 ELF 中的代码/数据复制到对应的物理页中（处理好 .bss 清零）
//    └──> 4. 申请用户栈物理页，在页表中映射为用户栈虚拟地址
//
// [准备上下文 (Trapframe)]
//    │
//    ├──> 5. 在内存中初始化该进程的上下文（寄存器镜像）
//    │       ├── sepc  <- ELF 入口地址 (Entry Point)
//    │       ├── sp    <- 用户栈顶虚拟地址
//    │       └── a0/a1 <- 传入的参数 (argc, argv)
//    └──> 6. 设置 sstatus.SPP = 0 (使其能返回到 U-Mode)
//
// [切换并运行 (汇编跳板)]
//    │
//    ├──> 7. 切换 satp 寄存器指向新页表，并执行 sfence.vma 刷新 TLB
//    ├──> 8. 从 Trapframe 中恢复用户寄存器 (包括 sp, a0-a7 等)
//    └──> 9. 执行 sret ──> 正式进入 U Mode 执行用户程序
