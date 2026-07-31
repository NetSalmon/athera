use core::alloc::Layout;

use crate::{
    bits,
    error::{Error, Result},
    mem::{alloc_page::AllocPage, allocators::FRAME_ALLOCATOR},
};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
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
#[derive(Debug, Copy, Clone)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: VirtqRing<u16>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
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

#[repr(C, align(4096))]
pub struct Queue {
    pub desc: [VRingDesc; RING_SIZE],
    pub avail: VirtqAvail,
    pub used: VirtqUsed,
}

impl Queue {
    pub fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

pub struct Virtq {
    _mem: AllocPage,
}

impl Virtq {
    pub fn new() -> Result<Self> {
        let layout = Layout::new::<Queue>();
        let start = FRAME_ALLOCATOR
            .force()
            .lock()
            .alloc_frame(layout.size())
            .ok_or(Error::OutOfMemory)?;
        unsafe {
            core::ptr::write_bytes(start as *mut u8, 0, layout.size());
        }
        Ok(Virtq {
            _mem: AllocPage {
                start,
                size: layout.size(),
            },
        })
    }

    pub fn desc_addr(&self) -> u64 {
        self._mem.start as u64
    }

    pub fn avail_addr(&self) -> u64 {
        self._mem.start as u64 + core::mem::offset_of!(Queue, avail) as u64
    }

    pub fn used_addr(&self) -> u64 {
        self._mem.start as u64 + core::mem::offset_of!(Queue, used) as u64
    }

    pub fn queue_ptr(&self) -> u64 {
        self._mem.start as u64
    }

    pub fn as_mut(&mut self) -> &mut Queue {
        unsafe { &mut *(self._mem.start as *mut Queue) }
    }
}

#[novus_const::const_val(multiple_of = 2, max = 1024, min = 64)]
pub const RING_SIZE: usize = 256;

pub type VirtqRing<T> = [T; RING_SIZE];
