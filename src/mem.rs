pub mod addr;
pub mod buddy_system;
pub mod frame_allocator;
pub mod linked_list;
pub mod page_table;
pub mod slub;

#[const_val::const_val]
pub const PAGE_SIZE: usize = 4096;

pub fn align_up(bytes: usize) -> usize {
    bytes.div_ceil(PAGE_SIZE)
}

pub fn bytes_to_order(bytes: usize) -> usize {
    bytes.leading_zeros() as usize
}
