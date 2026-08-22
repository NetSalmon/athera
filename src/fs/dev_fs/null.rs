use crate::fs::{FsResult, vfs::file_ops::FileOps};

pub struct Null;

impl FileOps for Null {
    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Ok(buf.len())
    }

    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        Ok(0)
    }
}
