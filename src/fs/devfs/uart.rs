//! 串口设备的 VFS 文件操作。

use crate::fs::fs_error::{FsError, FsResult};
use crate::{
    driver::tree::DEVICE_MANAGER,
    fs::vfs::file_ops::FileOps,
};

/// 通过 VFS `File` 访问设备管理器中的串口。
///
/// 每次读写都从 [`crate::driver::tree::DEVICE_MANAGER`] 获取当前串口，避免文件
/// 系统层直接持有或操作具体驱动。
#[derive(Debug, Default)]
pub(crate) struct Uart;

impl FileOps for Uart {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_char().ok_or(FsError::NotFound)?;
        Ok(manager.read(did, buf)?)
    }

    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_char().ok_or(FsError::NotFound)?;
        Ok(manager.write(did, buf)?)
    }
}
