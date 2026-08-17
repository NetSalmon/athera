//! 系统调用号、错误码与用户态 ABI 类型。

use crate::numeric;

numeric! {
    pub enum ErrorCode : isize {
        EPERM = -1,
        ENOENT = -2,
        ESRCH = -3,
        EINTR = -4,
        EIO = -5,
        ENXIO = -6,
        E2BIG = -7,
        ENOEXEC = -8,
        EBADF = -9,
        ECHILD = -10,
        EAGAIN = -11,
        ENOMEM = -12,
        EACCES = -13,
        EFAULT = -14,
        EBUSY = -16,
        EEXIST = -17,
        EXDEV = -18,
        ENODEV = -19,
        ENOTDIR = -20,
        EISDIR = -21,
        EINVAL = -22,
        ENFILE = -23,
        EMFILE = -24,
        ENOTTY = -25,
        EFBIG = -27,
        ENOSPC = -28,
        ESPIPE = -29,
        EROFS = -30,
        EMLINK = -31,
        EPIPE = -32,
        ERANGE = -34,
        ENAMETOOLONG = -36,
        ENOSYS = -38,
        ENOTEMPTY = -39,
        ELOOP = -40,
    }
}

// 系统调用号与 Linux asm-generic（riscv64）ABI 对齐。
numeric! {
    pub enum Syscall: u64 {
        READ = 63,
        WRITE = 64,
        EXIT = 93,
        REBOOT = 142,
        MUNMAP = 215,
        MREMAP = 216,
        CLONE = 220,
        EXECVE = 221,
        MMAP = 222,
        WAIT4 = 260,
    }
}

numeric! {
    pub enum RebootCmd : u64 {
        RESTART = 0x1234567,
        POWER_OFF = 0x4321fedc,
        HALT = 0xcdef0123,
    }
}
