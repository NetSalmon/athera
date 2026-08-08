#![no_std]
//! ID 分配器 [`IdAllocator`]。
//!
//! 基于 `BTreeMap` 维护空闲区间（区间起点为键、终点为值），提供
//! 分配、指定分配、释放与重置；内核用它分配 TID。
//!
//! 分配器对 ID 类型泛型化：实现 [`Id`] 的类型均可使用（`usize`、`u32`
//! 等原生无符号整数，或 `#[derive(Id)]` 自动实现的元组结构体包装类型）。
//! 此外通过 [`alloc_range`](IdAllocator::alloc_range) 支持一级子分配：从
//! 父分配器中划出一段区间得到 [`SubAllocator`]，它可独立在该区间内分配，
//! 释放（drop）时自动把区间归还给父分配器。

extern crate alloc;

use alloc::collections::BTreeMap;
use core::ops::Range;

pub use athera_macros::Id;

/// ID 类型的抽象接口。
///
/// 内置实现于 `u8`、`u16`、`u32`、`u64`、`usize`；对自定义单字段包装
/// 类型可用 `#[derive(Id)]`（由 `athera_macros` 提供并经本 crate 再导出）
/// 自动实现，且派生宏会一并自动实现 `Debug`、`Clone`、`Copy`、`PartialEq`、
/// `Eq`、`PartialOrd`、`Ord`。
pub trait Id: Sized + Copy + Ord + core::fmt::Debug {
    /// 该 ID 类型的最小值。
    const MIN: Self;
    /// 该 ID 类型的最大值。
    const MAX: Self;

    /// 返回 `self + 1`；已达最大值时返回 `None`。
    fn next(&self) -> Option<Self>;

    /// 返回 `self - 1`；已达最小值时返回 `None`。
    fn prev(&self) -> Option<Self>;

    /// `self..=*other` 区间内 ID 的数量（要求 `*other >= *self`）。
    fn distance_to(&self, other: &Self) -> usize;
}

macro_rules! impl_id_for_uint {
    ($($t:ty),* $(,)?) => {
        $(
            impl $crate::Id for $t {
                const MIN: Self = 0;
                const MAX: Self = <$t>::MAX;

                fn next(&self) -> Option<Self> {
                    self.checked_add(1)
                }

                fn prev(&self) -> Option<Self> {
                    self.checked_sub(1)
                }

                fn distance_to(&self, other: &Self) -> usize {
                    (other - self) as usize
                }
            }
        )*
    };
}

impl_id_for_uint!(u8, u16, u32, u64, usize);

/// ID 分配错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdAllocError {
    /// ID 超出分配器管理范围。
    #[error("id out of range")]
    OutOfRange,
    /// 尝试分配的 ID 已被占用。
    #[error("id is already allocated")]
    AlreadyAllocated,
    /// 释放的 ID 已是空闲状态。
    #[error("id is already free")]
    AlreadyFree,
    /// 子分配区间为空。
    #[error("range is empty")]
    EmptyRange,
    /// 子分配区间内存在已分配的 ID。
    #[error("range contains allocated ids")]
    RangeBusy,
}

/// 把空闲 ID `n` 插入 `ranges`，并与相邻空闲区间自动合并。
fn insert_free_range<T: Id>(ranges: &mut BTreeMap<T, T>, n: T) {
    let mut new_start = n;
    let mut new_end = n.next().unwrap_or(n);

    if let Some((&s, &e)) = ranges.range(..n).next_back()
        && e == n
    {
        new_start = s;
        ranges.remove(&s);
    }

    if let Some(next) = n.next()
        && let Some((&s, &e)) = ranges.get_key_value(&next)
    {
        new_end = e;
        ranges.remove(&s);
    }

    ranges.insert(new_start, new_end);
}

#[derive(Debug)]
/// 基于空闲区间（`BTreeMap<start, end>`）的 ID 分配器。
pub struct IdAllocator<T: Id = usize> {
    start: T,
    end: T,
    inuse: usize,
    ranges: BTreeMap<T, T>,
}

impl<T: Id> Default for IdAllocator<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Id> IdAllocator<T> {
    /// 创建空分配器（需先 [`init`](Self::init) 或使用
    /// [`from_range`](Self::from_range)）。
    pub fn new() -> IdAllocator<T> {
        Self {
            start: T::MIN,
            end: T::MIN,
            inuse: 0,
            ranges: BTreeMap::new(),
        }
    }

    /// 用 `range` 作为整个空闲区间创建分配器。
    pub fn from_range(range: Range<T>) -> IdAllocator<T> {
        let mut map = BTreeMap::new();
        map.insert(range.start, range.end);
        Self {
            start: range.start,
            end: range.end,
            inuse: 0,
            ranges: map,
        }
    }

    /// 重置并设为给定区间。
    pub fn init(&mut self, range: Range<T>) {
        self.start = range.start;
        self.end = range.end;
        self.inuse = 0;
        self.ranges.clear();
        self.ranges.insert(range.start, range.end);
    }

    pub fn size(&self) -> usize {
        self.start.distance_to(&self.end)
    }

    pub fn used(&self) -> usize {
        self.inuse
    }

    pub fn available(&self) -> usize {
        self.size() - self.inuse
    }

    pub fn is_full(&self) -> bool {
        self.available() == 0
    }

    pub fn is_empty(&self) -> bool {
        self.inuse == 0
    }

    pub fn contains(&self, id: T) -> bool {
        id >= self.start && id < self.end
    }

    pub fn is_allocated(&self, id: T) -> bool {
        if !self.contains(id) {
            return false;
        }
        for (&start, &end) in &self.ranges {
            if id >= start && id < end {
                return false;
            }
            if start > id {
                break;
            }
        }
        true
    }

    /// 分配一个 ID；区间用尽时返回 `None`。
    pub fn alloc(&mut self) -> Option<T> {
        if self.is_full() {
            return None;
        }

        let (id, end) = self.ranges.pop_first()?;
        self.inuse += 1;

        if let Some(next) = id.next()
            && next < end
        {
            self.ranges.insert(next, end);
        }

        Some(id)
    }

    /// 尝试分配指定的 ID。
    pub fn alloc_specific(&mut self, id: T) -> Result<(), IdAllocError> {
        if !self.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }

        if self.is_allocated(id) {
            return Err(IdAllocError::AlreadyAllocated);
        }

        let (&start, &end) = self
            .ranges
            .range(..=id)
            .next_back()
            .expect("空闲 ID 必被某个空闲区间覆盖");
        self.ranges.remove(&start);
        if start < id {
            self.ranges.insert(start, id);
        }
        if let Some(next) = id.next()
            && next < end
        {
            self.ranges.insert(next, end);
        }

        self.inuse += 1;
        Ok(())
    }

    /// 释放一个 ID（与相邻空闲区间自动合并）。
    pub fn dealloc(&mut self, id: T) -> Result<(), IdAllocError> {
        if !self.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }

        if !self.is_allocated(id) {
            return Err(IdAllocError::AlreadyFree);
        }

        insert_free_range(&mut self.ranges, id);
        self.inuse -= 1;

        Ok(())
    }

    /// 释放全部 ID，回到初始状态。
    pub fn reset(&mut self) {
        self.ranges.clear();
        self.ranges.insert(self.start, self.end);
        self.inuse = 0;
    }

    /// 判断 `range` 内是否全部空闲（只读，不修改分配器）。
    ///
    /// `range` 为空或超出管理范围时返回 `false`。
    pub fn is_range_free(&self, range: Range<T>) -> bool {
        if range.start >= range.end {
            return false;
        }
        if range.start < self.start || range.end > self.end {
            return false;
        }
        let fe = match self.ranges.range(..=range.start).next_back() {
            Some((&_s, &e)) => e,
            None => return false,
        };
        range.end <= fe
    }

    /// 从分配器中划出 `range` 区间作为一级子分配器。
    ///
    /// 要求 `range` 非空、位于管理范围内且全部空闲；成功返回一个
    /// [`SubAllocator`]，它可在该区间内独立分配，释放（drop）时自动把
    /// 整段区间归还给本分配器。不支持递归子分配。
    pub fn alloc_range(&mut self, range: Range<T>) -> Result<SubAllocator<'_, T>, IdAllocError> {
        if range.start >= range.end {
            return Err(IdAllocError::EmptyRange);
        }
        if range.start < self.start || range.end > self.end {
            return Err(IdAllocError::OutOfRange);
        }

        let (fs, fe) = match self.ranges.range(..=range.start).next_back() {
            Some((&s, &e)) => (s, e),
            None => return Err(IdAllocError::RangeBusy),
        };
        if range.end > fe {
            return Err(IdAllocError::RangeBusy);
        }

        self.ranges.remove(&fs);
        if fs < range.start {
            self.ranges.insert(fs, range.start);
        }
        if range.end < fe {
            self.ranges.insert(range.end, fe);
        }
        self.inuse += range.start.distance_to(&range.end);

        Ok(SubAllocator::new(self, range))
    }
}

impl<T: Id> From<Range<T>> for IdAllocator<T> {
    fn from(range: Range<T>) -> Self {
        Self::from_range(range)
    }
}

/// 一级子分配器，由 [`IdAllocator::alloc_range`] 创建。
///
/// 在父分配器划出的区间内独立分配 ID；区间释放时（drop）自动归还给
/// 父分配器。
#[must_use]
#[derive(Debug)]
pub struct SubAllocator<'a, T: Id> {
    parent: &'a mut IdAllocator<T>,
    inner: IdAllocator<T>,
    range: Range<T>,
}

impl<'a, T: Id> SubAllocator<'a, T> {
    fn new(parent: &'a mut IdAllocator<T>, range: Range<T>) -> SubAllocator<'a, T> {
        Self {
            parent,
            inner: IdAllocator::from_range(range.clone()),
            range,
        }
    }

    /// 在子区间内分配一个 ID；用尽时返回 `None`。
    pub fn alloc(&mut self) -> Option<T> {
        self.inner.alloc()
    }

    /// 释放子区间内的一个 ID。
    pub fn dealloc(&mut self, id: T) -> Result<(), IdAllocError> {
        self.inner.dealloc(id)
    }

    pub fn size(&self) -> usize {
        self.inner.size()
    }

    pub fn used(&self) -> usize {
        self.inner.used()
    }

    pub fn available(&self) -> usize {
        self.inner.available()
    }

    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn contains(&self, id: T) -> bool {
        self.inner.contains(id)
    }

    pub fn is_allocated(&self, id: T) -> bool {
        self.inner.is_allocated(id)
    }
}

impl<T: Id> Drop for SubAllocator<'_, T> {
    fn drop(&mut self) {
        let mut id = self.range.start;
        while id < self.range.end {
            let _ = self.parent.dealloc(id);
            id = match id.next() {
                Some(next) => next,
                None => break,
            };
        }
    }
}
