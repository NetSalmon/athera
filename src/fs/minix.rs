#![allow(unused)]
//! MINIX V1 文件系统驱动。
//!
//! 从 virtio-blk 块设备读写 MINIX V1 文件系统：超级块（[`DiskSuperBlock`]）、
//! 磁盘 inode（[`DiskInode`]）与目录项（[`DirEntryRaw`]）；inode/zone 位图
//! 用 [`BitMapView`] 零拷贝维护，支持按路径查找、顺序读写文件、创建文件，
//! 以及分配/释放 inode 与数据块。
//!
//! 模块结构：磁盘结构（`types`）、路径类型（`path`，见 [`crate::fs`]）、
//! 目录迭代器（`dir`）、打开的文件（`file`）、写路径（`write`）与路径
//! 解析（`open`）。
//!
//! [`MinixFs`] 自持底层块设备的共享句柄（`Arc<RwLock<T>>`），读写时在
//! 方法内部临时持锁，不再把设备引用传出。

mod dir;
mod file;
mod open;
mod types;
mod write;

use alloc::{sync::Arc, vec, vec::Vec};
use core::{fmt, slice};

use athera_bitmap::BitMapView;
pub(crate) use dir::{DirEntries, DirEntry};
pub(crate) use file::File;
pub(crate) use types::{
    DirEntryRaw, DirEntryV1_14, DirEntryV1_30, DiskInode, DiskSuperBlock, MinixFsMagic, MinixString,
};

pub(crate) use super::{
    path::{Component, Path, PathBuf},
    types::{FileType, Mode, S_IFMT},
};
use crate::{
    constants::SUPERBLOCK_OFFSET,
    driver::traits::{BlockDevice, IoResult},
    fs::vfs::{
        File as VfsFile, FileSystem, FsError, FsResult, OpenFlags, Stat,
        file_ops::{FileOps, Whence},
    },
    sync::rwlock::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

/// MINIX V1 文件系统：超级块元数据 + 底层块设备的共享句柄。
///
/// 设备被包装在 `Arc<RwLock<T>>` 中，所有磁盘操作在方法内部临时持锁，
/// 因此 `MinixFs` 可被多个打开的 [`File`] 共享（为未来的 fd 表 / VFS 铺路），
/// 不需要独占设备引用。
pub struct MinixFs<T> {
    pub(crate) superblock: DiskSuperBlock,
    device: Arc<RwLock<T>>,
}

impl<T> MinixFs<T> {
    /// 尝试把设备当作 MINIX v1 文件系统打开。
    ///
    /// 从第 1 块（偏移 [`SUPERBLOCK_OFFSET`]）读取超级块并校验 magic：
    /// 设备读取失败返回 `Err`；magic 不匹配（设备上不是 MINIX 文件系统）
    /// 返回 `Ok(None)`；成功则返回 `Ok(Some(Self))`。`MinixFs` 克隆共享的
    /// 设备句柄，与调用方共用同一个底层设备。
    pub fn from_device(device: &Arc<RwLock<T>>) -> IoResult<Option<Self>>
    where
        T: BlockDevice,
    {
        let dev = device.read();

        let mut buffer = vec![0u8; size_of::<DiskSuperBlock>()];
        dev.read_at(&mut buffer, SUPERBLOCK_OFFSET)?;

        // SAFETY: `read_unaligned` 不要求对齐，可安全读取 u8 缓冲中的结构体。
        let superblock = unsafe { (buffer.as_ptr() as *const DiskSuperBlock).read_unaligned() };

        if superblock.magic == MinixFsMagic::V1_14 || superblock.magic == MinixFsMagic::V1_30 {
            Ok(Some(Self {
                superblock,
                device: device.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// 获取底层块设备的临时排他访问（内部使用）。
    pub(crate) fn lock(&self) -> RwLockWriteGuard<'_, T> {
        self.device.write()
    }

    pub(crate) fn read_lock(&self) -> RwLockReadGuard<'_, T> {
        self.device.read()
    }

    pub fn read_inode(&self, ino: u16, device: &T) -> IoResult<DiskInode>
    where
        T: BlockDevice,
    {
        let offset = (ino - 1) as usize * size_of::<DiskInode>();

        let mut buffer = vec![0u8; size_of::<DiskInode>()];

        device.read_at(&mut buffer, self.superblock.inode_table_offset() + offset)?;

        let inode: DiskInode = unsafe { (buffer.as_ptr() as *const DiskInode).read_unaligned() };

        Ok(inode)
    }

    /// 读取 inode 指向的全部数据块，返回文件内容（末尾按 `inode.size` 截断）。
    pub fn data(&self, inode: &DiskInode, device: &T) -> IoResult<Vec<u8>>
    where
        T: BlockDevice,
    {
        if inode.size == 0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::with_capacity(inode.size as usize);
        for zone in self.data_zones(inode, device)? {
            self.read_zone(device, zone, &mut out)?;
        }

        out.truncate(inode.size as usize);
        Ok(out)
    }

    /// 依次收集 inode 引用的数据块号：7 个直接块 → 一级间接块 → 二级间接块。
    /// 块号为 0 表示“没有更多块”，遇到即停止。
    pub(crate) fn data_zones(&self, inode: &DiskInode, device: &T) -> IoResult<Vec<u16>>
    where
        T: BlockDevice,
    {
        let mut zones = Vec::new();

        // zone[0..7]：7 个直接数据块。
        for &zone in &inode.zone[..7] {
            if zone == 0 {
                return Ok(zones);
            }
            zones.push(zone);
        }

        // zone[7]：一级间接块，里面存放数据块号。
        for zone in self.zone_table(device, inode.zone[7])? {
            zones.push(zone);
        }

        // zone[8]：二级间接块，里面存放“一级间接块”的块号。
        for indirect_zone in self.zone_table(device, inode.zone[8])? {
            for zone in self.zone_table(device, indirect_zone)? {
                zones.push(zone);
            }
        }

        Ok(zones)
    }

    /// 读取 `zone` 指向的一个数据块，追加到 `out` 末尾。
    fn read_zone(&self, device: &T, zone: u16, out: &mut Vec<u8>) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let mut buffer = vec![0u8; self.superblock.zone_size()];
        self.read_zone_into(device, zone, &mut buffer)?;
        out.extend_from_slice(&buffer);
        Ok(())
    }

    /// 把 `zone` 指向的数据块开头读入 `out`（`out` 长度不能超过 zone 大小）。
    pub(crate) fn read_zone_into(&self, device: &T, zone: u16, out: &mut [u8]) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        debug_assert!(out.len() <= zone_size);
        device.read_at(out, zone as usize * zone_size)?;
        Ok(())
    }

    /// 读取一个“块号表”：把 `zone` 指向的块按 `u16` 数组解释并返回其中的块号。
    /// `zone == 0` 表示该级间接块不存在，返回空表；表内遇到 0 提前结束。
    pub(crate) fn zone_table(&self, device: &T, zone: u16) -> IoResult<Vec<u16>>
    where
        T: BlockDevice,
    {
        if zone == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .read_zone_table(device, zone)?
            .into_iter()
            .take_while(|&z| z != 0)
            .collect())
    }

    /// 读取一个“块号表”的完整内容（含中间为 0 的空槽位），供写入路径使用。
    pub(crate) fn read_zone_table(&self, device: &T, zone: u16) -> IoResult<Vec<u16>>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        let mut buffer = vec![0u8; zone_size];
        device.read_at(&mut buffer, zone as usize * zone_size)?;

        // SAFETY: 分配器返回的内存满足 u16 对齐；`zone_size` 是 2 的幂，可被 u16 大小整除。
        let table = unsafe {
            slice::from_raw_parts(buffer.as_ptr() as *const u16, zone_size / size_of::<u16>())
        };

        Ok(table.to_vec())
    }

    /// 把一个“块号表”写回 `zone` 指向的块（表长必须恰好为一个 zone）。
    pub(crate) fn write_zone_table(&self, device: &T, zone: u16, table: &[u16]) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        debug_assert_eq!(table.len(), zone_size / size_of::<u16>());

        // SAFETY: u16 数组与字节切片长度一致；块号按小端写在盘上（riscv 为小端）。
        let bytes =
            unsafe { slice::from_raw_parts(table.as_ptr() as *const u8, size_of_val(table)) };
        device.write_at(bytes, zone as usize * zone_size)?;
        Ok(())
    }

    /// 把 inode 写回磁盘（`ino` 从 1 起）。
    pub fn write_inode(&self, ino: u16, inode: &DiskInode, device: &T) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let offset =
            self.superblock.inode_table_offset() + (ino - 1) as usize * size_of::<DiskInode>();

        // SAFETY: `DiskInode` 是 `#[repr(C)]` 的纯数据（无 padding），按原样写盘，
        // 与读取路径 `read_unaligned` 对称。
        let bytes = unsafe {
            slice::from_raw_parts(
                (inode as *const DiskInode).cast::<u8>(),
                size_of::<DiskInode>(),
            )
        };
        device.write_at(bytes, offset)?;
        Ok(())
    }

    /// 把 `data` 写入 `zone` 指向的数据块开头（`data` 长度不能超过 zone 大小）。
    pub fn write_zone(&self, zone: u16, data: &[u8], device: &T) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        debug_assert!(data.len() <= zone_size);
        device.write_at(data, zone as usize * zone_size)?;
        Ok(())
    }

    /// 读入一段位图区域，对 [`BitMapView`] 执行 `f`，再把结果写回磁盘。
    ///
    /// `map_offset` 是位图在磁盘上的字节偏移，`map_bytes` 是位图占用的字节数，
    /// `bits` 是其中可用的位数（超出部分视为保留位）。
    fn with_bitmap<F, R>(
        &self,
        device: &T,
        map_offset: usize,
        map_bytes: usize,
        bits: usize,
        f: F,
    ) -> IoResult<R>
    where
        T: BlockDevice,
        F: FnOnce(&mut BitMapView<'_>) -> R,
    {
        let words = map_bytes / size_of::<usize>();
        let mut buffer = vec![0usize; words];

        {
            // SAFETY: `Vec<usize>` 堆内存按 usize 对齐；`map_bytes` 是 zone 大小的整数倍。
            let bytes =
                unsafe { slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), map_bytes) };
            device.read_at(bytes, map_offset)?;
        }

        let result = {
            let bits = bits.min(words * (usize::BITS as usize));
            let mut map = BitMapView::new_with_capacity(buffer.as_mut_slice(), bits);
            f(&mut map)
        };

        {
            // SAFETY: 同上。
            let bytes = unsafe { slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), map_bytes) };
            device.write_at(bytes, map_offset)?;
        }

        Ok(result)
    }

    /// 分配一个空闲 inode 并置位，返回 inode 号（从 1 起）；inode 用尽返回
    /// `Ok(None)`。位图由 [`BitMapView`] 维护。
    ///
    /// MINIX v1 的 inode 位图按“第 i 位对应 inode i”索引（位 0 保留），
    /// 因此从位 1 起搜索，位号即 inode 号。
    pub fn alloc_inode(&self, device: &T) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        let map_bytes = self.superblock.imap_blocks as usize * zone_size;
        self.with_bitmap(
            device,
            2 * zone_size,
            map_bytes,
            self.superblock.ninodes as usize + 1,
            |map| map.alloc_from(1).map(|bit| bit as u16),
        )
    }

    /// 释放 inode（清 inode 位图对应位，位号 = inode 号）；越界时忽略。
    pub fn free_inode(&self, ino: u16, device: &T) -> IoResult<()>
    where
        T: BlockDevice,
    {
        if ino == 0 || ino > self.superblock.ninodes {
            return Ok(());
        }
        let zone_size = self.superblock.zone_size();
        let map_bytes = self.superblock.imap_blocks as usize * zone_size;
        self.with_bitmap(
            device,
            2 * zone_size,
            map_bytes,
            self.superblock.ninodes as usize + 1,
            |map| {
                let _ = map.clear(ino as usize);
            },
        )
    }

    /// 分配一个空闲数据块并置位，返回块号；磁盘用尽返回 `Ok(None)`。
    ///
    /// MINIX v1 的 zone 位图按“第 i 位对应 zone `first_data_zone - 1 + i`”
    /// 索引（位 0 是最后一个元数据块），因此从位 1 起搜索，从
    /// `first_data_zone` 起分配。位图由 [`BitMapView`] 维护。
    pub fn alloc_zone(&self, device: &T) -> IoResult<Option<u16>>
    where
        T: BlockDevice,
    {
        let zone_size = self.superblock.zone_size();
        let map_offset = (2 + self.superblock.imap_blocks as usize) * zone_size;
        let map_bytes = self.superblock.zmap_blocks as usize * zone_size;
        let first = self.superblock.first_data_zone as usize;
        let bits = self.superblock.nzones as usize - first + 1;
        self.with_bitmap(device, map_offset, map_bytes, bits, |map| {
            map.alloc_from(1).map(|bit| (bit + first - 1) as u16)
        })
    }

    /// 释放数据块（清 zone 位图对应位）；元数据区或越界块号忽略。
    pub fn free_zone(&self, zone: u16, device: &T) -> IoResult<()>
    where
        T: BlockDevice,
    {
        let first = self.superblock.first_data_zone;
        if zone < first || zone >= self.superblock.nzones {
            return Ok(());
        }
        let zone_size = self.superblock.zone_size();
        let map_offset = (2 + self.superblock.imap_blocks as usize) * zone_size;
        let map_bytes = self.superblock.zmap_blocks as usize * zone_size;
        let bits = self.superblock.nzones as usize - first as usize + 1;
        self.with_bitmap(device, map_offset, map_bytes, bits, |map| {
            let bit = zone as usize - first as usize + 1;
            let _ = map.clear(bit);
        })
    }
}

impl<T> fmt::Debug for MinixFs<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MinixFs")
            .field("superblock", &self.superblock)
            .finish()
    }
}

/// 把 MINIX 的路径文件接口适配为 VFS 文件系统接口。
///
/// MINIX 的打开文件借用 `MinixFs`，而 VFS 文件句柄要求可独立保存，因此
/// 适配器按路径重新打开文件，并在 `FileOps` 内维护当前偏移。
pub(crate) struct MinixVfs<T> {
    fs: Arc<MinixFs<T>>,
}

impl<T> MinixVfs<T> {
    pub(crate) fn new(fs: Arc<MinixFs<T>>) -> Self {
        Self { fs }
    }
}

impl<T> FileSystem for MinixVfs<T>
where
    T: BlockDevice + 'static,
{
    fn open(&self, path: &Path, flags: OpenFlags, _mode: Mode) -> FsResult<VfsFile> {
        let file = self
            .fs
            .open(path)
            .map_err(FsError::from)?
            .ok_or(FsError::NotFound)?;

        let mode = file.mode();

        Ok(VfsFile::from_ops(
            path.file_name().unwrap_or("/"),
            file.ino() as u64,
            mode,
            Arc::new(MinixFileOps {
                fs: self.fs.clone(),
                path: path.to_path_buf(),
            }),
            flags,
        ))
    }

    fn stat(&self, path: &Path) -> FsResult<Stat> {
        let file = self
            .fs
            .open(path)
            .map_err(FsError::from)?
            .ok_or(FsError::NotFound)?;
        Ok(Stat {
            ino: file.ino() as u64,
            mode: file.mode(),
            size: file.size() as u64,
            nlinks: 1,
            mtime: 0,
            uid: 0,
            gid: 0,
        })
    }

    fn mkdir(&self, _: &Path, _: Mode) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn unlink(&self, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn rmdir(&self, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn rename(&self, _: &Path, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn link(&self, _: &Path, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn symlink(&self, _: &str, _: &Path) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    fn readlink(&self, _: &Path) -> FsResult<PathBuf> {
        Err(FsError::Unsupported)
    }

    fn sync(&self) -> FsResult<()> {
        Ok(())
    }
}

struct MinixFileOps<T> {
    fs: Arc<MinixFs<T>>,
    path: PathBuf,
}

impl<T> MinixFileOps<T>
where
    T: BlockDevice,
{
    fn open(&self) -> FsResult<File<'_, T>> {
        self.fs
            .open(&self.path)
            .map_err(FsError::from)?
            .ok_or(FsError::NotFound)
    }
}

impl<T> FileOps for MinixFileOps<T>
where
    T: BlockDevice + 'static,
{
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let file = self.open()?;
        file.read_at(buf, offset as usize).map_err(FsError::from)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        let mut file = self.open()?;
        file.write_at(buf, offset as usize).map_err(FsError::from)
    }

    fn seek(&self, current: u64, offset: i64, whence: Whence) -> FsResult<i64> {
        let file = self.open()?;
        let current = current as i64;
        let size = file.size() as i64;
        let new = match whence {
            Whence::SEEK_SET => offset,
            Whence::SEEK_CUR => current.saturating_add(offset),
            Whence::SEEK_END => size.saturating_add(offset),
            _ => return Err(FsError::Invalid),
        };
        if new < 0 {
            return Err(FsError::Invalid);
        }
        Ok(new)
    }
}
