//! 用户程序的堆分配器：talc + mmap / mremap / munmap。
//!
//! 分配器核心使用 [`talc`]（no_std、无 libc 依赖）管理空闲块、相邻合并与
//! 原地扩容；内存来源由自定义 [`MmapSource`] 提供，全部经内核的 `mmap` /
//! `mremap` / `munmap` 系统调用按需获取与归还，**不再像旧实现那样在 `.bss`
//! 里预留固定大小的堆**——第一块堆内存也要到第一次分配时才经 `mmap` 获得。
//!
//! 三个系统调用各自的分工：
//!
//! - `mmap`：堆内存耗尽（`acquire`）时获取一块新区域，交给 talc 建立新堆；
//! - `mremap`：a) 最近创建的区域上方虚拟区间空闲时，原地扩大该区域，避免
//!   新建整块区域（`acquire`）；b) 区域只有尾部空闲时，收缩映射把尾部页面
//!   归还内核（`resize`）；
//! - `munmap`：区域整块空闲（`resize` 的 `is_heap_base`）时整块归还内核。
//!
//! # 映射尺寸约定
//!
//! 内核伙伴系统按 2 的幂分配物理帧（单段上限 [`MAX_MAP_SIZE`]，4 MiB；
//! 超过的映射由多段拼接，段总大小等于请求的 2 的幂），且 `mmap` /
//! `mremap` 只返回起始地址、不回报实际映射大小。为了精确追踪每块区域的
//! 实际大小（`munmap` / `mremap` 都要求精确尺寸），本分配器把所有映射
//! 尺寸取为 **2 的幂**（1 MiB 起，可超过 4 MiB）：向 `mmap` 请求 2 的幂
//! 大小，内核必定返回总大小相同的映射（多段拼接），`mremap` 扩/缩也以
//! 2 的幂为目标，于是 [`Region`] 里的 `map_size` 永远等于内核映射的真实
//! 总大小，归还时不留未追踪的映射尾段。
//!
//! # 约束与不变量
//!
//! - [`MmapSource`] 内不得调用任何可能分配内存的代码（`Vec`、`println!`
//!   等），否则会与全局分配器死锁或破坏内部状态；
//! - talc 的分配元数据内嵌在首块堆区域里，因此首块区域在进程生命周期内
//!   不会被归还（talc 不给首堆打 `HEAP_BASE` 标记，`resize` 永远不会收到
//!   它的 `is_heap_base = true`，只会被收缩到一页左右）；
//! - 每块区域的头部 [`Region`] 位于该区域自己的起始地址，通过 `next` 串成
//!   链表，链表头在 [`MmapSource`] 里（永不悬垂）；先摘链、后 `munmap`；
//! - 手动调用 `Talc::claim` / `extend` / `truncate` 会破坏本 Source 维护的
//!   区域链表与堆末端视图，请不要与全局分配器混用。

use core::{
    alloc::{GlobalAlloc, Layout},
    mem::size_of,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use spinning_top::RawSpinlock;
use talc::{
    TalcLock,
    base::{CHUNK_UNIT, Talc, binning::Binning},
    source::Source,
};

use crate::syscall;

/// 页大小（与内核 `PAGE_SIZE` 一致）。
const PAGE_SIZE: usize = 4096;
/// 新 `mmap` 区域的最小尺寸（1 MiB，可按需调整；须为 2 的幂）。
const BLOCK_SIZE: usize = 1 << 20;
/// `mremap` 原地扩大的目标上限（启发式）：内核重建映射是复制式的，过大的
/// 原地扩大代价高，超过此值直接 `mmap` 新块。单次分配本身不再受 4 MiB
/// 限制——内核会把超过伙伴单段上限的映射拆成多段拼接。
const MAX_MAP_SIZE: usize = 4 * 1024 * 1024;
/// 每块区域头部 [`Region`] 的字节数。
const HEADER_SIZE: usize = size_of::<Region>();

/// 向上取整到 `align` 的整数倍（`align` 须为 2 的幂）。
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// 一块由 `mmap` 得到的连续区域。
///
/// 头部写在区域自己的起始地址（`base` 页对齐）；通过 `next` 串成链表，链表
/// 头在 [`MmapSource`] 里。只有区域仍被映射时它的头部才有效，因此归还
/// （`munmap`）前必须先摘链。
///
/// `map_size` 是内核映射的真实大小（2 的幂，见模块文档），`heap_end` 是
/// talc 当前把这块区域当作堆末端的位置（`base < heap_end <= base + map_size`，
/// 两者之差为尚未交给 talc 的映射尾段）。
#[repr(C)]
#[derive(Debug)]
struct Region {
    /// 链表中的下一块区域。
    next: *mut Region,
    /// mmap 起始虚拟地址（页对齐）。
    base: usize,
    /// 内核映射的总长度（2 的幂，页的整数倍）。
    map_size: usize,
    /// talc 的堆末端（当前真实值，恒等于 talc 视图中的 chunk_end）。
    heap_end: usize,
}

/// 向 talc 提供 / 回收内存的 [`Source`]：全部经 mmap / mremap / munmap。
#[derive(Debug)]
pub struct MmapSource {
    /// 区域链表头。最近创建的区域在链表头部，也是 `acquire` 里 `mremap`
    /// 原地扩大的候选区域。
    head: *mut Region,
}

// SAFETY: `MmapSource` 的所有可变访问都发生在 `TalcLock` 的自旋锁内；区域
// 头部所在的内存由本 Source 独占管理（mmap 得到、munmap 归还），且 fork
// 时整个地址空间（含区域头与链表）被深拷贝到子进程，父子互不共享。
unsafe impl Send for MmapSource {}
// SAFETY: 同上；全局分配器通过 `TalcLock` 串行化所有访问。
unsafe impl Sync for MmapSource {}

impl MmapSource {
    const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
        }
    }

    /// 按堆末端查找区域，返回区域指针与指向链入它的字段的指针（用于摘链）。
    /// 找不到时区域指针为 null。
    fn find_region(&mut self, chunk_end: usize) -> (*mut Region, *mut *mut Region) {
        let mut link = &mut self.head as *mut *mut Region;
        loop {
            let current = unsafe { *link };
            if current.is_null() {
                return (core::ptr::null_mut(), link);
            }
            let region = unsafe { &mut *current };
            if region.heap_end == chunk_end {
                return (current, link);
            }
            link = &mut region.next as *mut *mut Region;
        }
    }
}

unsafe impl Source for MmapSource {
    const TRACK_HEAP_END: bool = true;

    fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
        // 需要的新内存量 = 请求大小 + 对齐与元数据开销，向上取 2 的幂且
        // 不小于 BLOCK_SIZE；第一次还要包含 talc 内嵌元数据的大小。
        let mut required = layout
            .size()
            .saturating_add(layout.align())
            .saturating_add(2 * CHUNK_UNIT)
            .saturating_add(HEADER_SIZE);
        if !talc.is_metadata_established() {
            required = required.saturating_add(talc::min_first_heap_size::<B>());
        }
        let Some(needed) = required.max(BLOCK_SIZE).checked_next_power_of_two() else {
            return Err(());
        };

        // 1) 优先尝试用 mremap 把最近创建的区域原地扩大（其上方虚拟区间
        //    空闲时，内核才会原地扩展并返回原地址）。目标也取 2 的幂，且
        //    不得超过内核单块上限，否则回退到 mmap 新块。
        let head = talc.source.head;
        if !head.is_null() {
            let region = unsafe { &mut *head };
            let Some(target) = (region.heap_end - region.base)
                .saturating_add(needed)
                .checked_next_power_of_two()
            else {
                return Err(());
            };
            if target > region.map_size && target <= MAX_MAP_SIZE {
                let ret = syscall::mremap(region.base, region.map_size, target, 0, 0);
                if ret as usize == region.base {
                    let old_end =
                        NonNull::new(region.heap_end as *mut u8).expect("heap end is never null");
                    // 把新增的尾段交给 talc 扩展当前堆。
                    let new_end =
                        unsafe { talc.extend(old_end, (region.heap_end + needed) as *mut u8) };
                    debug_assert!(new_end.as_ptr() > old_end.as_ptr());
                    region.heap_end = new_end.as_ptr() as usize;
                    region.map_size = target;
                    STAT_GROWS.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
            }
        }

        // 2) mmap 一块新区域（2 的幂大小，内核伙伴系统按阶返回同尺寸映射）。
        let ret = syscall::mmap(
            0,
            needed,
            syscall::PROT_READ | syscall::PROT_WRITE,
            syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS,
            -1,
            0,
        );
        if ret < 0 {
            return Err(());
        }
        let base = ret as usize;
        STAT_MMAPS.fetch_add(1, Ordering::Relaxed);

        // 3) 写入区域头部并链入链表，再把 [base + HEADER_SIZE, base + needed)
        //    交给 talc 建立新堆。
        let region_ptr = base as *mut Region;
        unsafe {
            region_ptr.write(Region {
                next: talc.source.head,
                base,
                map_size: needed,
                heap_end: base,
            });
        }
        talc.source.head = region_ptr;

        let claim = unsafe { talc.claim((base + HEADER_SIZE) as *mut u8, needed - HEADER_SIZE) };
        match claim {
            Some(heap_end) => {
                unsafe { (*region_ptr).heap_end = heap_end.as_ptr() as usize };
                Ok(())
            }
            None => {
                // 理论上不会发生（needed 已含元数据开销且页对齐）；为安全起见
                // 摘链并归还，保持状态一致，返回失败以免 talc 无限循环。
                unsafe {
                    talc.source.head = (*region_ptr).next;
                }
                let _ = syscall::munmap(base, needed);
                Err(())
            }
        }
    }

    unsafe fn resize(
        &mut self,
        chunk_base: *mut u8,
        heap_end: *mut u8,
        is_heap_base: bool,
    ) -> *mut u8 {
        let chunk_base = chunk_base as usize;
        let chunk_end = heap_end as usize;

        let (region_ptr, link) = self.find_region(chunk_end);
        if region_ptr.is_null() {
            // 防御：找不到对应区域（理论上不会发生），保持现状。
            return heap_end;
        }
        let region = unsafe { &mut *region_ptr };

        if is_heap_base {
            // 整块区域空闲：摘链后 munmap 整块归还内核。
            let next = region.next;
            unsafe {
                *link = next;
            }
            let (base, map_size) = (region.base, region.map_size);
            let _ = syscall::munmap(base, map_size);
            STAT_MUNMAPS.fetch_add(1, Ordering::Relaxed);
            return chunk_base as *mut u8;
        }

        // 仅尾部空闲：若空闲尾部超过一页，尝试用 mremap 把映射收缩到覆盖
        // 保留区所需的最小 2 的幂，把尾部页面归还内核。
        if chunk_end - chunk_base <= PAGE_SIZE {
            return heap_end;
        }
        let reserved = chunk_base - region.base;
        let Some(new_map) = align_up(reserved, PAGE_SIZE).checked_next_power_of_two() else {
            return heap_end;
        };
        if new_map >= region.map_size {
            return heap_end;
        }
        let ret = syscall::mremap(region.base, region.map_size, new_map, 0, 0);
        if ret as usize == region.base {
            region.map_size = new_map;
            region.heap_end = region.base + new_map;
            STAT_SHRINKS.fetch_add(1, Ordering::Relaxed);
            return (region.base + new_map) as *mut u8;
        }
        heap_end
    }
}

/// 全局堆分配器：talc（自旋锁保护）+ mmap 内存来源。
#[global_allocator]
static ALLOCATOR: TalcLock<RawSpinlock, MmapSource> = TalcLock::new(MmapSource::new());

/// 分配器统计（供测试与调试，与内核交互的计数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// `mmap` 调用次数（获取新内存块）。
    pub mmaps: usize,
    /// `mremap` 原地扩大调用次数。
    pub mremap_grows: usize,
    /// `mremap` 收缩调用次数。
    pub mremap_shrinks: usize,
    /// `munmap` 调用次数（整块归还内核）。
    pub munmaps: usize,
}

static STAT_MMAPS: AtomicUsize = AtomicUsize::new(0);
static STAT_GROWS: AtomicUsize = AtomicUsize::new(0);
static STAT_SHRINKS: AtomicUsize = AtomicUsize::new(0);
static STAT_MUNMAPS: AtomicUsize = AtomicUsize::new(0);

/// 读取分配器统计。
pub fn stats() -> Stats {
    Stats {
        mmaps: STAT_MMAPS.load(Ordering::Relaxed),
        mremap_grows: STAT_GROWS.load(Ordering::Relaxed),
        mremap_shrinks: STAT_SHRINKS.load(Ordering::Relaxed),
        munmaps: STAT_MUNMAPS.load(Ordering::Relaxed),
    }
}

/// 分配 `size` 字节，返回按 `u128` 对齐的地址。
///
/// 返回空指针表示请求大小为零或内存不足（不会 panic）。分配出的内存不会
/// 自动初始化，释放时须调用 [`deallocate`] 并传回相同的 `size`。
///
/// 内核按需把大映射拆成多个物理段拼接，单次分配的上限取决于可用物理内存。
pub fn allocate(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let layout = Layout::from_size_align(size, core::mem::align_of::<u128>()).unwrap();
    unsafe { ALLOCATOR.alloc(layout) }
}

/// 释放 [`allocate`] 分配的 `size` 字节。
///
/// # Safety
///
/// `ptr` 必须由 [`allocate`]（或等价的 `ALLOCATOR.alloc`）返回，且 `size`
/// 必须与分配时传入的大小一致；二者不满足时行为未定义。传入 `null` 或
/// `size == 0` 是安全无操作。
pub unsafe fn deallocate(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }

    let layout = Layout::from_size_align(size, core::mem::align_of::<u128>()).unwrap();
    // SAFETY: 调用方按本函数的安全约定保证 `ptr` 有效且 `size` 与分配一致。
    unsafe { ALLOCATOR.dealloc(ptr, layout) }
}
