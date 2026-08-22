//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里。
//! 系统调用号与返回值约定对齐 Linux asm-generic（riscv64）ABI：调用号
//! 经 `a7` 传入，返回值经 `a0` 传回，出错时返回 `-errno`。

mod abi;

use abi::{ErrorCode, Syscall};
#[allow(unused_imports)]
pub use abi::{RUsage, TimeVal, WaitOptions, WaitSignal, WaitStatus};

use crate::{
    arch::riscv64::trap::A0_INDEX,
    debug, error,
    error::{Error, MemError},
    task::{
        process::{self, FdError},
        task::clone_task,
    },
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
            let ret = match process::read_fd(trap_context[A0_INDEX], unsafe { &mut *buf }) {
                Ok(size) => size as u64,
                Err(err) => fd_errno(err),
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = trap_context[A0_INDEX + 1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, trap_context[A0_INDEX + 2] as usize);
            let buf = unsafe { &*buf };
            let ret = match process::write_fd(trap_context[A0_INDEX], buf) {
                Ok(size) => size as u64,
                Err(err) => fd_errno(err),
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::EXIT => {
            let code = trap_context[A0_INDEX] as i32;
            process::exit(code);
            SyscallResult::Yield
        }
        Syscall::WAIT4 => {
            let tid = trap_context[A0_INDEX] as isize;
            let wait_status = trap_context[A0_INDEX + 1] as *mut WaitStatus;
            let options = WaitOptions::from(trap_context[A0_INDEX + 2] as u32);
            let r_usage = trap_context[A0_INDEX + 3] as *mut RUsage;

            match process::wait4(tid, options.nohang()) {
                Some(result) => {
                    if result.tid != 0 && !wait_status.is_null() {
                        let status = WaitStatus::from(((result.exit_code as u32) & 0xff) << 8);
                        unsafe { wait_status.write(status) };
                    }
                    if result.tid != 0 && !r_usage.is_null() {
                        unsafe { r_usage.write(RUsage::default()) };
                    }
                    SyscallResult::Return(result.tid as u64, sepc + 4)
                }
                None => SyscallResult::Yield,
            }
        }
        Syscall::REBOOT => {
            let magic = trap_context[A0_INDEX];
            let magic2 = trap_context[A0_INDEX + 1];
            let command = trap_context[A0_INDEX + 2];
            if magic != 0xfee1_dead || magic2 != 0x2812_1969 {
                debug!("reboot: invalid magic (magic = {magic:#x}, magic2 = {magic2:#x})");
                return SyscallResult::Return(ErrorCode::EINVAL.0 as u64, sepc + 4);
            }
            if !crate::driver::reboot::reboot(command) {
                debug!("reboot: unsupported command {command:#x}");
                return SyscallResult::Return(ErrorCode::EINVAL.0 as u64, sepc + 4);
            }
            SyscallResult::Return(ErrorCode::EINVAL.0 as u64, sepc + 4)
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

fn fd_errno(error: FdError) -> u64 {
    match error {
        FdError::NoTask => ErrorCode::ESRCH.0 as u64,
        FdError::BadFd | FdError::NotReadable | FdError::NotWritable => ErrorCode::EBADF.0 as u64,
        FdError::Io(error) => (-(error.errno())) as u64,
    }
}
