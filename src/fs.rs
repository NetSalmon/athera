#![allow(unused)]
//! 文件系统：与具体实现无关的公共类型（路径、文件类型、mode），
//! 以及具体文件系统实现。

pub(crate) mod devfs;
pub(crate) mod minix;
mod path;
mod types;
pub(crate) mod vfs;

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

use athera_macros::lazy;
pub(crate) use path::{Component, Path, PathBuf};
pub(crate) use types::{FileType, Mode, S_IFMT};
pub(crate) use vfs::{FsError, FsResult};

use crate::{driver::tree::DEVICE_MANAGER, sync::rwlock::RwLock};

#[lazy]
pub static VFS: vfs::Vfs = vfs::Vfs::new();

pub(crate) static VFS_CONSOLE_READY: AtomicBool = AtomicBool::new(false);

pub(crate) fn enable_vfs_console() {
    VFS_CONSOLE_READY.store(true, Ordering::Release);
}

/// 初始化根文件系统并挂载设备文件系统。
pub(crate) fn init() -> FsResult<()> {
    let device = DEVICE_MANAGER
        .force()
        .read()
        .block_handle()
        .ok_or(FsError::NotFound)?;
    let device = Arc::new(RwLock::new(device));
    let minix = minix::MinixFs::from_device(&device)
        .map_err(FsError::from)?
        .ok_or(FsError::NotFound)?;

    VFS.force().mount(
        &Path::from("/"),
        Arc::new(minix::MinixVfs::new(Arc::new(minix))),
    )?;
    VFS.force()
        .mount(&Path::from("/dev"), Arc::new(devfs::DevFs::new()))
}
