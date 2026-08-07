#![no_std]
#![no_main]
//! 用户程序运行库。
//!
//! `_start` 是用户程序入口（链接脚本指定）：调用各 bin 的 `main` 后
//! 通过 `exit` 系统调用结束；`syscall` / `panic` 模块提供用户态系统
//! 调用封装与 panic 处理。

pub mod panic;
pub mod stdio;
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
