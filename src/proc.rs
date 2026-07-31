use alloc::{collections::BTreeMap, rc::Weak, sync::Arc, vec::Vec};

use novus_const::lazy;
use novus_id_alloc::IdAllocator;

use crate::{
    mem::{addr::PhysicalAddr, alloc_page::AllocPage, page_table::PageTable},
    trap::TrapContext,
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
