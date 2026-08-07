#![allow(unused)]

//! 编译期常量汇总。
//!
//! 包含内存布局（[`memory`]）、链接器符号（[`symbols`]）、内嵌用户程序
//! ELF（[`elf`]）、任务（[`task`]）、版本（[`uname`]）与 virtio（[`virtio`]）
//! 常量，并在本模块统一 `pub use` 导出。
pub mod memory;
pub mod symbols;
pub mod task;
pub mod uname;
pub mod virtio;

pub use memory::*;
pub use symbols::*;
pub use task::*;
pub use uname::*;
pub use virtio::*;
