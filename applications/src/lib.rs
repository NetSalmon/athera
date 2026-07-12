#![no_std]
#![no_main]

pub mod panic;
pub mod syscall;

unsafe extern "Rust" {
    fn main();
}

#[unsafe(no_mangle)]
pub fn _start() -> ! {
    unsafe {
        main();
    }

    syscall::exit(0);
}
