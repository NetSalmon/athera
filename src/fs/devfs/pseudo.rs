//! 内核伪设备。

use crate::fs::{fs_error::FsResult, vfs::file_ops::FileOps};

/// 丢弃所有写入并立即返回 EOF 的字符设备。
pub(crate) struct Null;

impl FileOps for Null {
    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Ok(buf.len())
    }

    fn read(&self, _: &mut [u8]) -> FsResult<usize> {
        Ok(0)
    }
}

/// 读取时返回零字节，写入时丢弃数据的字符设备。
pub(crate) struct Zero;

impl FileOps for Zero {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Ok(buf.len())
    }
}
