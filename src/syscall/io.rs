//! 文件描述符相关的系统调用。

use crate::{
    dev::{UART, traits::CharDevice},
    syscall::abi::ErrorCode,
};

pub(super) fn read(_fd: u64, buf: &mut [u8]) -> u64 {
    let uart = match UART.force().as_ref() {
        Some(u) => u,
        None => return ErrorCode::EIO.0 as u64,
    };

    let mut read = 0;
    for chunk in buf.chunks_mut(64) {
        let bytes = match uart.lock().read(chunk) {
            Ok(bytes) => bytes,
            Err(_) => return ErrorCode::EIO.0 as u64,
        };
        read += bytes;
        if bytes < chunk.len() {
            break;
        }
    }
    read as u64
}

pub(super) fn write(_fd: u64, buf: &[u8]) -> u64 {
    // 分段持锁，避免大块输出长时间关闭中断。
    if let Some(uart) = UART.force().as_ref() {
        let mut written = 0;
        for chunk in buf.chunks(64) {
            match uart.lock().write(chunk) {
                Ok(bytes) => written += bytes,
                Err(_) => return ErrorCode::EIO.0 as u64,
            }
        }
        return written as u64;
    }
    ErrorCode::EIO.0 as u64
}
