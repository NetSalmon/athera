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

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use athera_macros::lazy;

use crate::{
    bits,
    fs::{FileType, Mode, Path, PathBuf},
    numeric,
};

use crate::dev::traits::IoError;

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

/// 定位基准，对应 `lseek` 的三种模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    /// 从文件开头偏移。
    Start(u64),
    /// 从文件末尾偏移（负值向前）。
    End(i64),
    /// 从当前读写位置偏移。
    Current(i64),
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

/// 文件对象操作：pread / pwrite 语义的底层原语 + 元数据 / 同步。
///
/// `read` / `write` / `seek`（偏移管理）由 [`File`] 在此之上组合。
pub trait FileOps: Send + Sync {
    /// 从 `offset` 处读取，返回实际读取字节数；到文件末尾返回 0。
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize>;
    /// 从 `offset` 处写入，返回实际写入字节数。
    fn write_at(&mut self, buf: &[u8], offset: u64) -> FsResult<usize>;
    fn stat(&self) -> FsResult<Stat>;
    /// 把文件截断到 `len`。
    fn truncate(&mut self, len: u64) -> FsResult<()>;
    /// 把该打开文件未落盘的数据写回磁盘。
    fn sync(&mut self) -> FsResult<()>;
    /// 读取下一个目录项（仅目录文件有效），读完返回 `Ok(None)`。
    fn read_dir(&mut self) -> FsResult<Option<DirEntry>>;
}

/// 内存超级块：一个已挂载文件系统的元数据与路径级操作。
pub struct SuperBlock {
    /// 该文件系统的路径级操作实现。
    pub fs: Arc<dyn FileSystem>,
    /// 根目录项。
    pub root: Arc<Dentry>,
}

/// 内存 inode：文件 / 目录在内存中的元数据缓存。
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
pub struct Dentry {
    pub name: String,
    pub inode: Arc<INode>,
    pub parent: Weak<Dentry>,
    /// 子目录项（按名字索引）。
    pub children: BTreeMap<String, Arc<Dentry>>,
}

/// 打开的文件描述：类型擦除的文件操作 + 共享偏移 + 打开标志。
///
/// 偏移属于“打开文件描述”（`dup` / `fork` 后共享）；若后续要支持该语义，
/// 需要把 `offset` 改为可共享访问（例如放入自旋锁），目前先按独占持有设计。
pub struct File {
    /// 具体文件系统提供的文件操作（含该打开文件的内部状态）。
    pub ops: Box<dyn FileOps>,
    /// 对应目录项（元数据 / 挂载信息）。
    pub dentry: Arc<Dentry>,
    /// 当前读写位置。
    pub offset: u64,
    pub flags: OpenFlags,
}

impl File {
    /// 从当前偏移读取，到文件末尾返回 0；推进内部偏移。
    pub fn read(&mut self, buf: &mut [u8]) -> FsResult<usize> {
        let n = self.ops.read_at(buf, self.offset)?;
        self.offset += n as u64;
        Ok(n)
    }

    /// 从 `offset` 处读取（pread 语义），不改变内部偏移。
    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        self.ops.read_at(buf, offset)
    }

    /// 从当前偏移写入；`append` 模式下无视偏移、总是写到文件末尾。
    pub fn write(&mut self, buf: &[u8]) -> FsResult<usize> {
        let offset = if self.flags.append() {
            self.stat()?.size
        } else {
            self.offset
        };
        let n = self.ops.write_at(buf, offset)?;
        self.offset = offset + n as u64;
        Ok(n)
    }

    /// 从 `offset` 处写入（pwrite 语义），不改变内部偏移。
    pub fn write_at(&mut self, buf: &[u8], offset: u64) -> FsResult<usize> {
        self.ops.write_at(buf, offset)
    }

    /// 移动读写位置，返回新的偏移。
    pub fn seek(&mut self, pos: SeekFrom) -> FsResult<u64> {
        let size = self.stat()?.size as i64;
        let new = match pos {
            SeekFrom::Start(off) => off as i64,
            SeekFrom::End(rel) => size + rel,
            SeekFrom::Current(rel) => self.offset as i64 + rel,
        };
        self.offset = new.max(0) as u64;
        Ok(self.offset)
    }

    pub fn stat(&self) -> FsResult<Stat> {
        self.ops.stat()
    }

    pub fn truncate(&mut self, len: u64) -> FsResult<()> {
        self.ops.truncate(len)
    }

    pub fn sync(&mut self) -> FsResult<()> {
        self.ops.sync()
    }

    /// 读取下一个目录项（仅目录文件有效）。
    pub fn read_dir(&mut self) -> FsResult<Option<DirEntry>> {
        self.ops.read_dir()
    }

    pub fn flags(&self) -> OpenFlags {
        self.flags
    }

    pub fn offset(&self) -> u64 {
        self.offset
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

/// 已挂载的全部超级块。
#[lazy(spin)]
pub static SUPER_BLOCKS: Vec<Arc<SuperBlock>> = Vec::new();

/// 挂载表：挂载点路径 → 挂载项。
#[lazy(spin)]
pub static MOUNT_TABLE: BTreeMap<Path, MountEntry> = BTreeMap::new();

/// 内核统一文件系统入口：基于全局挂载表做路径前缀分发。
///
/// 逻辑尚未实现：各方法只是占位，后续会按路径最长前缀在 [`MOUNT_TABLE`]
/// 中查找挂载点，再把剩余路径交给对应 [`SuperBlock`] 的 [`FileSystem`]。
pub struct Vfs;

impl Vfs {
    pub fn open(path: &Path, flags: OpenFlags, mode: Mode) -> FsResult<File> {
        todo!()
    }

    pub fn stat(path: &Path) -> FsResult<Stat> {
        todo!()
    }

    pub fn mkdir(path: &Path, mode: Mode) -> FsResult<()> {
        todo!()
    }

    pub fn unlink(path: &Path) -> FsResult<()> {
        todo!()
    }

    pub fn rmdir(path: &Path) -> FsResult<()> {
        todo!()
    }

    pub fn rename(old: &Path, new: &Path) -> FsResult<()> {
        todo!()
    }

    pub fn link(old: &Path, new: &Path) -> FsResult<()> {
        todo!()
    }

    pub fn symlink(target: &str, linkpath: &Path) -> FsResult<()> {
        todo!()
    }

    pub fn readlink(path: &Path) -> FsResult<PathBuf> {
        todo!()
    }

    pub fn sync() -> FsResult<()> {
        todo!()
    }
}
