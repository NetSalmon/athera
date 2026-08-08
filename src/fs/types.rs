//! 文件系统通用类型：文件类型与 mode 位，与 POSIX 布局一致。
//!
//! 与具体文件系统无关，供内核各模块与具体文件系统（如 minix_fs）共用。

#![allow(unused)]

pub use crate::constants::S_IFMT;
use crate::{bits, numeric};

// 文件 mode 位，布局与 Unix `st_mode` 一致：
//
// ```text
// 15..12      11      10     9    8 7 6   5 4 3   2 1 0
// 文件类型  setuid setgid sticky  属主    属组    其他
//                                rwx     rwx     rwx
// ```
bits! {
    pub type Mode : u16 {
        // 文件类型位（bits 15..12），取值见下方 `FileType` 枚举。
        file_type: FileType : 12 => 15,

        // 特殊权限位。
        setuid: 11,
        setgid: 10,
        sticky: 9,

        // 属主（owner）rwx 权限位。
        user_execute: 6,
        user_write: 7,
        user_read: 8,
        user: 6 => 8,

        // 属组（group）rwx 权限位。
        group_execute: 3,
        group_write: 4,
        group_read: 5,
        group: 3 => 5,

        // 其他用户（other）rwx 权限位。
        other_execute: 0,
        other_write: 1,
        other_read: 2,
        other: 0 => 2,
    }
}

/// 文件类型（mode 高 4 位，bits 15..12 的取值），与 POSIX `S_IF*` 一致。
numeric! {
    pub enum FileType : u16 {
        FIFO = 0o1,   // 命名管道  S_IFIFO
        CHR = 0o2,    // 字符设备  S_IFCHR
        DIR = 0o4,    // 目录      S_IFDIR
        BLK = 0o6,    // 块设备    S_IFBLK
        REG = 0o10,   // 普通文件  S_IFREG
        LNK = 0o12,   // 符号链接  S_IFLNK
        SOCK = 0o14,  // 套接字    S_IFSOCK
    }
}
