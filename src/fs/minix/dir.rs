//! 目录项与按需读取的目录迭代器 [`DirEntries`]。

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use super::{MinixFs, types::DirEntryRaw};
use crate::driver::traits::{BlockDevice, IoResult};

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
pub(crate) enum EntryFormat {
    /// 旧版 MINIX v1（`MAGIC`）：文件名 14 字节。
    V1_14,
    /// 新版 MINIX v1（`MAGIC_2`）：文件名 30 字节。
    V1_30,
}

/// 目录项的惰性迭代器：每次 [`Iterator::next`] 只解析一个目录项，
/// 数据块按需从设备读取，不会一次性把整个目录读进内存。
///
/// 设备由调用方以 `&'d T`（通常是一次持读锁得到的守卫）传入，迭代期间
/// 需要一直持有该设备访问权。
pub struct DirEntries<'a, 'd, T> {
    fs: &'a MinixFs<T>,
    device: &'d T,
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

impl<'a, 'd, T> DirEntries<'a, 'd, T> {
    /// 构造目录迭代器（由 `MinixFs::dir_entries_iter` 使用）。
    pub(crate) fn new(
        fs: &'a MinixFs<T>,
        device: &'d T,
        zone_size: usize,
        zones: Vec<u16>,
        remaining: usize,
        entry_size: usize,
        format: EntryFormat,
    ) -> Self {
        DirEntries {
            fs,
            device,
            zone_size,
            zones,
            next_zone: 0,
            remaining,
            buffer: Vec::new(),
            offset: 0,
            entry_size,
            format,
            failed: false,
        }
    }

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

impl<'a, 'd, T> Iterator for DirEntries<'a, 'd, T>
where
    T: BlockDevice,
{
    type Item = IoResult<DirEntry>;

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
