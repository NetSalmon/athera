//! 串口设备的 VFS 文件操作。

use crate::{
    dev::{
        UART,
        traits::{Read, Write},
    },
    fs::{FsError, FsResult, vfs::file_ops::FileOps},
};

/// 通过 VFS `File` 访问内核串口。
///
/// 串口本身由 [`crate::dev::UART`] 全局持有，因此该类型不需要保存设备引用；
/// 每次读写只在操作期间取得串口锁。
#[derive(Debug, Default)]
pub(crate) struct Uart;

impl FileOps for Uart {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        let uart = UART.force().as_ref().ok_or(FsError::Io)?;
        Ok(uart.lock().read(buf)?)
    }

    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        let uart = UART.force().as_ref().ok_or(FsError::Io)?;
        Ok(uart.lock().write(buf)?)
    }
}
