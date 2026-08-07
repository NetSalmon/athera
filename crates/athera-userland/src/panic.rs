use core::panic::PanicInfo;

use crate::syscall;

/// 用户程序 panic 时直接通过 `exit` 系统调用退出。
#[panic_handler]
pub fn panic_handle(_info: &PanicInfo) -> ! {
    syscall::exit(1);
}
