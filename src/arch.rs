use core::arch::asm;

#[allow(unused)]
pub mod registers;
#[allow(unused)]
pub mod sbi;

#[inline]
pub fn breakpoint() {
    unsafe {
        asm!("ebreak");
    }
}
