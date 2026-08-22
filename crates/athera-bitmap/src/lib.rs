#![no_std]
//! 定长位图 [`BitMap`] 与零拷贝位图视图 [`BitMapView`]。
//!
//! 用 `usize` 字数组按位存储占用状态（1 = 非空闲/已占用，0 = 空闲）。
//!
//! - [`BitMap`]：自有的 `[usize; WORDS]` 数组，零堆分配；支持从
//!   `[usize; WORDS]`、`&[usize]`、`&[u8]`、`[u8; N]` 转换（见
//!   [`From`] / [`TryFrom`]）；
//! - [`BitMapView`]：借用已有内存（包括从裸指针零拷贝创建），直接读写
//!   底层数据，不复制。
//!
//! 两类接口：
//!
//! - 分配器风格：找空闲位并占用（[`alloc`](BitMap::alloc)、
//!   [`alloc_specific`](BitMap::alloc_specific)、[`free`](BitMap::free) 等）；
//! - 原始按位操作：直接读写某一位（[`set`](BitMap::set)、
//!   [`clear`](BitMap::clear)、[`get`](BitMap::get) 等）。
//!
//! # 容量
//!
//! 泛型参数 `WORDS` 是内部机器字数，`new()` 时可用位数即为
//! `WORDS * usize::BITS`；需要精确位数可用 [`words_for`] 计算字数，
//! 再配合 [`new_with_capacity`](BitMap::new_with_capacity)：
//!
//! ```
//! use athera_bitmap::{words_for, BitMap};
//!
//! let mut bm = BitMap::<{ words_for(1024) }>::new_with_capacity(1024);
//! let idx = bm.alloc().unwrap();          // 找到第一个空闲位并占用
//! assert!(bm.is_allocated(idx));
//! bm.alloc_specific(42).unwrap();         // 把指定下标设为非空闲
//! bm.free(idx).unwrap();                  // 释放
//! ```
//!
//! # 零拷贝视图
//!
//! ```
//! use athera_bitmap::BitMapView;
//!
//! let mut frames = [0usize; 64];          // 假设是映射到的一段内存
//! let mut view = unsafe { BitMapView::from_raw(frames.as_mut_ptr(), 64) };
//! let idx = view.alloc().unwrap();
//! assert!(view.is_allocated(idx));
//! ```

use core::{mem::size_of, ops::Range};

/// 一个机器字包含的位数。
const WORD_BITS: usize = usize::BITS as usize;

/// 位图操作错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BitMapError {
    /// 位下标超出位图容量。
    #[error("bit index out of range")]
    OutOfRange,
    /// 目标位已置位（非空闲）。
    #[error("bit is already set")]
    AlreadySet,
    /// 目标位已空闲。
    #[error("bit is already free")]
    AlreadyFree,
    /// 输入数据长度与位图容量不匹配。
    #[error("input length does not match bitmap capacity")]
    LengthMismatch,
    /// 输入的字节缓冲区未按 `usize` 对齐。
    #[error("input buffer is not aligned")]
    Misaligned,
}

/// 计算表示 `bits` 个位所需的机器字数（`bits.div_ceil(usize::BITS)`）。
///
/// 用于在泛型参数中按位数指定容量，例如
/// `BitMap::<{ words_for(1024) }>`。
#[must_use]
pub const fn words_for(bits: usize) -> usize {
    bits.div_ceil(WORD_BITS)
}

/// 与 `usize` 等长的字节数组（`size_of::<usize>()` 字节）。
type WordBytes = [u8; size_of::<usize>()];

// ---------------------------------------------------------------------------
// 共享底层：对一段 `[usize]` 内存做位操作
// ---------------------------------------------------------------------------

/// 读取 `index` 位是否为 1（调用方须保证 `index < bits`）。
#[inline]
fn is_bit_set(words: &[usize], index: usize) -> bool {
    words[index / WORD_BITS] & (1 << (index % WORD_BITS)) != 0
}

/// 置位 `index`（调用方须保证 `index < bits`）。
#[inline]
fn set_bit(words: &mut [usize], index: usize) {
    words[index / WORD_BITS] |= 1 << (index % WORD_BITS);
}

/// 清零 `index`（调用方须保证 `index < bits`）。
#[inline]
fn clear_bit(words: &mut [usize], index: usize) {
    words[index / WORD_BITS] &= !(1 << (index % WORD_BITS));
}

/// 统计已占用位数。
fn count_used(words: &[usize]) -> usize {
    words.iter().map(|w| w.count_ones() as usize).sum()
}

/// 在 `words` 中从 `start`（含）起查找第一个满足 `want_free` 的位下标。
fn scan_words(words: &[usize], bits: usize, start: usize, want_free: bool) -> Option<usize> {
    if words.is_empty() || start >= bits {
        return None;
    }
    let last = words.len() - 1;
    let mut word = start / WORD_BITS;
    let mut low_mask = (1usize << (start % WORD_BITS)) - 1;
    loop {
        let mut w = words[word];
        if want_free {
            // start 之前的位视为已占用，之后按存储值取反得到空闲位。
            w |= low_mask;
            w = !w;
        } else {
            // start 之前的位视为空闲，直接忽略。
            w &= !low_mask;
        }
        if word == last && !bits.is_multiple_of(WORD_BITS) {
            // 屏蔽最后一字中超出可用位数的保留位。
            w &= (1usize << (bits % WORD_BITS)) - 1;
        }
        if w != 0 {
            let bit = w.trailing_zeros() as usize;
            return Some(word * WORD_BITS + bit);
        }
        if word == last {
            return None;
        }
        word += 1;
        low_mask = 0;
    }
}

/// 在 `words` 中分配 `n` 个连续空闲位并置位，返回起始下标。
fn alloc_range_in(words: &mut [usize], bits: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let mut run_start = 0;
    let mut run_len = 0;
    for i in 0..bits {
        if is_bit_set(words, i) {
            run_len = 0;
        } else {
            if run_len == 0 {
                run_start = i;
            }
            run_len += 1;
            if run_len == n {
                for j in run_start..run_start + n {
                    set_bit(words, j);
                }
                return Some(run_start);
            }
        }
    }
    None
}

/// 释放 `range` 内连续位（左闭右开）；其中任意一位已空闲时返回
/// [`BitMapError::AlreadyFree`] 且不修改任何位。
fn free_range_in(words: &mut [usize], bits: usize, range: Range<usize>) -> Result<(), BitMapError> {
    if range.start > range.end || range.end > bits {
        return Err(BitMapError::OutOfRange);
    }
    for i in range.clone() {
        if !is_bit_set(words, i) {
            return Err(BitMapError::AlreadyFree);
        }
    }
    for i in range {
        clear_bit(words, i);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自有位图
// ---------------------------------------------------------------------------

/// 固定 `WORDS` 个机器字的自有位图。
///
/// 可用位数由 [`bits`](Self::bits) 给出：`[new](Self::new)` 为
/// `WORDS * usize::BITS`，`[new_with_capacity](Self::new_with_capacity)`
/// 为给定值；最后一字超出可用位数的保留位不会通过任何接口被分配或返回。
/// `used()` 为 O(1)（内部维护计数器）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitMap<const WORDS: usize> {
    words: [usize; WORDS],
    bits: usize,
    used: usize,
}

impl<const WORDS: usize> Default for BitMap<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const WORDS: usize> BitMap<WORDS> {
    /// 底层可表示的最大位数（`WORDS * usize::BITS`）。
    pub const CAPACITY: usize = WORDS * WORD_BITS;

    /// 创建位图，可用全部 `WORDS * usize::BITS` 位。
    pub const fn new() -> Self {
        Self {
            words: [0; WORDS],
            bits: WORDS * WORD_BITS,
            used: 0,
        }
    }

    /// 创建位图，只使用前 `bits` 位（超出部分为保留位）。
    ///
    /// `bits` 必须不超过 [`CAPACITY`](Self::CAPACITY)，否则 panic。
    pub const fn new_with_capacity(bits: usize) -> Self {
        assert!(bits <= WORDS * WORD_BITS);
        Self {
            words: [0; WORDS],
            bits,
            used: 0,
        }
    }

    /// 可用位数（容量）。
    #[must_use]
    pub const fn bits(&self) -> usize {
        self.bits
    }

    /// 底层可表示的最大位数。
    #[must_use]
    pub const fn capacity(&self) -> usize {
        WORDS * WORD_BITS
    }

    /// 已占用的位数。
    #[must_use]
    pub const fn used(&self) -> usize {
        self.used
    }

    /// 剩余空闲位数。
    #[must_use]
    pub const fn available(&self) -> usize {
        self.bits - self.used
    }

    /// 是否已全部占用。
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.used == self.bits
    }

    /// 是否全部空闲。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// `index` 是否在可用范围内。
    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        index < self.bits
    }

    /// 读取某一位；越界返回 [`BitMapError::OutOfRange`]。
    pub fn get(&self, index: usize) -> Result<bool, BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        Ok(is_bit_set(&self.words[..], index))
    }

    /// 某位是否已占用；越界视为未占用（`false`）。
    #[must_use]
    pub fn is_allocated(&self, index: usize) -> bool {
        self.contains(index) && is_bit_set(&self.words[..], index)
    }

    /// 某位是否空闲；越界视为未占用（`false`）。
    #[must_use]
    pub fn is_free(&self, index: usize) -> bool {
        self.contains(index) && !self.is_allocated(index)
    }

    /// 直接把指定下标的位设为非空闲（幂等：已置位时仍返回 `Ok`）。
    pub fn set(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if !self.is_allocated(index) {
            set_bit(&mut self.words, index);
            self.used += 1;
        }
        Ok(())
    }

    /// 直接把指定下标的位清为空闲（幂等：已空闲时仍返回 `Ok`）。
    pub fn clear(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if self.is_allocated(index) {
            clear_bit(&mut self.words, index);
            self.used -= 1;
        }
        Ok(())
    }

    /// 分配一个空闲位并占用，返回其下标；位图已满时返回 `None`。
    #[must_use]
    pub fn alloc(&mut self) -> Option<usize> {
        let index = self.find_free()?;
        set_bit(&mut self.words, index);
        self.used += 1;
        Some(index)
    }

    /// 从 `start`（含）起分配第一个空闲位并占用。
    #[must_use]
    pub fn alloc_from(&mut self, start: usize) -> Option<usize> {
        let index = self.find_free_from(start)?;
        set_bit(&mut self.words, index);
        self.used += 1;
        Some(index)
    }

    /// 占用指定下标的位（类似“把指定 index 的 bit 设为非空闲”）。
    ///
    /// 越界返回 [`BitMapError::OutOfRange`]；该位已占用返回
    /// [`BitMapError::AlreadySet`]。幂等置位请用 [`set`](Self::set)。
    pub fn alloc_specific(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if self.is_allocated(index) {
            return Err(BitMapError::AlreadySet);
        }
        set_bit(&mut self.words, index);
        self.used += 1;
        Ok(())
    }

    /// 释放指定下标的位。
    ///
    /// 越界返回 [`BitMapError::OutOfRange`]；该位已空闲返回
    /// [`BitMapError::AlreadyFree`]。幂等清零请用 [`clear`](Self::clear)。
    pub fn free(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if !self.is_allocated(index) {
            return Err(BitMapError::AlreadyFree);
        }
        clear_bit(&mut self.words, index);
        self.used -= 1;
        Ok(())
    }

    /// 分配 `n` 个连续空闲位，返回起始下标；找不到连续区间时返回
    /// `None`（`n == 0` 也返回 `None`）。时间复杂度 `O(bits)`。
    #[must_use]
    pub fn alloc_range(&mut self, n: usize) -> Option<usize> {
        if n == 0 || n > self.available() {
            return None;
        }
        let start = alloc_range_in(&mut self.words, self.bits, n)?;
        self.used += n;
        Some(start)
    }

    /// 释放一段连续位（`range` 为左闭右开区间）。
    ///
    /// 越界返回 [`BitMapError::OutOfRange`]；其中任意一位已空闲返回
    /// [`BitMapError::AlreadyFree`]，且此时不修改任何位。
    pub fn free_range(&mut self, range: Range<usize>) -> Result<(), BitMapError> {
        let len = range.end.saturating_sub(range.start);
        free_range_in(&mut self.words, self.bits, range)?;
        self.used -= len;
        Ok(())
    }

    /// 查找第一个空闲位的下标（只读，不占用）。
    #[must_use]
    pub fn find_free(&self) -> Option<usize> {
        self.find_free_from(0)
    }

    /// 从 `start`（含）起查找第一个空闲位的下标（只读）。
    #[must_use]
    pub fn find_free_from(&self, start: usize) -> Option<usize> {
        if self.used == self.bits {
            return None;
        }
        scan_words(&self.words[..], self.bits, start, true)
    }

    /// 查找第一个已占用位的下标（只读）。
    #[must_use]
    pub fn find_used(&self) -> Option<usize> {
        self.find_used_from(0)
    }

    /// 从 `start`（含）起查找第一个已占用位的下标（只读）。
    #[must_use]
    pub fn find_used_from(&self, start: usize) -> Option<usize> {
        if self.used == 0 {
            return None;
        }
        scan_words(&self.words[..], self.bits, start, false)
    }

    /// 迭代所有已占用位的下标（升序）。
    #[must_use]
    pub fn iter_used(&self) -> BitIter<'_> {
        BitIter {
            words: &self.words[..],
            bits: self.bits,
            want_free: false,
            next: 0,
        }
    }

    /// 迭代所有空闲位的下标（升序）。
    #[must_use]
    pub fn iter_free(&self) -> BitIter<'_> {
        BitIter {
            words: &self.words[..],
            bits: self.bits,
            want_free: true,
            next: 0,
        }
    }

    /// 全部清零，回到全空闲状态。
    pub fn reset(&mut self) {
        self.words.fill(0);
        self.used = 0;
    }
}

// ---------------------------------------------------------------------------
// 自有位图的转换
// ---------------------------------------------------------------------------

impl<const WORDS: usize> From<[usize; WORDS]> for BitMap<WORDS> {
    /// 从精确的字数组转换（按字内容统计已占用位数）。
    fn from(words: [usize; WORDS]) -> Self {
        let used = count_used(&words);
        Self {
            words,
            bits: WORDS * WORD_BITS,
            used,
        }
    }
}

impl<const WORDS: usize> TryFrom<&[usize]> for BitMap<WORDS> {
    type Error = BitMapError;

    /// 从字切片拷贝转换；长度不超过 `WORDS`，超出时只使用前
    /// `len * usize::BITS` 位（类似 [`new_with_capacity`](BitMap::new_with_capacity)）。
    fn try_from(words: &[usize]) -> Result<Self, Self::Error> {
        if words.len() > WORDS {
            return Err(BitMapError::LengthMismatch);
        }
        let mut out = [0usize; WORDS];
        out[..words.len()].copy_from_slice(words);
        Ok(Self {
            words: out,
            bits: words.len() * WORD_BITS,
            used: count_used(words),
        })
    }
}

impl<const WORDS: usize> TryFrom<&[u8]> for BitMap<WORDS> {
    type Error = BitMapError;

    /// 从字节切片拷贝转换，按宿主字节序（native endian）解释；
    /// 长度必须是 `size_of::<usize>()` 的整数倍且换算后不超过 `WORDS`。
    ///
    /// 需要固定字节序时请用 [`try_from_le_bytes`](BitMap::try_from_le_bytes)
    /// 或 [`try_from_be_bytes`](BitMap::try_from_be_bytes)。
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_bytes_with(bytes, usize::from_ne_bytes)
    }
}

impl<const WORDS: usize> BitMap<WORDS> {
    /// 从字节切片拷贝转换，按 **little-endian** 解释；
    /// 长度必须是 `size_of::<usize>()` 的整数倍且换算后不超过 `WORDS`。
    ///
    /// ```
    /// use athera_bitmap::BitMap;
    ///
    /// let bytes = [1u8, 0, 0, 0, 0, 0, 0, 0]; // usize = 1（LE）
    /// let bm = BitMap::<1>::try_from_le_bytes(&bytes).unwrap();
    /// assert!(bm.is_allocated(0));
    /// ```
    pub fn try_from_le_bytes(bytes: &[u8]) -> Result<Self, BitMapError> {
        Self::try_from_bytes_with(bytes, usize::from_le_bytes)
    }

    /// 从字节切片拷贝转换，按 **big-endian** 解释；
    /// 长度必须是 `size_of::<usize>()` 的整数倍且换算后不超过 `WORDS`。
    pub fn try_from_be_bytes(bytes: &[u8]) -> Result<Self, BitMapError> {
        Self::try_from_bytes_with(bytes, usize::from_be_bytes)
    }

    /// `try_from_le_bytes` / `try_from_be_bytes` / `TryFrom<&[u8]>` 的公共实现。
    fn try_from_bytes_with(
        bytes: &[u8],
        from_bytes: fn(WordBytes) -> usize,
    ) -> Result<Self, BitMapError> {
        if !bytes.len().is_multiple_of(size_of::<usize>()) {
            return Err(BitMapError::LengthMismatch);
        }
        let n = bytes.len() / size_of::<usize>();
        if n > WORDS {
            return Err(BitMapError::LengthMismatch);
        }
        let mut out = [0usize; WORDS];
        let (chunks, _) = bytes.as_chunks::<{ size_of::<usize>() }>();
        for (i, chunk) in chunks.iter().enumerate() {
            out[i] = from_bytes(*chunk);
        }
        Ok(Self {
            words: out,
            bits: n * WORD_BITS,
            used: count_used(&out[..n]),
        })
    }
}

impl<const WORDS: usize, const N: usize> TryFrom<[u8; N]> for BitMap<WORDS> {
    type Error = BitMapError;

    /// 从字节数组拷贝转换（语义同 [`TryFrom<&[u8]>`]）。
    fn try_from(bytes: [u8; N]) -> Result<Self, Self::Error> {
        Self::try_from(&bytes[..])
    }
}

impl<const WORDS: usize, const N: usize> TryFrom<&[usize; N]> for BitMap<WORDS> {
    type Error = BitMapError;

    /// 从字数组引用拷贝转换（语义同 [`TryFrom<&[usize]>`]）。
    fn try_from(words: &[usize; N]) -> Result<Self, Self::Error> {
        Self::try_from(&words[..])
    }
}

impl<const WORDS: usize, const N: usize> TryFrom<&[u8; N]> for BitMap<WORDS> {
    type Error = BitMapError;

    /// 从字节数组引用拷贝转换（语义同 [`TryFrom<&[u8]>`]）。
    fn try_from(bytes: &[u8; N]) -> Result<Self, Self::Error> {
        Self::try_from(&bytes[..])
    }
}

// ---------------------------------------------------------------------------
// 零拷贝位图视图
// ---------------------------------------------------------------------------

/// 借用已有内存（`&mut [usize]`）作为位图的零拷贝视图。
///
/// 不复制数据，所有读写都直接作用于底层内存；适用于页帧表、磁盘
/// inode/数据块位图等映射到固定内存区域的场景。可用
/// [`from_raw`](Self::from_raw) 从裸指针零拷贝创建。
///
/// 视图不维护占用计数器，[`used`](Self::used) 等按需扫描，复杂度
/// `O(words)`；其余查找/迭代同样按字加速。
#[derive(Debug)]
pub struct BitMapView<'a> {
    words: &'a mut [usize],
    bits: usize,
}

impl<'a> BitMapView<'a> {
    /// 借用一段可变内存作为位图，可用全部 `words.len() * usize::BITS` 位。
    pub fn new(words: &'a mut [usize]) -> Self {
        let bits = words.len() * WORD_BITS;
        Self { words, bits }
    }

    /// 借用一段可变内存，只使用前 `bits` 位（超出部分为保留位）。
    ///
    /// `bits` 必须不超过 `words.len() * usize::BITS`，否则 panic。
    pub fn new_with_capacity(words: &'a mut [usize], bits: usize) -> Self {
        assert!(bits <= words.len() * WORD_BITS);
        Self { words, bits }
    }

    /// 从裸指针零拷贝创建位图视图，可用全部 `words * usize::BITS` 位。
    ///
    /// # Safety
    ///
    /// - `ptr` 必须按 `usize` 对齐，且指向 `words` 个连续的、已初始化的
    ///   `usize`；
    /// - 视图存活期间，该内存必须保持有效，且不得通过其他引用/指针
    ///   同时可变访问。
    pub unsafe fn from_raw(ptr: *mut usize, words: usize) -> Self {
        let bits = words * WORD_BITS;
        // SAFETY: 由调用方保证指针有效、对齐且未别名。
        let words = unsafe { core::slice::from_raw_parts_mut(ptr, words) };
        Self { words, bits }
    }

    /// 从裸指针零拷贝创建位图视图，只使用前 `bits` 位。
    ///
    /// # Safety
    ///
    /// 同 [`from_raw`](Self::from_raw)；`bits` 必须不超过
    /// `words * usize::BITS`，否则 panic。
    pub unsafe fn from_raw_with_capacity(ptr: *mut usize, words: usize, bits: usize) -> Self {
        assert!(bits <= words * WORD_BITS);
        // SAFETY: 由调用方保证指针有效、对齐且未别名。
        let words = unsafe { core::slice::from_raw_parts_mut(ptr, words) };
        Self { words, bits }
    }

    /// 可用位数（容量）。
    #[must_use]
    pub const fn bits(&self) -> usize {
        self.bits
    }

    /// 已占用的位数（按需扫描，`O(words)`）。
    #[must_use]
    pub fn used(&self) -> usize {
        count_used(self.words)
    }

    /// 剩余空闲位数（按需扫描，`O(words)`）。
    #[must_use]
    pub fn available(&self) -> usize {
        self.bits - self.used()
    }

    /// 是否已全部占用（按需扫描）。
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.used() == self.bits
    }

    /// 是否全部空闲（按需扫描）。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.used() == 0
    }

    /// `index` 是否在可用范围内。
    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        index < self.bits
    }

    /// 读取某一位；越界返回 [`BitMapError::OutOfRange`]。
    pub fn get(&self, index: usize) -> Result<bool, BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        Ok(is_bit_set(&*self.words, index))
    }

    /// 某位是否已占用；越界视为未占用（`false`）。
    #[must_use]
    pub fn is_allocated(&self, index: usize) -> bool {
        self.contains(index) && is_bit_set(&*self.words, index)
    }

    /// 某位是否空闲；越界视为未占用（`false`）。
    #[must_use]
    pub fn is_free(&self, index: usize) -> bool {
        self.contains(index) && !self.is_allocated(index)
    }

    /// 直接把指定下标的位设为非空闲（幂等：已置位时仍返回 `Ok`）。
    pub fn set(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        set_bit(self.words, index);
        Ok(())
    }

    /// 直接把指定下标的位清为空闲（幂等：已空闲时仍返回 `Ok`）。
    pub fn clear(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        clear_bit(self.words, index);
        Ok(())
    }

    /// 分配一个空闲位并占用，返回其下标；位图已满时返回 `None`。
    #[must_use]
    pub fn alloc(&mut self) -> Option<usize> {
        let index = self.find_free()?;
        set_bit(self.words, index);
        Some(index)
    }

    /// 从 `start`（含）起分配第一个空闲位并占用。
    #[must_use]
    pub fn alloc_from(&mut self, start: usize) -> Option<usize> {
        let index = self.find_free_from(start)?;
        set_bit(self.words, index);
        Some(index)
    }

    /// 占用指定下标的位；越界返回 [`BitMapError::OutOfRange`]，该位已占用
    /// 返回 [`BitMapError::AlreadySet`]。
    pub fn alloc_specific(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if self.is_allocated(index) {
            return Err(BitMapError::AlreadySet);
        }
        set_bit(self.words, index);
        Ok(())
    }

    /// 释放指定下标的位；越界返回 [`BitMapError::OutOfRange`]，该位已空闲
    /// 返回 [`BitMapError::AlreadyFree`]。
    pub fn free(&mut self, index: usize) -> Result<(), BitMapError> {
        if !self.contains(index) {
            return Err(BitMapError::OutOfRange);
        }
        if !self.is_allocated(index) {
            return Err(BitMapError::AlreadyFree);
        }
        clear_bit(self.words, index);
        Ok(())
    }

    /// 分配 `n` 个连续空闲位，返回起始下标；找不到连续区间时返回
    /// `None`（`n == 0` 也返回 `None`）。时间复杂度 `O(bits)`。
    #[must_use]
    pub fn alloc_range(&mut self, n: usize) -> Option<usize> {
        if n == 0 || n > self.available() {
            return None;
        }
        alloc_range_in(self.words, self.bits, n)
    }

    /// 释放一段连续位（`range` 为左闭右开区间）。
    ///
    /// 越界返回 [`BitMapError::OutOfRange`]；其中任意一位已空闲返回
    /// [`BitMapError::AlreadyFree`]，且此时不修改任何位。
    pub fn free_range(&mut self, range: Range<usize>) -> Result<(), BitMapError> {
        free_range_in(self.words, self.bits, range)
    }

    /// 查找第一个空闲位的下标（只读，不占用）。
    #[must_use]
    pub fn find_free(&self) -> Option<usize> {
        self.find_free_from(0)
    }

    /// 从 `start`（含）起查找第一个空闲位的下标（只读）。
    #[must_use]
    pub fn find_free_from(&self, start: usize) -> Option<usize> {
        scan_words(&*self.words, self.bits, start, true)
    }

    /// 查找第一个已占用位的下标（只读）。
    #[must_use]
    pub fn find_used(&self) -> Option<usize> {
        self.find_used_from(0)
    }

    /// 从 `start`（含）起查找第一个已占用位的下标（只读）。
    #[must_use]
    pub fn find_used_from(&self, start: usize) -> Option<usize> {
        scan_words(&*self.words, self.bits, start, false)
    }

    /// 迭代所有已占用位的下标（升序）。
    #[must_use]
    pub fn iter_used(&self) -> BitIter<'_> {
        BitIter {
            words: &*self.words,
            bits: self.bits,
            want_free: false,
            next: 0,
        }
    }

    /// 迭代所有空闲位的下标（升序）。
    #[must_use]
    pub fn iter_free(&self) -> BitIter<'_> {
        BitIter {
            words: &*self.words,
            bits: self.bits,
            want_free: true,
            next: 0,
        }
    }

    /// 全部清零，回到全空闲状态。
    pub fn reset(&mut self) {
        self.words.fill(0);
    }
}

impl<'a> From<&'a mut [usize]> for BitMapView<'a> {
    fn from(words: &'a mut [usize]) -> Self {
        Self::new(words)
    }
}

impl<'a> TryFrom<&'a mut [u8]> for BitMapView<'a> {
    type Error = BitMapError;

    /// 借用一段可变字节内存作为位图视图（零拷贝，不复制）。
    ///
    /// 要求长度是 `size_of::<usize>()` 的整数倍且按 `usize` 对齐，否则分别
    /// 返回 [`BitMapError::LengthMismatch`] / [`BitMapError::Misaligned`]。
    fn try_from(bytes: &'a mut [u8]) -> Result<Self, Self::Error> {
        if !bytes.len().is_multiple_of(size_of::<usize>()) {
            return Err(BitMapError::LengthMismatch);
        }
        if !(bytes.as_ptr() as usize).is_multiple_of(core::mem::align_of::<usize>()) {
            return Err(BitMapError::Misaligned);
        }
        let words = bytes.len() / size_of::<usize>();
        let bits = words * WORD_BITS;
        // SAFETY: 长度已校验为 usize 整数倍，且缓冲区已按 usize 对齐。
        let words =
            unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<usize>(), words) };
        Ok(Self { words, bits })
    }
}

// ---------------------------------------------------------------------------
// 迭代器
// ---------------------------------------------------------------------------

/// 位图下标的升序迭代器，由 [`BitMap::iter_used`] / [`BitMap::iter_free`]
/// 与 [`BitMapView::iter_used`] / [`BitMapView::iter_free`] 产生。
#[derive(Clone)]
pub struct BitIter<'a> {
    words: &'a [usize],
    bits: usize,
    want_free: bool,
    next: usize,
}

impl Iterator for BitIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let found = scan_words(self.words, self.bits, self.next, self.want_free)?;
        self.next = found + 1;
        Some(found)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.bits.saturating_sub(self.next)))
    }
}
