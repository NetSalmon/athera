//! 系统调用处理。
//!
//! 用户态 `ecall`（`U_MODE_ECALL`）陷入后由 `trap_handler` 分派到这里，
//! 目前支持 read / write / exit / reboot 等。

use alloc::vec::Vec;

use crate::{
    arch::{
        registers::values::{SatpMode, SatpValue},
        sbi::{
            self,
            srst::{ResetReason, ResetType, system_reset},
        },
    },
    debug,
    dev::{UART, traits::CharDevice},
    info,
    mem::{
        allocators::alloc_frame,
        page_table::{AddressSpaceId, PAGE_TABLE_MANAGER},
    },
    numeric,
    proc::{
        CURRENT_TASK, CurrentTask,
        task::{MemorySet, TASKS, TID_ALLOCATOR, TaskControlBlock, TaskStatus},
    },
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

/// 陷阱帧（32 个通用寄存器数组）中 `a0` 的下标（x10），
/// 系统调用参数依次存放于 `a0..a7`。
const A0_INDEX: usize = 10;

/// 系统调用处理结果。
pub enum SyscallResult {
    /// 恢复用户态继续执行：`(返回值, 下一条指令地址)` 会分别写入 `a0`
    /// 与 `sepc`，随后照常 `sret` 回用户态。
    Return(u64, u64),
    /// 当前任务退出：不再 `sret` 回用户态，由陷阱处理侧直接在内核态
    /// 切换到下一个任务（退出码已记录到 `CURRENT_TASK`）。
    Exit,
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

            if let Some(ref mut current) = *CURRENT_TASK.current() {
                current.exit_code = Some(code)
            }

            SyscallResult::Exit
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
        Syscall::FORK => {
            info!("receive fork req");

            let CurrentTask { tid, .. } = CURRENT_TASK.current().unwrap();
            let current_tid = tid;

            let new_tid = TID_ALLOCATOR.force().lock().alloc().unwrap();

            info!("new tid: {:?}", new_tid);

            // 克隆当前任务的地址空间（页表深拷贝），并登记到新 TID 名下。
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

            let (mut new_context, source_pages) = {
                let mut tasks = TASKS.force().lock();
                let current_tcb = tasks.get_mut(&current_tid).unwrap();
                current_tcb.children.push(new_tid);

                (
                    current_tcb.trap_context.clone(),
                    current_tcb
                        .memory_set
                        .used_page
                        .iter()
                        .map(|page| (page.start, page.size))
                        .collect::<Vec<_>>(),
                )
            };

            // Allocate and copy pages without holding the global task-table lock.
            let mut pages = Vec::with_capacity(source_pages.len());
            for (source_start, size) in source_pages {
                let new_page = alloc_frame(Some(size)).expect("out of memory");

                unsafe {
                    core::ptr::copy(source_start as *const u8, new_page.start as *mut u8, size)
                }
                pages.push(new_page);
            }

            info!("pages clone ok");

            let satp = SatpValue::builder()
                .set_ppn(root_page_table_address.ppn() as u64)
                .set_mode(SatpMode::SV39.into())
                .build();

            new_context.context = *trap_context;
            new_context.satp = satp.into();
            new_context.sepc = sepc + 4;
            new_context.context[A0_INDEX] = 0;

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

            TASKS
                .force()
                .lock()
                .add(new_tid, Some(current_tid), new_tcb);

            SyscallResult::Return(new_tid.0 as u64, sepc + 4)
        }
        _ => SyscallResult::Return(ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}
