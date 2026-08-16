//! 打开的文件 [`File`]：保存 inode 信息、数据块号与所属文件系统的引用，
//! 读写接口与标准库 `std::fs::File` 类似。

use alloc::vec::Vec;
use core::fmt;

use super::{FileType, MinixFs, types::DINode};
use crate::{
    dev::traits::{BlockDevice, IoResult},
    fs::{
        path::{Path, PathBuf},
        vfs::file_ops::Whence,
    },
};

/// 打开的文件：保存 inode 信息、数据块号与所属文件系统的引用。
///
/// 文件系统（[`MinixFs`]）自持底层块设备的共享句柄，因此 [`File`] 不再
/// 独占设备引用：每次读 / 写 / 定位都在方法内部临时持锁。一个文件系统上
/// 可以同时打开多个 `File`。
///
/// 注意：`File` 内的 `inode`/`zones` 是打开时的快照，写入会就地更新并
/// 写回磁盘，但不会重读磁盘上的最新状态。
pub struct File<'a, T>
where
    T: BlockDevice,
{
    /// 所属文件系统。
    fs: &'a MinixFs<T>,
    /// 打开文件时使用的路径（符号链接写入等内部临时文件为空）。
    path: PathBuf,
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

impl<'a, T> File<'a, T>
where
    T: BlockDevice,
{
    /// 按路径打开文件，等价于 `fs.open(path)`。
    ///
    /// 路径不存在返回 `Ok(None)`；设备出错返回 `Err`。
    pub fn open(fs: &'a MinixFs<T>, path: &Path) -> IoResult<Option<Self>> {
        fs.open(path)
    }

    /// 构造已打开的文件（由 `open` 与符号链接写入等内部流程使用）。
    pub(crate) fn new(
        fs: &'a MinixFs<T>,
        path: PathBuf,
        ino: u16,
        inode: DINode,
        zone_size: usize,
        zones: Vec<u16>,
    ) -> Self {
        File {
            fs,
            path,
            ino,
            inode,
            zone_size,
            zones,
            offset: 0,
        }
    }

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

    /// 打开文件时使用的路径。
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// 文件名（最后一个分量）。
    pub fn name(&self) -> Option<&str> {
        self.path.file_name()
    }

    /// 按 Linux 风格的起点移动读写位置，返回新的位置。
    ///
    /// 目标位置超过文件末尾时钳制到末尾。返回 `Ok(new_offset)`，错误类型
    /// 为底层设备错误（当前定位本身不会失败，保留 `Result` 以对齐标准库）。
    pub fn seek(&mut self, offset: i64, whence: Whence) -> IoResult<u64> {
        let size = self.size() as i64;
        let new = match whence {
            Whence::SEEK_SET => offset,
            Whence::SEEK_END => size + offset,
            Whence::SEEK_CUR => self.offset as i64 + offset,
            _ => return Err(crate::dev::traits::IoError::Request),
        };
        self.offset = new.clamp(0, size) as usize;
        Ok(self.offset as u64)
    }

    /// 从当前读写位置读取最多 `buf.len()` 字节，返回实际读取的字节数；
    /// 到达文件末尾时返回 0。读取会推进内部读写位置。
    pub fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let n = self.read_at(buf, self.offset)?;
        self.offset += n;
        Ok(n)
    }

    /// 从文件 `offset` 处读取最多 `buf.len()` 字节，返回实际读取的字节数；
    /// 到达文件末尾时返回 0。不会改变内部读写位置。
    pub fn read_at(&self, buf: &mut [u8], offset: usize) -> IoResult<usize> {
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

            {
                let dev = self.fs.read_lock();
                dev.read_at(
                    &mut buf[done..done + n],
                    zone as usize * self.zone_size + in_zone,
                )?;
            }

            pos += n;
            done += n;
        }

        Ok(done)
    }

    /// 从 `offset` 处写入 `buf`，返回实际写入字节数。
    ///
    /// 数据块不足时通过 [`MinixFs::alloc_zone`] 分配新块，并同步 inode
    /// 的 zone 数组与间接块表，文件大小随之增长。`offset` 超过文件末尾时
    /// 按“追加到末尾”处理；磁盘空间不足时返回实际写入的字节数
    /// （小于 `buf.len()`）。不会改变内部读写位置。
    pub fn write_at(&mut self, buf: &[u8], offset: usize) -> IoResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let zone_size = self.zone_size;
        let size = self.size() as usize;
        let offset = offset.min(size); // 不支持空洞，超末尾按追加处理
        let end = offset.saturating_add(buf.len());
        let need_zones = end.div_ceil(zone_size);

        // 1. 补齐不足的数据块（超出部分保持原样，不重复分配）。
        let grew = self.zones.len() < need_zones;
        while self.zones.len() < need_zones {
            let zone = {
                let mut dev = self.fs.lock();
                self.fs.alloc_zone(&dev)?
            };
            let Some(zone) = zone else {
                break; // 磁盘空间不足
            };
            self.zones.push(zone);
        }

        if grew {
            let mut dev = self.fs.lock();
            self.fs
                .write_file_zones(&mut self.inode, &self.zones, &dev)?;
        }

        // 2. 写入数据（只写能落到已分配数据块的部分）。
        let mut pos = offset;
        let mut done = 0;
        while pos < end && done < buf.len() {
            let zone_index = pos / zone_size;
            let Some(&zone) = self.zones.get(zone_index) else {
                break; // 数据块号缺失（磁盘已满等），按实际写入量返回
            };
            let in_zone = pos % zone_size;
            let n = (zone_size - in_zone).min(buf.len() - done);
            {
                let mut dev = self.fs.lock();
                dev.write_at(&buf[done..done + n], zone as usize * zone_size + in_zone)?;
            }
            pos += n;
            done += n;
        }

        // 3. 扩展文件大小并写回 inode。
        let new_size = size.max(offset.saturating_add(done));
        if new_size > self.inode.size as usize {
            self.inode.size = new_size as u32;
            let mut dev = self.fs.lock();
            self.fs.write_d_inode(self.ino, &self.inode, &dev)?;
        }

        Ok(done)
    }

    /// 从当前读写位置写入 `buf`，返回实际写入字节数；写入会推进内部读写位置。
    pub fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        let n = self.write_at(buf, self.offset)?;
        self.offset += n;
        Ok(n)
    }

    /// 文件类型（普通文件 / 目录 / 符号链接 / 设备等）。
    pub fn r#type(&self) -> FileType {
        self.inode.mode.file_type()
    }
}

impl<T> fmt::Debug for File<'_, T>
where
    T: BlockDevice,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            .field("path", &self.path)
            .field("ino", &self.ino)
            .field("inode", &self.inode)
            .field("zone_size", &self.zone_size)
            .field("zones", &self.zones)
            .field("offset", &self.offset)
            .finish()
    }
}
