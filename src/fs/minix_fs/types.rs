#![allow(unused)]
//! MINIX V1 磁盘结构：超级块、磁盘 inode、目录项与魔数。
//!
//! 磁盘布局与 Linux `minix` 文件系统 v1 一致：超级块位于偏移 1024，
//! 文件名长度由魔数 `0x137F`（14 字节）/ `0x138F`（30 字节）决定。
//! 文件类型与 mode 位见 [`crate::fs::types`]。

use core::fmt::{Debug, Display, Formatter, Write};

use super::super::types::{FileType, Mode, S_IFMT};
use crate::numeric;

#[repr(transparent)]
#[derive(Clone)]
pub struct MinixString<const T: usize>(pub [u8; T]);

impl<const T: usize> Debug for MinixString<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::result::Result<(), core::fmt::Error> {
        f.write_char('"')?;
        for i in self.0 {
            match i {
                0 => break,
                i if i.is_ascii() => f.write_char(i as char)?,
                _ => continue,
            }
        }
        f.write_char('"')
    }
}

impl<const T: usize> Display for MinixString<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        for i in self.0 {
            match i {
                0 => break,
                i if i.is_ascii() => f.write_char(i as char)?,
                _ => continue,
            }
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct DINode {
    pub mode: Mode, // 文件类型和 RWX 访问控制位
    pub uid: u16,   // 文件属主的用户 ID
    pub size: u32,  // 文件大小, 以 byte 计数
    pub mtime: u32, // 自从 1970.1.1 以来的秒数
    pub gid: u8,    // 文件属主 所属的组
    pub nlinks: u8, // 该节点被多少个目录所链接

    /*
     * zone[0] - zone[6] 分别指向 7 个直接块
     * zone[7] 指向间接块
     * zone[8] 指向双重间接块
     */
    pub zone: [u16; 9],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct DirEntryRaw<const T: usize> {
    pub ino: u16,
    pub name: MinixString<T>,
}

pub type DirEntryV1_14 = DirEntryRaw<14>;
pub type DirEntryV1_30 = DirEntryRaw<30>;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct SuperBlock {
    pub ninodes: u16,         // number of inodes
    pub nzones: u16,          // number of zones
    pub imap_blocks: u16,     // i 节点位图 占用块的数目
    pub zmap_blocks: u16,     // 数据块位图 占用的块的数目
    pub first_data_zone: u16, // 第一个 数据块 的块号
    pub log_zone_size: u16,   // 一个虚拟块的大小 = 1024 << log_zone_size

    pub max_size: u32,       // 能存放的最大文件大小(以 byte 计数)
    pub magic: MinixFsMagic, // magic number
    pub state: u16,
}

impl SuperBlock {
    #[inline]
    pub fn zone_size(&self) -> usize {
        1024 << self.log_zone_size
    }

    #[inline]
    pub fn d_inode_start(&self) -> usize {
        (2 + self.imap_blocks + self.zmap_blocks) as usize * self.zone_size()
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct INode {
    pub dev: u16,   // i 节点所在的磁盘
    pub ino: u16,   // i 节点号码
    pub r#ref: u16, // 内存引用计数
    pub flags: u16,
    pub atime: u16,
    pub ctime: u16,
    pub mode: Mode,
    pub uid: u16,
    pub size: u32,
    pub mtime: u32,
    pub gid: u8,
    pub nlinks: u8,
    pub zone: [u16; 9],
}

numeric! {
    pub enum MinixFsMagic : u16 {
        MAGIC = 0x137F,     // MINIX_SUPER_MAGIC, NAME_LEN 14
        MAGIC_2 = 0x138F,   // MINIX_SUPER_MAGIC2, NAME_LEN 30
    }
}
