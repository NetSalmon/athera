#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use core::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdAllocError {
    OutOfRange,
    AlreadyFree,
}

#[derive(Debug)]
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
    pub fn new() -> IdAllocator {
        Self {
            start: 0,
            end: 0,
            inuse: 0,
            ranges: BTreeMap::new(),
        }
    }

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
