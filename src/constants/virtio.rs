#![allow(dead_code)]
//! virtio 常量：魔数、版本、环大小等。
use athera_macros::const_val;

pub const MAGIC_VALUE: u32 = 0x74726976;

pub const VIRTIO_VERSION_LEGACY: u32 = 1;

#[const_val(multiple_of = 2, max = 1024, min = 64)]
pub const RING_SIZE: usize = 256;

pub const RING_MAX_SIZE: usize = 32;

/// virtio-blk 请求类型：读（设备写入数据缓冲区）。
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// virtio-blk 请求类型：写（设备读取数据缓冲区）。
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// virtio-blk 请求完成状态：成功。
pub const VIRTIO_BLK_S_OK: u8 = 0;
/// 块设备扇区大小（字节）。
pub const SECTOR_SIZE: usize = 512;
