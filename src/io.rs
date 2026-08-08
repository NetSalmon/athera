#![allow(dead_code)]
//! 控制台输出层。
//!
//! 基于 `dev::UART`（ns16550a）提供 `print!` / `println!` 宏与字符读取；
//! `Ns16550a` 对 `core::fmt::Write` 的实现见 `dev::ns16550a`。
use core::fmt;

use crate::dev::{UART, traits::CharDevice};

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let _ = ConsoleWriter.write_fmt(args);
}

struct ConsoleWriter;

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for chunk in s.as_bytes().chunks(64) {
            let Some(uart) = UART.force().as_ref() else {
                return Err(fmt::Error);
            };
            uart.lock().write(chunk).map_err(|_| fmt::Error)?;
        }
        Ok(())
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
    let uart = UART.force().as_ref()?;
    let mut byte = [0u8; 1];
    (uart.lock().read(&mut byte).ok()? == 1).then_some(byte[0])
}
