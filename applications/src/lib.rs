#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

pub fn read(fd: u64, buf: &mut [u8]) -> isize {
    unsafe {
        asm!(
        "ecall",
        in("a7") 0u64,
        in("a0") fd,
        in("a1") buf.as_ptr(),
        in("a2") buf.len(),
        )
    }

    let ret: isize;
    unsafe {
        asm!(
        "mv {0}, a0", out(reg) ret,
        )
    }

    ret
}

pub fn write(fd: u64, buf: &mut [u8]) -> isize {
    unsafe {
        asm!(
            "ecall",
            in("a7") 1u64,
            in("a0") fd,
            in("a1") buf.as_ptr(),
            in("a2") buf.len(),
        )
    }

    let ret: isize;
    unsafe {
        asm!(
            "mv {0}, a0", out(reg) ret,
        )
    }

    ret
}

pub fn exit(code: u64) -> ! {
    unsafe {
        asm!(
            "ecall",
            in("a7") 0u64,
            in("a0") code,
        )
    }

    // 保底
    loop { core::hint::spin_loop() }
}

/// 如果`panic`直接退出程序
#[panic_handler]
pub fn panic_handle(_info: &PanicInfo) -> ! {
    exit(1);
}