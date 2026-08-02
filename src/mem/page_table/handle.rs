#![allow(dead_code)]
//! 页表句柄 [`PageTableHandle`]。
//!
//! 包装根页表指针与对应的 [`AllocPage`]，保证页表页的生命周期（`Drop`
//! 时归还物理页），并提供高低半区复制（用于在用户地址空间内继承内核
//! 映射）。
use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::{
    constants::PTE_NUMBER,
    error::{Error, Result},
    mem::{
        alloc_page::AllocPage,
        allocators::alloc_frame,
        page_table::{PageTable, PageTableEntry},
    },
};

#[derive(Debug)]
pub struct PageTableHandle {
    ptr: NonNull<PageTable>,
    page: AllocPage,
    _phantom: PhantomData<PageTable>,
}

impl PageTableHandle {
    pub fn new(page_table: PageTable, page: AllocPage) -> Self {
        Self {
            ptr: NonNull::from(&page_table),
            page,
            _phantom: PhantomData,
        }
    }

    /// 从伙伴系统分配一页并初始化为空的根页表。
    pub fn create() -> Result<Self> {
        let page = alloc_frame(None).ok_or(Error::OutOfMemory)?;

        let page_table = unsafe { &mut *(page.start as *mut PageTable) };

        page_table.entries = [PageTableEntry::default(); PTE_NUMBER];

        Ok(Self {
            ptr: NonNull::from(page_table),
            page,
            _phantom: PhantomData,
        })
    }

    /// 从裸地址包装一个已有的页表（用于接管预分配页表）。
    ///
    /// # Safety
    ///
    /// `addr` 必须指向合法且对齐的页表内存，`size` 与页表的分配大小
    /// 一致；句柄 `Drop` 时会按 `size` 归还物理页。
    pub unsafe fn from_raw(addr: usize, size: usize) -> Self {
        let page = unsafe { AllocPage::from_raw(addr, size) };

        Self {
            ptr: NonNull::new(addr as *mut PageTable)
                .expect("PageTableHandle::from_raw requires a non-null address"),
            page,
            _phantom: PhantomData,
        }
    }

    pub fn copy_high_half(&self, to: &mut PageTableHandle) {
        for i in 256..PTE_NUMBER {
            to.entries[i] = self.entries[i];
        }
    }

    pub fn copy_low_half(&self, to: &mut PageTableHandle) {
        for i in 0..256 {
            to.entries[i] = self.entries[i];
        }
    }

    pub fn copy_from(&self, to: &mut PageTableHandle) {
        self.copy_low_half(to);
        self.copy_high_half(to);
    }
}

impl Deref for PageTableHandle {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl DerefMut for PageTableHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

unsafe impl Send for PageTableHandle {}
unsafe impl Sync for PageTableHandle {}

