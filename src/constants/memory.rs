use core::ops::Range;

use fdt::Fdt;
use novus_const::{const_val, lazy};

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

pub const PHY_PAGE_SIZE: usize = 4096;

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
