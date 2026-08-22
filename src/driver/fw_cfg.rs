#![allow(dead_code)]
//! fw_cfg（QEMU firmware config）MMIO 驱动。
//!
//! virt 机型上 fw_cfg 位于 `0x10100000`，寄存器布局：`data`(0x00)、
//! `ctl`/selector(0x08)、`dma`(0x10)。提供基于 DMA 的文件读写与按名查找
//! （file directory，selector `0x19`）。QEMU 11 的 ramfb 即通过 fw_cfg 的
//! `etc/ramfb` 文件下发帧缓冲配置。
use core::sync::atomic::{Ordering, fence};

use fdt::Fdt;

use crate::{
    driver::device::{DeviceInfo, Resource},
    mmio_regs,
};

/// fw_cfg 文件目录（file directory）的固定 selector。
pub const FW_CFG_FILE_DIR: u16 = 0x19;

const FW_CFG_DMA_CTL_ERROR: u32 = 0x01;
const FW_CFG_DMA_CTL_READ: u32 = 0x02;
const FW_CFG_DMA_CTL_SKIP: u32 = 0x04;
const FW_CFG_DMA_CTL_SELECT: u32 = 0x08;
const FW_CFG_DMA_CTL_WRITE: u32 = 0x10;

/// 文件目录条目固定大小：size(4) + select(2) + reserved(2) + name(56)。
const FW_CFG_FILE_ENTRY_SIZE: usize = 64;
/// 默认文件槽位数（与 QEMU `x-file-slots` 默认值一致）。
const FW_CFG_FILE_SLOTS: u32 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwCfgError {
    /// 无 DMA 寄存器（不支持 DMA 写）。
    NoDma,
    /// DMA 传输超时。
    Timeout,
    /// DMA 传输失败（设备返回错误位）。
    Io,
}

/// fw_cfg 设备：`device.mmio` 覆盖 data/ctl/dma 三个寄存器区间。
pub struct FwCfg {
    pub device: DeviceInfo,
}

mmio_regs! {
    FwCfg: [
        data: u64 => 0x00,
        ctl: u16 => 0x08,
        dma: u64 => 0x10,
    ]
}

/// DMA 访问结构（QEMU 以大端读取/回写）。
#[repr(C, align(8))]
struct FwCfgDmaAccess {
    control: u32,
    length: u32,
    address: u64,
}

impl FwCfg {
    /// 从设备树探测 fw_cfg（compatible `qemu,fw-cfg-mmio`）。
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let node = fdt.find_compatible(&["qemu,fw-cfg-mmio"])?;
        let reg = node.reg()?.next()?;
        let start = reg.starting_address as usize;
        let size = reg.size.unwrap_or(0);

        Some(Self {
            device: DeviceInfo {
                mmio: Resource::new(start, size),
                irq: None,
            },
        })
    }

    /// 一次 DMA 传输：`control` 含方向与 selector（高 16 位），数据位于
    /// `buf`（必须落在 guest 物理内存中，本内核恒等映射下可直接传地址）。
    fn dma_transfer(&self, control: u32, buf: &[u8]) -> Result<(), FwCfgError> {
        if self.device.mmio.size < 0x18 {
            return Err(FwCfgError::NoDma);
        }

        let access = FwCfgDmaAccess {
            control: control.to_be(),
            length: (buf.len() as u32).to_be(),
            address: (buf.as_ptr() as u64).to_be(),
        };

        // 先让结构体内容对设备可见，再写入 DMA 地址寄存器触发传输。
        // 注意：fw_cfg 的 MMIO 区域是 DEVICE_BIG_ENDIAN，QEMU 会把写入
        // 的字节按大端解释，因此这里要把地址字节序反转后再写入。
        fence(Ordering::SeqCst);
        self.write_dma((&access as *const FwCfgDmaAccess as u64).swap_bytes());

        // 轮询 control：QEMU 完成后回写 0（成功）或 ERROR 位（失败）。
        // 该字段由设备异步回写，必须易失读，防止被优化器缓存。
        let ctl_ptr = &access.control as *const u32;
        for _ in 0..100_000_000 {
            fence(Ordering::SeqCst);
            let ctl = u32::from_be(unsafe { ctl_ptr.read_volatile() });
            if ctl == 0 {
                // 确保控制字观测后，DMA 写入的数据也已可见。
                fence(Ordering::SeqCst);
                return Ok(());
            }
            if ctl & FW_CFG_DMA_CTL_ERROR != 0 {
                crate::warn!(
                    "fw_cfg: dma error, control={:#x}, len={}, addr={:#x}",
                    control,
                    buf.len(),
                    buf.as_ptr() as usize
                );
                return Err(FwCfgError::Io);
            }
            core::hint::spin_loop();
        }
        crate::warn!(
            "fw_cfg: dma timeout, control={:#x}, len={}",
            control,
            buf.len()
        );
        Err(FwCfgError::Timeout)
    }

    /// 读取指定 selector 的文件内容到 `buf`。
    pub fn read(&self, selector: u16, buf: &mut [u8]) -> Result<(), FwCfgError> {
        let control = ((selector as u32) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_READ;
        self.dma_transfer(control, buf)
    }

    /// 写入指定 selector 的文件内容（要求一次性写满整个文件）。
    pub fn write(&self, selector: u16, buf: &[u8]) -> Result<(), FwCfgError> {
        let control = ((selector as u32) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE;
        self.dma_transfer(control, buf)
    }

    /// 在 file directory 中按名查找文件，返回 `(selector, size)`。
    ///
    /// 目录数据由设备经 DMA 写入 `dir`，编译器无法感知该外部写，因此
    /// 读取一律走易失读，避免被优化缓存或重排。
    pub fn find_file(&self, name: &str) -> Option<(u16, u32)> {
        let mut dir = [0u8; 4 + FW_CFG_FILE_ENTRY_SIZE * FW_CFG_FILE_SLOTS as usize];
        if self.read(FW_CFG_FILE_DIR, &mut dir).is_err() {
            return None;
        }

        let count = read_be32(&dir, 0);
        if count > FW_CFG_FILE_SLOTS {
            return None;
        }

        for i in 0..count as usize {
            let base = 4 + i * FW_CFG_FILE_ENTRY_SIZE;
            let size = read_be32(&dir, base);
            let select = read_be16(&dir, base + 4);
            // name 为 56 字节 NUL 结尾字符串。
            let mut len = 0;
            while len < 56 && read_u8(&dir, base + 8 + len) != 0 {
                len += 1;
            }
            if name.len() == len
                && (0..len).all(|j| read_u8(&dir, base + 8 + j) == name.as_bytes()[j])
            {
                return Some((select, size));
            }
        }

        None
    }
}

/// 易失读取 `buf` 中第 `off` 字节（缓冲由设备 DMA 写入，禁缓存）。
fn read_u8(buf: &[u8], off: usize) -> u8 {
    unsafe { buf.as_ptr().add(off).read_volatile() }
}

/// 易失读取大端 u16。
fn read_be16(buf: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([read_u8(buf, off), read_u8(buf, off + 1)])
}

/// 易失读取大端 u32。
fn read_be32(buf: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([
        read_u8(buf, off),
        read_u8(buf, off + 1),
        read_u8(buf, off + 2),
        read_u8(buf, off + 3),
    ])
}
