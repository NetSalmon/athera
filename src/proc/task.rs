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
    arch::registers::values::{SatpMode, SatpValue},
    constants::TID_MAX,
    error,
    error::{Error, MemError, ProcError},
    info,
    mem::{
        addr::PhysicalAddr,
        frame::Frame,
        page_table::{AddressSpaceId, PAGE_TABLE_MANAGER},
    },
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

impl MemorySet {
    /// 深拷贝本内存集，产出登记到 `new_tid` 名下的独立副本。
    ///
    /// 页表克隆由 [`PageTableManager::clone`] 完成，物理帧逐帧经
    /// [`Frame::try_clone`] 分配并拷贝内容；失败时已分配的帧随 `Drop`
    /// 自动归还。`owner` 为本内存集所属任务的 TID。
    pub fn try_clone(&self, owner: Tid, new_tid: Tid) -> Result<Self, Error> {
        PAGE_TABLE_MANAGER
            .force()
            .lock()
            .clone(AddressSpaceId::User(owner), new_tid)?;
        let user_root_page_table = PAGE_TABLE_MANAGER.force().lock().user_root_addr(new_tid)?;

        let mut used_page = Vec::with_capacity(self.used_page.len());
        for frame in &self.used_page {
            used_page.push(frame.try_clone().ok_or(MemError::OutOfMemory)?);
        }

        info!("memory set cloned: {owner:?} -> {new_tid:?}");

        Ok(Self {
            used_page,
            user_root_page_table,
        })
    }
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

    /// 以本任务为父进程克隆子进程控制块（fork 语义）。
    ///
    /// 内存集与陷阱上下文的克隆分别由 [`MemorySet::try_clone`] 与
    /// [`TrapContext::clone_child`] 实现；`tid` / `new_tid` 为父、子
    /// 任务的 TID，`frame` / `sepc` 为父进程陷入内核时的陷阱帧与
    /// `sepc`。
    pub fn try_clone(
        &self,
        tid: Tid,
        new_tid: Tid,
        frame: &[u64; 32],
        sepc: u64,
    ) -> Result<Self, Error> {
        let memory_set = self.memory_set.try_clone(tid, new_tid)?;

        let satp = SatpValue::builder()
            .set_ppn(memory_set.user_root_page_table.ppn() as u64)
            .set_mode(SatpMode::SV39.into())
            .build();

        Ok(Self {
            parent: Some(tid),
            children: Vec::new(),
            status: TaskStatus::Running,
            trap_context: self.trap_context.clone_child(frame, sepc, satp.into()),
            memory_set,
            exit_code: 0,
            priority: 0,
        })
    }
}

/// 以当前任务为父进程克隆一个子任务（`clone` 系统调用入口）。
///
/// 组合各部分的克隆：[`TaskControlBlock::try_clone`] 深拷贝地址空间与
/// 陷阱上下文，随后把子任务登记到任务表（父子关系由 [`Tasks::add`]
/// 维护）。`frame` / `sepc` 为父进程陷入内核时的陷阱帧与 `sepc`。
/// 返回子任务的 TID；失败时回收已分配的 TID。
pub fn clone_task(frame: &[u64; 32], sepc: u64) -> Result<Tid, Error> {
    let Some(CurrentTask { tid, .. }) = *CURRENT_TASK.current() else {
        return Err(ProcError::NoOtherTask.into());
    };

    let new_tid = alloc_tid().ok_or(Error::NoTidAvailable)?;

    info!("clone task: {tid:?} -> {new_tid:?}");

    // 持任务表锁完成整个克隆（期间会再取页表与帧分配器的锁，
    // 全内核不存在反向的加锁顺序，单核下无死锁风险）。
    let cloned = {
        let tasks = TASKS.force().lock();
        match tasks.get(&tid) {
            Some(tcb) => tcb.try_clone(tid, new_tid, frame, sepc),
            None => Err(ProcError::NoOtherTask.into()),
        }
    };

    match cloned {
        Ok(tcb) => {
            TASKS.force().lock().add(new_tid, Some(tid), tcb);
            Ok(new_tid)
        }
        Err(err) => {
            dealloc_tid(new_tid);
            Err(err)
        }
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
