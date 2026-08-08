//! 启动编排。
//!
//! 从 virtio-blk 上的 MINIX 文件系统按路径读取用户程序 ELF，并交给
//! `proc::exec::spawn_buffer` 加载执行。

use alloc::vec;

use crate::{
    dev::VIRTIO_BLK,
    error,
    fs::{Path, minix_fs::MinixFs},
    info,
    proc::exec::spawn_buffer,
};

/// 从 virtio-blk 上的 MINIX 文件系统按路径读取文件并加载为进程。
pub(crate) fn spawn_from_disk(path: &str) {
    let blk = {
        let guard = VIRTIO_BLK.force().lock();
        match guard.as_ref() {
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

    let Some(mut f) = fs.open(&Path::from_str(path)).unwrap() else {
        error!("{path} not found on disk");
        return;
    };

    let mut buf = vec![0u8; f.size() as usize];
    info!("{:#?}", f);

    if f.read(&mut buf).unwrap() != buf.len() {
        error!("short read, skip {path}");
        return;
    }

    if let Err(err) = spawn_buffer(&buf, None) {
        error!("failed to execute user program: {err}");
    }
}
