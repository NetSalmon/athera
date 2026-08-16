//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里。
//! 系统调用号与返回值约定对齐 Linux asm-generic（riscv64）ABI：调用号
//! 经 `a7` 传入，返回值经 `a0` 传回，出错时返回 `-errno`。

use crate::{
    arch::sbi::{
        self,
        srst::{ResetReason, ResetType, system_reset},
    },
    bits, debug,
    dev::{UART, traits::CharDevice},
    error,
    error::{Error, MemError},
    info, numeric,
    proc::{
        CURRENT_TASK,
        task::{TASKS, TaskStatus, Tid, clone_task},
    },
    trap::A0_INDEX,
};

// Linux errno（负数形式，直接作为系统调用的错误返回值）。
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

// 系统调用号：对齐 Linux asm-generic（riscv64）ABI。
//
// asm-generic 没有 fork / waitpid，libc 分别用 `clone`（flags 为
// `SIGCHLD`）与 `wait4` 实现它们；执行新程序对应 `execve`。
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

/// 把内核错误映射为 Linux errno。
fn errno_of(err: &Error) -> ErrorCode {
    match err {
        Error::NoTidAvailable => ErrorCode::EAGAIN,
        Error::Mem(MemError::OutOfMemory) => ErrorCode::ENOMEM,
        Error::Proc(_) => ErrorCode::ESRCH,
        _ => ErrorCode::EINVAL,
    }
}

fn read(_fd: u64, buf: &mut [u8]) -> u64 {
    let uart = match UART.force().as_ref() {
        Some(u) => u,
        None => return ErrorCode::EIO.0 as u64,
    };

    let mut read = 0;
    for chunk in buf.chunks_mut(64) {
        let bytes = match uart.lock().read(chunk) {
            Ok(bytes) => bytes,
            Err(_) => return ErrorCode::EIO.0 as u64,
        };
        read += bytes;
        if bytes < chunk.len() {
            break;
        }
    }
    read as u64
}

fn write(_fd: u64, buf: &[u8]) -> u64 {
    // 分段持锁，避免大块输出长时间关闭中断。
    if let Some(uart) = UART.force().as_ref() {
        let mut written = 0;
        for chunk in buf.chunks(64) {
            match uart.lock().write(chunk) {
                Ok(bytes) => written += bytes,
                Err(_) => return ErrorCode::EIO.0 as u64,
            }
        }
        return written as u64;
    }
    ErrorCode::EIO.0 as u64
}

fn reboot(magic: u64, magic2: u64, cmd: u64) -> isize {
    if magic != 0xfee1dead || magic2 != 0x28121969 {
        debug!("reboot: invalid magic (magic = {magic:#x}, magic2 = {magic2:#x})");
        return ErrorCode::EINVAL.0;
    }

    match RebootCmd::from(cmd) {
        RebootCmd::POWER_OFF => {
            info!("reboot: power off requested");
            system_reset(ResetType::SHUTDOWN, ResetReason::NONE);
        }
        RebootCmd::RESTART => {
            info!("reboot: restart requested");
            system_reset(ResetType::COLD_REBOOT, ResetReason::NONE);
        }
        RebootCmd::HALT => {
            info!("reboot: halt requested");
            sbi::hsm::hart_stop();
        }
        _ => {
            debug!("reboot: unsupported command {cmd:#x}");
            return ErrorCode::EINVAL.0;
        }
    }
    ErrorCode::EINVAL.0
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

#[allow(dead_code)]
fn wait4(
    _pid: isize,
    _wstatus: *mut WaitStatus,
    _options: WaitOptions,
    _rusage: *mut RUsage,
) -> SyscallResult {
    todo!()
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

            if let Some(tid) = *CURRENT_TASK.current() {
                if tid.0 == 1 {
                    panic!("pid 1 exit")
                }

                let ch = if let Some(task) = TASKS.force().lock().get_mut(&tid) {
                    task.exit_code = code;
                    task.status = TaskStatus::Zombie;
                    let children = task.children.clone();
                    task.children.clear();
                    Some(children)
                } else {
                    None
                };

                if let Some(children) = ch
                    && let Some(init) = TASKS.force().lock().get_mut(&Tid(1))
                {
                    init.children.extend(children);
                }
            }

            SyscallResult::Yield
        }
        Syscall::WAIT4 => SyscallResult::Yield,
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
