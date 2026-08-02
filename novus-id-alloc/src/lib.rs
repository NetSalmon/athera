#![no_std]
//! ID 分配器 [`IdAllocator`]。
//!
//! 基于 `BTreeMap` 维护空闲区间（区间起点为键、终点为值），提供
//! 分配、指定分配、释放与重置；内核用它分配 TID。

extern crate alloc;

use alloc::collections::BTreeMap;
use core::ops::Range;

/// ID 分配错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdAllocError {
    OutOfRange,
    AlreadyFree,
}

#[derive(Debug)]
/// 基于空闲区间（`BTreeMap<start, end>`）的 ID 分配器。
pub struct IdAllocator {
    start: usize,
    end: usize,
    inuse: usize,
    ranges: BTreeMap<usize, usize>,
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdAllocator {
    /// 创建空分配器（需先 [`init`](Self::init) 或使用
    /// [`from_range`](Self::from_range)）。
    pub fn new() -> IdAllocator {
        Self {
            start: 0,
            end: 0,
            inuse: 0,
            ranges: BTreeMap::new(),
        }
    }

    /// 用 `range` 作为整个空闲区间创建分配器。
    pub fn from_range(range: Range<usize>) -> IdAllocator {
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
    pub fn init(&mut self, range: Range<usize>) {
        self.start = range.start;
        self.end = range.end;
        self.inuse = 0;
        self.ranges.clear();
        self.ranges.insert(range.start, range.end);
    }

    pub const fn size(&self) -> usize {
        self.end - self.start
    }

    pub const fn used(&self) -> usize {
        self.inuse
    }

    pub const fn available(&self) -> usize {
        self.size() - self.inuse
    }

    pub fn is_full(&self) -> bool {
        self.available() == 0
    }

    pub fn is_empty(&self) -> bool {
        self.inuse == 0
    }

    pub fn contains(&self, id: usize) -> bool {
        id >= self.start && id < self.end
    }

    pub fn is_allocated(&self, id: usize) -> bool {
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
    pub fn alloc(&mut self) -> Option<usize> {
        if self.is_full() {
            return None;
        }

        let (id, end) = self.ranges.pop_first()?;
        self.inuse += 1;

        let start = id + 1;
        if start < end {
            self.ranges.insert(start, end);
        }

        Some(id)
    }

    /// 尝试分配指定的 ID。
    pub fn alloc_specific(&mut self, id: usize) -> Result<(), IdAllocError> {
        if !self.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }

        if self.is_allocated(id) {
            return Err(IdAllocError::AlreadyFree);
        }

        let ranges: alloc::vec::Vec<_> = self.ranges.iter().map(|(&s, &e)| (s, e)).collect();
        for (start, end) in ranges {
            if id >= start && id < end {
                self.ranges.remove(&start);
                if start < id {
                    self.ranges.insert(start, id);
                }
                if id + 1 < end {
                    self.ranges.insert(id + 1, end);
                }
                self.inuse += 1;
                return Ok(());
            }
        }

        Err(IdAllocError::AlreadyFree)
    }

    fn insert_free_range(&mut self, n: usize) {
        let mut new_start = n;
        let mut new_end = n + 1;

        if let Some((&s, &e)) = self.ranges.range(..n).next_back()
            && e == n
        {
            new_start = s;
            self.ranges.remove(&s);
        }

        if let Some((&s, &e)) = self.ranges.get_key_value(&(n + 1)) {
            new_end = e;
            self.ranges.remove(&s);
        }

        self.ranges.insert(new_start, new_end);
    }

    /// 释放一个 ID（与相邻空闲区间自动合并）。
    pub fn dealloc(&mut self, id: usize) -> Result<(), IdAllocError> {
        if !self.contains(id) {
            return Err(IdAllocError::OutOfRange);
        }

        if !self.is_allocated(id) {
            return Err(IdAllocError::AlreadyFree);
        }

        self.insert_free_range(id);
        self.inuse -= 1;

        Ok(())
    }

    /// 释放一段连续 ID。
    pub fn dealloc_range(&mut self, range: Range<usize>) -> Result<(), IdAllocError> {
        if range.start < self.start || range.end > self.end {
            return Err(IdAllocError::OutOfRange);
        }

        for id in range.clone() {
            if !self.is_allocated(id) {
                return Err(IdAllocError::AlreadyFree);
            }
        }

        for id in range {
            self.insert_free_range(id);
            self.inuse -= 1;
        }

        Ok(())
    }

    /// 释放全部 ID，回到初始状态。
    pub fn reset(&mut self) {
        self.ranges.clear();
        self.ranges.insert(self.start, self.end);
        self.inuse = 0;
    }
}

impl From<Range<usize>> for IdAllocator {
    fn from(range: Range<usize>) -> Self {
        Self::from_range(range)
    }
}
