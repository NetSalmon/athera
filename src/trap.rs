#![allow(dead_code)]
//! 陷阱（异常 / 中断）处理与用户态上下文切换。
//!
//! `entry.asm` 的 `trap_entry` 保存现场后调用 [`trap_handler`]；用户态
//! 进程的上下文由 [`TrapContext`] 描述，经 [`restore_context`] 恢复到
//! 用户态。
use core::arch::asm;

use crate::{
    arch,
    arch::{
        registers::{csr::Sscratch, gpr::Sp},
        sbi::srst::{ResetReason, ResetType, system_reset},
    },
    debug, error, info, numeric,
    proc::sched::{save_current, switch},
    syscall::{self, SyscallResult},
    user_trap_entry,
};

const INTERRUPT_MASK: i64 = 1 << 63;

numeric! {
    pub enum Interrupt: i64 {
        U_MODE_SOFTWARE = INTERRUPT_MASK,
        S_MODE_SOFTWARE = INTERRUPT_MASK | 1,
        M_MODE_SOFTWARE = INTERRUPT_MASK | 3,
        USER_TIMER = INTERRUPT_MASK | 4,
        SUPERVISOR_TIMER = INTERRUPT_MASK | 5,
        MACHINE_TIMER = INTERRUPT_MASK | 7,
        USER_EXTERNAL = INTERRUPT_MASK | 8,
        SUPERVISOR_EXTERNAL = INTERRUPT_MASK | 9,
        MACHINE_EXTERNAL = INTERRUPT_MASK | 11,
    }
}

numeric! {
    pub enum Exception: i64 {
        INSTRUCTION_ADDRESS_MISALIGNED = 0,
        INSTRUCTION_ACCESS_FAULT = 1,
        ILLEGAL_INSTRUCTION = 2,
        BREAKPOINT = 3,
        LOAD_ADDRESS_MISALIGNED = 4,
        LOAD_ACCESS_FAULT = 5,
        STORE_ADDRESS_MISALIGNED = 6,
        STORE_ACCESS_FAULT = 7,
        U_MODE_ECALL = 8,
        S_MODE_ECALL = 9,
        M_MODE_ECALL = 11,
        INSTRUCTION_PAGE_FAULT = 12,
        LOAD_PAGE_FAULT = 13,
        STORE_PAGE_FAULT = 15,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Trap {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl From<i64> for Trap {
    fn from(value: i64) -> Trap {
        if value > 0 {
            Trap::Exception(Exception::from(value))
        } else {
            Trap::Interrupt(Interrupt::from(value))
        }
    }
}

impl From<Exception> for Trap {
    fn from(value: Exception) -> Trap {
        Trap::Exception(value)
    }
}

impl From<Interrupt> for Trap {
    fn from(value: Interrupt) -> Trap {
        Trap::Interrupt(value)
    }
}

impl From<Trap> for i64 {
    fn from(value: Trap) -> i64 {
        match value {
            Trap::Exception(i) => i.into(),
            Trap::Interrupt(i) => i.into(),
        }
    }
}

/// 陷阱入口 C 侧处理函数，由 `entry.asm` 的 `trap_entry` 调用。
///
/// 参数依次为 `scause` / `sepc` / `stval` / `sstatus` / `satp` 与陷阱
/// 帧栈指针（`sp`）。用户态 `ecall` 在此分派给 [`syscall::handle`]，
/// 其余异常/中断按类型处理或直接停机。
#[unsafe(no_mangle)]
fn trap_handler(
    scause: u64,
    sepc: u64,
    _stval: u64,
    _sstatus: u64,
    _satp: u64,
    trap_frame_sp: u64,
) {
    let trap = Trap::from(scause as i64);

    match trap {
        Trap::Interrupt(Interrupt::SUPERVISOR_TIMER) => {
            save_current(trap_frame_sp, sepc, _sstatus, _satp);
            set_next_timer();
            switch();
        }
        Trap::Exception(Exception::U_MODE_ECALL) => {
            let trap_context = unsafe { &*((trap_frame_sp) as *const [u64; 32]) };

            match syscall::handle(sepc, trap_context) {
                SyscallResult::Return(ret, next) => {
                    unsafe { (trap_frame_sp as *mut u64).add(10).write(ret) };
                    arch::registers::csr::Sepc::write(next);
                }
                SyscallResult::Yield => {
                    // 仍在陷阱处理期间，SIE 已由硬件清零；不能在这里
                    // 自旋等待定时器，而应直接切换到下一个任务。
                    switch();
                }
            }
        }
        Trap::Exception(Exception::BREAKPOINT) => {
            debug!("breakpoint");
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::S_MODE_ECALL | Exception::M_MODE_ECALL) => {
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::ILLEGAL_INSTRUCTION) => {
            error!("illegal instruction at sepc = {:#x}", sepc);
            system_reset(ResetType::SHUTDOWN, ResetReason::SYS_FAIL);
        }
        Trap::Interrupt(Interrupt::SUPERVISOR_EXTERNAL) => {}
        Trap::Exception(
            Exception::LOAD_ACCESS_FAULT
            | Exception::LOAD_PAGE_FAULT
            | Exception::STORE_ACCESS_FAULT
            | Exception::INSTRUCTION_ACCESS_FAULT,
        ) => {
            error!("memory access fault: {trap:?}, sepc = {sepc:#x}");
            system_reset(ResetType::SHUTDOWN, ResetReason::SYS_FAIL);
        }
        other => {
            error!(
                "unhandled trap: {other:?}, halting, sepc={sepc:#x}, stval={_stval:#x}, sstatus={_sstatus:#x}, satp={_satp:#x}, frame={trap_frame_sp:#x}"
            );
            loop {
                core::hint::spin_loop();
            }
        }
    }
}

#[inline]
pub fn set_next_timer() {
    const GAP: u64 = 1_000_000; // 10 Hz
    let t = arch::registers::csr::Time::read();
    arch::registers::csr::Stimecmp::write(t + GAP);
}

/// 陷阱帧（32 个通用寄存器数组）中 `a0` 的下标（x10），
/// 系统调用参数依次存放于 `a0..a7`。
pub(crate) const A0_INDEX: usize = 10;

/// 用户态陷阱上下文：32 个通用寄存器 + `satp` / `sepc` / `sstatus`。
#[derive(Clone, Debug)]
pub struct TrapContext {
    pub context: [u64; 32],
    pub satp: u64,
    pub sepc: u64,
    pub sstatus: u64,
}

impl TrapContext {
    /// 克隆出子进程的用户态上下文（fork 语义）。
    ///
    /// 以本上下文为模板（沿用 `sstatus` 等），用父进程陷入内核时的
    /// 陷阱帧 `frame` 覆盖通用寄存器现场，并改写三处：地址空间切换为
    /// 子进程的 `satp`、从 `sepc + 4` 继续执行、返回值 `a0` 置 0。
    pub fn clone_child(&self, frame: &[u64; 32], sepc: u64, satp: u64) -> Self {
        let mut context = self.clone();
        context.context = *frame;
        context.satp = satp;
        context.sepc = sepc + 4;
        context.context[A0_INDEX] = 0;
        context
    }
}

/// 恢复用户态上下文并跳转到用户态（`sret`），不会返回。
///
/// 依次写入 `sstatus` / `sepc` / `stvec` / `satp` 并刷新 TLB，然后从
/// 上下文数组装载通用寄存器。
pub fn restore_context(context: &TrapContext) {
    let context_addr = context.context.as_ptr() as u64;

    info!("sscratch: {:#p}", Sscratch::read() as *const u8);
    info!("sp: {:#p}", Sp::read() as *const u8);

    unsafe {
        asm!(
            r#"
            csrrw sp, sscratch, sp
            csrw stvec, {stvec}
            csrw sstatus, {sstatus}
            csrw sepc, {sepc}
            csrw satp, {satp}
            sfence.vma

            ld x1, 8(t0)
            ld x2, 16(t0)
            ld x3, 24(t0)
            ld x4, 32(t0)
            ld x6, 48(t0)
            ld x7, 56(t0)
            ld x8, 64(t0)
            ld x9, 72(t0)
            ld x10, 80(t0)
            ld x11, 88(t0)
            ld x12, 96(t0)
            ld x13, 104(t0)
            ld x14, 112(t0)
            ld x15, 120(t0)
            ld x16, 128(t0)
            ld x17, 136(t0)
            ld x18, 144(t0)
            ld x19, 152(t0)
            ld x20, 160(t0)
            ld x21, 168(t0)
            ld x22, 176(t0)
            ld x23, 184(t0)
            ld x24, 192(t0)
            ld x25, 200(t0)
            ld x26, 208(t0)
            ld x27, 216(t0)
            ld x28, 224(t0)
            ld x29, 232(t0)
            ld x30, 240(t0)
            ld x31, 248(t0)

            # restore t0
            ld x5, 40(t0)

            sret
            "#,
            sstatus = in(reg) context.sstatus,
            stvec = in(reg) user_trap_entry as *const u8,
            sepc = in(reg) context.sepc,
            satp = in(reg) context.satp,

            in("t0") context_addr,
            options(noreturn),
        )
    }
}
