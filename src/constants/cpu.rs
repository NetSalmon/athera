#![allow(dead_code)]
//! CPU 常量：编译期配置的最大 hart 数等。
use athera_macros::const_val;

/// 编译期配置的最大 hart 数。
#[const_val]
pub const MAX_CPU: usize = 1;
