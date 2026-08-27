//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里。
//! 系统调用号与返回值约定对齐 Linux asm-generic（riscv64）ABI：调用号
//! 经 `a7` 传入，返回值经 `a0` 传回，出错时返回 `-errno`。

pub(crate) mod abi;

use alloc::{borrow::ToOwned, string::String, vec::Vec};

use abi::{ErrorCode, Syscall};
#[allow(unused_imports)]
pub use abi::{ResourceUsage, TimeVal, WaitOptions, WaitSignal, WaitStatus};

use crate::{
    arch::riscv64::trap::A0_INDEX,
    debug, error,
    error::{Error, MemError},
    info,
    mm::memory_map,
    task::{
        CURRENT_TASK, TASKS, Tid, clone_task,
        process::{self, FdError},
    },
};

/// 当前任务的 TID；无任务上下文时返回 `ESRCH`。
fn current_tid() -> Result<Tid, ErrorCode> {
    (*CURRENT_TASK.current()).ok_or(ErrorCode::ESRCH)
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

/// 验证用户虚拟地址是否在当前任务的用户地址空间内。
///
/// 遍历当前任务的 `MemorySet.mappings`，检查 `addr` 是否落在某个
/// `UserMapping` 的虚拟地址范围内。
fn validate_user_address(addr: usize) -> Result<(), ErrorCode> {
    let Some(tid) = *CURRENT_TASK.current() else {
        return Err(ErrorCode::ESRCH);
    };

    let tasks = TASKS.force().lock();
    let Some(tcb) = tasks.get(&tid) else {
        return Err(ErrorCode::ESRCH);
    };

    for mapping in &tcb.memory_set.mappings {
        let start = usize::from(mapping.va);
        let size: usize = mapping.frames.iter().map(|f| f.size).sum();
        let end = start + size;
        if addr >= start && addr < end {
            return Ok(());
        }
    }

    Err(ErrorCode::EFAULT)
}

/// 验证用户虚拟地址范围是否在当前任务的用户地址空间内。
fn validate_user_range(addr: usize, len: usize) -> Result<(), ErrorCode> {
    if len == 0 {
        return Ok(());
    }
    let end = addr.checked_add(len).ok_or(ErrorCode::EFAULT)?;
    validate_user_address(addr)?;
    validate_user_address(end - 1)?;
    Ok(())
}

/// 单个用户参数字符串的最大长度（避免野指针导致无界扫描）。
const MAX_ARG_LEN: usize = 4096;

/// 从用户地址空间拷贝一个 NUL 结尾字符串。
///
/// # Safety
///
/// `ptr` 必须指向当前任务用户地址空间内的合法映射。
unsafe fn read_user_cstr(ptr: *const u8) -> Result<String, ErrorCode> {
    let addr = ptr as usize;
    validate_user_address(addr)?;

    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
        if len > MAX_ARG_LEN {
            return Err(ErrorCode::E2BIG);
        }
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    core::str::from_utf8(bytes)
        .map(ToOwned::to_owned)
        .map_err(|_| ErrorCode::EINVAL)
}

/// 从用户地址空间拷贝一个 NULL 结尾的字符串指针数组（argv / envp）。
///
/// # Safety
///
/// `ptr` 必须指向当前任务用户地址空间内的合法映射，且数组以 NULL 指针
/// 结尾。
unsafe fn read_user_string_array(ptr: *const *const u8) -> Result<Vec<String>, ErrorCode> {
    let addr = ptr as usize;
    validate_user_address(addr)?;

    let mut out = Vec::new();
    let mut cur = ptr;
    loop {
        let item = unsafe { cur.read() };
        if item.is_null() {
            break;
        }
        out.push(unsafe { read_user_cstr(item) }?);
        cur = unsafe { cur.add(1) };
    }
    Ok(out)
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
            let ptr = trap_context[A0_INDEX + 1] as usize;
            let len = trap_context[A0_INDEX + 2] as usize;

            if let Err(errno) = validate_user_range(ptr, len) {
                return SyscallResult::Return(errno.0 as u64, sepc + 4);
            }

            let buf = core::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len);
            let ret = match process::read_fd(trap_context[A0_INDEX], unsafe { &mut *buf }) {
                Ok(size) => size as u64,
                Err(err) => fd_errno(err),
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = trap_context[A0_INDEX + 1] as usize;
            let len = trap_context[A0_INDEX + 2] as usize;

            if let Err(errno) = validate_user_range(ptr, len) {
                return SyscallResult::Return(errno.0 as u64, sepc + 4);
            }

            let buf = core::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len);
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
            let wait_status = trap_context[A0_INDEX + 1] as usize;
            let options = WaitOptions::from(trap_context[A0_INDEX + 2] as u32);
            let resource_usage = trap_context[A0_INDEX + 3] as usize;

            match process::wait4(tid, options.nohang()) {
                Some(result) => {
                    if result.tid != 0 && wait_status != 0 {
                        if let Err(errno) = validate_user_range(wait_status, size_of::<WaitStatus>()) {
                            return SyscallResult::Return(errno.0 as u64, sepc + 4);
                        }
                        let status = WaitStatus::from(((result.exit_code as u32) & 0xff) << 8);
                        unsafe { (wait_status as *mut WaitStatus).write(status) };
                    }
                    if result.tid != 0 && resource_usage != 0 {
                        if let Err(errno) = validate_user_range(resource_usage, size_of::<ResourceUsage>()) {
                            return SyscallResult::Return(errno.0 as u64, sepc + 4);
                        }
                        unsafe { (resource_usage as *mut ResourceUsage).write(ResourceUsage::default()) };
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
            // Linux riscv64 ABI：a0..a5 = addr / length / prot / flags / fd / offset。
            let addr = trap_context[A0_INDEX] as usize;
            let length = trap_context[A0_INDEX + 1] as usize;
            let prot = trap_context[A0_INDEX + 2] as usize;
            let flags = trap_context[A0_INDEX + 3] as usize;
            let fd = trap_context[A0_INDEX + 4] as isize;
            let offset = trap_context[A0_INDEX + 5] as usize;

            let ret = match current_tid()
                .and_then(|tid| memory_map::mmap(tid, addr, length, prot, flags, fd, offset))
            {
                Ok(va) => va as u64,
                Err(errno) => errno.0 as u64,
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::MUNMAP => {
            // Linux riscv64 ABI：a0 = addr、a1 = length。
            let addr = trap_context[A0_INDEX] as usize;
            let length = trap_context[A0_INDEX + 1] as usize;

            let ret = match current_tid().and_then(|tid| memory_map::munmap(tid, addr, length)) {
                Ok(()) => 0,
                Err(errno) => errno.0 as u64,
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::MREMAP => {
            // Linux riscv64 ABI：a0..a4 = old_address / old_size / new_size / flags / new_address。
            let old_address = trap_context[A0_INDEX] as usize;
            let old_size = trap_context[A0_INDEX + 1] as usize;
            let new_size = trap_context[A0_INDEX + 2] as usize;
            let flags = trap_context[A0_INDEX + 3] as usize;
            let new_address = trap_context[A0_INDEX + 4] as usize;

            let ret = match current_tid().and_then(|tid| {
                memory_map::mremap(tid, old_address, old_size, new_size, flags, new_address)
            }) {
                Ok(va) => va as u64,
                Err(errno) => errno.0 as u64,
            };
            SyscallResult::Return(ret, sepc + 4)
        }
        Syscall::CLONE => {
            info!("sepc: {:#p}", sepc as *const u8);
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
        Syscall::EXECVE => {
            // Linux riscv64 ABI：a0 = pathname、a1 = argv、a2 = envp。
            let path_ptr = trap_context[A0_INDEX] as *const u8;
            let argv_ptr = trap_context[A0_INDEX + 1] as *const *const u8;
            let envp_ptr = trap_context[A0_INDEX + 2] as *const *const u8;

            let path = match unsafe { read_user_cstr(path_ptr) } {
                Ok(path) => path,
                Err(err) => return SyscallResult::Return(err.0 as u64, sepc + 4),
            };
            let argv = match unsafe { read_user_string_array(argv_ptr) } {
                Ok(argv) => argv,
                Err(err) => return SyscallResult::Return(err.0 as u64, sepc + 4),
            };
            let envp = match unsafe { read_user_string_array(envp_ptr) } {
                Ok(envp) => envp,
                Err(err) => return SyscallResult::Return(err.0 as u64, sepc + 4),
            };

            let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
            let envp: Vec<&str> = envp.iter().map(String::as_str).collect();

            let tid = match current_tid() {
                Ok(tid) => tid,
                Err(err) => return SyscallResult::Return(err.0 as u64, sepc + 4),
            };

            match crate::binfmt::route_at(tid, &path, &argv, &envp) {
                // execve 成功不返回调用者：让出 CPU，由调度器按替换后的
                // `memory_set` / `trap_context` 在入口处恢复新程序。
                Ok(()) => SyscallResult::Yield,
                Err(err) => {
                    let errno = binfmt_errno(&err).0 as u64;
                    SyscallResult::Return(errno, sepc + 4)
                }
            }
        }
        _ => SyscallResult::Return(ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}

fn binfmt_errno(error: &crate::binfmt::Error) -> ErrorCode {
    use crate::binfmt::Error;

    match error {
        Error::NoAccess => ErrorCode::EACCES,
        Error::FsError(err) => ErrorCode::from(-err.errno()),
        Error::UnsupportedBinfmt => ErrorCode::ENOEXEC,
        Error::ExecFailed => ErrorCode::ENOENT,
    }
}

fn fd_errno(error: FdError) -> u64 {
    match error {
        FdError::NoTask => ErrorCode::ESRCH.0 as u64,
        FdError::BadFd | FdError::NotReadable | FdError::NotWritable => ErrorCode::EBADF.0 as u64,
        FdError::Io(error) => -error.errno() as u64,
    }
}
