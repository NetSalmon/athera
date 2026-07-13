use crate::dev::DEV_TREE;
use crate::dev::ns16550a::Ns16550a;
use core::fmt;

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
    if let Some(uart) = DEV_TREE.force().ns16550a.as_ref() {
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
    DEV_TREE.force().ns16550a.as_ref()?.lock().getchar()
}
