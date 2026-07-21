use crate::bits;

#[repr(C, align(4096))]
#[derive(Debug)]
pub struct Queue {
    pub desc: VRingDesc,
    pub avail: VirtqAvail,
    pub used: VirtqUsed,
}

#[novus_const::const_val(multiple_of = 2, max = 1024, min = 64)]
pub const RING_SIZE: usize = 256;

pub type VirtqRing<T> = [T; RING_SIZE];

#[repr(C)]
#[derive(Debug)]
pub struct VRingDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: Flags,
    pub next: u16,
}

bits! {
    pub type Flags : u16 {
        next: 0,
        write: 1,
        indirect: 2,
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: VirtqRing<u16>,
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: VirtqRing<VirtqUsedElem>,
}
