//! 写优先的读写自旋锁 [`RwLock`]。
//!
//! 获取锁时关闭中断，避免普通执行流和中断处理程序互相等待。多个读者可以
//! 同时持有读锁；一旦有写者等待，新的读者会让路，避免写者被持续到来的读者
//! 饿死。

#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};
use crate::arch::riscv64::{disable_interrupt, enable_interrupt};
use crate::arch::riscv64::registers::csr::Sie;

const WRITER: usize = 1;
const READER: usize = 2;

/// 写优先的读写自旋锁。
pub struct RwLock<T> {
    state: AtomicUsize,
    waiting_writers: AtomicUsize,
    value: UnsafeCell<T>,
}

// SAFETY: 访问 `value` 由 `state` 中的读者/写者状态协调，读者要求 `T: Sync`，
// 写者要求 `T: Send`。关中断保证当前执行流不会被中断处理程序重入。
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}
unsafe impl<T: Send> Send for RwLock<T> {}

impl<T> RwLock<T> {
    /// 创建一个未加锁的读写锁。
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicUsize::new(0),
            waiting_writers: AtomicUsize::new(0),
            value: UnsafeCell::new(value),
        }
    }

    /// 获取读锁。有写者等待时不会接纳新的读者。
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        let sie = disable_interrupt();

        loop {
            if self.waiting_writers.load(Ordering::Acquire) != 0 {
                core::hint::spin_loop();
                continue;
            }

            let state = self.state.load(Ordering::Acquire);
            if state & WRITER != 0 {
                core::hint::spin_loop();
                continue;
            }

            let Some(next) = state.checked_add(READER) else {
                core::hint::spin_loop();
                continue;
            };

            if self
                .state
                .compare_exchange(state, next, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return RwLockReadGuard { lock: self, sie };
            }
        }
    }

    /// 获取写锁。等待中的写者会阻止后续读者进入。
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        let sie = Sie::read();
        Sie::write(0);
        self.waiting_writers.fetch_add(1, Ordering::AcqRel);

        loop {
            if self
                .state
                .compare_exchange(0, WRITER, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                self.waiting_writers.fetch_sub(1, Ordering::Release);
                return RwLockWriteGuard { lock: self, sie };
            }
            core::hint::spin_loop();
        }
    }
}

/// [`RwLock::read`] 返回的读守卫。
pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
    sie: u64,
}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: 读状态保证写者不会同时访问，且 `T: Sync`。
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.fetch_sub(READER, Ordering::Release);
        Sie::write(self.sie);
    }
}

/// [`RwLock::write`] 返回的写守卫。
pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
    sie: u64,
}

impl<T> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: 写状态保证当前写者独占访问 `value`。
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: 写守卫是 `value` 的唯一访问者。
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Ordering::Release);
        enable_interrupt(self.sie);
    }
}
