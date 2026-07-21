use core::panic::PanicInfo;

use crate::syscall;

/// 如果`panic`直接退出程序
#[panic_handler]
pub fn panic_handle(_info: &PanicInfo) -> ! {
    syscall::exit(1);
}
