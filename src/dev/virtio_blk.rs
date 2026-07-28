use alloc::vec::Vec;
use core::{ptr::addr_of, sync::atomic::Ordering};

use fdt::Fdt;

use crate::{
    bits, debug,
    dev::{
        Device, Resource,
        virtio_mmio::{
            VirtqCfg,
            handshake::QueueConfig,
            queue::{Flags, VRingDesc, Virtq},
        },
    },
    error::Error,
    mmio_regs, print, println,
};

pub struct VirtioBlk {
    pub device: Device,
    pub queues: Option<Vec<Virtq>>,
}

mmio_regs! {
    VirtioBlk: [
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

const MAGIC_VALUE: u32 = 0x74726976;

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

const VIRTIO_VERSION_LEGACY: u32 = 1;

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

    pub fn handshake(&mut self) -> Result<(), Error> {
        let mut cfg = VirtqCfg {
            device: self.device,
        };
        let hs = cfg.handshake(
            |_f| 0u32,
            |_low, high| {
                if high & 1 != 0 { (0, 1) } else { (0, 0) }
            },
        )?;

        let ready = hs.setup_queues(&[QueueConfig {
            index: 0,
            size: RING_MAX_SIZE as u32,
        }])?;
        self.queues = Some(ready.finish());
        Ok(())
    }

    pub fn from(dev: Device) -> VirtioBlk {
        VirtioBlk {
            device: dev,
            queues: None,
        }
    }

    pub fn print_info(&mut self) -> Result<(), Error> {
        let start = self.device.mmio.start;
        let size = self.device.mmio.size;

        let magic_value = self.magic_value();
        let is_virtio_mmio = magic_value == MAGIC_VALUE;

        let version = self.version();

        debug!(
            "version: {} - {}",
            version,
            if version == 1 { "legacy" } else { "modern" }
        );

        let device_id = self.device_id();

        if is_virtio_mmio && device_id == 2 {
            self.handshake()?;
            debug!("handshake success");
        } else {
            return Err(Error::VirtioNotSupported);
        }

        debug!("start print");
        debug!("start: {}", start);
        debug!("size: {}", size);
        debug!("magic value: {}", magic_value);
        debug!("version: {}", version);
        debug!("device_id: {}", device_id);
        debug!("is virtio_mmio: {}", is_virtio_mmio);

        Ok(())
    }

    pub unsafe fn test_read(&mut self) {
        const VIRTIO_BLK_T_GET_ID: u32 = 8;
        const NEXT: Flags = Flags::from(1);

        static mut DISK_REQ: VirtioBlkReq = VirtioBlkReq {
            type_: 0,
            reserved: 0,
            sector: 0,
        };

        let req_addr = core::ptr::addr_of_mut!(DISK_REQ) as u64;
        static mut DISK_BUF: [u8; 512] = [0u8; 512];
        let buf_addr = core::ptr::addr_of_mut!(DISK_BUF) as u64;
        static mut DISK_STATUS: u8 = 0;
        let status_addr = core::ptr::addr_of_mut!(DISK_STATUS) as u64;

        unsafe {
            core::ptr::write(
                core::ptr::addr_of_mut!(DISK_REQ),
                VirtioBlkReq {
                    type_: VIRTIO_BLK_T_GET_ID,
                    reserved: 0,
                    sector: 0,
                },
            );
        }

        let last_used;
        let queue_ptr;

        {
            let queues = self.queues.as_mut().unwrap();
            let queue = queues[0].as_mut();

            last_used = queue.used.idx;

            queue.desc[0] = VRingDesc {
                addr: req_addr,
                len: size_of::<VirtioBlkReq>() as u32,
                flags: NEXT,
                next: 1,
            };

            queue.desc[1] = VRingDesc {
                addr: buf_addr,
                len: 512,
                flags: 3.into(),
                next: 2,
            };

            queue.desc[2] = VRingDesc {
                addr: status_addr,
                len: 1,
                flags: 2.into(),
                next: 0,
            };

            queue.avail.ring[0] = 0;
            queue.avail.idx = 2;

            queue_ptr = queue as *const _ as usize;
        }

        core::sync::atomic::fence(Ordering::SeqCst);

        debug!("BEFORE NOTIFY: avail_idx queue @ 0x{:x}", queue_ptr,);

        self.write_queue_notify(0);
        core::sync::atomic::fence(Ordering::SeqCst);
        debug!("AFTER NOTIFY");

        let mut guard = 0usize;
        {
            let queues = self.queues.as_mut().unwrap();
            let queue = queues[0].as_mut();

            let idx_ptr = &queue.used.idx as *const u16;

            while unsafe { idx_ptr.read_volatile() } == last_used {
                core::sync::atomic::fence(Ordering::SeqCst);
                guard += 1;
                if guard.is_multiple_of(100000000) {
                    debug!(
                        "polling used: idx={} last={} guard={}",
                        queue.used.idx, last_used, guard
                    );
                }
            }
        }

        debug!("DONE polling, guard={}", guard);

        let buf_slice =
            unsafe { core::slice::from_raw_parts(addr_of!(DISK_BUF) as *const u8, 512) };

        debug!("buffer: {:?}", buf_slice);

        for i in buf_slice.iter() {
            print!("{}", *i as char);
        }

        println!();
    }
}

const RING_MAX_SIZE: usize = 32;

#[repr(C)]
#[derive(Default)]
struct VirtioBlkReq {
    type_: u32,
    reserved: u32,
    sector: u64,
}
