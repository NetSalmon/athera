use alloc::vec::Vec;
use crate::arch::registers::csr::Sstatus;
use crate::bits;
use crate::debug;
use crate::elf::{Class, Elf64Ehdr, Elf64Phdr, Endianness, PType};
use crate::error::Error;
use crate::mem::PAGE_SIZE;
use crate::mem::constants::PHY_PAGE_SIZE;
use crate::mem::frame::{AllocPage, FRAME_ALLOCATOR};
use core::ptr;

bits! {
    pub type SStatusBits: u64 {
        spp: 8,
        sie: 1
    }
}

#[const_val::const_val(multiple_of = PHY_PAGE_SIZE)]
const USER_STACK_SIZE: usize = PHY_PAGE_SIZE * 8;

/// 解析并加载`ELF`各段
pub fn exec(elf: &[u8]) -> Result<Vec<AllocPage>, Error> {
    let ptr = elf.as_ptr();
    let header = unsafe { &*(ptr as *const Elf64Ehdr) };

    if !header.e_ident.is_elf() {
        return Err(Error::NotElf);
    }

    if header.e_ident.class() != Class::CLASS64 {
        return Err(Error::Not64Bit);
    }

    if header.e_ident.data() != Endianness::LSB {
        return Err(Error::NotLsb);
    }

    let ph_offset = header.e_phoff;
    let ph_size = header.e_phnum;

    debug!("elf machine: {:?}", header.e_machine);
    debug!("elf os abi: {:?}", header.e_ident.os_abi());
    
    let mut alloc_pages = Vec::new();

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
            .ok_or(Error::OutOfMemory)?;
        
        alloc_pages.push(AllocPage { start: alloc_page, size: mem_size });

        unsafe { ptr::copy(ptr.add(offset), alloc_page as *mut u8, file_size) }

        for i in file_size..mem_size {
            unsafe { (alloc_page as *mut u8).add(i).write(0) }
        }

        debug!("load ok");
    }

    Ok(alloc_pages)
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
pub fn alloc_user_stack() -> Result<AllocPage, Error> {
    let page = FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc_frame(USER_STACK_SIZE)
        .ok_or(Error::OutOfMemory)?;

    Ok(unsafe { AllocPage::from_raw(page, USER_STACK_SIZE) })
}

// [解析ELF]
//    │
//    ├──> 1. 创建用户根页表，并拷贝内核空间的页表项（确保切到用户态后还能切回内核）
//    ├──> 2. 遍历 ELF Phdr，为各 LOAD 段申请物理页，分配虚拟地址映射（U+R+W/X 权限）
//    ├──> 3. 将 ELF 中的代码/数据复制到对应的物理页中（处理好 .bss 清零）
//    └──> 4. 申请用户栈物理页，在页表中映射为用户栈虚拟地址
//
// [准备上下文 (TrapFrame)]
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
