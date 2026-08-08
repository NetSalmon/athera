#![allow(dead_code)]
//! RISC-V 64 架构相关封装。
//!
//! 本模块是内核与 RISC-V 特权架构之间的边界。上层代码应优先使用这里的
//! 类型化接口，而不是直接嵌入架构指令或手工填写 SBI 参数。
//!
//! - [`registers`]：CSR、通用寄存器和常用寄存器位域；
//! - [`sbi`]：Supervisor Binary Interface 调用封装；
//! - [`wait_for_interrupt`]、[`instruction_fence`]、[`address_translation_fence`]
//!   等：没有合适寄存器类型的架构指令。
use core::arch::asm;

pub(crate) mod registers;
pub(crate) mod sbi;

/// 停止当前 hart，直到有中断到达。
///
/// `wfi` 只是低功耗等待提示；实现可以提前返回，因此调用者不能把它当作
/// 永久阻塞或同步原语使用。
#[inline]
pub fn wait_for_interrupt() {
    unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) }
}

/// 使指令缓存和取指流水线观察到此前写入的代码。
#[inline]
pub fn instruction_fence() {
    unsafe { asm!("fence.i", options(nostack, preserves_flags)) }
}

/// 刷新当前 hart 的全部地址转换缓存。
///
/// 该操作只影响当前 hart；多 hart 场景需要额外使用 SBI RFENCE 或 IPI。
#[inline]
pub fn address_translation_fence() {
    unsafe { asm!("sfence.vma", options(nostack, preserves_flags)) }
}

/// 返回启动入口保存到 `tp` 的当前 hart 编号。
///
/// OpenSBI 会在进入 S 模式时把 hart 编号作为启动参数传入，启动汇编将其
/// 保存到 `tp`。相比读取 M-mode 的 `mhartid`，这种方式适用于 S 模式内核。
#[inline]
pub fn hart_id() -> usize {
    registers::gpr::Tp::read() as usize
}

/// 触发一个软件断点。
#[inline]
pub fn breakpoint() {
    unsafe {
        asm!("ebreak");
    }
}
