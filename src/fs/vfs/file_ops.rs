//! 文件操作 trait（[`FileOps`]）。
//!
//! 定义文件对象级别的操作接口：读写、偏移定位、ioctl、目录遍历等。
//! 设备节点（devfs）和文件系统（minix）各自实现此 trait 提供具体行为。

pub use crate::driver::traits::Whence;
use crate::fs::fs_error::{FsError, FsResult};
use crate::fs::
vfs::DirEntry;

/// 文件对象级别的操作接口。
///
/// 每个打开的文件（[`crate::fs::vfs::File`]）持有一个 `Arc<dyn FileOps>`，
/// 所有 I/O 操作委托给此 trait 的实现。默认实现返回 [`FsError::Unsupported`]，
/// 实现者只需覆盖所需方法。
pub trait FileOps: Send + Sync {
    /// 从当前位置读取数据到 `buf`，返回实际读取的字节数。
    fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }

    /// 将 `buf` 中的数据写入当前位置，返回实际写入的字节数。
    fn write(&self, buf: &[u8]) -> FsResult<usize> {
        Err(FsError::Unsupported)
    }

    /// 从指定偏移读取，不改变打开文件的当前偏移。
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let _ = offset;
        self.read(buf)
    }

    /// 从指定偏移写入数据，不改变打开文件的当前偏移。
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

    /// 设备控制操作（ioctl），`command` 为操作码，`arg` 为参数。
    fn ioctl(&self, command: u32, arg: u64) -> FsResult<u64> {
        Err(FsError::Unsupported)
    }

    /// 读取下一个目录项，用于目录遍历。返回 `None` 表示目录已遍历完毕。
    fn read_dir(&self) -> FsResult<Option<DirEntry>> {
        Err(FsError::Unsupported)
    }
}
