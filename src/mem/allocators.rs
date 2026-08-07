#![allow(dead_code)]
//! 分配器全局实例。
//!
//! - [`FRAME_ALLOCATOR`]：伙伴系统物理页分配器（懒加载 + 自旋锁保护）；
//! - [`CACHES`]：SLUB 全局分配器，经 [`global_allocator`] 注册，使
//!   `Vec` / `String` / `BTreeMap` 等 `alloc` 结构可用。
use core::alloc::{GlobalAlloc, Layout};

use athera_const::lazy;

use crate::{
    constants::{AVAIL_RANGE, PAGE_SIZE},
    debug,
    mem::{
        alloc_page::AllocPage,
        allocators::{buddy::BuddyAllocator, slub::Caches},
    },
    sync::{lazy::LazyLock, spin::SpinLock},
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

/// 从伙伴系统分配一个物理页帧；`size` 为 `None` 时使用默认页大小。
pub fn alloc_frame(size: Option<usize>) -> Option<AllocPage> {
    let size = size.unwrap_or(PAGE_SIZE);

    FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc_frame(size)
        .map(|start| AllocPage { start, size })
}

/// 归还一个物理页帧（解构 `AllocPage` 后直接归还，不触发 `Drop`）。
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
