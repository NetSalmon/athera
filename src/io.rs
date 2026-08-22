#![allow(dead_code)]
//! 控制台输出层。
//!
//! 启动早期通过 SBI 提供 `print!` / `println!` 宏；VFS 初始化后切换到
//! `/dev/console`，设备访问由设备管理器负责。
use core::fmt;

use crate::{
    arch::riscv64::sbi::legacy,
    fs::{
        self, Path, VFS,
        vfs::{FileSystem, OpenFlags},
    },
};

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let _ = ConsoleWriter.write_fmt(args);
}

struct ConsoleWriter;

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if fs::VFS_CONSOLE_READY.load(core::sync::atomic::Ordering::Acquire) {
            let file = VFS
                .force()
                .open(
                    &Path::from("/dev/console"),
                    OpenFlags::write_only(),
                    fs::Mode::from(0),
                )
                .map_err(|_| fmt::Error)?;
            file.write(s.as_bytes()).map_err(|_| fmt::Error)?;
        } else {
            for byte in s.bytes() {
                legacy::console_putchar(byte);
            }
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
    if fs::VFS_CONSOLE_READY.load(core::sync::atomic::Ordering::Acquire) {
        let file = VFS
            .force()
            .open(
                &Path::from("/dev/console"),
                OpenFlags::read_only(),
                fs::Mode::from(0),
            )
            .ok()?;
        let mut byte = [0u8; 1];
        (file.read(&mut byte).ok()? == 1).then_some(byte[0])
    } else {
        legacy::console_getchar()
    }
}
