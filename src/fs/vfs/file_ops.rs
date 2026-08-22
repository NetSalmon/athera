pub use crate::driver::traits::Whence;
use crate::fs::{
    FsError, FsResult, Path,
    vfs::{DirEntry, Stat, Vec},
};

pub trait FileOps: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }

    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }

    /// 从指定偏移读取，不改变打开文件的当前偏移。
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let _ = offset;
        self.read(buf)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        let _ = offset;
        self.write(buf)
    }

    /// 计算新的文件位置。`current` 由 VFS 打开的文件对象提供，设备和
    /// 文件系统实现不应自行保存用户可见的文件偏移。
    fn seek(&self, current: u64, offset: i64, whence: Whence) -> FsResult<i64> {
        let _ = (current, offset, whence);
        Err(FsError::Unsupported)
    }

    fn ioctl(&self, command: u32, arg: u64) -> FsResult<u64> {
        Err(FsError::Unsupported)
    }

    fn read_dir(&self) -> FsResult<Option<DirEntry>> {
        Err(FsError::Unsupported)
    }
}
