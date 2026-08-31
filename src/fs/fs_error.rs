use crate::driver::traits::IoError;

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

        use crate::fs::FsError::{
            AlreadyExists, CrossDevice, Invalid, Io, IsDir, NameTooLong, NoSpace, NotDir, NotEmpty,
            NotFound, OutOfMemory, PermissionDenied, TooManyLinks, Unsupported,
        };
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
