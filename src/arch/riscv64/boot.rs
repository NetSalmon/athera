//! 启动编排。
//!
//! 从 virtio-blk 上的 MINIX 文件系统按路径读取用户程序 ELF，并交给
//! `task::exec::spawn_buffer` 加载执行。

use alloc::vec;

use crate::{
    error,
    fs::{
        Path, VFS,
        vfs::{FileSystem, OpenFlags},
    },
    task::exec::spawn_buffer,
};

/// 从 virtio-blk 上的 MINIX 文件系统按路径读取文件并加载为进程。
pub(crate) fn spawn_from_disk(path: &str) {
    let f = match VFS.force().open(
        &Path::from(path),
        OpenFlags::read_only(),
        crate::fs::Mode::from(0),
    ) {
        Ok(file) => file,
        Err(err) => {
            error!("failed to open {path}: {err}");
            return;
        }
    };

    let Ok(size) = usize::try_from(match VFS.force().stat(&Path::from(path)) {
        Ok(stat) => stat.size,
        Err(err) => {
            error!("failed to stat {path}: {err}");
            return;
        }
    }) else {
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

pub(crate) fn spawn_init() {
    for path in ["/sbin/init", "/etc/init", "/bin/init", "/bin/sh"] {
        let f = match VFS.force().open(
            &Path::from(path),
            OpenFlags::read_only(),
            crate::fs::Mode::from(0),
        ) {
            Ok(file) => file,
            Err(err) => {
                error!("failed to open {path}: {err}");
                continue;
            }
        };

        let Ok(size) = usize::try_from(match VFS.force().stat(&Path::from(path)) {
            Ok(stat) => stat.size,
            Err(err) => {
                error!("failed to stat {path}: {err}");
                continue;
            }
        }) else {
            error!("{path} is too large to load");
            continue;
        };
        let mut buf = vec![0u8; size];

        let read = match f.read(&mut buf) {
            Ok(read) => read,
            Err(err) => {
                error!("failed to read {path}: {err}");
                continue;
            }
        };
        if read != buf.len() {
            error!("short read for {path}: expected {}, got {read}", buf.len());
            continue;
        }

        if let Err(err) = spawn_buffer(&buf, None) {
            error!("failed to execute user program: {err}");
            continue;
        }

        return;
    }

    panic!("failed to start init");
}
