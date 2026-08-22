//! 设备抽象与统一的 I/O 结果类型。
//!
//! [`Device`] 是所有设备的公共接口；[`CharDevice`] / [`BlockDevice`] 分别描述
//! 流式与按字节偏移的读写设备。读写统一返回 [`IoResult`]，错误类型
//! [`IoError`] 自持、不依赖具体驱动或 [`crate::error`] 中的设备错误。

#![allow(dead_code)]

use crate::numeric;

/// I/O 错误：设备读写失败的统一错误类型。
#[derive(Debug, Clone, thiserror::Error)]
pub enum IoError {
    /// 设备请求执行失败（如 virtio-blk 请求返回错误状态）。
    #[error("device request failed")]
    Request,
    /// 设备尚未就绪（握手或特性协商失败）。
    #[error("device not ready")]
    NotReady,
}

/// 设备层统一结果类型。进入 VFS 后通过 `From<IoError>` 映射为 `FsError::Io`。
pub type IoResult<T> = core::result::Result<T, IoError>;

pub trait Read: Device {
    fn read(&self, buf: &mut [u8]) -> IoResult<usize>;
}

pub trait Write: Device {
    fn write(&self, buf: &[u8]) -> IoResult<usize>;
}

pub trait Seek: Device {
    fn seek(&self, current: u64, offset: i64, whence: Whence) -> IoResult<i64>;
}

pub trait ReadAt: Device {
    fn read_at(&self, buf: &mut [u8], offset: usize) -> IoResult<usize>;
}

pub trait WriteAt: Device {
    fn write_at(&self, buf: &[u8], offset: usize) -> IoResult<usize>;
}

pub trait CharDevice: Device + Read + Write {}
impl<T: Device + Read + Write> CharDevice for T {}

pub trait BlockDevice: Device + ReadAt + WriteAt {}
impl<T: Device + ReadAt + WriteAt> BlockDevice for T {}

pub trait Device: Send + Sync {
    fn name(&self) -> &'static str;
    fn irq(&self) -> Option<usize>;
}

numeric! {
    pub enum Whence : i32 {
        SEEK_SET = 0,
        SEEK_CUR = 1,
        SEEK_END = 2,
    }
}
