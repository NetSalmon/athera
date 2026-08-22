//! 用户程序的静态堆分配器。
//!
//! 这个分配器不向内核申请内存，而是从用户程序自己的 `.bss` 中切出一块
//! 固定大小的区域。它是 bump allocator：分配只会向前移动，整个进程的
//! 生命周期内不支持释放。

use core::{
    alloc::{GlobalAlloc, Layout},
    sync::atomic::{AtomicUsize, Ordering},
};

/// 用户程序堆的大小。修改这个常量即可在编译时调整每个用户程序的堆大小。
pub const HEAP_SIZE: usize = 64 * 1024;

const HEAP_ALIGNMENT: usize = 4096;

#[repr(align(4096))]
struct AlignedHeap([u8; HEAP_SIZE]);

static mut HEAP: AlignedHeap = AlignedHeap([0; HEAP_SIZE]);
static HEAP_OFFSET: AtomicUsize = AtomicUsize::new(0);

struct StaticAllocator;

#[global_allocator]
static GLOBAL_ALLOCATOR: StaticAllocator = StaticAllocator;

/// 从静态用户堆中分配 `size` 字节。
///
/// 返回的地址按 `u128` 对齐，适合放置普通 Rust 类型。返回空指针表示
/// 请求大小为零或静态堆空间不足。分配出的内存不会被自动初始化，也不能
/// 通过释放函数回收；调用者必须保证不越过请求的大小访问它。
pub fn allocate(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let layout = Layout::from_size_align(size, core::mem::align_of::<u128>()).unwrap();
    allocate_layout(layout)
}

fn allocate_layout(layout: Layout) -> *mut u8 {
    if layout.align() > HEAP_ALIGNMENT {
        return core::ptr::null_mut();
    }

    let allocation = HEAP_OFFSET.try_update(Ordering::Relaxed, Ordering::Relaxed, |offset| {
        let start = offset.checked_add(layout.align() - 1)? & !(layout.align() - 1);
        let end = start.checked_add(layout.size())?;
        (end <= HEAP_SIZE).then_some(end)
    });

    let start = match allocation {
        Ok(offset) => (offset + layout.align() - 1) & !(layout.align() - 1),
        Err(_) => return core::ptr::null_mut(),
    };

    // `HEAP` is never moved, and `start` was checked against `HEAP_SIZE` above.
    unsafe { core::ptr::addr_of_mut!(HEAP.0).cast::<u8>().add(start) }
}

unsafe impl GlobalAlloc for StaticAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = allocate_layout(layout);
        if ptr.is_null() {
            panic!("user heap exhausted");
        }
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // This bump allocator keeps allocations until the process exits.
    }
}
