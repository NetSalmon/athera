#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

#[allow(unused)]
const EINVAL: isize = -22;
#[allow(unused)]
const EIO: isize = -5;
#[allow(unused)]
const ENOSYS: isize = -38;

const READ : u64 = 63;
const WRITE: u64  = 64;
const EXIT : u64 = 93;


pub fn read(fd: u64, buf: &mut [u8]) -> isize {
    unsafe {
        asm!(
        "ecall",
        in("a7") READ,
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
            in("a7") WRITE,
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
            in("a7") EXIT,
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