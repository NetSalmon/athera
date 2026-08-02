//! 每 CPU（per-hart）存储 [`PerCpu`]。
//!
//! 把一份数据按 hart 复制成 `N` 份，通过 `tp` 寄存器里的 hart id 索引
//! 当前 hart 的槽位，避免不同 hart 之间互相竞争同一把锁。
//!
//! # 安全性
//!
//! - [`PerCpuStorage`] 用 `UnsafeCell` 包装，允许通过 `&self` 获得内部
//!   可变访问；调用方必须保证同一 hart 不会同时持有同一槽位的两个
//!   可变引用。
//! - 槽位下标取自 `tp`（启动时由 `main(hart_id, ...)` 写入），超出
//!   数组长度会越界访问，因此要求 `MAX_CPU` 不小于实际 hart 数。

use core::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

use novus_const::const_val;

use crate::arch::registers::gpr::Tp;

/// 当前 hart 槽位的守卫，由 [`PerCpu::current`] 返回。
///
/// 存活期间独占借用当前 hart 的槽位，`Deref` / `DerefMut` 直接访问
/// 该 hart 的值。
pub struct PerCpuGuard<'a, T> {
    storage: &'a PerCpuStorage<T>,
}

impl<'a, T> Deref for PerCpuGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `UnsafeCell` 允许通过共享引用访问；见模块文档的借用约定。
        unsafe { &*self.storage.0.get() }
    }
}

impl<'a, T> DerefMut for PerCpuGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: `&mut self` 保证不存在其它活跃引用，同一 hart 的槽位
        // 不会被同时可变借用两次。
        unsafe { &mut *self.storage.0.get() }
    }
}

/// 编译期配置的最大 hart 数。
#[const_val]
pub const MAX_CPU: usize = 4;

/// 每 hart 一份的数据集合。
///
/// 槽位 `i` 属于 hart `i`，各 hart 只会访问自己的槽位，因此并发访问
/// 总是落在不相交的内存上；这要求 `T` 在 hart 之间可 `Send` / `Sync`。
#[derive(Debug)]
pub struct PerCpu<T, const N: usize> {
    data: [PerCpuStorage<T>; N],
}

// SAFETY: 各 hart 只读写自己的槽位（互不相交），共享 `&PerCpu` 不会
// 造成数据竞争。
unsafe impl<T, const N: usize> Sync for PerCpu<T, N> {}
unsafe impl<T, const N: usize> Send for PerCpu<T, N> {}

impl<T, const N: usize> PerCpu<T, N> {
    /// 用 `N` 份初始值构建每 hart 存储。
    pub fn new(items: [T; N]) -> Self {
        let data = items.map(PerCpuStorage::new);

        Self { data }
    }

    /// 返回当前 hart（由 `tp` 决定）槽位的可变守卫。
    pub fn current(&self) -> PerCpuGuard<'_, T> {
        let tp = Tp::read() as usize;
        PerCpuGuard {
            storage: &self.data[tp],
        }
    }
}

/// 单个 hart 的存储单元：把 `UnsafeCell` 包进 `ManuallyDrop`，避免
/// 静态（`static`）在进程退出时自动析构。
#[derive(Debug)]
pub struct PerCpuStorage<T>(pub ManuallyDrop<UnsafeCell<T>>);

// SAFETY: 与 `PerCpu` 相同，各 hart 只访问自己的槽位。
unsafe impl<T> Sync for PerCpuStorage<T> {}
unsafe impl<T> Send for PerCpuStorage<T> {}

impl<T> PerCpuStorage<T> {
    /// 包装一个初始值。
    pub const fn new(item: T) -> Self {
        Self(ManuallyDrop::new(UnsafeCell::new(item)))
    }
}
