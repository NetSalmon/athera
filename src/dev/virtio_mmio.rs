#![allow(dead_code)]
//! virtio-mmio 传输层。
//!
//! 定义 MMIO 寄存器布局（`VirtqCfg`）、设备状态/类型枚举与虚拟队列
//! 结构；设备初始化流程见 [`handshake`]，队列实现见 [`queue`]。
mod handshake;
pub(crate) mod queue;

use alloc::vec::Vec;
use core::sync::atomic::{Ordering, fence};

use fdt::Fdt;

use self::{handshake::QueueConfig, queue::Virtq};
use crate::{
    bits,
    constants::{RING_SIZE, VIRTIO_VERSION_LEGACY},
    dev::device::{Device, Resource},
    error::DevError,
    mmio_regs, numeric,
};

/// 本模块统一结果类型。
pub type DevResult<T> = core::result::Result<T, DevError>;

pub struct VirtqCfg {
    pub device: Device,
}

numeric! {
    pub enum DeviceType : u32 {
        NET = 1,
        BLOCK = 2,
        CONSOLE = 3,
        ENTROPY_SOURCE = 4,
        MEMORY_BALLOONING = 5,
        IO_MEMORY = 6,
        RPMSG = 7,
        SCSI_HOST = 8,
        TRANSPORT = 9,
        MAC80211_WLAN = 10,
        RPROC_SERIAL = 11,
        VIRTIO_CAIF = 12,
    }
}

bits! {
    pub type DeviceStatus : u32 {
        acknowledge: 0,
        driver: 1,
        driver_ok: 2,
        features_ok: 3,
        device_needs_reset: 6,
        failed: 7,
    }
}

numeric! {
    pub enum VirtqVersion : u32 {
        LEGACY = 1,
        MODERN = 2,
    }
}

// generate:
// #[inline]
// pub fn magic_value(&self) -> u32 {
//     self.device.mmio.read::<u32>(0x000)
// }
// #[inline]
// pub fn write_magic_value(&self, val: u32) {
//     self.device.mmio.write::<u32>(0x000, val);
// }
// ...
mmio_regs! {
    VirtqCfg: [
        magic_value: u32 => 0x000,
        version: u32 => 0x004,
        device_id: u32 => 0x008,
        vendor_id: u32 => 0x00C,
        device_features: u32 => 0x010,
        device_features_sel: u32 => 0x014,
        driver_features: u32 => 0x020,
        driver_features_sel: u32 => 0x024,
        queue_sel: u32 => 0x030,
        queue_num_max: u32 => 0x034,
        queue_num: u32 => 0x038,
        queue_align: u32 => 0x03C,   // legacy
        queue_pfn: u32 => 0x040, // legacy
        queue_ready: u32 => 0x044,
        queue_notify: u32 => 0x050,
        guest_page_size: u32 => 0x028,
        interrupt_status: u32 => 0x060,
        interrupt_ack: u32 => 0x064,
        status: u32 => 0x070,
        queue_desc_low: u32 => 0x080,
        queue_desc_high: u32 => 0x084,
        queue_driver_low: u32 => 0x090,
        queue_driver_high: u32 => 0x094,
        queue_device_low: u32 => 0x0A0,
        queue_device_high: u32 => 0x0A4,
        config_generation: u32 => 0x0FC,
    ]
}

/// virtio-mmio 设备抽象。
///
/// 提供与具体设备类型无关的探测（[`Self::probe`]）、特征协商与队列
/// 配置握手（[`Self::handshake`]）以及队列 0 的收发原语（[`Self::queue`] /
/// [`Self::notify`]，配合 [`queue::Virtq`] 的 `post_avail` / `wait_used`）。
///
/// 新设备驱动只需实现 [`Self::DEVICE_TYPE`] 与 `device` / `queues_mut`
/// 两个访问器，并按需覆写 [`Self::negotiate`] / [`Self::negotiate_legacy`]，
/// 即可获得完整的初始化与提交流程（见 `virtio_blk` / `virtio_rng`）。
pub trait VirtioDevice: Sized {
    /// 设备类型（virtio 规范编号），探测时按此匹配 `device_id` 寄存器。
    const DEVICE_TYPE: DeviceType;

    /// 返回设备的 MMIO 描述。
    fn device(&self) -> Device;

    /// 已配置虚拟队列的可变访问（未握手时为 `None`）。
    fn queues_mut(&mut self) -> &mut Option<Vec<Virtq>>;

    /// modern 特性协商：输入设备特性（低 / 高 32 位），返回驱动协商值。
    ///
    /// 默认不协商任何特性（`(0, 0)`）。
    fn negotiate(&self, _features_low: u32, _features_high: u32) -> (u32, u32) {
        (0, 0)
    }

    /// legacy 特性协商：输入设备特性，返回驱动协商值。默认 `0`。
    fn negotiate_legacy(&self, _features: u32) -> u32 {
        0
    }

    /// 遍历设备树中所有 virtio,mmio 节点，逐个读取其 MMIO 寄存器中的
    /// `device_id`，返回第一个匹配 [`Self::DEVICE_TYPE`] 的设备；空槽位
    /// 或其它 virtio 设备会被跳过。
    fn probe(fdt: &Fdt) -> Option<Self>
    where
        Self: From<Device>,
    {
        fdt.all_nodes().find_map(|node| {
            let is_mmio = node
                .compatible()
                .map(|c| c.all().any(|c| c == "virtio,mmio"))
                .unwrap_or(false);
            if !is_mmio {
                return None;
            }

            let reg = node.reg()?.next()?;
            let start = reg.starting_address as usize;
            let size = reg.size.unwrap_or(0);
            let interrupts = node.interrupts()?.next()?;

            let device = Device {
                mmio: Resource::new(start, size),
                irq: Some(interrupts),
            };

            // 空槽位返回 0，其它类型设备返回各自的 device_id，均跳过。
            let cfg = VirtqCfg { device };
            if cfg.device_id() != Self::DEVICE_TYPE.0 {
                return None;
            }

            Some(Self::from(device))
        })
    }

    /// 完成 ACK / DRIVER 状态、特性协商与队列 0 配置的完整握手。
    ///
    /// 队列尚未配置时，[`Self::queue`] 与提交流程会自动补一次握手。
    fn handshake(&mut self) -> DevResult<()> {
        let mut cfg = VirtqCfg {
            device: self.device(),
        };
        let is_modern = cfg.version() != VIRTIO_VERSION_LEGACY;

        let queues = if is_modern {
            cfg.handshake()
                .modern(|low, high| self.negotiate(low, high))?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_SIZE as u32,
                })?
                .finish()
        } else {
            cfg.handshake()
                .legacy(|f| self.negotiate_legacy(f))?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_SIZE as u32,
                })?
                .finish()
        };

        *self.queues_mut() = Some(queues);
        Ok(())
    }

    /// 队列 0 的可变引用；未配置时先补一次握手。
    fn queue(&mut self) -> DevResult<&mut Virtq> {
        if self.queues_mut().is_none() {
            self.handshake()?;
        }
        self.queues_mut()
            .as_mut()
            .and_then(|queues| queues.first_mut())
            .ok_or(DevError::VirtioHandshakeFailed)
    }

    /// 通知设备处理队列 0。
    fn notify(&self) {
        // `QUEUE_NOTIFY_OFFSET` 由 `mmio_regs!` 根据 queue_notify 寄存器生成。
        self.device().mmio.write::<u32>(QUEUE_NOTIFY_OFFSET, 0);
        fence(Ordering::SeqCst);
    }
}
