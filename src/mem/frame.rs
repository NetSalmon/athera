#![allow(dead_code)]
//! 物理页帧句柄 [`Frame`]。
//!
//! 持有起始物理地址与大小；`Drop` 时自动归还给 `FRAME_ALLOCATOR`。
use core::ptr;

use crate::mem::allocators::{FRAME_ALLOCATOR, alloc_frame};

/// 物理页帧句柄：持有物理内存区间的起始地址与大小。
///
/// `Drop` 时会把该区间归还给 `FRAME_ALLOCATOR`。
#[derive(Debug)]
pub struct Frame {
    pub start: usize,
    pub size: usize,
}

impl Frame {
    /// 从裸物理地址构造句柄（不经过伙伴系统分配）。
    ///
    /// # Safety
    ///
    /// `start..start+size` 必须是合法、未归还的物理内存区间，且后续
    /// 归还（`Drop`）时不得与其它分配重叠。
    pub unsafe fn from_raw(start: usize, size: usize) -> Self {
        Self { start, size }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { &*ptr::slice_from_raw_parts(self.start as *const u8, self.size) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { &mut *ptr::slice_from_raw_parts_mut(self.start as *mut u8, self.size) }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.start as *const u8
    }

    /// 克隆本帧：分配一个同尺寸的新物理帧并拷贝全部内容。
    ///
    /// 用于 fork 深拷贝父进程的物理页；分配失败返回 `None`。
    pub fn try_clone(&self) -> Option<Frame> {
        let frame = alloc_frame(Some(self.size))?;
        unsafe {
            ptr::copy(self.start as *const u8, frame.start as *mut u8, self.size);
        }
        Some(frame)
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        FRAME_ALLOCATOR
            .force()
            .lock()
            .dealloc_frame(self.start, self.size);
    }
}
