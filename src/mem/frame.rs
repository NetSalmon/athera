use crate::dev::DEV_TREE;
use crate::locks::{LazyLock, SpinLock};
use crate::mem::buddy::BuddyAllocator;
use crate::{_end, debug};
use core::ptr;
use crate::mem::constants::PHY_PAGE_SIZE;

pub static FRAME_ALLOCATOR: LazyLock<SpinLock<BuddyAllocator>> = LazyLock::new(|| {
    let mut allocator = BuddyAllocator::new();
    let start = DEV_TREE.memory.device.mmio.start;
    let end = start + DEV_TREE.memory.device.mmio.size;

    let kernel_end = _end as *const () as usize;

    allocator.add(kernel_end..end);

    debug!("allocator init ok");
    SpinLock::new(allocator)
});

pub fn alloc_frame() -> Option<usize> {
    FRAME_ALLOCATOR.force().lock().alloc_frame(PHY_PAGE_SIZE)
}

pub fn dealloc_frame(addr: usize) {
    FRAME_ALLOCATOR.force().lock().dealloc_frame(addr, PHY_PAGE_SIZE);
}

pub struct AllocPage {
    start: usize,
    size: usize,
}

impl AllocPage {
    pub unsafe fn from_raw(start: usize, size: usize) -> Self {
        Self { start, size }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { &*ptr::slice_from_raw_parts(self.start as *const u8, self.size) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { &mut *ptr::slice_from_raw_parts_mut(self.start as *mut u8, self.size) }
    }
}

impl Drop for AllocPage {
    fn drop(&mut self) {
        FRAME_ALLOCATOR
            .force()
            .lock()
            .dealloc_frame(self.start, self.size);
    }
}
