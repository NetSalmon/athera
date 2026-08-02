//! 内核同步原语。
//!
//! 本模块按实现拆分为四个子模块：
//!
//! - [`spin`]：关中断自旋锁 [`spin::SpinLock`]，用于保护被普通执行流与
//!   中断/异常处理共享的数据；
//! - [`once`]：一次性初始化原语 [`once::OnceLock`]，是懒加载的底层实现；
//! - [`lazy`]：懒加载静态 [`lazy::LazyLock`]，供 `novus_const` 的
//!   `#[lazy]` / `#[lazy(spin)]` 属性宏展开使用；
//! - [`per_cpu`]：每 hart（CPU）一份的存储 [`per_cpu::PerCpu`]。
//!
//! # 说明
//!
//! 内核当前为单核运行，因此 [`spin::SpinLock`] 通过“进入临界区前关闭
//! SIE 中断位、退出时恢复”的方式实现互斥，避免临界区被中断重入。

pub mod lazy;
pub mod once;
pub mod per_cpu;
pub mod spin;
