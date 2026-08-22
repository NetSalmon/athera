//! Process lifecycle and file-descriptor services.

use crate::{
    fs::{FsError, vfs::File},
    task::{
        CURRENT_TASK,
        task::{TASKS, TaskStatus, Tid},
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
        let Some(tid) = get_zombie_child(current, target_tid) else {
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

fn get_zombie_child(parent: Tid, target: Option<Tid>) -> Option<usize> {
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
