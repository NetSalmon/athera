//! 目录读取与路径解析（含符号链接解引用）。

use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec::Vec,
};

use super::{
    File, FileType, MinixFs,
    dir::{DirEntries, DirEntry, EntryFormat},
    types::{DINode, DirEntryRaw, MinixFsMagic},
};
use crate::constants::{MAX_SYMLINK_HOPS, PATH_SEPARATOR};
use crate::dev::traits::BlockDevice;
use crate::fs::path::Path;

impl<T> MinixFs<T> {
    /// 读取目录内容，返回目录项列表（一次性读完全部数据）。
    ///
    /// 需要按需逐项读取、避免一次读完整目录时，请使用 [`Self::dir_entries_iter`]。
    /// `device` 为一次持锁得到的块设备守卫（见 [`MinixFs::lock`]）。
    pub fn dir_entries<E>(&self, d_inode: &DINode, device: &mut T) -> Result<Vec<DirEntry>, E>
    where
        T: BlockDevice<Error = E>,
    {
        self.dir_entries_iter(d_inode, device)?.collect()
    }

    /// 创建按需读取的目录项迭代器：每次 [`Iterator::next`] 只解析一个目录项，
    /// 数据块按需从设备读取，不会一次性把整个目录读进内存。
    pub fn dir_entries_iter<'a, 'd, E>(
        &'a self,
        d_inode: &DINode,
        device: &'d mut T,
    ) -> Result<DirEntries<'a, 'd, T>, E>
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

        Ok(DirEntries::new(
            self,
            device,
            self.superblock.zone_size(),
            zones,
            d_inode.size as usize,
            entry_size,
            format,
        ))
    }

    /// 按路径打开文件：从根目录（inode 1）逐级查找目录项并读取目标 inode。
    ///
    /// 路径中的任意分量（含最后一个）若是符号链接，会读取其目标路径并继续
    /// 解析：绝对目标从根目录重新开始，相对目标以当前目录为基准。链接跳数
    /// 超过 [`MAX_SYMLINK_HOPS`] 视为循环；路径分量不存在、或中间分量不是
    /// 目录时，返回 `Ok(None)`。设备出错时返回 `Err`。
    ///
    /// 底层设备在方法内部临时持锁，返回的 [`File`] 不占用设备引用。
    pub fn open<'a, E>(&'a self, path: &Path) -> Result<Option<File<'a, T>>, E>
    where
        T: BlockDevice<Error = E>,
    {
        let mut dev = self.lock();
        self.resolve_path(path, &mut dev, 0)
    }

    /// `open` 的公共实现：用待解析分量的队列逐级查找，遇到符号链接时把
    /// 目标路径的分量插回队首继续，直到消费完所有分量。
    fn resolve_path<'a, E>(
        &'a self,
        path: &Path,
        device: &mut T,
        mut hops: usize,
    ) -> Result<Option<File<'a, T>>, E>
    where
        T: BlockDevice<Error = E>,
    {
        // 待解析分量队列（原路径的分量，以及符号链接目标的路径分量）。
        let mut components: VecDeque<String> = path
            .as_str()
            .split(PATH_SEPARATOR)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        // 当前目录（在其中查找下一个分量）；相对链接目标以此为基准，
        // 绝对链接目标会重置为根目录。
        let mut dir_ino: u16 = 1;
        let mut dir = self.d_inode(dir_ino, device)?;

        while let Some(name) = components.pop_front() {
            // 在当前目录里查找名为 `name` 的目录项。
            let mut next_ino = None;
            for entry in self.dir_entries_iter(&dir, device)? {
                let entry = entry?;
                if entry.name == name {
                    next_ino = Some(entry.ino);
                    break;
                }
            }
            let Some(next_ino) = next_ino else {
                return Ok(None); // 路径分量不存在
            };

            let next = self.d_inode(next_ino, device)?;

            // 符号链接：读取目标路径，把它的分量插回队首继续解析。
            if next.mode.file_type() == FileType::LNK {
                hops += 1;
                if hops > MAX_SYMLINK_HOPS {
                    return Ok(None); // 链接循环
                }

                let target = self.data(&next, device)?;
                let target = String::from_utf8_lossy(&target).into_owned();
                if target.starts_with(PATH_SEPARATOR) {
                    // 绝对目标：从根目录重新开始。
                    dir_ino = 1;
                    dir = self.d_inode(1, device)?;
                }
                for part in target.split('/').filter(|s| !s.is_empty()).rev() {
                    components.push_front(part.to_string());
                }
                continue;
            }

            // 若后面还有分量，则本分量必须是目录。
            if !components.is_empty() && next.mode.file_type() != FileType::DIR {
                return Ok(None); // 中间分量不是目录
            }

            dir_ino = next_ino;
            dir = next;
        }

        let zones = self.data_zones(&dir, device)?;
        Ok(Some(File::new(
            self,
            path.to_path_buf(),
            dir_ino,
            dir,
            self.superblock.zone_size(),
            zones,
        )))
    }
}
