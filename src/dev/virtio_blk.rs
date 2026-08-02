#![allow(dead_code)]
//! virtio-blk 块设备驱动。
//!
//! 基于 virtio-mmio 传输层（见 [`super::virtio_mmio`]）完成设备探测、
//! 特征协商与队列配置。
use alloc::vec::Vec;

use fdt::Fdt;

use crate::{
    bits,
    constants::{RING_MAX_SIZE, VIRTIO_VERSION_LEGACY},
    dev::{
        device::{Device, Resource},
        virtio_mmio::{
            handshake::QueueConfig,
            queue::Virtq,
            VirtqCfg,
        },
    },
    error::Result,
};

pub struct VirtioBlk {
    pub device: Device,
    pub queues: Option<Vec<Virtq>>,
}

bits! {
    pub type Status: u32 {
        acknowledge: 0,
        driver: 1,
        driver_ok: 2,
        features_ok: 3,
        failed: 7,
    }
}

bits! {
    pub type VirtioBlkFeaturesLow: u32 {
        geometry: 4,
        readonly: 6,
        scsi: 7,
        flush: 9,
        any_layout: 11,
        write_zeroes: 14,
        blk_size: 24,
        flush_cmd: 28,
        reserved_transport: 0 => 23,
        reserved_device: 24 => 31,
    }
}

bits! {
    pub type VirtioBlkFeaturesHigh: u32 {
        version_1: 0,
        access_platform: 1,
        ring_packed: 2,
        in_order: 3,
        order_platform: 4,
        sr_iov: 5,
        notification_data: 6,
        notif_config_data: 7,
        ring_reset: 8,
    }
}

impl VirtioBlk {
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let virtio = fdt.all_nodes().find(|node| {
            node.compatible()
                .map(|c| c.all().any(|c| c == "virtio,mmio"))
                .unwrap_or(false)
        })?;

        let reg = virtio.reg()?;
        let i = reg.into_iter().next()?;
        let start = i.starting_address as usize;
        let size = i.size.unwrap_or(0);
        let interrupts = virtio.interrupts()?;
        let irq = interrupts.into_iter().next()?;

        Some(VirtioBlk {
            device: Device {
                mmio: Resource::new(start, size),
                irq: Some(irq),
            },
            queues: None,
        })
    }

    pub fn handshake(&mut self) -> Result<()> {
        let mut cfg = VirtqCfg {
            device: self.device,
        };
        let is_modern = cfg.version() != VIRTIO_VERSION_LEGACY;

        let queues = if is_modern {
            cfg.handshake()
                .modern(|_low, high| if high & 1 != 0 { (0, 1) } else { (0, 0) })?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_MAX_SIZE as u32,
                })?
                .finish()
        } else {
            cfg.handshake()
                .legacy(|_f| 0u32)?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_MAX_SIZE as u32,
                })?
                .finish()
        };

        self.queues = Some(queues);
        Ok(())
    }

    pub fn from(dev: Device) -> VirtioBlk {
        VirtioBlk {
            device: dev,
            queues: None,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct VirtioBlkReq {
    type_: u32,
    reserved: u32,
    sector: u64,
}
