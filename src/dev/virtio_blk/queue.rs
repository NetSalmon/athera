use crate::{bits, dev::virtio_blk::RING_MAX_SIZE};

#[unsafe(link_section = ".bss.queue")]
static mut QUEUE: Queue = queue_init();

#[inline]
const fn queue_init() -> Queue {
    let desc = VirtioDescTable::new();
    let avail = VirtioAvail::new();
    let used = VirtioUsed::new();

    Queue { desc, avail, used }
}

pub fn get_mut<'a>() -> &'a mut Queue {
    unsafe { &mut *get_queue_mut() }
}

pub fn get_queue_ptr() -> *const Queue {
    core::ptr::addr_of!(QUEUE)
}

pub fn get_queue_mut() -> *mut Queue {
    core::ptr::addr_of_mut!(QUEUE)
}

bits! {
    pub type Flags: u16 {
        next: 0,
        write: 1,
    }
}

#[repr(C, align(4096))]
#[derive(Debug)]
pub struct Queue {
    pub desc: VirtioDescTable,
    pub avail: VirtioAvail,
    pub used: VirtioUsed,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VirtioDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: Flags,
    pub next: u16,
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtioDescTable {
    pub data: [VirtioDesc; RING_MAX_SIZE],
}

impl VirtioDescTable {
    #[inline]
    pub const fn new() -> Self {
        let data = [VirtioDesc::new(); RING_MAX_SIZE];
        Self { data }
    }
}

impl VirtioDesc {
    #[inline]
    pub const fn new() -> VirtioDesc {
        VirtioDesc {
            addr: 0,
            len: 0,
            flags: Flags(0),
            next: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtioAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; RING_MAX_SIZE],
}

impl VirtioAvail {
    #[inline]
    pub const fn new() -> VirtioAvail {
        Self {
            flags: 0,
            idx: 0,
            ring: [0u16; RING_MAX_SIZE],
        }
    }
}

impl VirtioAvail {
    pub fn push_event(&mut self, desc_idx: u16) {
        self.ring[self.idx as usize % RING_MAX_SIZE] = desc_idx;
        self.idx += 1;
    }
}

#[repr(C)]
#[derive(Default, Copy, Clone, Debug)]
pub struct VirtioUsedElem {
    pub id: u32,
    pub len: u32,
}

impl VirtioUsedElem {
    #[inline]
    pub const fn new() -> Self {
        Self { id: 0, len: 0 }
    }
}

#[repr(C, align(4096))]
#[derive(Debug)]
pub struct VirtioUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtioUsedElem; RING_MAX_SIZE],
}

impl VirtioUsed {
    #[inline]
    pub const fn new() -> Self {
        Self {
            flags: 0,
            idx: 0,
            ring: [VirtioUsedElem::new(); RING_MAX_SIZE],
        }
    }
}
