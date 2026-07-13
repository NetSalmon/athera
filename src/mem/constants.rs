#[const_val::const_val]
pub const PAGE_SIZE: usize = 4096;

#[inline]
pub fn ilog2_ceil(size: usize) -> usize {
    if size == 1 {
        0
    } else {
        (size - 1).ilog2() as usize + 1
    }
}
