#![allow(dead_code)]
//! 任务控制块与全局任务表。
//!
//! [`Tid`] 是任务 ID；[`TaskControlBlock`] 描述单个任务（父/子关系、
//! 状态、内存集、陷阱上下文）；[`TASKS`] 是按 TID 索引的全局任务表。
use alloc::{collections::BTreeMap, vec::Vec};
use core::ops::{Deref, DerefMut};

use athera_id_alloc::{Id, IdAllocator};
use athera_macros::lazy;

use crate::{
    constants::TID_MAX,
    error,
    error::{Error, ProcError},
    info,
    mem::{addr::PhysicalAddr, frame::Frame},
    proc::{CURRENT_TASK, CurrentTask},
    trap::TrapContext,
};

#[lazy(spin)]
pub static TID_ALLOCATOR: IdAllocator<Tid> = IdAllocator::from_range(Tid::MIN..Tid(TID_MAX));

#[derive(athera_macros::Id)]
pub struct Tid(pub usize);

pub fn alloc_tid() -> Option<Tid> {
    TID_ALLOCATOR.force().lock().alloc()
}

pub fn dealloc_tid(tid: Tid) {
    let _ = TID_ALLOCATOR.force().lock().dealloc(tid);
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
    pub used_page: Vec<Frame>,
    pub user_root_page_table: PhysicalAddr,
}

#[derive(Debug)]
pub struct TaskControlBlock {
    pub parent: Option<Tid>,
    pub children: Vec<Tid>,
    pub status: TaskStatus,
    pub memory_set: MemorySet,
    pub trap_context: TrapContext,
    pub exit_code: i32,
    pub priority: i8,
}

impl TaskControlBlock {
    pub fn run(&mut self) {
        self.status = TaskStatus::Running;
    }

    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = code;
    }
}

#[derive(Debug)]
pub struct Tasks(pub BTreeMap<Tid, TaskControlBlock>);

impl Tasks {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn run_first(&mut self) -> Result<TrapContext, Error> {
        if let Some((tid, tcb)) = self.0.iter_mut().next() {
            *CURRENT_TASK.current() = Some(CurrentTask {
                tid: *tid,
                exit_code: None,
            });

            tcb.run();

            Ok(tcb.trap_context.clone())
        } else {
            error!("tasks is empty");
            Err(ProcError::NoOtherTask.into())
        }
    }

    pub fn add(&mut self, tid: Tid, parent: Option<Tid>, tcb: TaskControlBlock) {
        if let Some(parent_tcb) = self.0.get_mut(&parent.unwrap_or(Tid(1))) {
            parent_tcb.children.push(tid);
        } else {
            error!("parent: {parent:?} not exist");
        }

        self.0.insert(tid, tcb);
    }

    pub fn snapshot(&self) {
        info!("Snapshot {:#?}", self);
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
