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
use crate::arch::registers::values::{SatpMode, SatpValue};
use crate::constants::USER_STACK_SIZE;
use crate::mem::allocators::alloc_frame;
use crate::mem::page_table::{AddressSpaceId, PAGE_TABLE_MANAGER};
use crate::proc::task::{MemorySet, TaskControlBlock, TaskStatus, TID_ALLOCATOR};
use crate::trap::{restore_context, TrapContext};

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

const ARGS_START: usize = 10;

/// 处理一次系统调用。
///
/// `args` 是陷阱帧里 `a0..a7` 的副本，`sepc` 为触发 `ecall` 的地址；
/// 返回 `(返回值, 下一条指令地址)`，其中返回值写入 `a0`，下一条地址写
/// 入 `sepc`。
pub fn handle(sepc: u64, trap_context: &[u64; 32]) -> (u64, u64) {
    match Syscall::from(trap_context[ARGS_START + 7]) {
        Syscall::READ => {
            let ptr = trap_context[ARGS_START + 1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, trap_context[ARGS_START + 2] as usize);
            let ret = read(trap_context[ARGS_START], unsafe { &mut *buf });
            (ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = trap_context[ARGS_START + 1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, trap_context[ARGS_START + 2] as usize);
            let buf = unsafe { &*buf };
            let ret = write(trap_context[ARGS_START], buf);
            (ret, sepc + 4)
        }
        Syscall::EXIT => {
            let code = trap_context[ARGS_START] as i32;
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

            (trap_context[ARGS_START], kernel_halt as *const () as u64)
        }
        Syscall::REBOOT => {
            let ret = reboot(trap_context[ARGS_START + 0], trap_context[ARGS_START + 1], trap_context[ARGS_START + 2]);

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
        Syscall::FORK => {
            info!("receive fork req");

            let current_tid = CURRENT_TASK.current().unwrap();

            let new_tid = TID_ALLOCATOR.force().lock().alloc().unwrap();
            let new_tid = Tid(new_tid);

            info!("new tid: {:?}", new_tid);

            // TODO(fork): 分配新 tid、克隆地址空间并创建子任务，目前仅占位。
            PAGE_TABLE_MANAGER
                .force()
                .lock()
                .clone(AddressSpaceId::User(current_tid), new_tid)
                .unwrap();

            let root_page_table_address = PAGE_TABLE_MANAGER
                .force()
                .lock()
                .user_root_addr(new_tid)
                .unwrap();

            let mut guard = TASKS
                .force()
                .lock();

            let current_tcb = guard
                .get_mut(&current_tid)
                .unwrap();

            current_tcb.children.push(new_tid);

            let mut pages = Vec::new();

            let mut new_context = current_tcb.trap_context.clone();
            for i in current_tcb.memory_set.used_page.iter() {
                let new_page = alloc_frame(Some(i.size)).expect("out of memory");

                unsafe {
                    core::ptr::copy(
                        i.start as *const u8,
                        new_page.start as *mut u8,
                        new_page.size,
                    )
                }

                pages.push(new_page);
            }

            drop(guard);

            info!("pages clone ok");

            let satp = SatpValue::builder()
                .set_ppn(root_page_table_address.ppn() as u64)
                .set_mode(SatpMode::SV39.into())
                .build();

            new_context.context = *trap_context;
            new_context.satp = satp.into();
            new_context.sepc = sepc + 4;

            let new_memory_set = MemorySet {
                used_page: pages,
                user_root_page_table: root_page_table_address,
            };

            let new_tcb = TaskControlBlock {
                parent: Some(current_tid),
                children: Vec::new(),
                status: TaskStatus::Running,
                memory_set: new_memory_set,
                trap_context: new_context.clone(),
                exit_code: 0,
                priority: 0,
            };

            TASKS.force().lock().insert(new_tid, new_tcb);

            *CURRENT_TASK.current() = Some(new_tid);

            info!("restore");

            restore_context(&new_context);

            (0, sepc + 4)
        }
        _ => (ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}
