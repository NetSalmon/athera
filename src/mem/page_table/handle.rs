use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use crate::constants::PHY_PAGE_SIZE;
use crate::mem::alloc_page::AllocPage;
use crate::mem::allocators::FRAME_ALLOCATOR;
use crate::mem::page_table::PageTable;

pub struct PageTableHandle<T> {
    ptr: NonNull<T>,
    page: AllocPage,
    _phantom: PhantomData<T>,
}

impl PageTableHandle<PageTable> {
    pub fn new(page_table: PageTable, page: AllocPage) -> Self {
        Self {
            ptr: NonNull::from(&page_table),
            page,
            _phantom: PhantomData
        }
    }

    pub fn create() -> Self {
        let page = ();

        todo!()
    }
}

impl Deref for PageTableHandle<PageTable> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl DerefMut for PageTableHandle<PageTable> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}