use crate::dev::DEV_TREE;
use crate::locks::{LazyLock, SpinLock};
use crate::mem::buddy_system::BuddyAllocator;
use crate::{_end, debug};

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
    FRAME_ALLOCATOR.force().lock().alloc(0)
}

pub fn dealloc_frame(addr: usize) {
    FRAME_ALLOCATOR.force().lock().dealloc(addr, 0);
}
