//! 写路径：创建 / 删除文件与目录、硬链接、符号链接，以及数据块与
//! 间接块表的同步。

use alloc::{vec, vec::Vec};

use super::{
    DINode, File, FileType, MinixFs, Mode,
    types::{DirEntryRaw, MinixFsMagic},
};
use crate::{
    constants::{MINIX_DIRECT_ZONES, PATH_SEPARATOR},
    dev::traits::{BlockDevice, IoResult},
    fs::path::PathBuf,
};

impl<T> MinixFs<T> {
    /// 把展开后的数据块号列表 `zones` 写回 inode 的 zone 数组与间接块表。
    ///
    /// `zones[0..7]` 对应直接块，`zones[7..7+per]` 走一级间接块，
    /// 其余走二级间接块；必要时会分配间接块并更新 `d_inode.zone[7]` /
    /// `zone[8]`。磁盘空间不足时提前返回（已分配的块不会回收）。
    ///
    /// 需要调用方已持锁（`device` 为块设备守卫）。
    pub(crate) fn write_file_zones(
        &self,
        d_inode: &mut DINode,
        zones: &[u16],
        device: &T,
    ) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        let per = zone_size / size_of::<u16>(); // 每个间接块可容纳的块号数

        // 直接块：zone[0..7]。
        for (i, slot) in d_inode.zone[..MINIX_DIRECT_ZONES].iter_mut().enumerate() {
            *slot = zones.get(i).copied().unwrap_or(0);
        }

        // 一级间接：zone[7]。
        let single_start = MINIX_DIRECT_ZONES;
        let single_end = MINIX_DIRECT_ZONES + per;
        if zones.len() <= single_start {
            d_inode.zone[MINIX_DIRECT_ZONES] = 0;
        } else {
            let mut table = zones[single_start..zones.len().min(single_end)].to_vec();
            table.resize(per, 0);
            if d_inode.zone[MINIX_DIRECT_ZONES] == 0 {
                let Some(ind) = self.alloc_zone(device)? else {
                    return Ok(()); // 磁盘已满，无法建立间接块
                };
                d_inode.zone[MINIX_DIRECT_ZONES] = ind;
            }
            self.write_zone_table(device, d_inode.zone[MINIX_DIRECT_ZONES], &table)?;
        }

        // 二级间接：zone[8]。
        let double_start = single_end;
        if zones.len() <= double_start {
            d_inode.zone[MINIX_DIRECT_ZONES + 1] = 0;
        } else {
            if d_inode.zone[MINIX_DIRECT_ZONES + 1] == 0 {
                let Some(ind) = self.alloc_zone(device)? else {
                    return Ok(());
                };
                d_inode.zone[MINIX_DIRECT_ZONES + 1] = ind;
                self.write_zone_table(device, ind, &vec![0u16; per])?;
            }
            let mut dbl = self.read_zone_table(device, d_inode.zone[MINIX_DIRECT_ZONES + 1])?;
            for (i, group) in zones[double_start..].chunks(per).enumerate() {
                if i >= per {
                    break; // 超出二级间接容量
                }
                let ind = if dbl[i] != 0 {
                    dbl[i]
                } else {
                    let Some(ind) = self.alloc_zone(device)? else {
                        return Ok(());
                    };
                    dbl[i] = ind;
                    ind
                };
                let mut table = group.to_vec();
                table.resize(per, 0);
                self.write_zone_table(device, ind, &table)?;
            }
            self.write_zone_table(device, d_inode.zone[MINIX_DIRECT_ZONES + 1], &dbl)?;
        }

        Ok(())
    }

    /// 在目录 `dir_ino` 下创建名为 `name` 的普通文件，返回新文件 inode 号。
    ///
    /// 依次分配 inode（[`alloc_inode`](Self::alloc_inode)）、写回空 inode、
    /// 在目录中追加目录项；inode 用尽或名字过长返回 `Ok(None)`，目录写入
    /// 失败时回收已分配的 inode。底层设备在方法内部临时持锁。
    pub fn create_file(&self, dir_ino: u16, name: &str) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) {
            return Ok(None);
        }

        let mode = Mode::from((u16::from(FileType::REG)) << 12 | 0o644);
        let mut dev = self.lock();
        let Some(ino) = self.alloc_empty_inode(mode, &mut dev)? else {
            return Ok(None);
        };

        if let Err(err) = self.add_dir_entry_at(dir_ino, name, ino, &mut dev) {
            let _ = self.free_inode(ino, &mut dev);
            return Err(err);
        }
        Ok(Some(ino))
    }

    /// 创建一条指向 `old_ino` 的硬链接（目录项），目标 inode 的链接数 +1。
    ///
    /// 名字不合法、inode 无效/未分配/是目录、链接数已达上限（`u8::MAX`）
    /// 时返回 `Ok(false)`；目录写入失败返回 `Err`。底层设备在方法内部临时
    /// 持锁。
    pub fn link(&self, old_ino: u16, dir_ino: u16, name: &str) -> IoResult<bool>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) || old_ino == 0 || old_ino > self.superblock.ninodes {
            return Ok(false);
        }

        let mut dev = self.lock();
        let d_inode = self.d_inode(old_ino, &mut dev)?;
        if d_inode.nlinks == 0 || d_inode.nlinks == u8::MAX {
            return Ok(false); // inode 未分配，或链接数已达上限
        }
        if d_inode.mode.file_type() == FileType::DIR {
            return Ok(false); // 目录不能硬链接
        }

        self.add_dir_entry_at(dir_ino, name, old_ino, &mut dev)?;

        let mut updated = d_inode;
        updated.nlinks += 1;
        self.write_d_inode(old_ino, &updated, &mut dev)?;
        Ok(true)
    }

    /// 删除目录 `dir_ino` 中名为 `name` 的目录项并递减目标 inode 的链接数；
    /// 链接数归零时释放全部数据块与 inode。
    ///
    /// 名字不合法或目录项不存在返回 `Ok(false)`；目录类型的目标拒绝删除。
    /// 底层设备在方法内部临时持锁。
    pub fn unlink(&self, dir_ino: u16, name: &str) -> IoResult<bool>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) {
            return Ok(false);
        }

        let mut dev = self.lock();

        // 先只读定位目标 inode 并检查类型，避免“删了目录项再回滚”。
        let Some(ino) = self.find_dir_entry_ino(dir_ino, name, &mut dev)? else {
            return Ok(false);
        };
        let d_inode = self.d_inode(ino, &mut dev)?;
        if d_inode.mode.file_type() == FileType::DIR {
            return Ok(false);
        }

        if !self.remove_dir_entry(dir_ino, name, &mut dev)? {
            return Ok(false);
        }

        let nlinks = d_inode.nlinks.saturating_sub(1);
        if nlinks == 0 {
            self.free_inode_blocks_at(ino, &mut dev)?;
        } else {
            let mut updated = d_inode;
            updated.nlinks = nlinks;
            self.write_d_inode(ino, &updated, &mut dev)?;
        }
        Ok(true)
    }

    /// 在目录 `dir_ino` 下创建符号链接 `name`，内容为 `target` 路径
    /// （按 MINIX v1 惯例存放在 inode 的数据块中）。返回新 inode 号；
    /// 名字/目标不合法或磁盘空间不足返回 `Ok(None)`。
    pub fn symlink(&self, dir_ino: u16, name: &str, target: &str) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) || target.is_empty() {
            return Ok(None);
        }

        let mode = Mode::from((u16::from(FileType::LNK)) << 12 | 0o777);

        // Phase 1：分配 inode 并读取其磁盘 inode（临时持锁）。
        let (ino, inode) = {
            let mut dev = self.lock();
            let Some(ino) = self.alloc_empty_inode(mode, &mut dev)? else {
                return Ok(None);
            };
            (ino, self.d_inode(ino, &mut dev)?)
        };

        // Phase 2：把目标路径作为符号链接内容写入（`File` 内部自行持锁）。
        let mut file = File::new(
            self,
            PathBuf::new(),
            ino,
            inode,
            self.superblock.zone_size(),
            Vec::new(),
        );
        let done = match file.write_at(target.as_bytes(), 0) {
            Ok(done) => done,
            Err(err) => {
                let _ = self.free_inode_blocks(ino);
                return Err(err);
            }
        };
        if done != target.len() {
            let _ = self.free_inode_blocks(ino); // 磁盘空间不足，回滚
            return Ok(None);
        }

        // Phase 3：在父目录追加目录项；失败时回滚。
        if let Err(err) = self.add_dir_entry(dir_ino, name, ino) {
            let _ = self.free_inode_blocks(ino);
            return Err(err);
        }
        Ok(Some(ino))
    }

    /// 名字是否合法：非空、长度不超限、不含路径分隔符。
    fn valid_name(&self, name: &str) -> bool {
        let max_name = match self.superblock.magic {
            MinixFsMagic::MAGIC_2 => 30,
            _ => 14,
        };
        !name.is_empty() && name.len() <= max_name && !name.contains(PATH_SEPARATOR)
    }

    /// 单个目录项的字节数（2 + 文件名长度）。
    fn entry_size(&self) -> usize {
        match self.superblock.magic {
            MinixFsMagic::MAGIC_2 => size_of::<DirEntryRaw<30>>(),
            _ => size_of::<DirEntryRaw<14>>(),
        }
    }

    /// 分配一个空 inode（size 0、nlinks 1、无数据块）并写回，返回 inode 号。
    fn alloc_empty_inode(&self, mode: Mode, device: &T) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        let Some(ino) = self.alloc_inode(device)? else {
            return Ok(None);
        };
        let d_inode = DINode {
            mode,
            uid: 0,
            size: 0,
            mtime: 0,
            gid: 0,
            nlinks: 1,
            zone: [0; 9],
        };
        self.write_d_inode(ino, &d_inode, device)?;
        Ok(Some(ino))
    }

    /// 在目录 `dir_ino` 中只读查找名为 `name` 的目录项，返回其 inode 号。
    fn find_dir_entry_ino(&self, dir_ino: u16, name: &str, device: &T) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        let entry_size = self.entry_size();
        let zone_size = self.superblock.zone_size();
        let dir = self.d_inode(dir_ino, device)?;

        for zone in self.data_zones(&dir, device)? {
            let mut block = vec![0u8; zone_size];
            self.read_zone_into(device, zone, &mut block)?;
            for slot in (0..zone_size).step_by(entry_size) {
                let ino = u16::from_le_bytes([block[slot], block[slot + 1]]);
                if ino == 0 {
                    continue;
                }
                if Self::slot_name(&block[slot + 2..slot + entry_size]) == name {
                    return Ok(Some(ino));
                }
            }
        }
        Ok(None)
    }

    /// 删除目录 `dir_ino` 中名为 `name` 的目录项（槽位清零并写回）。
    fn remove_dir_entry(&self, dir_ino: u16, name: &str, device: &T) -> IoResult<bool>
    where
        T: BlockDevice,
    {
        let entry_size = self.entry_size();
        let zone_size = self.superblock.zone_size();
        let dir = self.d_inode(dir_ino, device)?;

        for zone in self.data_zones(&dir, device)? {
            let mut block = vec![0u8; zone_size];
            self.read_zone_into(device, zone, &mut block)?;
            for slot in (0..zone_size).step_by(entry_size) {
                let ino = u16::from_le_bytes([block[slot], block[slot + 1]]);
                if ino == 0 {
                    continue;
                }
                if Self::slot_name(&block[slot + 2..slot + entry_size]) == name {
                    block[slot..slot + entry_size].fill(0);
                    self.write_zone(zone, &block, device)?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// 释放 inode 占用的全部数据块与间接块，清零并释放 inode 位图。
    /// 需要调用方已持锁。
    fn free_inode_blocks_at(&self, ino: u16, device: &T) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let d_inode = self.d_inode(ino, device)?;

        for zone in self.data_zones(&d_inode, device)? {
            self.free_zone(zone, device)?;
        }
        if d_inode.zone[7] != 0 {
            self.free_zone(d_inode.zone[7], device)?; // 一级间接块
        }
        if d_inode.zone[8] != 0 {
            for ind in self.zone_table(device, d_inode.zone[8])? {
                self.free_zone(ind, device)?; // 二级间接块指向的一级间接块
            }
            self.free_zone(d_inode.zone[8], device)?; // 二级间接块
        }

        let zero = DINode {
            mode: Mode::from(0),
            uid: 0,
            size: 0,
            mtime: 0,
            gid: 0,
            nlinks: 0,
            zone: [0; 9],
        };
        self.write_d_inode(ino, &zero, device)?;
        self.free_inode(ino, device)?;
        Ok(())
    }

    /// 释放 inode 占用的全部数据块与间接块并回收 inode（自持锁版本，
    /// 供不持锁的调用方使用）。
    fn free_inode_blocks(&self, ino: u16) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let mut dev = self.lock();
        self.free_inode_blocks_at(ino, &mut dev)
    }

    /// 从固定宽度的名字字段中取出 `NUL` 结尾的名字。
    fn slot_name(raw: &[u8]) -> &str {
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        core::str::from_utf8(&raw[..end]).unwrap_or("")
    }

    /// 在目录 `dir_ino` 的数据中追加一个指向 `new_ino` 的目录项。
    ///
    /// 逐块查找空闲槽位（`ino == 0`），不够时分配并追加新数据块；目录
    /// 的 inode 与间接块表同步写回。需要调用方已持锁。
    fn add_dir_entry_at(
        &self,
        dir_ino: u16,
        name: &str,
        new_ino: u16,
        device: &T,
    ) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let entry_size = self.entry_size();
        let zone_size = self.superblock.zone_size();
        let mut dir = self.d_inode(dir_ino, device)?;
        let mut zones = self.data_zones(&dir, device)?;

        let mut block_index = 0;
        loop {
            if block_index >= zones.len() {
                // 目录空间不足：追加一个清零的新数据块。
                let Some(zone) = self.alloc_zone(device)? else {
                    return Ok(()); // 磁盘已满
                };
                zones.push(zone);
                self.write_file_zones(&mut dir, &zones, device)?;
            }

            let zone = zones[block_index];
            let mut block = vec![0u8; zone_size];
            self.read_zone_into(device, zone, &mut block)?;

            // 在块内查找空闲槽位并写入目录项。
            for slot in (0..zone_size).step_by(entry_size) {
                let ino = u16::from_le_bytes([block[slot], block[slot + 1]]);
                if ino == 0 {
                    Self::write_dir_slot(&mut block, slot, entry_size, new_ino, name);
                    self.write_zone(zone, &block, device)?;

                    // 目录大小要覆盖到新目录项末尾，否则按 size 遍历时看不到它。
                    let end = block_index * zone_size + slot + entry_size;
                    if (dir.size as usize) < end {
                        dir.size = end as u32;
                        self.write_d_inode(dir_ino, &dir, device)?;
                    }
                    return Ok(());
                }
            }
            block_index += 1;
        }
    }

    /// 在目录 `dir_ino` 中追加一个指向 `new_ino` 的目录项（自持锁版本，
    /// 供不持锁的调用方使用）。
    fn add_dir_entry(&self, dir_ino: u16, name: &str, new_ino: u16) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let mut dev = self.lock();
        self.add_dir_entry_at(dir_ino, name, new_ino, &mut dev)
    }

    /// 把 `(ino, name)` 写入目录块 `block` 的 `offset` 字节偏移处的槽位
    /// （名字按固定宽度补零）。
    fn write_dir_slot(block: &mut [u8], offset: usize, entry_size: usize, ino: u16, name: &str) {
        let mut raw = [0u8; 32];
        raw[0..2].copy_from_slice(&ino.to_le_bytes());
        raw[2..2 + name.len()].copy_from_slice(name.as_bytes());
        block[offset..offset + entry_size].copy_from_slice(&raw[..entry_size]);
    }

    /// 在目录 `dir_ino` 下创建空子目录 `name`，返回新 inode 号。
    ///
    /// 新目录含 `.`（指向自身）与 `..`（指向父目录）两个目录项，父目录
    /// 的链接数 +1（子目录的 `..` 引用）。名字不合法或 inode/数据块用尽时
    /// 返回 `Ok(None)`。底层设备在方法内部临时持锁。
    pub fn create_dir(&self, dir_ino: u16, name: &str) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) {
            return Ok(None);
        }

        let mode = Mode::from((u16::from(FileType::DIR)) << 12 | 0o755);
        let mut dev = self.lock();
        let Some(ino) = self.alloc_empty_inode(mode, &mut dev)? else {
            return Ok(None);
        };

        // 分配新目录的第一个数据块，写入 `.` 与 `..`。
        let entry_size = self.entry_size();
        let zone_size = self.superblock.zone_size();
        let Some(zone) = self.alloc_zone(&mut dev)? else {
            let _ = self.free_inode(ino, &mut dev);
            return Ok(None);
        };
        let mut block = vec![0u8; zone_size];
        Self::write_dir_slot(&mut block, 0, entry_size, ino, ".");
        Self::write_dir_slot(&mut block, entry_size, entry_size, dir_ino, "..");
        self.write_zone(zone, &block, &mut dev)?;

        // 新目录 inode：zone[0] = 数据块，size = 两个目录项，nlinks = 2（`.` 与 `..`）。
        let mut d_inode = self.d_inode(ino, &mut dev)?;
        d_inode.nlinks = 2;
        d_inode.zone[0] = zone;
        d_inode.size = (2 * entry_size) as u32;
        self.write_d_inode(ino, &d_inode, &mut dev)?;

        // 父目录追加目录项；失败时回滚数据块与 inode。
        if let Err(err) = self.add_dir_entry_at(dir_ino, name, ino, &mut dev) {
            let _ = self.free_zone(zone, &mut dev);
            let _ = self.free_inode(ino, &mut dev);
            return Err(err);
        }

        // 父目录链接数 +1（新子目录的 `..` 引用）。
        let mut parent = self.d_inode(dir_ino, &mut dev)?;
        parent.nlinks = parent.nlinks.saturating_add(1);
        self.write_d_inode(dir_ino, &parent, &mut dev)?;
        Ok(Some(ino))
    }

    /// 删除目录 `dir_ino` 下的空子目录 `name`（仅含 `.` 与 `..`）。
    ///
    /// 非空目录、目标不是目录或名字不合法时返回 `Ok(false)`；成功后父目录
    /// 链接数 -1，并释放子目录的数据块与 inode。底层设备在方法内部临时持锁。
    pub fn remove_dir(&self, dir_ino: u16, name: &str) -> IoResult<bool>
    where
        T: BlockDevice,
    {
        if !self.valid_name(name) {
            return Ok(false);
        }

        let mut dev = self.lock();
        let Some(ino) = self.find_dir_entry_ino(dir_ino, name, &mut dev)? else {
            return Ok(false);
        };
        let d_inode = self.d_inode(ino, &mut dev)?;
        if d_inode.mode.file_type() != FileType::DIR {
            return Ok(false);
        }

        // 只能删除空目录：除 `.` 与 `..` 外不得有其他目录项。
        let entry_size = self.entry_size();
        let zone_size = self.superblock.zone_size();
        for zone in self.data_zones(&d_inode, &mut dev)? {
            let mut block = vec![0u8; zone_size];
            self.read_zone_into(&mut dev, zone, &mut block)?;
            for slot in (0..zone_size).step_by(entry_size) {
                let ino = u16::from_le_bytes([block[slot], block[slot + 1]]);
                if ino == 0 {
                    continue;
                }
                let entry_name = Self::slot_name(&block[slot + 2..slot + entry_size]);
                if entry_name != "." && entry_name != ".." {
                    return Ok(false); // 非空目录
                }
            }
        }

        if !self.remove_dir_entry(dir_ino, name, &mut dev)? {
            return Ok(false);
        }

        // 父目录链接数 -1（该子目录的 `..` 引用消失）。
        let mut parent = self.d_inode(dir_ino, &mut dev)?;
        parent.nlinks = parent.nlinks.saturating_sub(1);
        self.write_d_inode(dir_ino, &parent, &mut dev)?;

        // 释放子目录的数据块与 inode。
        self.free_inode_blocks_at(ino, &mut dev)?;
        Ok(true)
    }

    /// 删除目录 `dir_ino` 下的 `name`：文件/符号链接走 [`unlink`](Self::unlink)，
    /// 目录走 [`remove_dir`](Self::remove_dir)（仅空目录）。不存在或失败返回
    /// `Ok(false)`。
    pub fn remove(&self, dir_ino: u16, name: &str) -> IoResult<bool>
    where
        T: BlockDevice,
    {
        // 先短暂持锁判断目标类型，再调用不持锁的 `remove_dir` / `unlink`，
        // 避免在已持锁状态下再次进入会持锁的公共方法（自旋锁不可重入）。
        let is_dir = {
            let mut dev = self.lock();
            let Some(ino) = self.find_dir_entry_ino(dir_ino, name, &mut dev)? else {
                return Ok(false);
            };
            self.d_inode(ino, &mut dev)?.mode.file_type() == FileType::DIR
        };

        if is_dir {
            self.remove_dir(dir_ino, name)
        } else {
            self.unlink(dir_ino, name)
        }
    }
}
