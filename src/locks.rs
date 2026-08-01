use core::{
    cell::{Cell, UnsafeCell},
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use crate::arch::registers::{csr::Sie, gpr::Tp};

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
            let f = self
                .init
                .take()
                .expect("LazyLock initializer called more than once");
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

// ─── Per-CPU ─────────────────────────────────────────────────────────

/// Storage for [`PerCPU`].
///
/// The two union views are layout-identical: `UnsafeCell<T>` is
/// `#[repr(transparent)]` over `T`, so arrays of either type have the same
/// layout. The `items` view only exists so the const constructor can build
/// the array from plain values; every runtime access goes through `cells`.
#[repr(C)]
#[allow(dead_code)] // not wired into the kernel yet
union PerCpuStorage<T, const N: usize> {
    items: ManuallyDrop<[T; N]>,
    cells: ManuallyDrop<[UnsafeCell<T>; N]>,
}

/// A per-CPU variable: one `T` for each of the `N` harts.
///
/// Every access targets the slot of the *current* hart, selected by the
/// hart id stored in the `tp` register at boot (see
/// [`crate::arch::registers::gpr::Tp`]). The same static therefore refers
/// to different memory on different harts:
///
/// ```ignore
/// static A: PerCPU<u64, 4> = PerCPU::new([0; 4]);
///
/// *A.current() += 1; // guard derefs mutably to the current hart's slot
/// let b = *A.current() * 2;
/// ```
///
/// # Safety
///
/// `PerCPU` is `Sync` even though slots are mutable through `&self`; this
/// is sound only under the invariant that slot `i` is mutated exclusively
/// by hart `i`, and that no other hart touches it while hart `i` may be
/// mutating it. The caller must also not hold two mutable references to the
/// same slot simultaneously.
#[allow(dead_code)] // not wired into the kernel yet
pub struct PerCPU<T, const N: usize> {
    storage: PerCpuStorage<T, N>,
}

// SAFETY: slot `i` is only accessed by hart `i` (see struct docs), so
// concurrent harts always touch disjoint slots; sharing `&PerCPU` across
// harts is sound as long as `T` itself can be shared/sent between harts.
unsafe impl<T: Send + Sync, const N: usize> Sync for PerCPU<T, N> {}

#[allow(dead_code)] // not wired into the kernel yet
impl<T, const N: usize> PerCPU<T, N> {
    pub const fn new(items: [T; N]) -> Self {
        Self {
            storage: PerCpuStorage {
                items: ManuallyDrop::new(items),
            },
        }
    }

    /// Hart id of the current hart, stored in the `tp` register at boot.
    #[inline]
    fn hart_id() -> usize {
        let hart_id = Tp::read() as usize;
        assert!(
            hart_id < N,
            "PerCPU: hart id {hart_id} out of range (N = {N})"
        );
        hart_id
    }

    /// Shared view of the per-hart slots.
    #[inline]
    fn cells(&self) -> &ManuallyDrop<[UnsafeCell<T>; N]> {
        // SAFETY: `UnsafeCell<T>` is `#[repr(transparent)]` over `T`, so the
        // `items` and `cells` union views describe identical storage; this
        // only borrows the storage, never moving or aliasing it.
        unsafe { &self.storage.cells }
    }

    /// Returns a guard that mutably dereferences to the current hart's
    /// value, e.g. `*A.current() += 1`. See the struct docs for the
    /// aliasing contract.
    #[inline]
    pub fn current(&self) -> PerCPUGuard<'_, T> {
        // SAFETY: `UnsafeCell` grants interior mutability through `&self`;
        // slot `Self::hart_id()` belongs to the current hart, and the
        // caller must not keep two mutable references to it at once.
        PerCPUGuard {
            value: unsafe { &mut *self.cells()[Self::hart_id()].get() },
        }
    }
}

/// A mutable view of the current hart's slot, returned by
/// [`PerCPU::current`].
///
/// Holds the slot exclusively borrowed while alive and dereferences to `T`,
/// so reads (`*g`), writes (`*g = ...`, `g.field = ...`) and method calls
/// go straight to the current hart's value.
#[allow(dead_code)] // not wired into the kernel yet
pub struct PerCPUGuard<'a, T> {
    value: &'a mut T,
}

impl<T> Deref for PerCPUGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
    }
}

impl<T> DerefMut for PerCPUGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value
    }
}
