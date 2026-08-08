//! 内存管理。
//!
//! - [`addr`]：Sv39 虚拟/物理地址的位域抽象与地址转换；
//! - [`frame`]：物理页帧句柄 [`frame::Frame`]（`Drop` 时归还）；
//! - [`allocators`]：物理页分配器（伙伴系统）与全局对象分配器（SLUB）；
//! - [`page_table`]：Sv39 页表、内核/用户地址空间与映射管理。
pub(crate) mod addr;
pub(crate) mod allocators;
pub(crate) mod frame;
pub(crate) mod page_table;
