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
#[allow(unused_imports)]
pub use abi::{RUsage, TimeVal, WaitOptions, WaitSignal, WaitStatus};
use io::{read, write};
use process::{exit, wait4};
use reboot::reboot;

use crate::{
    error,
    error::{Error, MemError},
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
