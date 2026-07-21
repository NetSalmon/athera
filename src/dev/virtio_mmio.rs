pub mod handshake;
pub mod queue;

use crate::{bits, dev::Device, mmio_regs, numeric};

pub const MAGIC_VALUE: u32 = 0x74726976;

pub struct VirtqCfg {
    device: Device,
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
        MORDEN = 2,
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
