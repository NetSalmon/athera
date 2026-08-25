//! 系统调用号、错误码与用户态 ABI 类型。

use crate::{bits, numeric};

/// `wait4` 使用的 POSIX 时间值。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeVal {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

/// `wait4` 返回的资源使用统计，布局与 riscv64 用户态 ABI 一致。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceUsage {
    pub ru_utime: TimeVal,
    pub ru_stime: TimeVal,
    pub ru_maxrss: i64,
    pub ru_ixrss: i64,
    pub ru_idrss: i64,
    pub ru_isrss: i64,
    pub ru_minflt: i64,
    pub ru_majflt: i64,
    pub ru_nswap: i64,
    pub ru_inblock: i64,
    pub ru_oublock: i64,
    pub ru_msgsnd: i64,
    pub ru_msgrcv: i64,
    pub ru_nsignals: i64,
    pub ru_nvcsw: i64,
    pub ru_nivcsw: i64,
}

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
        EOVERFLOW = -75,
    }
}

// mmap 的 prot 参数（与 Linux <sys/mman.h> 对齐）。
numeric! {
    pub enum MmapProt: usize {
        NONE = 0,
        READ = 1,
        WRITE = 2,
        EXEC = 4,
    }
}

// mmap 的 flags 参数（与 Linux asm-generic/mman-common.h 对齐）。
numeric! {
    pub enum MmapFlags: usize {
        SHARED = 0x01,
        PRIVATE = 0x02,
        FIXED = 0x10,
        ANONYMOUS = 0x20,
        GROWSDOWN = 0x0100,
        STACK = 0x020000,
        FIXED_NOREPLACE = 0x100000,
    }
}

// mremap 的 flags 参数（与 Linux asm-generic/mman-common.h 对齐）。
numeric! {
    pub enum MremapFlags: usize {
        MAYMOVE = 1,
        FIXED = 2,
        DONTUNMAP = 4,
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

bits! {
    pub type WaitOptions: u32 {
        nohang: 0,
        untraced: 1,
        continued: 3,
        exited: 2,
        nowait: 24,
    }
}

// Linux wait status 的常用退出信息位：低 7 位为信号，第 7 位为 core dump，
// 第 8..=15 位为退出码。停止/继续状态使用特殊原始值，后续实现 wait4
// 时再通过 WaitStatus::from(raw) 处理。
numeric! {
    pub enum WaitSignal: u32 {
        NONE = 0,
        HUP = 1,
        INT = 2,
        QUIT = 3,
        ILL = 4,
        TRAP = 5,
        ABRT = 6,
        BUS = 7,
        FPE = 8,
        KILL = 9,
        USR1 = 10,
        SEGV = 11,
        USR2 = 12,
        PIPE = 13,
        ALRM = 14,
        TERM = 15,
        STKFLT = 16,
        CHLD = 17,
        CONT = 18,
        STOP = 19,
        TSTP = 20,
        TTIN = 21,
        TTOU = 22,
        URG = 23,
        XCPU = 24,
        XFSZ = 25,
        VTALRM = 26,
        PROF = 27,
        WINCH = 28,
        IO = 29,
        PWR = 30,
        SYS = 31,
    }
}

bits! {
    pub type WaitStatus: u32 {
        signal: WaitSignal : 0 => 6,
        core_dump: 7,
        exit_code: 8 => 15,
    }
}
