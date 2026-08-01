#![allow(dead_code)]
use novus_const::const_val;

pub const MAGIC_VALUE: u32 = 0x74726976;

pub const VIRTIO_VERSION_LEGACY: u32 = 1;

#[const_val(multiple_of = 2, max = 1024, min = 64)]
pub const RING_SIZE: usize = 256;

pub const RING_MAX_SIZE: usize = 32;
