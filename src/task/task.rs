#![allow(dead_code)]
//! 任务控制块与全局任务表。
//!
//! [`Tid`] 是任务 ID；[`TaskControlBlock`] 描述单个任务（父/子关系、
//! 状态、内存集、陷阱上下文）；[`TASKS`] 是按 TID 索引的全局任务表。
use alloc::{collections::BTreeMap, vec::Vec};
use core::ops::{Bound, Deref, DerefMut};

use athera_id_alloc::IdAlloc;
use athera_macros::lazy;

use crate::{
    arch::riscv64::{
        registers::values::{SatpMode, SatpValue},
        trap::TrapContext,
    },
    constants::{PAGE_SIZE, TID_MAX},
    error,
    error::{Error, MemError, ProcError},
    fs::vfs::File,
    info,
    mm::{
        address::{PhysicalAddr, VirtualAddr},
        frame::Frame,
        page_table::{ADDRESS_SPACE_MANAGER, AddressSpaceId, PageTableEntryFlags},
    },
    task::CURRENT_TASK,
};

#[lazy(spin)]
pub static TID_ALLOCATOR: IdAlloc<Tid, 0> =
    IdAlloc::from_range(Tid(1)..Tid(TID_MAX)).expect("tid range");

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
    /// 可被调度器选择并运行的任务。
    Running,
    /// 等待某个事件或资源，暂时不可运行。
    Waiting,
    /// 被主动挂起，等待后续唤醒。
    Sleeping,
    /// 已退出但仍保留退出状态，等待父任务回收。
    Zombie,
    /// 被暂停执行，暂不参与调度。
    Stopped,
    /// 已结束且不再参与任务生命周期。
    Dead,
}

impl TaskStatus {
    #[inline]
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Debug)]
pub struct UserMapping {
    pub va: VirtualAddr,
    pub frame: Frame,
    pub flags: PageTableEntryFlags,
}

#[derive(Debug)]
pub struct MemorySet {
    pub mappings: Vec<UserMapping>,
    pub user_root_page_table: PhysicalAddr,
}

impl MemorySet {
    /// 深拷贝本内存集，产出登记到 `new_tid` 名下的独立副本。
    ///
    /// 页表克隆由 [`PageTableManager::clone`] 完成，物理帧逐帧经
    /// [`Frame::try_clone`] 分配并拷贝内容；失败时已分配的帧随 `Drop`
    /// 自动归还。`owner` 为本内存集所属任务的 TID。
    pub fn try_clone(&self, owner: Tid, new_tid: Tid) -> Result<Self, Error> {
        ADDRESS_SPACE_MANAGER
            .force()
            .lock()
            .clone(AddressSpaceId::User(owner), new_tid)?;
        let user_root_page_table = ADDRESS_SPACE_MANAGER
            .force()
            .lock()
            .user_root_addr(new_tid)?;

        // 先为所有映射分配好子进程帧，再统一改写子进程页表；这样即使
        // 中途分配失败，已分配帧随 `Vec` 的 `Drop` 归还，不会出现页表
        // 指向已释放帧的情况。
        let mut frames = Vec::with_capacity(self.mappings.len());
        for mapping in &self.mappings {
            frames.push((
                mapping,
                mapping.frame.try_clone().ok_or(MemError::OutOfMemory)?,
            ));
        }

        let mut mappings = Vec::with_capacity(self.mappings.len());
        let mut manager = ADDRESS_SPACE_MANAGER.force().lock();
        for (mapping, frame) in frames {
            // 把子进程页表里对应虚拟地址的叶映射从父进程帧改写为刚
            // 分配的子进程帧，否则子进程仍会读写父进程的物理页。
            // `PROT_NONE`（rwx 全 0）的映射在父进程页表中不占条目，克隆时
            // 同样跳过，保持两进程行为一致（访问时均按缺页异常处理）。
            if mapping.flags.r() || mapping.flags.w() || mapping.flags.x() {
                for step in (0..frame.size).step_by(PAGE_SIZE) {
                    manager.user_map(
                        new_tid,
                        (*mapping.va + step).into(),
                        (frame.start + step).into(),
                        mapping.flags,
                        false,
                    )?;
                }
            }

            mappings.push(UserMapping {
                va: mapping.va,
                frame,
                flags: mapping.flags,
            });
        }

        info!("memory set cloned: {owner:?} -> {new_tid:?}");

        Ok(Self {
            mappings,
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
    pub fd_table: Vec<File>,
}

impl TaskControlBlock {
    /// 把本任务标记为可运行，使其可被调度器选中执行。
    pub fn mark_runnable(&mut self) {
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
            fd_table: self.fd_table.clone(),
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
    let Some(tid) = *CURRENT_TASK.current() else {
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
pub struct Tasks {
    pub map: BTreeMap<Tid, TaskControlBlock>,
    cursor: Option<Tid>,
}

pub struct TaskContext {
    pub tid: Tid,
    pub context: TrapContext,
}

impl Tasks {
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            cursor: None,
        }
    }

    pub fn select_first(&mut self) -> Result<TrapContext, Error> {
        if let Some((tid, tcb)) = self.map.iter_mut().next() {
            self.cursor = Some(*tid);
            *CURRENT_TASK.current() = Some(*tid);

            tcb.mark_runnable();

            Ok(tcb.trap_context.clone())
        } else {
            error!("tasks is empty");
            Err(ProcError::NoOtherTask.into())
        }
    }

    pub fn add(&mut self, tid: Tid, parent: Option<Tid>, tcb: TaskControlBlock) {
        if let Some(parent_tcb) = self.map.get_mut(&parent.unwrap_or(Tid(1))) {
            parent_tcb.children.push(tid);
        } else {
            error!("parent: {parent:?} not exist");
        }

        self.map.insert(tid, tcb);
    }

    /// 创建一个按 `Tid` 升序循环遍历任务的 cursor。
    pub fn cursor(&self) -> TasksCursor<'_> {
        TasksCursor {
            tasks: self,
            current: None,
        }
    }

    /// 选出一个可运行的任务（循环游标向后移动），作为下一次调度的目标。
    pub fn select_next(&mut self) -> Result<TaskContext, Error> {
        let next = |tasks: &BTreeMap<Tid, TaskControlBlock>, start| {
            tasks
                .range(start)
                .find(|(_, task)| task.status.is_runnable())
                .map(|(tid, _)| *tid)
        };

        let tid = match self.cursor {
            Some(current) => next(&self.map, (Bound::Excluded(current), Bound::Unbounded))
                .or_else(|| next(&self.map, (Bound::Unbounded, Bound::Unbounded))),
            None => next(&self.map, (Bound::Unbounded, Bound::Unbounded)),
        }
        .ok_or(ProcError::NoOtherTask)?;

        self.cursor = Some(tid);
        let tcb = self.map.get_mut(&tid).ok_or(ProcError::NoOtherTask)?;
        *CURRENT_TASK.current() = Some(tid);
        tcb.mark_runnable();
        Ok(TaskContext {
            tid,
            context: tcb.trap_context.clone(),
        })
    }

    pub fn snapshot(&self) {
        info!("Snapshot {:#?}", self);
    }
}

/// 任务表的循环顺序 cursor。
///
/// 非空任务表会在最大 `Tid` 后回到最小 `Tid`；空任务表的 `next` 返回
/// `None`。cursor 只保存当前位置，因此任务表增删后仍可继续遍历。
pub struct TasksCursor<'a> {
    tasks: &'a Tasks,
    current: Option<Tid>,
}

impl<'a> Iterator for TasksCursor<'a> {
    type Item = (&'a Tid, &'a TaskControlBlock);

    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.current {
            Some(current) => self
                .tasks
                .map
                .range((Bound::Excluded(current), Bound::Unbounded))
                .next()
                .or_else(|| self.tasks.map.iter().next()),
            None => self.tasks.map.iter().next(),
        }?;

        self.current = Some(*next.0);
        Some(next)
    }
}

impl Deref for Tasks {
    type Target = BTreeMap<Tid, TaskControlBlock>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl DerefMut for Tasks {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

#[lazy(spin)]
pub static TASKS: Tasks = Tasks::new();
