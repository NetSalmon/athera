#![allow(dead_code)]
//! 任务控制块与全局任务表。
//!
//! [`Tid`] 是任务 ID；[`TaskControlBlock`] 描述单个任务（父/子关系、
//! 状态、内存集、陷阱上下文）；[`TASKS`] 是按 TID 索引的全局任务表。
use alloc::{collections::BTreeMap, rc::Weak, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};

use athera_const::lazy;
use athera_id_alloc::IdAllocator;

use crate::{
    constants::TID_MAX,
    mem::{addr::PhysicalAddr, alloc_page::AllocPage},
    trap::TrapContext,
};

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

#[derive(Debug)]
pub struct MemorySet {
    pub used_page: Vec<AllocPage>,
    pub user_root_page_table: PhysicalAddr,
}

#[derive(Debug)]
pub struct TaskControlBlock {
    pub parent: Option<Weak<TaskControlBlock>>,
    pub children: Vec<Arc<TaskControlBlock>>,
    pub status: TaskStatus,
    pub memory_set: MemorySet,
    pub trap_context: TrapContext,
    pub exit_code: i32,
    pub priority: i8,
}

#[derive(Debug)]
pub struct Tasks(pub BTreeMap<Tid, TaskControlBlock>);

impl Tasks {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
}

impl Deref for Tasks {
    type Target = BTreeMap<Tid, TaskControlBlock>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Tasks {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[lazy(spin)]
pub static TASKS: Tasks = Tasks::new();
