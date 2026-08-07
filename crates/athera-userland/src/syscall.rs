//! 用户态系统调用封装。
//!
//! 定义 `ecall!` 宏与 read / write / exit / reboot 的 RISC-V 调用约定
//! 封装（系统调用号放 `a7`，返回值经 `a0` 传回）。
#[allow(unused)]
const EINVAL: isize = -22;
#[allow(unused)]
const EIO: isize = -5;
#[allow(unused)]
const ENOSYS: isize = -38;
const READ: u64 = 63;
const WRITE: u64 = 64;
const EXIT: u64 = 93;
const REBOOT: u64 = 142;
const FORK: u64 = 220;

#[macro_export]
macro_rules! ecall {
    ($syscall:expr $(=> $($register:tt = $value:expr),*$(,)?)?) => {
        {
            let ret: isize;
            #[allow(clippy::macro_metavars_in_unsafe)]
            unsafe {
                core::arch::asm! (
                    "ecall",
                    $(
                    $(
                    in($register) $value,
                    )*
                    )?
                    in("a7") $syscall,
                    lateout("a0") ret
                )
            }
            ret
        }
    };
}

#[repr(transparent)]
pub struct RebootCmd(u64);

impl RebootCmd {
    pub const HALT: Self = Self(0xcdef0123);
    pub const POWER_OFF: Self = Self(0x4321fedc);
    pub const RESTART: Self = Self(0x1234567);
}

pub fn read(fd: u64, buf: &mut [u8]) -> isize {
    ecall!(READ => "a0" = fd, "a1" = buf.as_ptr(), "a2" = buf.len())
}

pub fn write(fd: u64, buf: &[u8]) -> isize {
    ecall!(WRITE => "a0" = fd, "a1" = buf.as_ptr(), "a2" = buf.len())
}

pub fn reboot(cmd: RebootCmd) -> isize {
    ecall!(REBOOT => "a0" = 0xfee1deadu64, "a1" = 0x28121969, "a2" = cmd.0)
}

pub fn fork() -> isize {
    ecall!(FORK)
}

pub fn exit(code: u64) -> ! {
    ecall!(EXIT => "a0" = code);

    loop {
        core::hint::spin_loop()
    }
}
