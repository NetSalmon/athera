#![allow(dead_code)]
//! RISC-V 架构相关封装。
//!
//! - [`registers`]：CSR / 通用寄存器读写与位域值定义；
//! - [`sbi`]：SBI 调用封装（srst 复位/关机、hsm 停止等）。
use core::arch::asm;

pub(crate) mod registers;
pub(crate) mod sbi;

#[inline]
pub fn breakpoint() {
    unsafe {
        asm!("ebreak");
    }
}
