use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use crate::{
    constants::PTE_NUMBER,
    mem::{
        alloc_page::AllocPage,
        allocators::alloc_frame,
        page_table::{PageTable, PageTableEntry},
    },
};

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

    pub fn create() -> Self {
        let page = alloc_frame().expect("out of memory");

        let page_table = unsafe { &mut *(page.start as *mut PageTable) };

        page_table.entries = [PageTableEntry::default(); PTE_NUMBER];

        Self {
            ptr: NonNull::from(page_table),
            page,
            _phantom: PhantomData,
        }
    }

    pub unsafe fn from_raw(addr: usize, size: usize) -> Self {
        let page = unsafe { AllocPage::from_raw(addr, size) };

        Self {
            ptr: NonNull::new(addr as *mut PageTable).unwrap(),
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
