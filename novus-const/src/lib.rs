#![no_std]
//! 编译期常量与属性宏的再导出。
//!
//! - [`const_val`]：带约束（min/max/multiple_of）的编译期常量，值可经
//!   字符串解析（`0x...` / `0b...` 等）；
//! - [`lazy`]：把静态展开为懒加载（`LazyLock`，可选 `spin` 内层锁）；
//! - [`spin`]：把静态展开为自旋锁包装；
//! - 同时再导出 `novus-const-macros` 的全部过程宏。

pub mod num;
pub use novus_const_macros::*;

