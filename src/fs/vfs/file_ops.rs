use crate::fs::vfs::Vec;
use crate::fs::{FsError, FsResult, Path};
use crate::fs::vfs::{DirEntry, Stat};
use crate::numeric;

pub trait FileOps: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }
    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }

    fn seek(&self, offset: i64, whence: Whence) -> FsResult<i64> {
        Err(FsError::Unsupported)
    }

    fn ioctl(&self, command: u32, arg: u64) -> FsResult<u64> {
        Err(FsError::Unsupported)
    }

    fn read_dir(&self) -> FsResult<Option<DirEntry>> {
        Err(FsError::Unsupported)
    }
}

numeric! {
    pub enum Whence : i32 {
        SEEK_SET = 0,
        SEEK_CUR = 1,
        SEEK_END = 2,
    }
}
