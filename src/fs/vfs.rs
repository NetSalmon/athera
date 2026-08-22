//! VFS（虚拟文件系统层）：与具体文件系统解耦的统一访问接口。
//!
//! 提供：
//! - 统一错误 [`FsError`]（可映射为 POSIX errno，供系统调用层直接返回）；
//! - 元数据与打开标志（[`Stat`]、[`OpenFlags`]、[`DirEntry`]、[`SeekFrom`]）；
//! - 操作 trait：路径级 [`FileSystem`]、文件对象级 [`FileOps`]；
//! - 通用结构：内存超级块 [`SuperBlock`]、内存 inode [`INode`]、目录项缓存
//!   [`Dentry`]、打开的文件 [`File`]，以及挂载表（[`MOUNT_TABLE`]）。
//!
//! 目前只完成类型与接口设计，具体逻辑（挂载分发、路径解析、dentry 缓存、
//! 具体文件系统实现等）尚未实现。

#![allow(unused)]

pub mod file_ops;

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use core::{
    fmt::Debug,
    sync::atomic::{AtomicU64, Ordering},
};

use athera_macros::lazy;

use crate::{
    bits,
    dev::traits::IoError,
    fs::{
        FileType, Mode, Path, PathBuf,
        dev_fs::uart,
        vfs::file_ops::{FileOps, Whence},
    },
    numeric,
    sync::rwlock::RwLock,
};

/// 统一文件系统错误，语义对应 POSIX errno（见 [`FsError::errno`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FsError {
    /// 路径分量不存在（ENOENT）。
    #[error("not found")]
    NotFound,
    /// 路径上的某个分量不是目录（ENOTDIR）。
    #[error("not a directory")]
    NotDir,
    /// 目标本身是目录，却按普通文件操作（EISDIR）。
    #[error("is a directory")]
    IsDir,
    /// 已存在（EEXIST）。
    #[error("already exists")]
    AlreadyExists,
    /// 目录非空（ENOTEMPTY）。
    #[error("directory not empty")]
    NotEmpty,
    /// 权限不足（EACCES）。
    #[error("permission denied")]
    PermissionDenied,
    /// 文件名过长（ENAMETOOLONG）。
    #[error("name too long")]
    NameTooLong,
    /// 符号链接跳数过多（ELOOP）。
    #[error("too many symbolic links")]
    TooManyLinks,
    /// 跨设备（EXDEV）。
    #[error("cross-device operation")]
    CrossDevice,
    /// 磁盘空间不足（ENOSPC）。
    #[error("no space left on device")]
    NoSpace,
    /// 参数非法（EINVAL）。
    #[error("invalid argument")]
    Invalid,
    /// 设备 I/O 错误（EIO）。
    #[error("I/O error")]
    Io,
    /// 内存不足（ENOMEM）。
    #[error("out of memory")]
    OutOfMemory,
    /// 不支持的操作（ENOTSUP）。
    #[error("operation not supported")]
    Unsupported,
}

impl FsError {
    /// 映射为 Linux errno 数值，供系统调用层直接返回。
    pub const fn errno(self) -> isize {
        use FsError::*;
        match self {
            NotFound => 2,          // ENOENT
            Io => 5,                // EIO
            OutOfMemory => 12,      // ENOMEM
            PermissionDenied => 13, // EACCES
            AlreadyExists => 17,    // EEXIST
            CrossDevice => 18,      // EXDEV
            NotDir => 20,           // ENOTDIR
            IsDir => 21,            // EISDIR
            Invalid => 22,          // EINVAL
            NoSpace => 28,          // ENOSPC
            NameTooLong => 36,      // ENAMETOOLONG
            NotEmpty => 39,         // ENOTEMPTY
            TooManyLinks => 40,     // ELOOP
            Unsupported => 95,      // ENOTSUP
        }
    }
}

impl From<IoError> for FsError {
    fn from(_: IoError) -> Self {
        Self::Io
    }
}

/// 统一错误类型别名。
pub type FsResult<T> = core::result::Result<T, FsError>;

/// 文件元数据（对应 `stat` / `fstat` 系统调用返回的核心字段）。
#[derive(Debug, Clone)]
pub struct Stat {
    pub ino: u64,
    /// mode 位：文件类型（bits 15..12）+ 访问权限位。
    pub mode: Mode,
    pub size: u64,
    pub nlinks: u32,
    pub mtime: i64,
    pub uid: u32,
    pub gid: u32,
}

/// 访问模式：对应 POSIX `O_ACCMODE` 字段（低 2 位的取值）。
numeric! {
    pub enum AccessMode : u32 {
        READ_ONLY = 0,   // O_RDONLY
        WRITE_ONLY = 1,  // O_WRONLY
        READ_WRITE = 2,  // O_RDWR
    }
}

/// 打开标志：按 POSIX `open` 的位布局定义（位位置与 Linux `O_*` 常量一致），
/// 便于系统调用层直接用用户态传来的原始整数构造（`From<u32>`）。
bits! {
    pub type OpenFlags : u32 {
        // 访问模式（bits 0..1，即 POSIX O_ACCMODE）。
        accmode: AccessMode : 0 => 1,
        // O_CREAT（bit 6）：不存在则创建。
        create: 6,
        // O_EXCL（bit 7）：与 `create` 连用，已存在则报错。
        exclusive: 7,
        // O_TRUNC（bit 9）：打开即截断为零。
        truncate: 9,
        // O_APPEND（bit 10）：写入追加到文件末尾。
        append: 10,
        // O_NONBLOCK（bit 11）。
        nonblock: 11,
        // O_DIRECTORY（bit 16）：要求目标是目录。
        directory: 16,
    }
}

impl OpenFlags {
    /// 只读。
    pub const fn read_only() -> Self {
        Self::from(0) // O_RDONLY
    }

    /// 只写。
    pub const fn write_only() -> Self {
        Self::from(1) // O_WRONLY
    }

    /// 读写。
    pub const fn read_write() -> Self {
        Self::from(2) // O_RDWR
    }

    /// 是否允许读取（访问模式不是 O_WRONLY）。
    pub fn can_read(&self) -> bool {
        self.accmode() != AccessMode::WRITE_ONLY
    }

    /// 是否允许写入（访问模式不是 O_RDONLY）。
    pub fn can_write(&self) -> bool {
        self.accmode() != AccessMode::READ_ONLY
    }
}

/// 目录项（`getdents` / `readdir` 的底层产物）。
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub ino: u64,
    pub name: String,
    pub file_type: FileType,
}

/// 路径级文件系统操作：供挂载表分发，由具体文件系统实现。
///
/// 输入路径是相对本文件系统根目录的完整路径（`/` 开头）。返回的 [`File`]
/// 是类型擦除的文件句柄，不再依赖具体文件系统类型。
pub trait FileSystem: Send + Sync {
    /// 按 `flags` 打开（必要时创建）`path`，返回文件句柄。
    fn open(&self, path: &Path, flags: OpenFlags, mode: Mode) -> FsResult<File>;
    /// 读取路径元数据。
    fn stat(&self, path: &Path) -> FsResult<Stat>;
    /// 创建目录。
    fn mkdir(&self, path: &Path, mode: Mode) -> FsResult<()>;
    /// 删除非目录文件 / 符号链接。
    fn unlink(&self, path: &Path) -> FsResult<()>;
    /// 删除空目录。
    fn rmdir(&self, path: &Path) -> FsResult<()>;
    /// 重命名（`old` → `new`）。
    fn rename(&self, old: &Path, new: &Path) -> FsResult<()>;
    /// 创建硬链接（`new` 指向 `old`）。
    fn link(&self, old: &Path, new: &Path) -> FsResult<()>;
    /// 创建符号链接，内容为 `target` 路径。
    fn symlink(&self, target: &str, linkpath: &Path) -> FsResult<()>;
    /// 读取符号链接的目标路径。
    fn readlink(&self, path: &Path) -> FsResult<PathBuf>;
    /// 把本文件系统的缓存 / 脏数据写回磁盘。
    fn sync(&self) -> FsResult<()>;
}

/// 内存超级块：一个已挂载文件系统的元数据与路径级操作。
pub struct SuperBlock {
    /// 该文件系统的路径级操作实现。
    pub fs: Arc<dyn FileSystem>,
    /// 根目录项。
    pub root: Arc<Dentry>,
}

/// 内存 inode：文件 / 目录在内存中的元数据缓存。
#[derive(Debug)]
pub struct INode {
    pub ino: u64,
    pub mode: Mode,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: i64,
    pub nlinks: u32,
    /// 所属超级块（弱引用，避免 `SuperBlock → Dentry → INode` 成环泄漏）。
    pub superblock: Weak<SuperBlock>,
    /// 指向该 inode 的目录项（别名的弱引用，对应 Linux 的 `i_dentry`）。
    pub dentries: Vec<Weak<Dentry>>,
}

/// 目录项缓存：命名空间树上的一个节点。
#[derive(Debug)]
pub struct Dentry {
    pub name: String,
    pub inode: Arc<INode>,
    pub parent: Weak<Dentry>,
    /// 子目录项（按名字索引）。
    pub children: BTreeMap<String, Arc<Dentry>>,
}

pub struct File {
    /// 具体文件系统提供的文件操作（含该打开文件的内部状态）。
    pub ops: Arc<dyn FileOps>,
    /// 对应目录项（元数据 / 挂载信息）。
    pub dentry: Arc<Dentry>,
    /// 当前读写位置。
    pub offset: Arc<AtomicU64>,
    pub flags: OpenFlags,
}

impl Clone for File {
    fn clone(&self) -> Self {
        Self {
            ops: self.ops.clone(),
            dentry: self.dentry.clone(),
            offset: self.offset.clone(),
            flags: self.flags,
        }
    }
}

impl Debug for File {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("File")
            .field("ops", &"<dyn FileOps>")
            .field("dentry", &self.dentry)
            .field("offset", &self.offset.load(Ordering::Relaxed))
            .field("flags", &self.flags)
            .finish()
    }
}

impl File {
    /// 创建一个由文件系统提供操作实现的打开文件。
    pub(crate) fn from_ops(
        name: &str,
        ino: u64,
        mode: Mode,
        ops: Arc<dyn FileOps>,
        flags: OpenFlags,
    ) -> Self {
        let inode = Arc::new(INode {
            ino,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            mtime: 0,
            nlinks: 1,
            superblock: Weak::new(),
            dentries: Vec::new(),
        });
        let dentry = Arc::new(Dentry {
            name: String::from(name),
            inode,
            parent: Weak::new(),
            children: BTreeMap::new(),
        });

        Self {
            ops,
            dentry,
            offset: Arc::new(AtomicU64::new(0)),
            flags,
        }
    }

    /// 创建连接到内核串口的文件描述符。
    pub(crate) fn uart(flags: OpenFlags) -> Self {
        Self::from_ops(
            "console",
            0,
            Mode::from((FileType::CHR.0 << 12) | 0o666),
            Arc::new(uart::Uart),
            flags,
        )
    }

    /// 从当前偏移读取，到文件末尾返回 0；推进内部偏移。
    pub fn read(&self, buf: &mut [u8]) -> FsResult<usize> {
        let offset = self.offset.load(Ordering::Acquire);
        let n = self.ops.read_at(buf, offset)?;
        self.offset.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    /// 从当前偏移写入；`append` 模式下无视偏移、总是写到文件末尾。
    pub fn write(&self, buf: &[u8]) -> FsResult<usize> {
        let offset = self.offset.load(Ordering::Acquire);
        let n = self.ops.write_at(buf, offset)?;
        self.offset.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    /// 从指定偏移读取，不改变当前文件偏移。
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        self.ops.read_at(buf, offset)
    }

    /// 从指定偏移写入，不改变当前文件偏移。
    pub fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        self.ops.write_at(buf, offset)
    }

    /// 移动读写位置，返回新的偏移。
    pub fn seek(&self, offset: i64, whence: Whence) -> FsResult<i64> {
        let current = self.offset.load(Ordering::Acquire);
        let new_offset = self.ops.seek(current, offset, whence)?;
        if new_offset >= 0 {
            self.offset.store(new_offset as u64, Ordering::Relaxed);
        }
        Ok(new_offset)
    }

    pub fn stat(&self) -> FsResult<Stat> {
        Ok(Stat {
            ino: self.dentry.inode.ino,
            mode: self.dentry.inode.mode,
            size: self.dentry.inode.size,
            nlinks: self.dentry.inode.nlinks,
            mtime: self.dentry.inode.mtime,
            uid: self.dentry.inode.uid,
            gid: self.dentry.inode.gid,
        })
    }

    /// 读取下一个目录项（仅目录文件有效）。
    pub fn read_dir(&self) -> FsResult<Option<DirEntry>> {
        self.ops.read_dir()
    }

    pub fn flags(&self) -> OpenFlags {
        self.flags
    }

    pub fn offset(&self) -> u64 {
        self.offset.load(Ordering::Relaxed)
    }

    pub fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }
}

/// 一个挂载点：挂载位置对应的目录项 + 挂载上来的超级块。
pub struct MountEntry {
    pub dentry: Arc<Dentry>,
    pub superblock: Arc<SuperBlock>,
}

/// 内核统一文件系统入口：基于全局挂载表做路径前缀分发。
///
/// 逻辑尚未实现：各方法只是占位，后续会按路径最长前缀在 [`MOUNT_TABLE`]
/// 中查找挂载点，再把剩余路径交给对应 [`SuperBlock`] 的 [`FileSystem`]。
pub struct Vfs {
    mount_table: RwLock<BTreeMap<String, Arc<MountEntry>>>,
    super_blocks: RwLock<Vec<Arc<SuperBlock>>>,
}

impl Vfs {
    /// 创建一个空的 VFS。
    pub const fn new() -> Self {
        Self {
            mount_table: RwLock::new(BTreeMap::new()),
            super_blocks: RwLock::new(Vec::new()),
        }
    }

    /// 把文件系统挂载到指定路径。
    ///
    /// 挂载点必须是绝对路径；同一路径只能存在一个挂载。文件系统根目录的
    /// 元数据在挂载时读取一次，用于建立挂载表中的内存目录项。
    pub fn mount(&self, path: &Path, fs: Arc<dyn FileSystem>) -> FsResult<()> {
        let mount_path = Self::mount_path(path)?;
        let stat = fs.stat(&Path::from("/"))?;
        let mut mounts = self.mount_table.write();
        if mounts.contains_key(&mount_path) {
            return Err(FsError::AlreadyExists);
        }

        let root = Self::root_dentry(&mount_path, &stat);
        let superblock = Arc::new(SuperBlock {
            fs,
            root: root.clone(),
        });

        let entry = Arc::new(MountEntry {
            dentry: root,
            superblock: superblock.clone(),
        });

        mounts.insert(mount_path, entry);
        drop(mounts);
        self.super_blocks.write().push(superblock);
        Ok(())
    }

    fn mount_path(path: &Path) -> FsResult<String> {
        if !path.is_absolute() {
            return Err(FsError::Invalid);
        }
        let path = path.as_str().trim_end_matches('/');
        Ok(if path.is_empty() {
            String::from("/")
        } else {
            String::from(path)
        })
    }

    fn root_dentry(path: &str, stat: &Stat) -> Arc<Dentry> {
        let name = if path == "/" {
            "/"
        } else {
            path.rsplit('/').next().unwrap_or(path)
        };
        Arc::new(Dentry {
            name: String::from(name),
            inode: Arc::new(INode {
                ino: stat.ino,
                mode: stat.mode,
                uid: stat.uid,
                gid: stat.gid,
                size: stat.size,
                mtime: stat.mtime,
                nlinks: stat.nlinks,
                superblock: Weak::new(),
                dentries: Vec::new(),
            }),
            parent: Weak::new(),
            children: BTreeMap::new(),
        })
    }

    /// 解析路径，返回最长匹配挂载点、文件系统和文件系统内路径。
    fn resolve(&self, path: &Path) -> FsResult<(Arc<dyn FileSystem>, PathBuf)> {
        if !path.is_absolute() {
            return Err(FsError::Invalid);
        }

        let mounts = self.mount_table.read();
        let Some((mount_path, mount)) = mounts
            .iter()
            .filter(|(mount_path, _)| Self::mount_matches(mount_path, path.as_str()))
            .max_by_key(|(mount_path, _)| mount_path.len())
        else {
            return Err(FsError::NotFound);
        };

        let relative = Self::relative_path(mount_path, path.as_str());
        Ok((mount.superblock.fs.clone(), relative))
    }

    fn mount_matches(mount: &str, path: &str) -> bool {
        if mount == "/" {
            return path.starts_with('/');
        }
        let mount = mount.trim_end_matches('/');
        path == mount
            || path
                .strip_prefix(mount)
                .is_some_and(|rest| rest.starts_with('/'))
    }

    fn relative_path(mount: &str, path: &str) -> PathBuf {
        if mount == "/" {
            return PathBuf::from(path);
        }

        let mount = mount.trim_end_matches('/');
        let rest = path.strip_prefix(mount).unwrap_or("");
        if rest.is_empty() {
            PathBuf::from("/")
        } else {
            PathBuf::from(rest)
        }
    }

    /// 在两个路径属于同一文件系统时执行操作，否则返回 `EXDEV`。
    fn same_filesystem<T, F>(&self, old: &Path, new: &Path, operation: F) -> FsResult<T>
    where
        F: FnOnce(&dyn FileSystem, &Path, &Path) -> FsResult<T>,
    {
        let (old_fs, old_path) = self.resolve(old)?;
        let (new_fs, new_path) = self.resolve(new)?;
        if !Arc::ptr_eq(&old_fs, &new_fs) {
            return Err(FsError::CrossDevice);
        }
        operation(old_fs.as_ref(), &old_path, &new_path)
    }
}

impl FileSystem for Vfs {
    fn open(&self, path: &Path, flags: OpenFlags, mode: Mode) -> FsResult<File> {
        let (fs, relative) = self.resolve(path)?;
        fs.open(&relative, flags, mode)
    }

    fn stat(&self, path: &Path) -> FsResult<Stat> {
        let (fs, relative) = self.resolve(path)?;
        fs.stat(&relative)
    }

    fn mkdir(&self, path: &Path, mode: Mode) -> FsResult<()> {
        let (fs, relative) = self.resolve(path)?;
        fs.mkdir(&relative, mode)
    }

    fn unlink(&self, path: &Path) -> FsResult<()> {
        let (fs, relative) = self.resolve(path)?;
        fs.unlink(&relative)
    }

    fn rmdir(&self, path: &Path) -> FsResult<()> {
        let (fs, relative) = self.resolve(path)?;
        fs.rmdir(&relative)
    }

    fn rename(&self, old: &Path, new: &Path) -> FsResult<()> {
        self.same_filesystem(old, new, |fs, old, new| fs.rename(old, new))
    }

    fn link(&self, old: &Path, new: &Path) -> FsResult<()> {
        self.same_filesystem(old, new, |fs, old, new| fs.link(old, new))
    }

    fn symlink(&self, target: &str, linkpath: &Path) -> FsResult<()> {
        let (fs, relative) = self.resolve(linkpath)?;
        fs.symlink(target, &relative)
    }

    fn readlink(&self, path: &Path) -> FsResult<PathBuf> {
        let (fs, relative) = self.resolve(path)?;
        fs.readlink(&relative)
    }

    fn sync(&self) -> FsResult<()> {
        let super_blocks = self.super_blocks.read().clone();
        for superblock in super_blocks {
            superblock.fs.sync()?;
        }
        Ok(())
    }
}
