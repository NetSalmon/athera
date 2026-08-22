//! 标准输入输出。
//!
//! 通过 `core::fmt::Write` 将格式化输出交给 `syscall::write(1, …)`，
//! 并提供 `print!` / `println!` 宏（与 `std` 用法一致，但仅支持 fd 1）。

use core::fmt::{self, Write};

use crate::syscall;

/// 标准输出：对 `core::fmt::Write` 的实现，底层走 `write` 系统调用。
pub struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut buf = s.as_bytes();
        while !buf.is_empty() {
            let ret = syscall::write(1, buf);
            if ret <= 0 {
                return Err(fmt::Error);
            }
            buf = &buf[ret as usize..];
        }
        Ok(())
    }
}

/// 将格式化参数写入标准输出（宏内部入口）。
pub fn _print(args: fmt::Arguments<'_>) {
    Stdout.write_fmt(args).unwrap();
}

/// 输出到标准输出，不换行。
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::stdio::_print(::core::format_args!($($arg)*))
    };
}

/// 输出到标准输出并换行。
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", ::core::format_args!($($arg)*))
    };
}
