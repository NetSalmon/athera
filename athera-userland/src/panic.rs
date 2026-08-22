use core::panic::PanicInfo;

use crate::{println, syscall};

/// 用户程序 panic 时以 Rust 风格打印信息，然后通过 `exit` 系统调用退出。
#[panic_handler]
pub fn handle_panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "thread 'main' panicked at {}:{}:{}:",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        println!("thread 'main' panicked:");
    }

    println!("{}", info.message());

    println!("note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace");

    syscall::exit(1);
}
