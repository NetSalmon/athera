#![allow(dead_code)]
//! 文件系统常量：路径分隔符、POSIX `st_mode` 位与 MINIX 磁盘布局。

/// 路径分隔符。
pub const PATH_SEPARATOR: &str = "/";

/// 文件类型掩码：mode 高 4 位（bits 15..12），对应 POSIX `S_IFMT`。
pub const S_IFMT: u16 = 0o170000;

/// MINIX v1 超级块位于磁盘第 1 块（偏移 1024 字节）。
pub const SUPERBLOCK_OFFSET: usize = 1024;

/// 解析路径时允许的最大符号链接跳数（与 Linux `MAXSYMLINKS` 一致）。
pub const MAX_SYMLINK_HOPS: usize = 40;

/// MINIX v1 inode 的直接块（zone）数量；`zone[0..7]` 为直接块，
/// `zone[7]` / `zone[8]` 依次为一级 / 二级间接块。
pub const MINIX_DIRECT_ZONES: usize = 7;
