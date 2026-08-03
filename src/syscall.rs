//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里，
//! 目前支持 read / write / exit / reboot 等。
use alloc::vec::Vec;

use crate::{
    arch,
    arch::{
        registers::values::SStatusBits,
        sbi,
        sbi::srst::{ResetReason, ResetType, system_reset},
    },
    debug,
    dev::UART,
    error, info, kernel_halt, numeric,
    proc::{CURRENT_TASK, task::{TASKS, Tid}},
};

numeric! {
    pub enum ErrorCode : isize {
        EINVAL = -22,
        EIO = -5,
        ENOSYS = -38,
    }
}

numeric! {
    pub enum Syscall: u64 {
        READ = 63,
        WRITE = 64,
        EXIT = 93,
        REBOOT = 142,
        FORK = 220,
        WAITPID = 95,
        EXEC = 221,
        MMAP = 222,
        MUNMAP = 223,
        MREMAP = 224,
    }
}

numeric! {
    pub enum RebootCmd : u64 {
        RESTART = 0x1234567,
        POWER_OFF = 0x4321fedc,
        HALT = 0xcdef0123,
    }
}

fn read(_fd: u64, buf: &mut [u8]) -> u64 {
    let uart = match UART.force().as_ref() {
        Some(u) => u,
        None => return ErrorCode::EIO.0 as u64,
    };

    let mut bytes_read = 0;
    for i in buf.iter_mut() {
        if let Some(ch) = uart.lock().getchar() {
            *i = ch;
            bytes_read += 1;
        } else {
            break;
        }
    }
    bytes_read
}

fn write(_fd: u64, buf: &[u8]) -> u64 {
    // 一次持锁写完整个缓冲区，避免逐字节加锁/关中断。
    if let Some(uart) = UART.force().as_ref() {
        let guard = uart.lock();
        for &c in buf {
            guard.putchar(c);
        }
    }
    buf.len() as u64
}

fn reboot(magic: u64, magic2: u64, cmd: u64) -> isize {
    if magic != 0xfee1dead || magic2 != 0x28121969 {
        debug!("reboot: invalid magic (magic = {magic:#x}, magic2 = {magic2:#x})");
        return -1;
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
            return -1;
        }
    }
    -1
}

/// 处理一次系统调用。
///
/// `args` 是陷阱帧里 `a0..a7` 的副本，`sepc` 为触发 `ecall` 的地址；
/// 返回 `(返回值, 下一条指令地址)`，其中返回值写入 `a0`，下一条地址写
/// 入 `sepc`。
pub fn handle(args: &[u64; 8], sepc: u64) -> (u64, u64) {
    match Syscall::from(args[7]) {
        Syscall::READ => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            let ret = read(args[0], unsafe { &mut *buf });
            (ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            let buf = unsafe { &*buf };
            let ret = write(args[0], buf);
            (ret, sepc + 4)
        }
        Syscall::EXIT => {
            let code = args[0] as i32;
            let mut s: SStatusBits = arch::registers::csr::Sstatus::read().into();
            s.set_spp(true);
            arch::registers::csr::Sstatus::write(s.into());

            match *CURRENT_TASK.current() {
                Some(tid) => {
                    info!("task {} exited with code {code}", tid.0);

                    // 删除任务和收集快照都是短操作；完整任务表在锁外
                    // 打印，避免长时间关闭中断影响定时器。
                    let snapshot: Vec<Tid> = {
                        let mut tasks = TASKS.force().lock();
                        tasks.remove(&tid);
                        tasks.keys().copied().collect()
                    };
                    debug!("current tasks: {snapshot:?}");
                }
                None => {
                    error!("exit syscall with no current task (code {code})");
                }
            }

            (args[0], kernel_halt as *const () as u64)
        }
        Syscall::REBOOT => {
            let ret = reboot(args[0], args[1], args[2]);

            (ret as u64, sepc + 4)
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
        _ => (ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}
