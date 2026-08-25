//! 用户程序加载执行。
//!
//! [`exec_buffer`] 解析 ELF 程序头，为每个 `PT_LOAD` 段分配物理页并
//! 映射进用户地址空间，最后按 Linux 标准建立初始用户栈（argc、argv、
//! envp 依次压栈），并通过 [`restore_context`] 切换到用户态。

use crate::error;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr;
use crate::fs::vfs::FileSystem;
use crate::fs::{Path, VFS};
use crate::{
    arch::riscv64::{
        registers::{
            csr::Sstatus,
            values::{SatpMode, SatpValue, SstatusBits},
        },
        trap::TrapContext,
    },
    binfmt::elf::{ElfHeader, ProgramHeader, ProgramType},
    constants::{PAGE_SIZE, USER_STACK_LOWER_BOUND, USER_STACK_SIZE},
    error::{Error, MemError, ProcError, Result},
    fs::vfs::{File, OpenFlags},
    info,
    mm::{
        address::VirtualAddr,
        allocator::alloc_frame,
        frame::Frame,
        page_table::{ADDRESS_SPACE_MANAGER, PageTableEntryFlags},
    },
    task::task::{MemorySet, TASKS, TID_ALLOCATOR, TaskControlBlock, TaskStatus, Tid, UserMapping},
    trace,
};

/// 进程入口处栈指针的对齐要求（RISC-V psABI：16 字节对齐）。
const STACK_ALIGN: usize = 16;

// Linux auxv 类型（与 <elf.h> 的 ABI 一致）。
const AT_NULL: usize = 0;
const AT_PHDR: usize = 3;
const AT_PHENT: usize = 4;
const AT_PHNUM: usize = 5;
const AT_PAGESZ: usize = 6;
const AT_ENTRY: usize = 9;

/// 按 Linux 初始栈布局计算 argv/envp/auxv 占用的用户栈字节数。
///
/// 布局自低地址向高地址依次为：`argc`、argv 指针数组（NULL 结尾）、
/// envp 指针数组（NULL 结尾）、auxv 表（每项两个 word，含 AT_NULL）、
/// 对齐填充、argv 字符串、envp 字符串。总长按 16 字节向上取整，保证
/// 入口 `sp` 对齐到 [`STACK_ALIGN`]。
fn initial_stack_size(argv: &[&str], envp: &[&str], auxv: &[(usize, usize)]) -> usize {
    let word = core::mem::size_of::<usize>();
    let pointer_block = (1 + argv.len() + 1 + envp.len() + 1 + auxv.len() * 2) * word;
    let strings: usize = argv
        .iter()
        .chain(envp)
        .map(|s| s.len() + 1)
        .sum();
    (pointer_block + strings).next_multiple_of(STACK_ALIGN)
}

/// 把 `s` 以 NUL 结尾写入 `sp` 下方并返回其起始地址，`sp` 随之下降。
///
/// # Safety
///
/// 调用方须确保 `sp - (s.len() + 1)` 落在已映射的用户栈帧内。
unsafe fn push_cstr(sp: &mut usize, s: &str) -> usize {
    *sp -= s.len() + 1;
    // Safety: 前置条件保证写入区间落在已映射栈帧内。
    unsafe {
        ptr::copy_nonoverlapping(s.as_ptr(), *sp as *mut u8, s.len());
        (*sp as *mut u8).add(s.len()).write(0);
    }
    *sp
}

/// 按 Linux 标准建立初始用户栈：自栈顶向下压入 envp/argv 字符串，
/// 再自低地址向高地址写入 argc、argv 指针数组（NULL 结尾）、envp 指针
/// 数组（NULL 结尾）与 auxv 表，返回入口 `sp`（用户虚拟地址，已对齐到
/// [`STACK_ALIGN`]）。
///
/// `user_stack` 为映射到虚拟区间 `USER_STACK_LOWER_BOUND..` 的用户栈
/// 物理帧，用于把物理地址换算为用户可见的虚拟地址。
pub fn build_initial_stack(
    argv: &[&str],
    envp: &[&str],
    auxv: &[(usize, usize)],
    user_stack: &Frame,
) -> Result<usize> {
    let required = initial_stack_size(argv, envp, auxv);
    if required > user_stack.size {
        return Err(ProcError::ArgsTooLong.into());
    }

    // 用户栈帧物理区间 `start..start+size` 映射到虚拟地址
    // `USER_STACK_LOWER_BOUND..USER_STACK_TOP`，据此把物理地址换算为
    // 用户可见的虚拟地址。
    let stack_top = user_stack.start + user_stack.size;
    let to_va = |pa: usize| USER_STACK_LOWER_BOUND + (pa - user_stack.start);

    // 字符串自栈顶向下写入：envp 字符串在上、argv 字符串在下（与 Linux
    // `arg_start < arg_end <= env_start < env_end` 一致），各自区间内
    // `argv[0]` / `envp[0]` 位于最低地址。
    let mut sp = stack_top;

    let mut envp_addrs = Vec::with_capacity(envp.len());
    for env in envp.iter().rev() {
        envp_addrs.push(to_va(unsafe { push_cstr(&mut sp, env) }));
    }
    envp_addrs.reverse();

    let mut argv_addrs = Vec::with_capacity(argv.len());
    for arg in argv.iter().rev() {
        argv_addrs.push(to_va(unsafe { push_cstr(&mut sp, arg) }));
    }
    argv_addrs.reverse();

    // 预留指针数组并把 `sp` 对齐到 16 字节，随后自低地址向高地址写入：
    // argc、argv 指针（NULL 结尾）、envp 指针（NULL 结尾）、auxv 表。
    let word = core::mem::size_of::<usize>();
    let pointer_words = 1 + argv.len() + 1 + envp.len() + 1 + auxv.len() * 2;
    sp = (sp - pointer_words * word) & !(STACK_ALIGN - 1);

    unsafe {
        let mut w = sp as *mut usize;
        w.write(argv.len());
        w = w.add(1);
        for &va in &argv_addrs {
            w.write(va);
            w = w.add(1);
        }
        w.write(0); // argv 终止 NULL
        w = w.add(1);
        for &va in &envp_addrs {
            w.write(va);
            w = w.add(1);
        }
        w.write(0); // envp 终止 NULL
        w = w.add(1);
        for (a_type, a_val) in auxv {
            w.write(*a_type);
            w = w.add(1);
            w.write(*a_val);
            w = w.add(1);
        }
    }

    let sp_va = to_va(sp);
    debug_assert_eq!(sp_va % STACK_ALIGN, 0);
    Ok(sp_va)
}

pub struct Load {
    pub memory_set: MemorySet,
    pub trap_context: TrapContext,
}

/// 加载一段用户程序 ELF：为 `tid` 创建用户地址空间，逐个处理 `PT_LOAD`
/// 段（分配物理页、清零并拷贝段数据、映射到用户虚拟地址），建立 Linux
/// 标准的初始用户栈，构造入口 [`TrapContext`]，并返回 [`Load`]（含
/// [`MemorySet`] 与 [`TrapContext`]）。`argv[0]` 依惯例为程序名，`envp`
/// 为形如 `KEY=VALUE` 的环境变量列表。
pub fn load_elf(buffer: &[u8], argv: &[&str], envp: &[&str], tid: Tid) -> Result<Load> {
    let elf_header = ElfHeader::from(buffer);

    elf_header.validate()?;

    let entry = elf_header.e_entry as usize;

    let ph_num = elf_header.e_phnum;

    // 找到程序头表在用户虚拟地址空间中的位置（用于 auxv 的 AT_PHDR）。
    let mut phdr_va = 0usize;

    info!(
        "executing user program: tid = {}, entry = {entry:#x}",
        tid.0
    );

    trace!("tid: {:?}", tid);

    ADDRESS_SPACE_MANAGER
        .force()
        .lock()
        .create_user_address_space(tid)?;

    let page_table_address = ADDRESS_SPACE_MANAGER
        .force()
        .lock()
        .user_root_addr(tid)?;

    let mut memory_set = MemorySet {
        mappings: vec![],
        user_root_page_table: page_table_address,
    };

    let ph_base =
        unsafe { buffer.as_ptr().add(elf_header.e_phoff as usize) as *const ProgramHeader };

    for i in 0..ph_num as usize {
        let ph = unsafe { &*ph_base.add(i) };

        let flags = ph.p_flags;
        let offset = ph.p_offset;
        let vaddr = ph.p_vaddr;
        let filesz = ph.p_filesz;
        let memsz = ph.p_memsz as usize;
        let align = ph.p_align;

        if ph.p_type == ProgramType::LOAD
            && (elf_header.e_phoff as usize) >= ph.p_offset as usize
            && (elf_header.e_phoff as usize) < ph.p_offset as usize + ph.p_filesz as usize
        {
            phdr_va = vaddr as usize + (elf_header.e_phoff as usize - ph.p_offset as usize);
        }

        if ph.p_type != ProgramType::LOAD {
            continue;
        }

        let frame = alloc_frame(Some(memsz)).ok_or(MemError::OutOfMemory)?;

        let mapping_flags = PageTableEntryFlags::builder()
            .set_u(true)
            .set_w(flags.write())
            .set_x(flags.execute())
            .set_r(flags.read())
            .set_a(true)
            .set_d(true)
            .build();

        for step in (0..frame.size).step_by(PAGE_SIZE) {
            let va = vaddr as usize + step;
            let pa = frame.start + step;

            ADDRESS_SPACE_MANAGER.force().lock().user_map(
                tid,
                va.into(),
                pa.into(),
                mapping_flags,
                false,
            )?;
        }

        let start = frame.start as *mut u8;

        // clean
        unsafe { ptr::write_bytes(start, 0, memsz) };

        let inside_offset = (vaddr % align) as usize;

        // copy
        unsafe {
            // from buffer[offset] copy filesz bytes to page
            ptr::copy(
                buffer.as_ptr().add(offset as usize),
                start.add(inside_offset),
                filesz as usize,
            );
        }

        memory_set.mappings.push(UserMapping {
            va: VirtualAddr::from(vaddr as usize),
            frame,
            flags: mapping_flags,
        });
    }

    let user_stack = alloc_frame(Some(USER_STACK_SIZE)).ok_or(MemError::OutOfMemory)?;

    let stack_flags = PageTableEntryFlags::builder()
        .set_r(true)
        .set_w(true)
        .set_u(true)
        .set_a(true)
        .set_d(true)
        .build();

    for i in (0..user_stack.size).step_by(PAGE_SIZE) {
        ADDRESS_SPACE_MANAGER.force().lock().user_map(
            tid,
            (USER_STACK_LOWER_BOUND + i).into(),
            (user_stack.start + i).into(),
            stack_flags,
            false,
        )?
    }

    // ---- 建立 Linux 标准初始栈 ----

    let auxv: [(usize, usize); 6] = [
        (AT_PHDR, phdr_va),
        (AT_PHENT, elf_header.e_phentsize as usize),
        (AT_PHNUM, elf_header.e_phnum as usize),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_ENTRY, entry),
        (AT_NULL, 0),
    ];

    let sp_va = build_initial_stack(argv, envp, &auxv, &user_stack)?;

    memory_set.mappings.push(UserMapping {
        va: VirtualAddr::from(USER_STACK_LOWER_BOUND),
        frame: user_stack,
        flags: stack_flags,
    });

    let root_page_table_addr = ADDRESS_SPACE_MANAGER.force().lock().user_root_addr(tid)?;

    let satp = SatpValue::builder()
        .set_ppn(root_page_table_addr.ppn() as u64)
        .set_mode(SatpMode::SV39.into())
        .build();

    let mut sstatus = SstatusBits::from(Sstatus::read());
    sstatus.set_spp(false);
    sstatus.set_spie(true);

    info!("entry: {:#p}", entry as *const u8);
    info!("stack: {:#p}", sp_va as *const u8);

    let mut context = TrapContext {
        context: [0; 32],
        satp: satp.into(),
        sepc: entry as u64,
        sstatus: sstatus.into(),
    };

    // 初始 `sp` 指向栈上的 argc（Linux 初始栈入口约定）。
    context.context[2] = sp_va as u64;

    Ok(Load {
        memory_set,
        trap_context: context,
    })
}

/// 加载并执行一段用户程序 ELF。
///
/// 分配 TID，调用 [`load_elf`] 得到用户地址空间与入口上下文，据此构造
/// [`TaskControlBlock`] 并加入 [`TASKS`]，等待调度切换至用户态。
pub fn exec_buffer(buffer: &[u8], argv: &[&str], envp: &[&str]) -> Result<()> {
    let tid = TID_ALLOCATOR
        .force()
        .lock()
        .alloc()
        .ok_or(Error::NoTidAvailable)?;

    let Load {
        memory_set,
        trap_context,
    } = load_elf(buffer, argv, envp, tid)?;

    let tcb = TaskControlBlock {
        parent: None,
        children: vec![],
        status: TaskStatus::Running,
        memory_set,
        trap_context,
        exit_code: 0,
        // 标准输入、输出和错误输出都连接到同一个串口设备。
        fd_table: vec![
            File::uart(OpenFlags::read_only()),
            File::uart(OpenFlags::write_only()),
            File::uart(OpenFlags::write_only()),
        ],
    };

    TASKS.force().lock().add(tid, None, tcb);

    Ok(())
}

/// 从 virtio-blk 上的 MINIX 文件系统按路径读取文件并加载为进程。
pub(crate) fn kernel_execve(path: &str, argv: &[&str], envp: &[&str]) {
    let f = match VFS.force().open(
        &Path::from(path),
        OpenFlags::read_only(),
        crate::fs::Mode::from(0),
    ) {
        Ok(file) => file,
        Err(err) => {
            error!("failed to open {path}: {err}");
            return;
        }
    };

    let Ok(size) = usize::try_from(match VFS.force().stat(&Path::from(path)) {
        Ok(stat) => stat.size,
        Err(err) => {
            error!("failed to stat {path}: {err}");
            return;
        }
    }) else {
        error!("{path} is too large to load");
        return;
    };
    
    let mut buf = vec![0u8; size];

    let read = match f.read(&mut buf) {
        Ok(read) => read,
        Err(err) => {
            error!("failed to read {path}: {err}");
            return;
        }
    };
    if read != buf.len() {
        error!("short read for {path}: expected {}, got {read}", buf.len());
        return;
    }

    if let Err(err) = exec_buffer(&buf, argv, envp) {
        error!("failed to execute user program: {err}");
    }
}