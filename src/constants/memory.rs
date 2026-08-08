#![allow(dead_code)]
//! 内存布局常量与懒加载范围。
//!
//! 由设备树解析得到 [`MEMORY_RANGE`]；[`AVAIL_RANGE`] 为内核末尾
//! `_end` 到 `FDT_ADDR` 之间的可用物理内存。另有页大小、伙伴/SLUB
//! 参数与用户栈布局常量。
use core::ops::Range;

use athera_macros::{const_val, lazy};
use fdt::Fdt;

use crate::FDT_ADDR;

#[const_val]
const DEFAULT_MEMORY_START: usize = 0x80000000;
#[const_val]
const DEFAULT_MEMORY_SIZE: usize = 0x8000000;
const DEFAULT_MEMORY_RANGE: Range<usize> =
    DEFAULT_MEMORY_START..DEFAULT_MEMORY_START + DEFAULT_MEMORY_SIZE;
#[lazy]
pub static MEMORY_RANGE: Range<usize> = {
    match unsafe { Fdt::from_ptr(FDT_ADDR) } {
        Ok(fdt) => {
            let memory = fdt.memory();

            let Some(reg) = memory.regions().next() else {
                return DEFAULT_MEMORY_RANGE;
            };
            let start = reg.starting_address as usize;
            let Some(size) = reg.size else {
                return DEFAULT_MEMORY_RANGE;
            };

            start..start + size
        }
        Err(_) => DEFAULT_MEMORY_RANGE,
    }
};

#[lazy]

pub static AVAIL_RANGE: Range<usize> =
    { crate::_end as *const () as usize..unsafe { FDT_ADDR as usize } };

#[const_val]
pub const PAGE_SIZE: usize = 4096;

#[inline]
pub const fn ilog2_ceil(size: usize) -> usize {
    if size == 1 {
        0
    } else {
        (size - 1).ilog2() as usize + 1
    }
}

#[inline]
pub fn align(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

#[const_val]
pub const BUDDY_MAX_ORDER: usize = 11;
pub const PAGE_SIZE_LOG_2: usize = ilog2_ceil(PAGE_SIZE);
#[const_val]
pub const MAX_KERNEL_HEAP_SIZE: usize = 20 * 1024 * 1024;
#[const_val(max = 12)]
pub const SLUB_MAX_ORDER: usize = 12;
#[const_val(min = 3)]
pub const SLUB_MIN_ORDER: usize = 4;
pub const CACHES_MAX: usize = SLUB_MAX_ORDER - SLUB_MIN_ORDER;

/// 每个 SLUB 页最少容纳的对象数，不足则尝试扩大页大小。
pub const SLUB_MIN_OBJECTS: usize = 4;
/// 单个 SLUB 页大小上限（1 MiB），超过则不再扩大。
pub const SLUB_MAX_PAGE_SIZE: usize = 1 << 20;

/// 单次恒等映射的块大小（8 MiB）。分块映射可以限制单次页表更新的工作量，
/// 也避免启动阶段一次性占用过多内存管理器资源。
pub const IDENTITY_MAP_CHUNK: usize = 8 * 1024 * 1024;

pub const USERLAND_OFFSET: usize = 0xffff_ffc0_0000_0000;

#[const_val(multiple_of = PAGE_SIZE)]
pub const USER_STACK_SIZE: usize = PAGE_SIZE * 8;
#[const_val(multiple_of = PAGE_SIZE)]
pub const USER_STACK_TOP: usize = 0x1_0000_0000;

pub const USER_STACK_LOWER_BOUND: usize = USER_STACK_TOP - USER_STACK_SIZE;

pub const PTE_NUMBER: usize = 512;
