#![allow(dead_code)]
use core::fmt;

use crate::dev::{UART, ns16550a::Ns16550a};

impl fmt::Write for Ns16550a {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            self.putchar(c);
        }
        Ok(())
    }
}

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
