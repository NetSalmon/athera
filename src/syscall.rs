//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里。
//! 系统调用号与返回值约定对齐 Linux asm-generic（riscv64）ABI：调用号
//! 经 `a7` 传入，返回值经 `a0` 传回，出错时返回 `-errno`。

mod abi;
mod io;
mod process;
mod reboot;

use abi::{ErrorCode, Syscall};
use io::{read, write};
use process::{exit, wait4};
use reboot::reboot;

use crate::{
    bits, error,
    error::{Error, MemError},
    numeric,
    proc::task::clone_task,
    trap::A0_INDEX,
};

/// 把内核错误映射为 Linux errno。
fn errno_of(err: &Error) -> ErrorCode {
    match err {
        Error::NoTidAvailable => ErrorCode::EAGAIN,
        Error::Mem(MemError::OutOfMemory) => ErrorCode::ENOMEM,
        Error::Proc(_) => ErrorCode::ESRCH,
        _ => ErrorCode::EINVAL,
    }
}

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
pub struct RUsage {
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

/// 系统调用处理结果。
pub enum SyscallResult {
    /// 恢复用户态继续执行：`(返回值, 下一条指令地址)` 会分别写入 `a0`
    /// 与 `sepc`，随后照常 `sret` 回用户态。
    Return(u64, u64),
    /// 当前任务主动让出CPU
    Yield,
}

/// 处理一次系统调用。
///
/// `args` 是陷阱帧里 `a0..a7` 的副本，`sepc` 为触发 `ecall` 的地址。
pub fn handle(sepc: u64, trap_context: &[u64; 32]) -> SyscallResult {
    match Syscall::from(trap_context[A0_INDEX + 7]) {
        Syscall::READ => {
            let ptr = trap_context[A0_INDEX + 1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, trap_context[A0_INDEX + 2] as usize);
            let ret = read(trap_context[A0_INDEX], unsafe { &mut *buf });
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = trap_context[A0_INDEX + 1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, trap_context[A0_INDEX + 2] as usize);
            let buf = unsafe { &*buf };
            let ret = write(trap_context[A0_INDEX], buf);
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::EXIT => {
            let code = trap_context[A0_INDEX] as i32;
            exit(code);
            SyscallResult::Yield
        }
        Syscall::WAIT4 => {
            let tid = trap_context[A0_INDEX] as isize;
            let wait_status = trap_context[A0_INDEX + 1] as *mut WaitStatus;
            let options = WaitOptions::from(trap_context[A0_INDEX + 2] as u32);
            let r_usage = trap_context[A0_INDEX + 3] as *mut RUsage;

            match wait4(tid, wait_status, options, r_usage) {
                Some(ret) => SyscallResult::Return(ret, sepc + 4),
                None => SyscallResult::Yield,
            }
        }
        Syscall::REBOOT => {
            let ret = reboot(
                trap_context[A0_INDEX],
                trap_context[A0_INDEX + 1],
                trap_context[A0_INDEX + 2],
            );

            SyscallResult::Return(ret as u64, sepc + 4)
        }
        Syscall::MMAP => {
            todo!()
        }
        Syscall::MUNMAP => {
            todo!()
        }
        Syscall::MREMAP => {
            todo!()
        }
        Syscall::CLONE => {
            // 目前仅实现 fork 语义：忽略 flags / stack 等参数，子进程
            // 获得父进程地址空间的深拷贝，并从 `sepc + 4` 返回 0。
            match clone_task(trap_context, sepc) {
                Ok(child) => SyscallResult::Return(child.0 as u64, sepc + 4),
                Err(err) => {
                    error!("clone failed: {err}");
                    SyscallResult::Return(errno_of(&err).0 as u64, sepc + 4)
                }
            }
        }
        _ => SyscallResult::Return(ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}
