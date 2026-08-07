//! 用户程序加载执行。
//!
//! [`spawn_buffer`] 解析 ELF 程序头，为每个 `PT_LOAD` 段分配物理页并
//! 映射进用户地址空间，最后创建用户栈并通过 [`restore_context`] 切换到
//! 用户态。
use alloc::vec;
use core::ptr;

use crate::{
    arch::registers::{
        csr::Sstatus,
        values::{SStatusBits, SatpMode, SatpValue},
    },
    constants::{PAGE_SIZE, USER_STACK_LOWER_BOUND, USER_STACK_SIZE, USER_STACK_TOP},
    elf::{Elf64Ehdr, Elf64Phdr, PType},
    error::{Error, MemError, Result},
    info,
    mem::{
        allocators::alloc_frame,
        page_table::{PAGE_TABLE_MANAGER, PageTableEntryFlags},
    },
    proc::task::{MemorySet, TASKS, TID_ALLOCATOR, TaskControlBlock, TaskStatus, Tid},
    trace,
    trap::TrapContext,
};

/// 加载并执行一段用户程序 ELF。
///
/// 分配 TID、创建用户地址空间，逐个处理 `PT_LOAD` 段（分配物理页、
/// 清零并拷贝段数据、映射到用户虚拟地址），最后建立用户栈并切换到
/// 用户态。
pub fn spawn_buffer(buffer: &[u8], priority: Option<i8>) -> Result<()> {
    let elf_header = Elf64Ehdr::from(buffer);

    elf_header.avail()?;

    let entry = elf_header.e_entry as usize;

    let ph_num = elf_header.e_phnum;

    let tid = TID_ALLOCATOR
        .force()
        .lock()
        .alloc()
        .ok_or(Error::NoTidAvailable)?;

    let tid = Tid(tid);

    info!(
        "executing user program: tid = {}, entry = {entry:#x}",
        tid.0
    );
    trace!("tid: {:?}", tid);

    PAGE_TABLE_MANAGER
        .force()
        .lock()
        .create_user_address_space(tid)?;

    let page_table_address = PAGE_TABLE_MANAGER.force().lock().user_root_addr(tid)?;

    let mut memory_set = MemorySet {
        used_page: vec![],
        user_root_page_table: page_table_address,
    };

    let mut ph_ptr =
        unsafe { buffer.as_ptr().add(elf_header.e_phoff as usize) as *const Elf64Phdr };

    for i in 0..ph_num as usize {
        ph_ptr = unsafe { ph_ptr.add(i) };

        let ph = unsafe { &*ph_ptr };

        let flags = ph.p_flags;
        let offset = ph.p_offset;
        let vaddr = ph.p_vaddr;
        let filesz = ph.p_filesz;
        let memsz = ph.p_memsz as usize;
        let align = ph.p_align;

        if ph.p_type != PType::LOAD {
            continue;
        }

        let alloc_page = alloc_frame(Some(memsz)).ok_or(MemError::OutOfMemory)?;

        let mapping_flags = PageTableEntryFlags::builder()
            .set_u(true)
            .set_w(flags.write())
            .set_x(flags.execute())
            .set_r(flags.read())
            .set_a(true)
            .set_d(true)
            .build();

        for step in (0..alloc_page.size).step_by(PAGE_SIZE) {
            let va = vaddr as usize + step;
            let pa = alloc_page.start + step;

            PAGE_TABLE_MANAGER.force().lock().user_map(
                tid,
                va.into(),
                pa.into(),
                mapping_flags,
                false,
            )?;
        }

        let start = alloc_page.start as *mut u8;

        memory_set.used_page.push(alloc_page);

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
        PAGE_TABLE_MANAGER.force().lock().user_map(
            tid,
            (USER_STACK_LOWER_BOUND + i).into(),
            (user_stack.start + i).into(),
            stack_flags,
            false,
        )?
    }

    memory_set.used_page.push(user_stack);

    let root_page_table_addr = PAGE_TABLE_MANAGER.force().lock().user_root_addr(tid)?;

    let satp = SatpValue::builder()
        .set_ppn(root_page_table_addr.ppn() as u64)
        .set_mode(SatpMode::SV39.into())
        .build();

    let mut sstatus = SStatusBits::from(Sstatus::read());
    sstatus.set_spp(false);
    sstatus.set_spie(true);

    let mut context = TrapContext {
        context: [0; 32],
        satp: satp.into(),
        sepc: entry as u64,
        sstatus: sstatus.into(),
    };

    // set sp
    context.context[2] = USER_STACK_TOP as u64;

    let tcb = TaskControlBlock {
        parent: None,
        children: vec![],
        status: TaskStatus::Running,
        memory_set,
        trap_context: context.clone(),
        exit_code: 0,
        priority: priority.unwrap_or_default(),
    };

    TASKS.force().lock().add(tid, None, tcb);

    Ok(())
}
