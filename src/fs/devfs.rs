//! 最小设备文件系统。
//!
//! 设备节点是静态的：设备驱动只需要提供 [`FileOps`]，这里负责把固定的
//! 文件名映射到对应实现。真正的设备探测和生命周期仍归 [`crate::driver`] 负责。

use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use uart::Uart;
use virtio_blk::VirtioBlk;

use self::pseudo::{Null, Zero};
use super::{
    FileType, Mode, Path,
    vfs::{DirEntry, File, FileSystem, FsError, FsResult, OpenFlags, Stat, file_ops::FileOps},
};

mod pseudo;
pub mod uart;
pub mod virtio_blk;

struct DeviceNode {
    name: &'static str,
    ino: u64,
    mode: Mode,
    make_ops: fn() -> Arc<dyn FileOps>,
}

const DEVICES: &[DeviceNode] = &[
    DeviceNode {
        name: "null",
        ino: 2,
        mode: Mode::from((FileType::CHR.0 << 12) | 0o666),
        make_ops: || Arc::new(Null),
    },
    DeviceNode {
        name: "zero",
        ino: 3,
        mode: Mode::from((FileType::CHR.0 << 12) | 0o666),
        make_ops: || Arc::new(Zero),
    },
    DeviceNode {
        name: "console",
        ino: 4,
        mode: Mode::from((FileType::CHR.0 << 12) | 0o666),
        make_ops: || Arc::new(Uart),
    },
    DeviceNode {
        name: "vda",
        ino: 5,
        mode: Mode::from((FileType::BLK.0 << 12) | 0o660),
        make_ops: || Arc::new(VirtioBlk),
    },
];

/// 只包含固定设备节点的设备文件系统，通常挂载到 `/dev`。
#[derive(Debug, Default)]
pub struct DevFs;

impl DevFs {
    pub const fn new() -> Self {
        Self
    }

    fn node(path: &Path) -> FsResult<Option<&'static DeviceNode>> {
        if !path.is_absolute() {
            return Err(FsError::Invalid);
        }
        let name = path.as_str().trim_matches('/');
        if name.is_empty() {
            return Ok(None);
        }
        if name.contains('/') {
            return Err(FsError::NotFound);
        }
        Ok(DEVICES.iter().find(|node| node.name == name))
    }

    fn root(flags: OpenFlags) -> File {
        File::from_ops(
            "/",
            1,
            Mode::from((FileType::DIR.0 << 12) | 0o755),
            Arc::new(DeviceDirectory {
                next: AtomicUsize::new(0),
            }),
            flags,
        )
    }
}

impl FileSystem for DevFs {
    fn open(&self, path: &Path, flags: OpenFlags, _mode: Mode) -> FsResult<File> {
        if !path.is_absolute() {
            return Err(FsError::Invalid);
        }
        if path.as_str().trim_matches('/').is_empty() {
            return Ok(Self::root(flags));
        }
        let node = Self::node(path)?.ok_or(FsError::NotFound)?;
        if flags.directory() {
            return Err(FsError::NotDir);
        }
        Ok(File::from_ops(
            node.name,
            node.ino,
            node.mode,
            (node.make_ops)(),
            flags,
        ))
    }

    fn stat(&self, path: &Path) -> FsResult<Stat> {
        if !path.is_absolute() {
            return Err(FsError::Invalid);
        }
        if path.as_str().trim_matches('/').is_empty() {
            return Ok(Stat {
                ino: 1,
                mode: Mode::from((FileType::DIR.0 << 12) | 0o755),
                size: 0,
                nlinks: 1,
                mtime: 0,
                uid: 0,
                gid: 0,
            });
        }
        let node = Self::node(path)?.ok_or(FsError::NotFound)?;
        Ok(Stat {
            ino: node.ino,
            mode: node.mode,
            size: 0,
            nlinks: 1,
            mtime: 0,
            uid: 0,
            gid: 0,
        })
    }

    fn mkdir(&self, _: &Path, _: Mode) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn unlink(&self, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn rmdir(&self, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn rename(&self, _: &Path, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn link(&self, _: &Path, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn symlink(&self, _: &str, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn readlink(&self, _: &Path) -> FsResult<super::PathBuf> {
        Err(FsError::Unsupported)
    }

    fn sync(&self) -> FsResult<()> {
        Ok(())
    }
}

struct DeviceDirectory {
    next: AtomicUsize,
}

impl FileOps for DeviceDirectory {
    fn read_dir(&self) -> FsResult<Option<DirEntry>> {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        let Some(node) = DEVICES.get(index) else {
            return Ok(None);
        };
        Ok(Some(DirEntry {
            ino: node.ino,
            name: node.name.into(),
            file_type: node.mode.file_type(),
        }))
    }
}
