//! Process lifecycle and file-descriptor services.

use crate::fs::vfs::FileSystem;
use alloc::vec;
use alloc::vec::Vec;
use crate::task::exec::{load_elf, Load};
use crate::mm::page_table::{AddressSpaceId, ADDRESS_SPACE_MANAGER};
use crate::{error, fs::{FsError, vfs::File}, task::{
    CURRENT_TASK,
    task::{TASKS, TaskStatus, Tid},
}};
use crate::fs::{Path, VFS};
use crate::fs::vfs::OpenFlags;

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

pub fn user_execve(path: &str, argv: &[&str], envp: &[&str]) -> Option<()> {
    let current_tid = (*CURRENT_TASK.current())?;

    let f = match VFS.force().open(
        &Path::from(path),
        OpenFlags::read_only(),
        crate::fs::Mode::from(0),
    ) {
        Ok(file) => file,
        Err(err) => {
            error!("failed to open {path}: {err}");
            return None;
        }
    };

    let Ok(size) = usize::try_from(match VFS.force().stat(&Path::from(path)) {
        Ok(stat) => stat.size,
        Err(err) => {
            error!("failed to stat {path}: {err}");
            return None;
        }
    }) else {
        error!("{path} is too large to load");
        return None;
    };

    let mut buf = vec![0u8; size];

    let read = match f.read(&mut buf) {
        Ok(read) => read,
        Err(err) => {
            error!("failed to read {path}: {err}");
            return None;
        }
    };
    if read != buf.len() {
        error!("short read for {path}: expected {}, got {read}", buf.len());
        return None;
    }

    // ---- 重建地址空间 ----
    //
    // 此刻 `satp` 仍指向本任务的用户根页表（内核经 `copy_low_half`
    // 继承的低半区映射在其中）。必须先切回内核地址空间，再重建用户
    // 地址空间：否则旧页表页在被释放后会立即被新程序的段分配复用并
    // 清零，CPU 仍在用被抹掉的页表取指 / 访存，内核直接卡死。
    if let Err(err) = ADDRESS_SPACE_MANAGER
        .force()
        .lock()
        .activate(AddressSpaceId::Kernel)
    {
        error!("failed to activate kernel address space: {err}");
        return None;
    }
    // 旧地址空间先取走暂存：加载失败时还能放回去，让系统调用带着
    // -errno 安全返回旧程序继续执行。
    let old_space = ADDRESS_SPACE_MANAGER
        .force()
        .lock()
        .remove_user(current_tid);

    let load = match load_elf(buf.as_slice(), argv, envp, current_tid) {
        Ok(load) => load,
        Err(err) => {
            error!("failed to load {path}: {err}");
            let mut manager = ADDRESS_SPACE_MANAGER.force().lock();
            if let Some(old) = old_space {
                manager.insert_user(current_tid, old);
            }
            if let Err(err) = manager.activate(AddressSpaceId::User(current_tid)) {
                error!("failed to restore address space of {current_tid:?}: {err}");
            }
            return None;
        }
    };

    let Load {
        memory_set,
        trap_context,
    } = load;

    if let Some(ref mut tcb) = TASKS.force().lock().get_mut(&current_tid) {
        tcb.memory_set = memory_set;
        tcb.trap_context = trap_context;

        Some(())
    } else {
        error!("current task {current_tid:?} disappeared during execve");
        None
    }
    // 成功路径上，`old_space` 的旧页表页与 `tcb.memory_set` 替换时释放
    // 的旧数据帧都在内核 `satp` 下归还；新程序由调度器经
    // `restore_context` 写入新的 `satp` 后从入口处开始执行。
}