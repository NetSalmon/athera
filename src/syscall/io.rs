//! 文件描述符相关的系统调用。

use crate::{
    fs::vfs::FsError,
    proc::{CURRENT_TASK, task::TASKS},
    syscall::abi::ErrorCode,
};

pub(super) fn read(fd: u64, buf: &mut [u8]) -> u64 {
    let Some(current) = *CURRENT_TASK.current() else {
        return ErrorCode::ESRCH.0 as u64;
    };
    let fd = fd as usize;
    let file = {
        let tasks = TASKS.force().lock();
        let Some(file) = tasks.get(&current).and_then(|task| task.fd_table.get(fd)) else {
            return ErrorCode::EBADF.0 as u64;
        };
        file.clone()
    };
    if !file.flags().can_read() {
        return ErrorCode::EBADF.0 as u64;
    }
    fs_result(file.read(buf))
}

pub(super) fn write(fd: u64, buf: &[u8]) -> u64 {
    let Some(current) = *CURRENT_TASK.current() else {
        return ErrorCode::ESRCH.0 as u64;
    };
    let fd = fd as usize;
    let file = {
        let tasks = TASKS.force().lock();
        let Some(file) = tasks.get(&current).and_then(|task| task.fd_table.get(fd)) else {
            return ErrorCode::EBADF.0 as u64;
        };
        file.clone()
    };
    if !file.flags().can_write() {
        return ErrorCode::EBADF.0 as u64;
    }
    fs_result(file.write(buf))
}

fn fs_result(result: Result<usize, FsError>) -> u64 {
    match result {
        Ok(size) => size as u64,
        Err(err) => (-(err.errno())) as u64,
    }
}
