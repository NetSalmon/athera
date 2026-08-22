#![no_std]
//! ID 分配器 [`IdAlloc`]。
//!
//! 基于 `BTreeMap` 维护空闲区间，支持：
//! - 扁平分配：[`IdAlloc::alloc`] / [`IdAlloc::alloc_at`] /
//!   [`IdAlloc::dealloc`]；
//! - 类 dev_t 的两级分配：[`IdAlloc::alloc_major`]（或
//!   [`IdAlloc::alloc_major_at`]）预留一个 `2^MINOR_BITS` 大小、对齐的块，
//!   [`IdAlloc::alloc_minor`] 在该块内分配 minor。
//!
//! [`IdAlloc::dealloc`] 自动路由到 major 子分配器或扁平空闲区；
//! [`IdAlloc::dealloc_major`] 释放整个 major 块。
//!
//! 泛型于 `T: Id` 与常量泛型 `MINOR_BITS`。`MINOR_BITS = 0` 时 major
//! 块大小为 1。
//!
//! # 对齐
//!
//! 构造时要求 `range.start` 的低 `MINOR_BITS` 位为零（`2^MINOR_BITS`
//! 对齐）；`range.end` 无对齐要求。能完整放入 `2^MINOR_BITS` 大小块的
//! 区间才能被 [`IdAlloc::alloc_major`] 预留为 major；末尾不足一个块的
//! 部分只能用于扁平分配。
//!
//! # 路由
//!
//! major 基址是低 `MINOR_BITS` 位为零的完整 ID。子分配器内 minor = `id`
//! 的低 `MINOR_BITS` 位。major 预留本身不占用 minor；`dealloc(id)` 只释放
//! 单个 ID，`dealloc_major(major)` 才释放整个 major 块。

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
///
/// 除序号语义（`MIN` / `MAX` / `next` / `prev` / `distance_to`）外，还提供
/// 与原生位运算的转换（`BITS` / `to_bits` / `from_bits`），供 [`IdAlloc`]
/// 做位段路由（major = id >> MINOR_BITS）。
pub trait Id: Sized + Copy + Ord + core::fmt::Debug {
    /// 该 ID 类型的最小值。
    const MIN: Self;
    /// 该 ID 类型的最大值。
    const MAX: Self;
    /// 该 ID 类型的位宽。
    const BITS: u32;

    /// 返回 `self + 1`；已达最大值时返回 `None`。
    fn next(&self) -> Option<Self>;

    /// 返回 `self - 1`；已达最小值时返回 `None`。
    fn prev(&self) -> Option<Self>;

    /// `self..=*other` 区间内 ID 的数量（要求 `*other >= *self`）。
    fn distance_to(&self, other: &Self) -> usize;

    /// 把 ID 转为原始位（无符号整数，高位补零）。
    fn to_bits(&self) -> u128;

    /// 从原始位构造 ID；超出 `BITS` 的高位被截断。
    fn from_bits(bits: u128) -> Self;
}

macro_rules! impl_id_for_uint {
    ($($t:ty),* $(,)?) => {
        $(
            impl $crate::Id for $t {
                const MIN: Self = 0;
                const MAX: Self = <$t>::MAX;
                const BITS: u32 = core::mem::size_of::<$t>() as u32 * 8;

                fn next(&self) -> Option<Self> {
                    self.checked_add(1)
                }

                fn prev(&self) -> Option<Self> {
                    self.checked_sub(1)
                }

                fn distance_to(&self, other: &Self) -> usize {
                    (other - self) as usize
                }

                fn to_bits(&self) -> u128 {
                    *self as u128
                }

                fn from_bits(bits: u128) -> Self {
                    bits as $t
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
    /// 区间或 ID 未对齐到 `2^MINOR_BITS`。
    #[error("range or id not aligned to 2^MINOR_BITS")]
    NotAligned,
    /// major 不存在（已被释放或从未预留）。
    #[error("major not found")]
    MajorNotFound,
}

/// 返回 `2^n` 的低 `n` 位掩码；`n >= 128` 时返回全 1。
fn low_mask(n: usize) -> u128 {
    if n == 0 {
        0
    } else if n >= 128 {
        u128::MAX
    } else {
        (1u128 << n) - 1
    }
}

/// 返回 `2^n`；`n >= 128` 时返回 0（调用方应保证 `n < T::BITS`）。
fn block_size(n: usize) -> u128 {
    if n == 0 {
        1
    } else if n >= 128 {
        0
    } else {
        1u128 << n
    }
}

/// 把空闲 ID `n` 插入 `ranges`，并与相邻空闲区间自动合并。
///
/// 与 [`FlatAlloc::return_range`] 不同，本函数处理单个 ID 的释放
/// （包括 `T::MAX` 时插入零长区间的退化情形，与旧行为一致）。
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

/// 扁平空闲区管理器（私有）。
///
/// 在 [`IdAlloc`] 内部用于两处：
/// - 父分配器的扁平空闲区（被 major 划走的块已从中移除）；
/// - 每个 major 的子分配器（管理 `2^MINOR_BITS` 大小块内的 minor）。
#[derive(Debug)]
struct FlatAlloc<T: Id> {
    start: T,
    end: T,
    /// 已分配（扁平路径分配的单个 ID）计数；不含被 major 划走的块。
    inuse: usize,
    /// 空闲区间表（半开区间 `[start, end)`）。
    ranges: BTreeMap<T, T>,
}

impl<T: Id> FlatAlloc<T> {
    fn new(range: Range<T>) -> Self {
        let mut map = BTreeMap::new();
        if range.start < range.end {
            map.insert(range.start, range.end);
        }
        Self {
            start: range.start,
            end: range.end,
            inuse: 0,
            ranges: map,
        }
    }

    fn contains(&self, id: T) -> bool {
        id >= self.start && id < self.end
    }

    fn size(&self) -> usize {
        self.start.distance_to(&self.end)
    }

    fn used(&self) -> usize {
        self.inuse
    }

    fn available(&self) -> usize {
        self.ranges.iter().map(|(&s, &e)| s.distance_to(&e)).sum()
    }

    fn is_allocated(&self, id: T) -> bool {
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

    /// 在扁平空闲区中查找一个 `2^n` 对齐、大小为 `2^n` 的块的基址。
    fn find_aligned_block(&self, n: usize) -> Option<T> {
        let mask = low_mask(n);
        let block = block_size(n);
        if block == 0 {
            return None;
        }
        for (&s, &e) in &self.ranges {
            let sb = s.to_bits();
            let eb = e.to_bits();
            let aligned = (sb.wrapping_add(mask)) & !mask;
            if aligned + block <= eb {
                return Some(T::from_bits(aligned));
            }
        }
        None
    }

    fn alloc(&mut self) -> Option<T> {
        if self.available() == 0 {
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

    fn alloc_at(&mut self, id: T) -> Result<(), IdAllocError> {
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

    fn dealloc(&mut self, id: T) -> Result<(), IdAllocError> {
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

    /// 从扁平空闲区划出 `range`（要求当前完全空闲）。
    fn carve(&mut self, range: Range<T>) -> Result<(), IdAllocError> {
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
        Ok(())
    }

    /// 把 `range` 整段归还给扁平空闲区（与相邻区间自动合并）。
    fn return_range(&mut self, range: Range<T>) {
        if range.start >= range.end {
            return;
        }
        let mut new_start = range.start;
        let mut new_end = range.end;
        if let Some((&s, &e)) = self.ranges.range(..range.start).next_back()
            && e == range.start
        {
            new_start = s;
            self.ranges.remove(&s);
        }
        if let Some((&s, &e)) = self.ranges.get_key_value(&range.end)
            && s == range.end
        {
            new_end = e;
            self.ranges.remove(&s);
        }
        self.ranges.insert(new_start, new_end);
    }

    fn reset(&mut self, range: Range<T>) {
        self.start = range.start;
        self.end = range.end;
        self.inuse = 0;
        self.ranges.clear();
        if range.start < range.end {
            self.ranges.insert(range.start, range.end);
        }
    }
}

/// 一级子分配器槽位（私有），由 [`IdAlloc::alloc_major`] / 初始化创建。
#[derive(Debug)]
struct MajorSlot<T: Id> {
    inner: FlatAlloc<T>,
}

/// 基于 `BTreeMap` 的 ID 分配器。
///
/// 详见模块文档。`MINOR_BITS = 0` 时 major 块大小为 1。
pub struct IdAlloc<T: Id = usize, const MINOR_BITS: usize = 0> {
    flat: FlatAlloc<T>,
    majors: BTreeMap<T, MajorSlot<T>>,
}

impl<T: Id, const N: usize> Default for IdAlloc<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Id, const N: usize> IdAlloc<T, N> {
    /// 创建空分配器（需 [`init`](Self::init) 或
    /// [`from_range`](Self::from_range) 之后才能分配）。
    pub fn new() -> IdAlloc<T, N> {
        IdAlloc {
            flat: FlatAlloc::new(T::MIN..T::MIN),
            majors: BTreeMap::new(),
        }
    }

    /// 用 `range` 作为整个范围创建分配器。
    ///
    /// 当 `N > 0` 时要求 `range.start` 的低 `N` 位为零（`2^N` 对齐）；
    /// `range.end` 无对齐要求。`N >= T::BITS` 时视为非法（major 空间为空）。
    pub fn from_range(range: Range<T>) -> Result<IdAlloc<T, N>, IdAllocError> {
        Self::validate_range(&range)?;
        Ok(IdAlloc {
            flat: FlatAlloc::new(range),
            majors: BTreeMap::new(),
        })
    }

    /// 重置并设为给定范围（对齐要求同 [`from_range`](Self::from_range)）。
    pub fn init(&mut self, range: Range<T>) -> Result<(), IdAllocError> {
        Self::validate_range(&range)?;
        self.flat.reset(range);
        self.majors.clear();
        Ok(())
    }

    fn validate_range(range: &Range<T>) -> Result<(), IdAllocError> {
        if N >= T::BITS as usize {
            return Err(IdAllocError::OutOfRange);
        }
        if range.start.to_bits() & low_mask(N) != 0 {
            return Err(IdAllocError::NotAligned);
        }
        Ok(())
    }

    /// 块基址 `base` 对应的块末尾（半开区间）。
    fn block_end(base: T) -> T {
        T::from_bits(base.to_bits().wrapping_add(block_size(N)))
    }

    /// 计算 `id` 所属 major 的基址（低 `N` 位清零）。
    fn token_base(id: T) -> T {
        T::from_bits(id.to_bits() & !low_mask(N))
    }

    pub fn size(&self) -> usize {
        self.flat.size()
    }

    /// 已分配的 ID 总数（扁平分配 + 所有 major 内已分配 minor）。
    pub fn used(&self) -> usize {
        self.flat.used() + self.majors.values().map(|s| s.inner.used()).sum::<usize>()
    }

    /// 仍可分配的 ID 总数（扁平空闲 + 所有 major 内空闲 minor）。
    pub fn available(&self) -> usize {
        self.flat.available()
            + self
                .majors
                .values()
                .map(|s| s.inner.available())
                .sum::<usize>()
    }

    pub fn is_full(&self) -> bool {
        self.available() == 0
    }

    /// 是否没有任何已分配 ID（含 major 内 minor）。已预留但未分配 minor
    /// 的 major 不计入“已分配”。
    pub fn is_empty(&self) -> bool {
        self.used() == 0
    }

    pub fn contains(&self, id: T) -> bool {
        self.flat.contains(id)
    }

    pub fn is_allocated(&self, id: T) -> bool {
        if !self.flat.contains(id) {
            return false;
        }
        let base = Self::token_base(id);
        if let Some(slot) = self.majors.get(&base) {
            slot.inner.is_allocated(id)
        } else {
            self.flat.is_allocated(id)
        }
    }

    /// 释放全部 ID 与 major，回到初始范围。
    pub fn reset(&mut self) {
        self.flat.reset(self.flat.start..self.flat.end);
        self.majors.clear();
    }

    // ---- 扁平路径（只在未被 major 划走的空闲区分配）----

    /// 在扁平空闲区分配一个 ID；区间用尽时返回 `None`。
    ///
    /// 不会自动预留 major，也不会进入已预留 major 的块。
    pub fn alloc(&mut self) -> Option<T> {
        self.flat.alloc()
    }

    /// 在扁平空闲区或 major 子分配器中分配指定 ID（自动路由）。
    ///
    /// 若 `id` 落在某个已预留 major 的块内，则进入该子分配器；否则在
    /// 扁平空闲区分配。等价于“指定 ID 必为空闲则占用之”。
    pub fn alloc_at(&mut self, id: T) -> Result<(), IdAllocError> {
        if !self.flat.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }
        let base = Self::token_base(id);
        if let Some(slot) = self.majors.get_mut(&base) {
            slot.inner.alloc_at(id)
        } else {
            self.flat.alloc_at(id)
        }
    }

    /// 释放一个 ID（自动路由到所属 major 子分配器或扁平空闲区）。
    pub fn dealloc(&mut self, id: T) -> Result<(), IdAllocError> {
        if !self.flat.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }
        let base = Self::token_base(id);
        if let Some(slot) = self.majors.get_mut(&base) {
            slot.inner.dealloc(id)
        } else {
            self.flat.dealloc(id)
        }
    }

    // ---- major/minor 路径（两级分配）----

    /// 预留一个 `2^N` 大小、`2^N` 对齐的块，返回其基址。
    ///
    /// 在扁平空闲区中查找首个可用对齐块；找不到返回 `None`。
    pub fn alloc_major(&mut self) -> Option<T> {
        let base = self.flat.find_aligned_block(N)?;
        let end = Self::block_end(base);
        self.flat
            .carve(base..end)
            .expect("find_aligned_block 已验证可划走");
        self.majors.insert(
            base,
            MajorSlot {
                inner: FlatAlloc::new(base..end),
            },
        );
        Some(base)
    }

    /// 预留指定基址的块（要求 `base` 的低 `N` 位为零、范围全空闲）。
    pub fn alloc_major_at(&mut self, base: T) -> Result<T, IdAllocError> {
        if base.to_bits() & low_mask(N) != 0 {
            return Err(IdAllocError::NotAligned);
        }
        if !self.flat.contains(base) {
            return Err(IdAllocError::OutOfRange);
        }
        let end_bits = base.to_bits().wrapping_add(block_size(N));
        if end_bits == 0 || end_bits > self.flat.end.to_bits() {
            return Err(IdAllocError::OutOfRange);
        }
        if self.majors.contains_key(&base) {
            return Err(IdAllocError::AlreadyAllocated);
        }
        let end = T::from_bits(end_bits);
        self.flat.carve(base..end)?;
        self.majors.insert(
            base,
            MajorSlot {
                inner: FlatAlloc::new(base..end),
            },
        );
        Ok(base)
    }

    /// 在 major 块内分配一个 minor（ID）；用尽返回 `None`。
    pub fn alloc_minor(&mut self, major: T) -> Option<T> {
        let slot = self.majors.get_mut(&major)?;
        slot.inner.alloc()
    }

    /// 释放 major：强制清空其子分配器，整块归还给扁平空闲区。
    pub fn dealloc_major(&mut self, base: T) -> Result<(), IdAllocError> {
        if self.majors.remove(&base).is_none() {
            return Err(IdAllocError::MajorNotFound);
        }
        let end = Self::block_end(base);
        self.flat.return_range(base..end);
        Ok(())
    }
}
