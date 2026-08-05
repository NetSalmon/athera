//! 懒加载静态 [`LazyLock`]。
//!
//! `athera_const` 的 `#[lazy]` / `#[lazy(spin)]` 属性宏会展开为
//! [`LazyLock`]（必要时内层再包 [`super::spin::SpinLock`]），因此内核中的
//! `MEMORY_RANGE`、`FRAME_ALLOCATOR`、`UART` 等静态都是首次访问时才
//! 初始化的。

use core::{cell::Cell, ops::Deref};

use crate::sync::once::OnceLock;

/// 延迟到首次访问时才执行初始化闭包的静态值。
///
/// 语义与 `std::sync::LazyLock` 一致：`Deref` 或 [`LazyLock::force`]
/// 触发初始化，之后缓存结果。
pub struct LazyLock<T, F = fn() -> T> {
    cell: OnceLock<T>,
    init: Cell<Option<F>>,
}

// SAFETY: `OnceLock` 负责并发初始化；初始化闭包只被调用一次且发生在
// `force()` 内部，`F: Send` 保证闭包可跨执行流传递。
unsafe impl<T, F: Send> Sync for LazyLock<T, F> {}

impl<T, F: FnOnce() -> T> LazyLock<T, F> {
    /// 用初始化闭包 `f` 创建一个懒加载静态。
    pub const fn new(f: F) -> Self {
        Self {
            cell: OnceLock::new(),
            init: Cell::new(Some(f)),
        }
    }

    /// 确保值已初始化并返回其共享引用。
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
