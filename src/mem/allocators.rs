#![allow(dead_code)]
use core::alloc::{GlobalAlloc, Layout};

use novus_const::lazy;

use crate::{
    constants::{AVAIL_RANGE, PHY_PAGE_SIZE},
    debug,
    locks::{LazyLock, SpinLock},
    mem::{
        alloc_page::AllocPage,
        allocators::{buddy::BuddyAllocator, slub::Caches},
    },
};

pub mod buddy;
pub mod intrusive_list;
pub mod slub;

#[lazy(spin)]
pub static FRAME_ALLOCATOR: BuddyAllocator = {
    let mut allocator = BuddyAllocator::new();

    allocator.add(AVAIL_RANGE.force());

    debug!("allocator init ok");
    allocator
};

pub fn alloc_frame(size: Option<usize>) -> Option<AllocPage> {
    let size = size.unwrap_or(PHY_PAGE_SIZE);

    FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc_frame(size)
        .map(|start| AllocPage { start, size })
}

pub fn dealloc_frame(AllocPage { start, size }: AllocPage) {
    FRAME_ALLOCATOR.force().lock().dealloc_frame(start, size);
}

#[global_allocator]
#[lazy(spin)]
pub static CACHES: Caches = Caches::new();

unsafe impl GlobalAlloc for LazyLock<SpinLock<Caches>> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.force().lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.force().lock().dealloc(ptr, layout)
    }
}

pub fn caches_snapshot() {
    debug!("{:#?}", CACHES.force().lock().0);
}
