#![allow(dead_code)]
//! 控制台输出层。
//!
//! 基于 `dev::UART`（ns16550a）提供 `print!` / `println!` 宏与字符读取；
//! `Ns16550a` 对 `core::fmt::Write` 的实现见 `dev::ns16550a`。
use core::fmt;

use crate::dev::UART;

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    if let Some(uart) = UART.force().as_ref() {
        let _ = uart.lock().write_fmt(args);
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::io::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n"); };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)); };
}

pub fn getchar() -> Option<u8> {
    UART.force().as_ref()?.lock().getchar()
}
