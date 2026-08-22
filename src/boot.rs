//! 启动编排。
//!
//! 从 virtio-blk 上的 MINIX 文件系统按路径读取用户程序 ELF，并交给
//! `proc::exec::spawn_buffer` 加载执行。

use alloc::vec;

use crate::{
    dev::VIRTIO_BLK,
    error,
    fs::{Path, minix_fs::MinixFs},
    proc::exec::spawn_buffer,
};

/// 从 virtio-blk 上的 MINIX 文件系统按路径读取文件并加载为进程。
pub(crate) fn spawn_from_disk(path: &str) {
    let blk = {
        match VIRTIO_BLK.force().lock().as_ref() {
            Some(blk) => blk.clone(),
            None => {
                error!("no virtio-blk device, skip {path}");
                return;
            }
        }
    };

    let fs = match MinixFs::from_device(&blk) {
        Ok(Some(fs)) => fs,
        Ok(None) => {
            error!("not a MINIX filesystem, skip {path}");
            return;
        }
        Err(_) => {
            error!("failed to read MINIX superblock, skip {path}");
            return;
        }
    };

    let Some(mut f) = (match fs.open(&Path::from_str(path)) {
        Ok(file) => file,
        Err(err) => {
            error!("failed to open {path}: {err}");
            return;
        }
    }) else {
        error!("{path} not found on disk");
        return;
    };

    let Ok(size) = usize::try_from(f.size()) else {
        error!("{path} is too large to load");
        return;
    };
    let mut buf = vec![0u8; size];

    let read = match f.read(&mut buf) {
        Ok(read) => read,
        Err(err) => {
            error!("failed to read {path}: {err}");
            return;
        }
    };
    if read != buf.len() {
        error!("short read for {path}: expected {}, got {read}", buf.len());
        return;
    }

    if let Err(err) = spawn_buffer(&buf, None) {
        error!("failed to execute user program: {err}");
    }
}

/// 启动系统内置的用户程序。
pub(crate) fn spawn_default_programs() {
    for path in [
        "/bin/init",
        "/bin/hello_world",
        "/bin/quick_sort",
        "/bin/panic",
        "/bin/sort",
        "/bin/add",
        "/bin/fork",
        // "/bin/conway"
    ] {
        spawn_from_disk(path);
    }
}
