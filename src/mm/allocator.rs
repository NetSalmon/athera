#![allow(dead_code)]
//! 分配器全局实例。
//!
//! - [`FRAME_ALLOCATOR`]：伙伴系统物理页分配器（懒加载 + 自旋锁保护）；
//! - [`CACHES`]：SLUB 全局分配器，经 [`global_allocator`] 注册，使
//!   `Vec` / `String` / `BTreeMap` 等 `alloc` 结构可用。
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};

use athera_macros::lazy;

use crate::{
    constants::{AVAIL_RANGE, PAGE_SIZE},
    debug,
    mm::{
        allocator::{
            buddy::{order_size, size_to_order, BuddyAllocator},
            slub::Caches,
        },
        frame::Frame,
    },
    sync::lazy::LazyLock,
};

pub(crate) mod buddy;
pub(crate) mod intrusive_list;
pub(crate) mod slub;

#[lazy(spin)]
pub static FRAME_ALLOCATOR: BuddyAllocator = {
    let mut allocator = BuddyAllocator::new();
    allocator.add(AVAIL_RANGE.force());
    debug!("allocator init ok");
    allocator
};

/// 从伙伴系统分配一个物理页帧；`size` 为 `None` 时使用默认页大小。
pub fn alloc_frame(size: Option<usize>) -> Option<Frame> {
    let size = size.unwrap_or(PAGE_SIZE);

    FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc_frame(size)
        .map(|start| {
            // 伙伴系统按 2 的幂阶分配，实际块大小是 `size` 向上取整到最近
            // 的阶（单块上限 `BUDDY_MAX_ORDER - 1`，超出时实际只有上限那么
            // 大）。`Frame.size` 必须反映真实块大小，否则调用方会按请求大小
            // 越界访问（如 `mmap` 对超过 4 MiB 的请求整段清零时写穿内存）。
            let actual = order_size(size_to_order(size));
            Frame { start, size: actual }
        })
}

/// 归还一个物理页帧（解构 `Frame` 后直接归还，不触发 `Drop`）。
pub fn dealloc_frame(Frame { start, size }: Frame) {
    FRAME_ALLOCATOR.force().lock().dealloc_frame(start, size);
}

/// 分配总计 `size` 字节的物理内存，可能由多个伙伴块拼接而成（单块不超过
/// 伙伴系统上限 [`buddy::BUDDY_MAX_ORDER`]，因此超过约 4 MiB 的需求会被
/// 拆成多段）。段的总大小保证不小于 `size`。
///
/// `size == 0` 时返回空 `Vec`；任一段分配失败返回 `None`，此前已分配的段
/// 随 `Vec` 的 `Drop` 自动归还伙伴系统，不留泄漏。
pub fn alloc_frames(size: usize) -> Option<Vec<Frame>> {
    let mut frames = Vec::new();
    let mut remaining = size;
    while remaining > 0 {
        let frame = alloc_frame(Some(remaining))?;
        // 单段可能是 `remaining` 向上取整的 2 的幂（可能略大于 remaining），
        // saturating_sub 兜底，保证循环终止。
        remaining = remaining.saturating_sub(frame.size);
        frames.push(frame);
    }
    Some(frames)
}

#[global_allocator]
#[lazy]
pub static CACHES: Caches = Caches::new();

unsafe impl GlobalAlloc for LazyLock<Caches> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.force().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.force().dealloc(ptr, layout);
    }
}

pub fn caches_snapshot() {
    CACHES.force().snapshot();
}
