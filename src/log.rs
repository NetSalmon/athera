use core::{
    fmt,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Level {
    #[allow(dead_code)]
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

#[inline]
pub fn enabled(level: Level) -> bool {
    level as u8 <= LEVEL.load(Ordering::Relaxed)
}

fn color(level: Level) -> &'static str {
    match level {
        Level::Trace => "\x1b[90m",
        Level::Debug => "\x1b[32m",
        Level::Info => "\x1b[33m",
        Level::Warn => "\x1b[35m",
        Level::Error => "\x1b[31m",
        Level::Off => "",
    }
}

fn label(level: Level) -> &'static str {
    match level {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Off => "off",
    }
}

pub fn log(level: Level, module: &str, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    crate::print!(
        "{}[{} {}]\x1b[0m {}\n",
        color(level),
        label(level),
        module,
        args
    );
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Trace) {
            $crate::log::log($crate::log::Level::Trace, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Debug) {
            $crate::log::log($crate::log::Level::Debug, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Info) {
            $crate::log::log($crate::log::Level::Info, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Warn) {
            $crate::log::log($crate::log::Level::Warn, module_path!(), format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        if $crate::log::enabled($crate::log::Level::Error) {
            $crate::log::log($crate::log::Level::Error, module_path!(), format_args!($($arg)*));
        }
    };
}
