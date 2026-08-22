use crate::fs::{FsResult, vfs::file_ops::FileOps};

pub struct Zero;

impl FileOps for Zero {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Ok(buf.len())
    }
}
