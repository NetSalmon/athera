use core::ptr;

use crate::mem::allocators::FRAME_ALLOCATOR;

#[derive(Debug)]
pub struct AllocPage {
    pub start: usize,
    pub size: usize,
}

impl AllocPage {
    pub unsafe fn from_raw(start: usize, size: usize) -> Self {
        Self { start, size }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { &*ptr::slice_from_raw_parts(self.start as *const u8, self.size) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { &mut *ptr::slice_from_raw_parts_mut(self.start as *mut u8, self.size) }
    }
}

impl Drop for AllocPage {
    fn drop(&mut self) {
        FRAME_ALLOCATOR
            .force()
            .lock()
            .dealloc_frame(self.start, self.size);
    }
}
