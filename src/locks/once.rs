//! 一次性初始化原语 [`OnceLock`]。
//!
//! 状态机：`UNINITIALIZED -> INITIALIZING -> INITIALIZED`。
//! 多个调用者同时调用 [`OnceLock::get_or_init`] 时，只有一个执行初始化
//! 闭包，其余调用者自旋等待初始化完成，然后直接读取缓存的值。

use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicU8, Ordering},
};

/// 只初始化一次的值容器。
///
/// 与 `std::sync::OnceLock` 等价，但不依赖操作系统支持，可用于内核的
/// `const` 静态初始化。
pub struct OnceLock<T> {
    status: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

// SAFETY: 状态位保证 `value` 在 `INITIALIZED` 之后才对外可见，且初始化
// 期间有且仅有一个执行流写入；`T: Send + Sync` 使初始化完成后可被并发读取。
unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const INITIALIZED: u8 = 2;

impl<T> OnceLock<T> {
    /// 创建一个尚未初始化的 `OnceLock`。
    pub const fn new() -> OnceLock<T> {
        OnceLock {
            status: AtomicU8::new(UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// 若已初始化则返回值的共享引用，否则返回 `None`。
    pub fn get(&self) -> Option<&T> {
        if self.status.load(Ordering::Acquire) != INITIALIZED {
            None
        } else {
            // SAFETY: 状态为 `INITIALIZED` 时 `value` 一定已写入。
            unsafe { Some(&*(*self.value.get()).as_ptr()) }
        }
    }

    /// 返回已初始化的值；若尚未初始化，则执行闭包 `f` 初始化并缓存。
    ///
    /// 并发调用时只有一个执行流运行 `f`，其余自旋等待。
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        if let Some(val) = self.get() {
            return val;
        }

        while let Err(current) = self.status.compare_exchange(
            UNINITIALIZED,
            INITIALIZING,
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            if current == INITIALIZED {
                // SAFETY: 别的执行流已完成初始化并置位 `INITIALIZED`。
                return unsafe { &*(*self.value.get()).as_ptr() };
            }

            core::hint::spin_loop();
        }

        let val = f();

        // SAFETY: 只有赢得状态竞争的执行流会写 `value`，此时没有其它读者。
        unsafe {
            self.value.get().write(MaybeUninit::new(val));
        }

        self.status.store(INITIALIZED, Ordering::Release);

        // SAFETY: 刚完成写入并发布 `INITIALIZED`，后续 `Acquire` 读取可见。
        unsafe { &*(*self.value.get()).as_ptr() }
    }
}

impl<T> Drop for OnceLock<T> {
    fn drop(&mut self) {
        if *self.status.get_mut() == INITIALIZED {
            // SAFETY: `&mut self` 保证没有其它引用，可以就地析构值。
            unsafe {
                self.value.get_mut().assume_init_drop();
            }
        }
    }
}
