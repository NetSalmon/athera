//! SLUB 风格的对象分配器（全局堆）。
//!
//! 按对象大小分为多档 [`Cache`]；每档从伙伴系统取页并切成等大对象。
//! 大对象（超过 `SLUB_MAX_ORDER`）直接走伙伴系统。
use core::{alloc::Layout, cmp::max, fmt, marker::PhantomData, ptr::null_mut};

use crate::{
    constants::{
        CACHES_MAX, MAX_CPU, PAGE_SIZE, SLUB_MAX_ORDER, SLUB_MAX_PAGE_SIZE, SLUB_MIN_OBJECTS,
        SLUB_MIN_ORDER, align,
    },
    debug,
    mem::allocators::{FRAME_ALLOCATOR, intrusive_list::IntrusiveList},
    sync::{per_cpu::PerCpu, spin::SpinLock},
    trace,
};

#[derive(Debug)]
pub struct CpuCache(pub PerCpu<SlowCaches, MAX_CPU>);

#[derive(Debug)]
pub struct SlowCaches(pub(crate) [Cache; CACHES_MAX]);

pub struct Caches {
    cpu_caches: CpuCache,
    slow_caches: SpinLock<SlowCaches>,
}

impl Caches {
    pub fn new() -> Self {
        Self {
            cpu_caches: CpuCache(PerCpu::new(core::array::from_fn(|_| SlowCaches::new()))),
            slow_caches: SpinLock::new(SlowCaches::new()),
        }
    }

    pub fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.cpu_caches.0.current().alloc(layout);

        if !ptr.is_null() {
            return ptr;
        }

        self.slow_caches.lock().alloc(layout)
    }

    pub fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !self.cpu_caches.0.current().dealloc(ptr, layout) {
            self.slow_caches.lock().dealloc(ptr, layout);
        }
    }

    pub fn snapshot(&self) {
        debug!("{:#?}", self.slow_caches.lock().0);
    }
}

// SAFETY: `slow_caches` 由自旋锁串行化对 `SlowCaches` 的访问，
// `cpu_caches` 通过 `PerCpu` 让各 hart 只访问自己的槽位，互不相交，
// 因此 `Caches` 可安全地在执行流之间共享或转移。
unsafe impl Send for Caches {}
unsafe impl Sync for Caches {}

impl SlowCaches {
    pub fn new() -> Self {
        Self(core::array::from_fn(|i| {
            Cache::new(2usize.pow((i + SLUB_MIN_ORDER) as u32))
        }))
    }
}

impl SlowCaches {
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let order = layout_order(&layout);

        if order >= SLUB_MAX_ORDER {
            return FRAME_ALLOCATOR
                .force()
                .lock()
                .alloc_frame(1 << order)
                .map(|addr| addr as *mut _)
                .unwrap_or(null_mut());
        }

        let index = order - SLUB_MIN_ORDER;
        let ptr = self.0[index]
            .alloc()
            .map(|p| p as *mut u8)
            .unwrap_or(null_mut());

        trace!(
            "alloc size: {}, order: {}, ptr: {:#x}",
            layout.size(),
            order,
            ptr as usize
        );
        ptr
    }

    pub fn dealloc(&mut self, ptr: *mut u8, layout: Layout) -> bool {
        let order = layout_order(&layout);

        let found = if order >= SLUB_MAX_ORDER {
            FRAME_ALLOCATOR
                .force()
                .lock()
                .dealloc_frame(ptr as usize, 1 << order);
            true
        } else {
            let index = order - SLUB_MIN_ORDER;

            self.0[index].dealloc(ptr as usize)
        };

        trace!(
            "dealloc size: {}, order: {}, ptr: {:#x}",
            layout.size(),
            order,
            ptr as usize
        );

        found
    }
}

#[inline]
fn layout_order(layout: &Layout) -> usize {
    let adjusted_size = max(layout.size(), layout.align());
    let size = max(adjusted_size, 1 << SLUB_MIN_ORDER);
    size.next_power_of_two().trailing_zeros() as usize
}

#[derive(Debug)]
pub struct SlubPage {
    page_size: usize,
    next: *mut SlubPage,
    free_list: IntrusiveList,
    inuse: usize,
    objects: usize,
    page_start: *mut u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Cache {
    objects_size: usize,
    partial_slubs: SlubPageList,
    full_slubs: SlubPageList,
}

#[derive(Clone, Copy)]
pub struct SlubPageList {
    head: *mut SlubPage,
}

impl SlubPageList {
    pub const fn new() -> Self {
        Self { head: null_mut() }
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    pub fn push(&mut self, node: *mut SlubPage) {
        unsafe {
            (*node).next = self.head;
        }
        self.head = node;
    }

    pub fn pop(&mut self) -> Option<*mut SlubPage> {
        if self.is_empty() {
            None
        } else {
            let node = self.head;
            self.head = unsafe { (*node).next };
            Some(node)
        }
    }

    pub fn remove(&mut self, item: *mut SlubPage) -> bool {
        if self.head.is_null() {
            return false;
        }

        if self.head == item {
            self.head = unsafe { (*self.head).next };
            return true;
        }

        let mut current = self.head;
        while !current.is_null() {
            let next = unsafe { (*current).next };
            if next == item {
                unsafe {
                    (*current).next = (*next).next;
                }
                return true;
            }
            current = unsafe { (*current).next };
        }

        false
    }

    pub fn iter(&self) -> SlubPageListIter<'_> {
        SlubPageListIter {
            current: self.head,
            _marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> SlubPageListIterMut<'_> {
        SlubPageListIterMut {
            current: self.head,
            _marker: PhantomData,
        }
    }
}

impl fmt::Debug for SlubPageList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|p| p as *const _ as usize))
            .finish()
    }
}

pub struct SlubPageListIter<'a> {
    current: *mut SlubPage,
    _marker: PhantomData<&'a SlubPageList>,
}

impl<'a> Iterator for SlubPageListIter<'a> {
    type Item = &'a SlubPage;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            let node = unsafe { &*self.current };
            self.current = node.next;
            Some(node)
        }
    }
}

pub struct SlubPageListIterMut<'a> {
    current: *mut SlubPage,
    _marker: PhantomData<&'a mut SlubPageList>,
}

impl<'a> Iterator for SlubPageListIterMut<'a> {
    type Item = &'a mut SlubPage;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            let node = unsafe { &mut *self.current };
            self.current = node.next;
            Some(node)
        }
    }
}

impl SlubPage {
    pub fn is_full(&self) -> bool {
        self.inuse == self.objects
    }

    pub fn is_empty(&self) -> bool {
        self.inuse == 0
    }

    pub fn new<'a>(object_size: usize) -> Option<&'a mut SlubPage> {
        let header_size = size_of::<SlubPage>();

        let start_offset = align(header_size, object_size);

        let min_page_size = start_offset + object_size;
        let mut page_size = PAGE_SIZE;
        while page_size < min_page_size {
            page_size *= 2;
        }

        let desired_objects = page_size / object_size;
        if desired_objects < SLUB_MIN_OBJECTS && page_size < SLUB_MAX_PAGE_SIZE {
            let new_page_size = page_size * 2;
            if new_page_size / object_size >= 2 {
                page_size = new_page_size;
            }
        }

        let page = FRAME_ALLOCATOR.force().lock().alloc_frame(page_size)?;

        let page_ptr = page as *mut SlubPage;

        unsafe {
            page_ptr.write_volatile(SlubPage {
                page_size,
                inuse: 0,
                next: null_mut(),
                free_list: IntrusiveList::new(),
                objects: 0,
                page_start: page as *mut u8,
            });
        }

        let ptr = unsafe { &mut *page_ptr };

        let mut count = 0;
        let mut pos = page + start_offset;
        while pos + object_size <= page + page_size {
            ptr.free_list.push(pos as *mut usize);
            pos += object_size;
            count += 1;
        }

        ptr.objects = count;
        Some(ptr)
    }

    pub fn alloc_obj(&mut self) -> Option<usize> {
        if self.free_list.is_empty() {
            None
        } else {
            self.free_list.pop().map(|p| {
                self.inuse += 1;
                p as usize
            })
        }
    }

    pub fn dealloc_obj(&mut self, ptr: usize) {
        self.free_list.push(ptr as *mut usize);
        self.inuse -= 1;
    }
}

impl Cache {
    pub fn new(objects_size: usize) -> Self {
        Self {
            objects_size,
            partial_slubs: SlubPageList::new(),
            full_slubs: SlubPageList::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.partial_slubs.is_empty()
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if !self.partial_slubs.is_empty() {
            let page = self.partial_slubs.iter_mut().next()?;

            let ptr = page.alloc_obj()?;

            if page.is_full() {
                self.full_slubs.push(self.partial_slubs.pop()?);
            }

            return Some(ptr);
        }

        let page = SlubPage::new(self.objects_size)?;

        let ptr = page.alloc_obj()?;

        self.partial_slubs.push(page);

        Some(ptr)
    }

    pub fn dealloc(&mut self, ptr: usize) -> bool {
        let mut found_page: Option<*mut SlubPage> = None;
        for i in self.full_slubs.iter_mut() {
            let page_start = i.page_start as usize;
            if ptr >= page_start && ptr < page_start + i.page_size {
                i.dealloc_obj(ptr);

                found_page = Some(i as *mut SlubPage);
                break;
            }
        }

        if let Some(page_ptr) = found_page {
            self.full_slubs.remove(page_ptr);
            self.partial_slubs.push(page_ptr);

            return true;
        }

        let mut found_page = None;
        for i in self.partial_slubs.iter_mut() {
            let page_start = i.page_start as usize;
            if ptr >= page_start && ptr < page_start + i.page_size {
                i.dealloc_obj(ptr);

                if i.is_empty() {
                    found_page = Some(i as *mut SlubPage);
                    break;
                }

                return true;
            }
        }

        if let Some(page_ptr) = found_page {
            self.partial_slubs.remove(page_ptr);
            let page_size = unsafe { (*page_ptr).page_size };
            FRAME_ALLOCATOR
                .force()
                .lock()
                .dealloc_frame(page_ptr as usize, page_size);

            return true;
        }

        false
    }
}
