//! 分级日志。
//!
//! 日志级别 [`Level`]（OFF..TRACE），全局级别经原子变量保存，
//! 由 `trace!` / `debug!` / `info!` / `warn!` / `error!` 宏按
//! `module_path!()` 输出带颜色的行。
use core::{
    fmt,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::numeric;

numeric! {
    pub enum Level: u8 {
        OFF = 0,
        ERROR = 1,
        WARN = 2,
        INFO = 3,
        DEBUG = 4,
        TRACE = 5,
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::INFO.0);

pub fn set_level(level: Level) {
    LEVEL.store(level.0, Ordering::Relaxed);
}

#[inline]
pub fn enabled(level: Level) -> bool {
    level.0 <= LEVEL.load(Ordering::Relaxed)
}

fn color(level: Level) -> &'static str {
    match level {
        Level::TRACE => "\x1b[90m",
        Level::DEBUG => "\x1b[32m",
        Level::INFO => "\x1b[33m",
        Level::WARN => "\x1b[35m",
        Level::ERROR => "\x1b[31m",
        Level::OFF => "",
        _ => "",
    }
}

fn label(level: Level) -> &'static str {
    match level {
        Level::TRACE => "TRACE",
        Level::DEBUG => "DEBUG",
        Level::INFO => "INFO",
        Level::WARN => "WARN",
        Level::ERROR => "ERROR",
        Level::OFF => "OFF",
        _ => "?",
    }
}

/// Show only the last segment of a `module_path!()` (e.g. `athera::dev::virtio_blk`
/// becomes `virtio_blk`) so log lines stay short and aligned.
fn short_module(module: &str) -> &str {
    match module.rsplit_once("::") {
        Some((_, last)) => last,
        None => module,
    }
}

pub fn log(level: Level, module: &str, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    crate::print!(
        "{}[{: <5} {}]\x1b[0m {}\n",
        color(level),
        label(level),
        short_module(module),
        args
    );
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::TRACE) {
            $crate::log::log($crate::log::Level::TRACE, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::DEBUG) {
            $crate::log::log($crate::log::Level::DEBUG, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::INFO) {
            $crate::log::log($crate::log::Level::INFO, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::WARN) {
            $crate::log::log($crate::log::Level::WARN, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::ERROR) {
            $crate::log::log($crate::log::Level::ERROR, module_path!(), format_args!($($arg)*));
        }
    };
}
