use alloc::{collections::BTreeMap, rc::Weak, sync::Arc, vec, vec::Vec};
use core::ptr;

use novus_const::lazy;
use novus_id_alloc::IdAllocator;

use crate::{
    arch::registers::{
        csr::Sstatus,
        values::{SatpMode, SatpValue},
    },
    constants::PHY_PAGE_SIZE,
    elf::{Elf64Ehdr, Elf64Phdr, PFlags, PType},
    info,
    mem::{
        addr::PhysicalAddr,
        alloc_page::AllocPage,
        allocators::alloc_frame,
        page_table::{PAGE_TABLE_MANAGER, PageTableEntryFlags},
    },
    trace,
    trap::{TrapContext, restore_context},
    usr::{SStatusBits, USER_STACK_LOWER_BOUND, USER_STACK_SIZE, USER_STACK_TOP},
};

#[novus_const::const_val]
pub const TID_MAX: usize = 1024;

#[lazy(spin)]
pub static TID_ALLOCATOR: IdAllocator = IdAllocator::from_range(0..TID_MAX);

#[derive(Debug, PartialEq, Clone, Ord, Eq, PartialOrd, Copy)]
pub struct Tid(pub usize);

pub fn alloc_tid() -> Option<Tid> {
    TID_ALLOCATOR.force().lock().alloc().map(Tid)
}

pub fn dealloc_tid(tid: Tid) {
    let _ = TID_ALLOCATOR.force().lock().dealloc(tid.0);
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Running,
    Waiting,
    Sleeping,
    Zombie,
    Stopped,
    Dead,
}

pub struct MemorySet {
    pub used_page: Vec<AllocPage>,
    pub user_root_page_table: PhysicalAddr,
}

pub struct TaskControlBlock {
    pub parent: Option<Weak<TaskControlBlock>>,
    pub children: Vec<Arc<TaskControlBlock>>,
    pub status: TaskStatus,
    pub memory_set: MemorySet,
    pub trap_context: TrapContext,
    pub exit_code: i32,
}

#[lazy(spin)]
pub static TASKS: BTreeMap<Tid, Arc<TaskControlBlock>> = BTreeMap::new();

pub fn execute_buffer(buffer: &[u8]) {
    let elf_header = Elf64Ehdr::from(buffer);

    let entry = elf_header.e_entry as usize;

    let ph_num = elf_header.e_phnum;

    let tid = TID_ALLOCATOR
        .force()
        .lock()
        .alloc()
        .expect("tid out of range");

    let tid = Tid(tid);

    trace!("tid: {:?}", tid);

    PAGE_TABLE_MANAGER
        .force()
        .lock()
        .create_user_address_space(tid);

    let page_table_address = PAGE_TABLE_MANAGER.force().lock().user_root_addr(tid);

    let mut memory_set = MemorySet {
        used_page: vec![],
        user_root_page_table: page_table_address,
    };

    let mut ph_ptr =
        unsafe { buffer.as_ptr().add(elf_header.e_phoff as usize) as *const Elf64Phdr };

    for i in 0..ph_num as usize {
        info!("loop {i}");

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

        let alloc_page = alloc_frame(Some(memsz)).expect("out of memory");

        let mapping_flags = PageTableEntryFlags::builder()
            .set_u(true)
            .set_w(flags.write())
            .set_x(flags.execute())
            .set_r(flags.read())
            .set_a(true)
            .set_d(true)
            .build();

        for step in (0..alloc_page.size).step_by(PHY_PAGE_SIZE) {
            let va = vaddr as usize + step;
            let pa = alloc_page.start + step;

            PAGE_TABLE_MANAGER.force().lock().user_map(
                tid,
                va.into(),
                pa.into(),
                mapping_flags,
                false,
            );
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

    let user_stack = alloc_frame(Some(USER_STACK_SIZE)).expect("out of memory");

    let stack_flags = PageTableEntryFlags::builder()
        .set_r(true)
        .set_w(true)
        .set_u(true)
        .set_a(true)
        .set_d(true)
        .build();

    for i in (0..user_stack.size).step_by(PHY_PAGE_SIZE) {
        PAGE_TABLE_MANAGER.force().lock().user_map(
            tid,
            (USER_STACK_LOWER_BOUND + i).into(),
            (user_stack.start + i).into(),
            stack_flags,
            false,
        )
    }

    memory_set.used_page.push(user_stack);

    let root_page_table_addr = PAGE_TABLE_MANAGER.force().lock().user_root_addr(tid);

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
    };

    TASKS.force().lock().insert(tid, Arc::new(tcb));

    restore_context(&context);
}
