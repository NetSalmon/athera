use alloc::string::{String, ToString};
use core::{
    fmt::{Debug, Display, Formatter, Write},
    slice,
};

use crate::{bits, dev::abstracts::BlockDevice, numeric, vec, vec::Vec};

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
    pub fn zone_size(&self) -> usize {
        1024 << self.log_zone_size
    }

    #[inline]
    pub fn d_inode_start(&self) -> usize {
        (2 + self.imap_blk + self.zmap_blk) as usize * self.zone_size()
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

/// MINIX v1 的超级块位于磁盘第 1 块（偏移 1024 字节）。
const SUPERBLOCK_OFFSET: usize = 1024;

pub struct MinixFs {
    superblock: SuperBlock,
}

/// 打开的文件：保存 inode 信息与数据块号，支持按偏移顺序读取。
#[derive(Debug, Clone)]
pub struct File {
    /// inode 号。
    ino: u16,
    /// 磁盘 inode（含 size / mode / zone 等信息）。
    inode: DINode,
    /// 单个数据块的大小（字节）。
    zone_size: usize,
    /// 数据块号：7 个直接块 + 一级/二级间接块展开后的全部数据块。
    zones: Vec<u16>,
    /// 当前读写位置（字节偏移）。
    offset: usize,
}

impl File {
    /// inode 号。
    pub fn ino(&self) -> u16 {
        self.ino
    }

    /// 文件大小（字节）。
    pub fn size(&self) -> u32 {
        self.inode.size
    }

    /// 当前读写位置（字节偏移）。
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// 移动读写位置到 `pos`（超过文件末尾时钳制到末尾）。
    pub fn seek(&mut self, pos: usize) {
        self.offset = pos.min(self.size() as usize);
    }

    /// 从当前读写位置读取最多 `buf.len()` 字节，返回实际读取的字节数；
    /// 到达文件末尾时返回 0。读取会推进内部读写位置。
    pub fn read<T, E>(&mut self, buf: &mut [u8], device: &mut T) -> Result<usize, E>
    where
        T: BlockDevice<Error = E>,
    {
        let n = self.read_at(buf, self.offset, device)?;
        self.offset += n;
        Ok(n)
    }

    /// 从文件 `offset` 处读取最多 `buf.len()` 字节，返回实际读取的字节数；
    /// 到达文件末尾时返回 0。不会改变内部读写位置。
    pub fn read_at<T, E>(&self, buf: &mut [u8], offset: usize, device: &mut T) -> Result<usize, E>
    where
        T: BlockDevice<Error = E>,
    {
        let size = self.size() as usize;
        let mut pos = offset;
        let mut done = 0;

        while pos < size && done < buf.len() {
            let zone_index = pos / self.zone_size;
            let in_zone = pos % self.zone_size;

            // 本次最多读到：当前块末尾、文件末尾、调用方缓冲区末尾。
            let n = (self.zone_size - in_zone)
                .min(size - pos)
                .min(buf.len() - done);

            let Some(&zone) = self.zones.get(zone_index) else {
                break; // 数据块号缺失（磁盘空洞），按文件末尾处理
            };

            device.read_at(
                &mut buf[done..done + n],
                zone as usize * self.zone_size + in_zone,
            )?;

            pos += n;
            done += n;
        }

        Ok(done)
    }
}

#[derive(Debug)]
pub struct Path(pub String);

pub const SPILIT: &str = "/";

impl Path {
    pub fn new() -> Self {
        Path(String::new())
    }

    pub fn from_str(s: &str) -> Self {
        Path(String::from(s))
    }

    pub fn push(&mut self, s: &str) {
        match (self.0.ends_with(SPILIT), s.starts_with(SPILIT)) {
            (true, true) => self.0.push_str(s.strip_prefix(SPILIT).unwrap()),
            (false, false) => {
                self.0.push_str(SPILIT);
                self.0.push_str(s);
            }
            _ => self.0.push_str(s),
        }
    }
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

/// 目录项：目录中的一个文件或子目录。
#[derive(Debug, Clone)]
pub struct DirEntry {
    /// 该项对应的 inode 号。
    pub ino: u16,
    /// 文件名（不含结尾的 `\0`）。
    pub name: String,
}

/// 磁盘目录项的两种格式（决定文件名占用的字节数）。
#[derive(Clone, Copy)]
enum EntryFormat {
    /// 旧版 MINIX v1（`MAGIC`）：文件名 14 字节。
    V1_14,
    /// 新版 MINIX v1（`MAGIC_2`）：文件名 30 字节。
    V1_30,
}

/// 目录项的惰性迭代器：每次 [`Iterator::next`] 只解析一个目录项，
/// 数据块按需从设备读取，不会一次性把整个目录读进内存。
pub struct DirEntries<'a, T> {
    fs: &'a MinixFs,
    device: &'a mut T,
    /// 单个数据块的大小（字节）。
    zone_size: usize,
    /// 待读取的数据块号：7 个直接块 + 一级/二级间接块展开后的全部数据块。
    zones: Vec<u16>,
    /// 下一个要读取的数据块在 `zones` 中的下标。
    next_zone: usize,
    /// 目录数据剩余未解析的字节数（按 `d_inode.size` 限制，避免读到尺寸之外的脏数据）。
    remaining: usize,
    /// 当前数据块的内容（最多 `zone_size` 字节）。
    buffer: Vec<u8>,
    /// `buffer` 中下一个待解析槽位的字节偏移。
    offset: usize,
    /// 单个目录项的字节数（`2 + 文件名长度`）。
    entry_size: usize,
    /// 目录项格式。
    format: EntryFormat,
    /// 读取出错后置位，迭代随之终止。
    failed: bool,
}

impl<'a, T> DirEntries<'a, T> {
    /// 解析一个目录项槽位；空闲项（`ino == 0` 或空名）返回 `None`。
    fn parse_slot(&self, chunk: &[u8]) -> Option<DirEntry> {
        match self.format {
            EntryFormat::V1_14 => Self::parse_raw::<14>(chunk),
            EntryFormat::V1_30 => Self::parse_raw::<30>(chunk),
        }
    }

    /// 按 `N` 字节文件名解析一个槽位。
    fn parse_raw<const N: usize>(chunk: &[u8]) -> Option<DirEntry> {
        // SAFETY: `chunk` 长度由 `entry_size` 保证等于 `DirEntryRaw<N>` 的大小；
        // `read_unaligned` 不要求对齐，可安全读取 u8 切片中的结构体。
        let entry = unsafe { (chunk.as_ptr() as *const DirEntryRaw<N>).read_unaligned() };

        let name = entry.name.to_string();
        (entry.ino != 0 && !name.is_empty()).then_some(DirEntry {
            ino: entry.ino,
            name,
        })
    }
}

impl<'a, T, E> Iterator for DirEntries<'a, T>
where
    T: BlockDevice<Error = E>,
{
    type Item = Result<DirEntry, E>;

    fn next(&mut self) -> Option<Self::Item> {
        // 出错后不再产出任何项。
        if self.failed {
            return None;
        }

        loop {
            // 当前缓冲里不足一个完整的槽位 → 读取下一个数据块。
            if self.offset + self.entry_size > self.buffer.len() {
                if self.remaining == 0 {
                    return None;
                }

                let zone = *self.zones.get(self.next_zone)?;
                self.next_zone += 1;

                // 目录最后可能不满一个 zone，只读 size 范围内剩余的字节。
                let chunk_len = self.zone_size.min(self.remaining);
                self.buffer.resize(chunk_len, 0);
                if let Err(err) = self.fs.read_zone_into(self.device, zone, &mut self.buffer) {
                    self.failed = true;
                    return Some(Err(err));
                }
                self.remaining -= chunk_len;
                self.offset = 0;
            }

            // 解析当前槽位；空闲项（`ino == 0` 或空名）直接跳过。
            let chunk = &self.buffer[self.offset..self.offset + self.entry_size];
            self.offset += self.entry_size;

            if let Some(entry) = self.parse_slot(chunk) {
                return Some(Ok(entry));
            }
        }
    }
}

impl MinixFs {
    /// 尝试把设备当作 MINIX v1 文件系统打开。
    ///
    /// 从第 1 块（偏移 [`SUPERBLOCK_OFFSET`]）读取超级块并校验 magic：
    /// 设备读取失败返回 `Err`；magic 不匹配（设备上不是 MINIX 文件系统）
    /// 返回 `Ok(None)`；成功则返回 `Ok(Some(Self))`。
    pub fn from_device<T, E>(device: &mut T) -> Result<Option<Self>, E>
    where
        T: BlockDevice<Error = E>,
    {
        let mut buffer = vec![0u8; size_of::<SuperBlock>()];
        device.read_at(&mut buffer, SUPERBLOCK_OFFSET)?;

        // SAFETY: `read_unaligned` 不要求对齐，可安全读取 u8 缓冲中的结构体。
        let superblock = unsafe { (buffer.as_ptr() as *const SuperBlock).read_unaligned() };

        if superblock.magic == MinixFsMagic::MAGIC || superblock.magic == MinixFsMagic::MAGIC_2 {
            Ok(Some(Self { superblock }))
        } else {
            Ok(None)
        }
    }

    pub fn d_inode<T, E>(&self, ino: u16, device: &mut T) -> Result<DINode, E>
    where
        T: BlockDevice<Error = E>,
    {
        let offset = (ino - 1) as usize * size_of::<DINode>();

        let mut buffer = vec![0u8; size_of::<DINode>()];

        device.read_at(&mut buffer, self.superblock.d_inode_start() + offset)?;

        let d_inode: DINode = unsafe { (buffer.as_ptr() as *const DINode).read_unaligned() };

        Ok(d_inode)
    }

    /// 读取 inode 指向的全部数据块，返回文件内容（末尾按 `d_inode.size` 截断）。
    pub fn data<T, E>(&self, d_inode: &DINode, device: &mut T) -> Result<Vec<u8>, E>
    where
        T: BlockDevice<Error = E>,
    {
        if d_inode.size == 0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::with_capacity(d_inode.size as usize);
        for zone in self.data_zones(d_inode, device)? {
            self.read_zone(device, zone, &mut out)?;
        }

        out.truncate(d_inode.size as usize);
        Ok(out)
    }

    /// 依次收集 inode 引用的数据块号：7 个直接块 → 一级间接块 → 二级间接块。
    /// 块号为 0 表示“没有更多块”，遇到即停止。
    fn data_zones<T, E>(&self, d_inode: &DINode, device: &mut T) -> Result<Vec<u16>, E>
    where
        T: BlockDevice<Error = E>,
    {
        let mut zones = Vec::new();

        // zone[0..7]：7 个直接数据块。
        for &zone in &d_inode.zone[..7] {
            if zone == 0 {
                return Ok(zones);
            }
            zones.push(zone);
        }

        // zone[7]：一级间接块，里面存放数据块号。
        for zone in self.zone_table(device, d_inode.zone[7])? {
            zones.push(zone);
        }

        // zone[8]：二级间接块，里面存放“一级间接块”的块号。
        for indirect_zone in self.zone_table(device, d_inode.zone[8])? {
            for zone in self.zone_table(device, indirect_zone)? {
                zones.push(zone);
            }
        }

        Ok(zones)
    }

    /// 读取 `zone` 指向的一个数据块，追加到 `out` 末尾。
    fn read_zone<T, E>(&self, device: &mut T, zone: u16, out: &mut Vec<u8>) -> Result<(), E>
    where
        T: BlockDevice<Error = E>,
    {
        let mut buffer = vec![0u8; self.superblock.zone_size()];
        self.read_zone_into(device, zone, &mut buffer)?;
        out.extend_from_slice(&buffer);
        Ok(())
    }

    /// 把 `zone` 指向的数据块开头读入 `out`（`out` 长度不能超过 zone 大小）。
    fn read_zone_into<T, E>(&self, device: &mut T, zone: u16, out: &mut [u8]) -> Result<(), E>
    where
        T: BlockDevice<Error = E>,
    {
        let zone_size = self.superblock.zone_size();
        debug_assert!(out.len() <= zone_size);
        device.read_at(out, zone as usize * zone_size)?;
        Ok(())
    }

    /// 读取一个“块号表”：把 `zone` 指向的块按 `u16` 数组解释并返回其中的块号。
    /// `zone == 0` 表示该级间接块不存在，返回空表；表内遇到 0 提前结束。
    fn zone_table<T, E>(&self, device: &mut T, zone: u16) -> Result<Vec<u16>, E>
    where
        T: BlockDevice<Error = E>,
    {
        if zone == 0 {
            return Ok(Vec::new());
        }

        let zone_size = self.superblock.zone_size();
        let mut buffer = vec![0u8; zone_size];
        device.read_at(&mut buffer, zone as usize * zone_size)?;

        // SAFETY: 分配器返回的内存满足 u16 对齐；`zone_size` 是 2 的幂，可被 u16 大小整除。
        let table = unsafe {
            slice::from_raw_parts(buffer.as_ptr() as *const u16, zone_size / size_of::<u16>())
        };

        Ok(table.iter().copied().take_while(|&z| z != 0).collect())
    }

    /// 读取目录内容，返回目录项列表（一次性读完全部数据）。
    ///
    /// 需要按需逐项读取、避免一次读完整目录时，请使用 [`Self::dir_entries_iter`]。
    pub fn dir_entries<T, E>(&self, d_inode: &DINode, device: &mut T) -> Result<Vec<DirEntry>, E>
    where
        T: BlockDevice<Error = E>,
    {
        self.dir_entries_iter(d_inode, device)?.collect()
    }

    /// 创建按需读取的目录项迭代器：每次 [`Iterator::next`] 只解析一个目录项，
    /// 数据块按需从设备读取，不会一次性把整个目录读进内存。
    pub fn dir_entries_iter<'a, T, E>(
        &'a self,
        d_inode: &DINode,
        device: &'a mut T,
    ) -> Result<DirEntries<'a, T>, E>
    where
        T: BlockDevice<Error = E>,
    {
        let zones = self.data_zones(d_inode, device)?;

        // 根据 superblock 的 magic 决定文件名长度：
        // MAGIC_2 → 30 字节，其余（MAGIC）→ 14 字节。
        let (entry_size, format) = match self.superblock.magic {
            MinixFsMagic::MAGIC_2 => (size_of::<DirEntryRaw<30>>(), EntryFormat::V1_30),
            _ => (size_of::<DirEntryRaw<14>>(), EntryFormat::V1_14),
        };

        Ok(DirEntries {
            fs: self,
            device,
            zone_size: self.superblock.zone_size(),
            zones,
            next_zone: 0,
            remaining: d_inode.size as usize,
            buffer: Vec::new(),
            offset: 0,
            entry_size,
            format,
            failed: false,
        })
    }

    /// 按路径打开文件：从根目录（inode 1）逐级查找目录项并读取目标 inode。
    ///
    /// 路径中某个分量不存在时返回 `Ok(None)`，设备出错时返回 `Err`。
    pub fn open<T, E>(&self, path: &Path, device: &mut T) -> Result<Option<File>, E>
    where
        T: BlockDevice<Error = E>,
    {
        let mut ino: u16 = 1; // MINIX v1 的根目录固定为 inode 1
        let mut inode = self.d_inode(ino, device)?;

        for name in path.0.split(SPILIT) {
            if name.is_empty() {
                continue; // 跳过空段（前导 / 连续 / 末尾斜杠）
            }

            // 在当前目录里查找名为 `name` 的目录项。
            let mut next_ino = None;
            for entry in self.dir_entries_iter(&inode, device)? {
                let entry = entry?;
                if entry.name == name {
                    next_ino = Some(entry.ino);
                    break;
                }
            }

            let Some(next_ino) = next_ino else {
                return Ok(None); // 路径分量不存在
            };

            ino = next_ino;
            inode = self.d_inode(ino, device)?;
        }

        let zones = self.data_zones(&inode, device)?;
        Ok(Some(File {
            ino,
            inode,
            zone_size: self.superblock.zone_size(),
            zones,
            offset: 0,
        }))
    }
}
