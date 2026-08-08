#![no_std]
//! 内核属性 / 派生宏与编译期常量的再导出。
//!
//! - [`const_val`]：带约束（min/max/multiple_of）的编译期常量，值可经
//!   字符串解析（整数 `0x...` / `0b...` 等，布尔 `true`/`false`）；
//! - [`lazy`]：把静态展开为懒加载（`LazyLock`，可选 `spin` 内层锁）；
//! - [`spin`]：把静态展开为自旋锁包装；
//! - [`Id`]：为单字段结构体自动实现 `athera_id_alloc::Id` 的派生宏；
//! - 同时再导出 `athera-macros-impl` 的全部过程宏。

pub mod num;
pub use athera_macros_impl::*;
