//! 关中断自旋锁 [`SpinLock`]。
//!
//! 进入临界区前先把 SIE（`sie` CSR）置 0 关中断，然后通过原子
//! `compare_exchange` 自旋抢锁；释放时清锁并恢复进入前的 SIE 值。
//! 这样普通执行流与中断处理程序之间的竞争被“关中断”消解，同一时刻
//! 只有一条执行流能进入临界区。
//!
//! # 安全性
//!
//! - 本内核当前为单核：互斥靠关中断实现。若将来支持多核，需要改为
//!   原子自旋 + 内存屏障，并处理关中断与抢锁之间的顺序。
//! - `SpinLock` 基于 `UnsafeCell` 提供内部可变性，`unsafe impl Sync`
//!   要求 `T: Send`。

use crate::arch::riscv64;
use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// 关中断自旋锁。
///
/// 持锁期间中断被关闭，因此临界区应尽量短小，且不能调用任何会长时间
/// 阻塞或依赖中断的代码。
pub struct SpinLock<T> {
    lock: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: 临界区通过关中断保证同一时刻只有一条执行流访问 `value`，
// 因此只要 `T` 本身可以在执行流之间转移（`Send`），共享 `&SpinLock`
// 就是安全的。
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// 创建一个未加锁的 `SpinLock`（可用于 `const` 上下文）。
    pub const fn new(value: T) -> SpinLock<T> {
        SpinLock {
            lock: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// 关闭中断并自旋获取锁，返回一个在 `Drop` 时释放锁并恢复 SIE 的守卫。
    ///
    /// 守卫持有期间调用者拥有 `T` 的独占访问权。
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let sie = riscv64::disable_interrupt();
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinLockGuard { lock: self, sie }
    }
}

/// [`SpinLock::lock`] 返回的守卫。
///
/// 通过 `Deref` / `DerefMut` 访问被保护的数据；`Drop` 时释放自旋锁并
/// 把 SIE 恢复为加锁前的值。
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    sie: u64,
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: 守卫存活期间本执行流持有锁，是 `value` 的唯一访问者。
        unsafe { &*self.lock.value.get() }
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: 同上，且 `&mut self` 保证不存在其它活跃引用。
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
        riscv64::enable_interrupt(self.sie);
    }
}
