pub mod addr;
pub mod buddy_system;
pub mod frame_allocator;
pub mod linked_list;
pub mod page_table;
pub mod slub;

#[const_val::const_val]
pub const PAGE_SIZE: usize = 4096;
