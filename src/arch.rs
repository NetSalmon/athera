#![allow(dead_code)]
use core::arch::asm;

pub mod registers;
pub mod sbi;

#[inline]
pub fn breakpoint() {
    unsafe {
        asm!("ebreak");
    }
}
