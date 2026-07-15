#[const_val::const_val]
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