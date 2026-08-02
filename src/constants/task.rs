//! 任务常量。
//!
//! [`TID_MAX`] 为 TID 分配器的最大值（见 [`crate::proc::task`]）。
use novus_const::const_val;

#[const_val]
pub const TID_MAX: usize = 1024;
