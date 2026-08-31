//! `/dev/vda` 块设备节点的 [`FileOps`] 实现。
//!
//! 通过设备管理器（[`crate::driver::tree::DEVICE_MANAGER`]）获取首个块设备句柄，
//! 委托 `read_at` / `write_at` 完成实际 I/O。

use crate::fs::fs_error::{FsError, FsResult};
use crate::{
    driver::tree::DEVICE_MANAGER,
    fs::vfs::file_ops::FileOps,
};

/// virtio-blk 块设备节点（`/dev/vda`）的文件操作实现。
///
/// 读写均委托给设备管理器中的首个块设备（[`DEVICE_MANAGER`]）。
pub struct VirtioBlk;

impl FileOps for VirtioBlk {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_block().ok_or(FsError::NotFound)?;
        Ok(manager.read_at(did, buf, offset as usize)?)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_block().ok_or(FsError::NotFound)?;
        Ok(manager.write_at(did, buf, offset as usize)?)
    }
}
