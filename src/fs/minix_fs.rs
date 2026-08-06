use core::fmt::{Debug, Display, Formatter, Write};

use crate::{bits, numeric};
// struct d_inode{
//     uint16_t mode;  // 文件类型和 RWX 访问控制位
//     uint16_t uid;   // 文件属主的用户 ID
//     uint32_t size;  // 文件大小, 以 byte 计数
//     uint32_t mtime; // 自从 1970.1.1 以来的秒数     (unused)
//     uint8_t gid;    // 文件属主 所属的组
//     uint8_t nlinks; // 该节点被多少个目录所链接
//
//     /*
//      * zone[0] - zone[6] 分别指向 7 个直接块
//      * zone[7] 指向间接块
//      * zone[8] 指向双重间接块
//      */
//     uint16_t zone[9];
// };

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
pub struct DirEntry<const T: usize> {
    pub ino: u16,
    pub name: MinixString<T>,
}

pub type DirEntryV1_14 = DirEntry<14>;
pub type DirEntryV1_30 = DirEntry<30>;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct SuperBlock {
    pub ninodes: u16,       // number of inodes
    pub nzones: u16,        // number of zones
    pub imap_blk: u16,      // i 节点位图 占用块的数目
    pub zmap_blk: u16,      // 数据块位图 占用的块的数目
    pub fst_data_zone: u16, // 第一个 数据块 的块号
    pub log_zone_size: u16, // 一个虚拟块的大小 = 1024 << log_zone_size

    pub max_size: u32,       // 能存放的最大文件大小(以 byte 计数)
    pub magic: MinixFsMagic, // magic number
    pub state: u16,
}

impl SuperBlock {
    #[inline]
    pub fn block_size(&self) -> usize {
        1024 << self.log_zone_size
    }

    #[inline]
    pub fn d_inode_start(&self) -> usize {
        (2 + self.imap_blk + self.zmap_blk) as usize * self.block_size()
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

/// 文件类型掩码：mode 高 4 位（bits 15..12），对应 POSIX `S_IFMT`。
pub const S_IFMT: u16 = 0o170000;

// 文件类型（mode 高 4 位，bits 15..12 的取值），与 POSIX `S_IF*` 一致。
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

numeric! {
    pub enum MinixFsMagic : u16 {
        MAGIC = 0x137F,     // MINIX_SUPER_MAGIC, NAME_LEN 14
        MAGIC_2 = 0x138F,   // MINIX_SUPER_MAGIC2, NAME_LEN 30
    }
}
