#![allow(dead_code)]
//! 内核错误类型。
//!
//! 按域拆分为 [`ElfError`]、[`MemError`]、[`DevError`]；顶层 [`Error`]
//! 聚合各域错误与启动 / 进程级错误，并通过 `From` 转换支持跨模块 `?`。

use alloc::{format, string::String};

use athera_rand::EntropyError;
use fdt::FdtError;

use crate::driver::traits::IoError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProcError {
    #[error("no other task")]
    NoOtherTask,
    /// 参数或环境变量超出用户栈容量（对应 Linux 的 `E2BIG`）。
    #[error("argument list too long")]
    ArgsTooLong,
}

/// ELF 加载错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ElfError {
    /// 不是 ELF 文件。
    #[error("not an ELF file")]
    NotElf,
    /// 不是 64 位 ELF。
    #[error("not a 64-bit ELF")]
    Not64Bit,
    /// ELF 不是小端。
    #[error("ELF is not little-endian")]
    NotLsb,
    /// 不是 RISC-V ELF。
    #[error("not a RISC-V ELF")]
    NotRiscv,
}

/// 内存子系统错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum MemError {
    /// 内存不足。
    #[error("out of memory")]
    OutOfMemory,
    /// 页表缺失。
    #[error("page table not found")]
    PageTableMissing,
    /// 找不到用户地址空间。
    #[error("user address space not found")]
    AddressSpaceNotFound,
    /// 不是用户地址空间。
    #[error("not an user address space")]
    NotUserAddressSpace,
    /// 空指针。
    #[error("null pointer")]
    NullPointer,
}

/// 设备子系统错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum DevError {
    /// 内存分配失败（设备驱动同样依赖伙伴分配器）。
    #[error(transparent)]
    Mem(#[from] MemError),
    /// 探测系统内存失败。
    #[error("failed to probe system memory")]
    MemoryProbeFailed,
    /// Virtio 握手失败。
    #[error("virtio handshake failed")]
    VirtioHandshakeFailed,
    /// Virtio 设备类型不支持。
    #[error("virtio device not supported")]
    VirtioNotSupported,
    /// Virtio 特性协商失败。
    #[error("virtio features negotiation failed")]
    VirtioFeaturesNotOk,
    /// Virtio-blk 请求失败。
    #[error("virtio-blk request failed")]
    VirtioBlockFailed,
    /// Virtio-rng 请求失败。
    #[error("virtio-rng request failed")]
    VirtioRngFailed,
    /// 未找到 Virtio Block 设备。
    #[error("no Virtio Block device found")]
    NoVirtioBlock,
    /// 未找到 UART 设备。
    #[error("no UART device found")]
    NoUart,
}

/// 顶层统一错误：聚合各域错误与启动 / 进程级错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// 设备树无效。
    #[error("fdt error: {0}")]
    Fdt(String),
    /// 没有可用的任务 ID。
    #[error("no task id available")]
    NoTidAvailable,
    /// ELF 加载错误。
    #[error(transparent)]
    Elf(#[from] ElfError),
    /// 内存子系统错误。
    #[error(transparent)]
    Mem(#[from] MemError),
    /// 设备子系统错误。
    #[error(transparent)]
    Dev(#[from] DevError),
    #[error(transparent)]
    Proc(#[from] ProcError),
    #[error(transparent)]
    Io(#[from] IoError),
}

impl From<FdtError> for Error {
    fn from(err: FdtError) -> Self {
        Error::Fdt(format!("{:?}", err))
    }
}

impl From<EntropyError> for Error {
    fn from(_: EntropyError) -> Self {
        Error::Dev(DevError::VirtioRngFailed)
    }
}

pub type Result<T> = core::result::Result<T, Error>;
