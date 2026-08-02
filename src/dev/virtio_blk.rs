#![allow(dead_code)]
//! virtio-blk 块设备驱动。
//!
//! 基于 virtio-mmio 传输层（见 [`super::virtio_mmio`]）完成设备探测、
//! 特征协商与队列配置，并通过 [`BlockDevice`] trait 提供按字节偏移的
//! 扇区读写。
use alloc::vec::Vec;
use core::{
    mem::size_of,
    ptr::{addr_of, addr_of_mut},
    sync::atomic::{Ordering, fence},
};

use fdt::Fdt;

use crate::{
    bits,
    constants::{RING_SIZE, VIRTIO_VERSION_LEGACY},
    dev::{
        abstracts::BlockDevice,
        device::{Device, Resource},
        virtio_mmio::{
            VirtqCfg,
            handshake::QueueConfig,
            queue::{Flags, VRingDesc, Virtq},
        },
    },
    error::{Error, Result},
};

/// virtio-blk 请求类型：读（设备写入数据缓冲区）。
const VIRTIO_BLK_T_IN: u32 = 0;
/// virtio-blk 请求类型：写（设备读取数据缓冲区）。
const VIRTIO_BLK_T_OUT: u32 = 1;
/// virtio-blk 请求完成状态：成功。
const VIRTIO_BLK_S_OK: u8 = 0;
/// 设备扇区大小（字节）。
const SECTOR_SIZE: usize = 512;
/// `queue_notify` 寄存器偏移（见 [`VirtqCfg`]）。
const QUEUE_NOTIFY_OFFSET: usize = 0x050;

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
                    size: RING_SIZE as u32,
                })?
                .finish()
        } else {
            cfg.handshake()
                .legacy(|_f| 0u32)?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_SIZE as u32,
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

    /// 通过 virtqueue 0 提交一次 virtio-blk 请求并等待完成。
    ///
    /// `data` 为数据缓冲区；`data_write` 为真表示设备向 `data` 写入
    /// （读请求），为假表示设备从 `data` 读取（写请求）。描述符 0/1/2
    /// 分别对应请求头、数据与状态字节，每次请求复用同一组描述符（同一
    /// 时刻最多一个在途请求）。
    fn submit(&mut self, req_type: u32, sector: u64, data: &[u8], data_write: bool) -> Result<()> {
        // 探测成功后队列尚未配置，先补一次握手，保证 BlockDevice 可直接使用。
        if self.queues.is_none() {
            self.handshake()?;
        }

        let req = VirtioBlkReq {
            r#type: req_type,
            reserved: 0,
            sector,
        };
        let mut status: u8 = !VIRTIO_BLK_S_OK;

        // 组装描述符链并把头描述符追加到 avail 环，记录设备当前 used 位置。
        let last_used = {
            let queue = self
                .queues
                .as_mut()
                .and_then(|queues| queues.first_mut())
                .ok_or(Error::VirtioHandshakeFailed)?
                .as_mut();

            let last_used = queue.used.idx;

            let mut flags = Flags::new();
            flags.set_next(true);
            queue.desc[0] = VRingDesc {
                addr: addr_of!(req) as u64,
                len: size_of::<VirtioBlkReq>() as u32,
                flags,
                next: 1,
            };

            let mut flags = Flags::new();
            flags.set_next(true);
            flags.set_write(data_write);
            queue.desc[1] = VRingDesc {
                addr: data.as_ptr() as u64,
                len: data.len() as u32,
                flags,
                next: 2,
            };

            let mut flags = Flags::new();
            flags.set_write(true);
            queue.desc[2] = VRingDesc {
                addr: addr_of_mut!(status) as u64,
                len: size_of::<u8>() as u32,
                flags,
                next: 0,
            };

            // 环索引按队列大小（RING_SIZE）取模，与设备视角一致。
            let slot = queue.avail.idx as usize % RING_SIZE;
            queue.avail.ring[slot] = 0;
            queue.avail.idx = queue.avail.idx.wrapping_add(1);

            // 保证描述符表与 avail 环的写入先于 notify 被设备看到。
            fence(Ordering::SeqCst);
            last_used
        };

        // 通知设备处理队列 0。
        self.device.mmio.write::<u32>(QUEUE_NOTIFY_OFFSET, 0);
        fence(Ordering::SeqCst);

        // 等待设备产生 used 元素，并校验被消费的描述符链头。
        loop {
            let queue = self
                .queues
                .as_mut()
                .and_then(|queues| queues.first_mut())
                .ok_or(Error::VirtioHandshakeFailed)?
                .as_mut();

            let used_idx = unsafe { addr_of!(queue.used.idx).read_volatile() };
            if used_idx != last_used {
                let slot = used_idx.wrapping_sub(1) as usize % RING_SIZE;
                let elem = unsafe { addr_of!(queue.used.ring[slot]).read_volatile() };
                if elem.id != 0 {
                    return Err(Error::VirtioBlockFailed);
                }
                break;
            }
            fence(Ordering::SeqCst);
            core::hint::spin_loop();
        }

        if unsafe { addr_of!(status).read_volatile() } != VIRTIO_BLK_S_OK {
            return Err(Error::VirtioBlockFailed);
        }

        Ok(())
    }
}

impl BlockDevice for VirtioBlk {
    type Error = Error;

    fn read_at(
        &mut self,
        buf: &mut [u8],
        offset: usize,
    ) -> core::result::Result<usize, Self::Error> {
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done;
            let sector = (pos / SECTOR_SIZE) as u64;
            let shift = pos % SECTOR_SIZE;

            let sector_buf = [0u8; SECTOR_SIZE];
            self.submit(VIRTIO_BLK_T_IN, sector, &sector_buf, true)?;

            let n = core::cmp::min(SECTOR_SIZE - shift, buf.len() - done);
            buf[done..done + n].copy_from_slice(&sector_buf[shift..shift + n]);
            done += n;
        }
        Ok(done)
    }

    fn write_at(&mut self, buf: &[u8], offset: usize) -> core::result::Result<usize, Self::Error> {
        let mut done = 0;
        while done < buf.len() {
            let pos = offset + done;
            let sector = (pos / SECTOR_SIZE) as u64;
            let shift = pos % SECTOR_SIZE;
            let n = core::cmp::min(SECTOR_SIZE - shift, buf.len() - done);

            if shift == 0 && n == SECTOR_SIZE {
                self.submit(VIRTIO_BLK_T_OUT, sector, &buf[done..done + n], false)?;
            } else {
                // 非整扇区写入先读回原扇区，再原地修改（read-modify-write）。
                let mut sector_buf = [0u8; SECTOR_SIZE];
                self.submit(VIRTIO_BLK_T_IN, sector, &sector_buf, true)?;
                sector_buf[shift..shift + n].copy_from_slice(&buf[done..done + n]);
                self.submit(VIRTIO_BLK_T_OUT, sector, &sector_buf, false)?;
            }
            done += n;
        }
        Ok(done)
    }
}

#[repr(C)]
#[derive(Default)]
struct VirtioBlkReq {
    r#type: u32,
    reserved: u32,
    sector: u64,
}
