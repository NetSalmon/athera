use crate::arch::registers::csr::Sie;
use core::cell::{Cell, UnsafeCell};
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub struct SpinLock<T> {
    lock: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> SpinLock<T> {
        SpinLock {
            lock: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let sie = Sie::read();
        Sie::write(0);
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

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    sie: u64,
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.lock.store(false, Ordering::Release);
        Sie::write(self.sie);
    }
}

pub struct OnceLock<T> {
    status: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const INITIALIZED: u8 = 2;

impl<T> OnceLock<T> {
    pub const fn new() -> OnceLock<T> {
        OnceLock {
            status: AtomicU8::new(UNINITIALIZED),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn get(&self) -> Option<&T> {
        if self.status.load(Ordering::Acquire) != INITIALIZED {
            None
        } else {
            unsafe { Some(&*(*self.value.get()).as_ptr()) }
        }
    }

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
                return unsafe { &*(*self.value.get()).as_ptr() };
            }

            core::hint::spin_loop();
        }

        let val = f();

        unsafe {
            self.value.get().write(MaybeUninit::new(val));
        }

        self.status.store(INITIALIZED, Ordering::Release);

        unsafe { &*(*self.value.get()).as_ptr() }
    }
}

impl<T> Drop for OnceLock<T> {
    fn drop(&mut self) {
        if *self.status.get_mut() == INITIALIZED {
            unsafe {
                self.value.get_mut().assume_init_drop();
            }
        }
    }
}

pub struct LazyLock<T, F = fn() -> T> {
    cell: OnceLock<T>,
    init: Cell<Option<F>>,
}

unsafe impl<T, F: Send> Sync for LazyLock<T, F> {}

impl<T, F: FnOnce() -> T> LazyLock<T, F> {
    pub const fn new(f: F) -> Self {
        Self {
            cell: OnceLock::new(),
            init: Cell::new(Some(f)),
        }
    }
    pub fn force(&self) -> &T {
        self.cell.get_or_init(|| {
            let f = self.init.take().unwrap();
            f()
        })
    }
}

impl<T, F: FnOnce() -> T> Deref for LazyLock<T, F> {
    type Target = T;
    fn deref(&self) -> &T {
        self.force()
    }
}
