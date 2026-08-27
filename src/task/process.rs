//! Process lifecycle and file-descriptor services.

use crate::{
    error,
    fs::{FsError, vfs::File},
    mm::page_table::{ADDRESS_SPACE_MANAGER, AddressSpaceId},
    task::{
        CURRENT_TASK, TASKS, TaskStatus, Tid,
        exec::{Load, load_elf, read_file},
    },
};

pub(crate) struct WaitResult {
    pub tid: usize,
    pub exit_code: i32,
}

pub(crate) enum FdError {
    NoTask,
    BadFd,
    NotReadable,
    NotWritable,
    Io(FsError),
}

pub(crate) fn exit(code: i32) {
    let Some(tid) = *CURRENT_TASK.current() else {
        return;
    };
    if tid.0 == 1 {
        panic!("pid 1 exit")
    }

    let children = if let Some(task) = TASKS.force().lock().get_mut(&tid) {
        task.exit_code = code;
        task.status = TaskStatus::Zombie;
        let children = task.children.clone();
        task.children.clear();
        Some(children)
    } else {
        None
    };

    if let Some(children) = children
        && let Some(init) = TASKS.force().lock().get_mut(&Tid(1))
    {
        init.children.extend(children);
    }
}

pub(crate) fn wait4(tid: isize, nohang: bool) -> Option<WaitResult> {
    let current = (*CURRENT_TASK.current())?;
    let target_tid = match tid {
        x if x <= 0 => None,
        _ => Some(Tid(tid as usize)),
    };

    if nohang {
        let Some(tid) = find_zombie_child(current, target_tid) else {
            return Some(WaitResult {
                tid: 0,
                exit_code: 0,
            });
        };
        let exit_code = TASKS
            .force()
            .lock()
            .get(&Tid(tid))
            .map(|task| task.exit_code)
            .unwrap_or_default();
        Some(WaitResult { tid, exit_code })
    } else {
        if let Some(task) = TASKS.force().lock().get_mut(&current) {
            task.status = TaskStatus::Waiting;
        }
        None
    }
}

pub(crate) fn read_fd(fd: u64, buf: &mut [u8]) -> Result<usize, FdError> {
    let file = file_for_fd(fd)?;
    if !file.flags().can_read() {
        return Err(FdError::NotReadable);
    }
    file.read(buf).map_err(FdError::Io)
}

pub(crate) fn write_fd(fd: u64, buf: &[u8]) -> Result<usize, FdError> {
    let file = file_for_fd(fd)?;
    if !file.flags().can_write() {
        return Err(FdError::NotWritable);
    }
    file.write(buf).map_err(FdError::Io)
}

fn file_for_fd(fd: u64) -> Result<File, FdError> {
    let Some(current) = *CURRENT_TASK.current() else {
        return Err(FdError::NoTask);
    };
    let fd = usize::try_from(fd).map_err(|_| FdError::BadFd)?;
    TASKS
        .force()
        .lock()
        .get(&current)
        .and_then(|task| task.fd_table.get(fd))
        .cloned()
        .ok_or(FdError::BadFd)
}

fn find_zombie_child(parent: Tid, target: Option<Tid>) -> Option<usize> {
    let children = TASKS
        .force()
        .lock()
        .get(&parent)
        .map(|task| task.children.clone())?;

    if let Some(target) = target {
        children.iter().find(|child| **child == target)?;
        if TASKS.force().lock().get(&target)?.status == TaskStatus::Zombie {
            return Some(target.0);
        }
    } else {
        for child in children {
            if TASKS.force().lock().get(&child)?.status == TaskStatus::Zombie {
                return Some(child.0);
            }
        }
    }
    None
}

/// execve 语义作用于任意任务：读取 ELF 文件，替换 `tid` 任务的用户地址
/// 空间（`memory_set`）与陷阱上下文（`trap_context`），并标记为可运行，
/// 等待调度器从新程序入口恢复执行；`fd_table` 等其余字段保持不变。
///
/// `tid` 为当前任务时即普通 `execve` 系统调用；为其他任务时其页表未被
/// 激活，无需切换 `satp` 即可安全重建。
pub fn execve_into(tid: Tid, path: &str, argv: &[&str], envp: &[&str]) -> Option<()> {
    if TASKS.force().lock().get(&tid).is_none() {
        error!("execve target {tid:?} not found");
        return None;
    }

    let buf = read_file(path)?;

    // ---- 重建地址空间 ----
    //
    // 若目标就是当前任务：此刻 `satp` 仍指向其用户根页表（内核经
    // `copy_low_half` 继承的低半区映射在其中）。必须先切回内核地址
    // 空间，再重建用户地址空间：否则旧页表页在被释放后会立即被新程序
    // 的段分配复用并清零，CPU 仍在用被抹掉的页表取指 / 访存，内核直接
    // 卡死。
    let is_current = *CURRENT_TASK.current() == Some(tid);

    if is_current
        && let Err(err) = ADDRESS_SPACE_MANAGER
            .force()
            .lock()
            .activate(AddressSpaceId::Kernel)
    {
        error!("failed to activate kernel address space: {err}");
        return None;
    }
    // 旧地址空间先取走暂存：加载失败时还能放回去，让系统调用带着
    // -errno 安全返回旧程序继续执行。
    let old_space = ADDRESS_SPACE_MANAGER.force().lock().remove_user(tid);

    let load = match load_elf(buf.as_slice(), argv, envp, tid) {
        Ok(load) => load,
        Err(err) => {
            error!("failed to load {path}: {err}");
            let mut manager = ADDRESS_SPACE_MANAGER.force().lock();
            if let Some(old) = old_space {
                manager.insert_user(tid, old);
            }
            if is_current && let Err(err) = manager.activate(AddressSpaceId::User(tid)) {
                error!("failed to restore address space of {tid:?}: {err}");
            }
            return None;
        }
    };

    let Load {
        memory_set,
        trap_context,
    } = load;

    let mut tasks = TASKS.force().lock();
    let Some(tcb) = tasks.get_mut(&tid) else {
        error!("task {tid:?} disappeared during execve");
        return None;
    };

    tcb.memory_set = memory_set;
    tcb.trap_context = trap_context;
    tcb.mark_runnable();

    Some(())
    // 成功路径上，`old_space` 的旧页表页与 `tcb.memory_set` 替换时释放
    // 的旧数据帧都在内核 `satp` 下（或本就未被激活）归还；新程序由调度
    // 器经 `restore_context` 写入新的 `satp` 后从入口处开始执行。
}
