use crate::debug;
use crate::locks::{LazyLock, SpinLock};
use crate::mem::constants::PAGE_SIZE;
use crate::mem::frame::FRAME_ALLOCATOR;
use crate::mem::intrusive_list::IntrusiveList;
use core::alloc::{GlobalAlloc, Layout};
use core::cmp::max;
use core::fmt;
use core::marker::PhantomData;
use core::ptr::null_mut;

#[const_val::const_val]
pub const MAX_KERNEL_HEAP_SIZE: usize = 20 * 1024 * 1024;

#[const_val::const_val]
pub const SLUB_MAX_ORDER: usize = 11;

#[const_val::const_val]
pub const SLUB_MIN_ORDER: usize = 4;

pub const CACHES_MAX: usize = SLUB_MAX_ORDER - SLUB_MIN_ORDER;

#[derive(Debug)]
pub struct Caches([Cache; CACHES_MAX]);

impl Caches {
    pub fn new() -> Self {
        Self(core::array::from_fn(|i| {
            Cache::new(2usize.pow((i + SLUB_MIN_ORDER) as u32))
        }))
    }
}

impl Caches {
    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let order = layout_order(&layout);

        if order > SLUB_MAX_ORDER {
            return FRAME_ALLOCATOR
                .force()
                .lock()
                .alloc_frame(1 << order)
                .unwrap() as *mut _;
        }

        let index = order - SLUB_MIN_ORDER;
        let ptr = self.0[index].alloc() as *mut u8;

        debug!(
            "alloc size: {}, order: {}, ptr: {:#x}",
            layout.size(),
            order,
            ptr as usize
        );
        ptr
    }

    fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let order = layout_order(&layout);

        if order > SLUB_MAX_ORDER {
            return FRAME_ALLOCATOR
                .force()
                .lock()
                .dealloc_frame(ptr as usize, 1 << order);
        }

        let index = order - SLUB_MIN_ORDER;

        self.0[index].dealloc(ptr as usize);

        debug!(
            "dealloc size: {}, order: {}, ptr: {:#x}",
            layout.size(),
            order,
            ptr as usize
        );
    }
}

#[inline]
fn layout_order(layout: &Layout) -> usize {
    let adjusted_size = max(layout.size(), layout.align());
    let size = max(adjusted_size, 1 << SLUB_MIN_ORDER);
    size.next_power_of_two().trailing_zeros() as usize
}

#[global_allocator]
pub static CACHES: LazyLock<SpinLock<Caches>> = LazyLock::new(|| SpinLock::new(Caches::new()));

unsafe impl GlobalAlloc for LazyLock<SpinLock<Caches>> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.force().lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.force().lock().dealloc(ptr, layout)
    }
}

#[derive(Debug)]
pub struct SlubPage {
    next: *mut SlubPage,
    free_list: IntrusiveList,
    inuse: usize,
    objects: usize,
    page_start: *mut u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Cache {
    objects_size: usize,
    free_slubs: SlubPageList,
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

    pub fn new<'a>(object_size: usize) -> &'a mut SlubPage {
        let page = FRAME_ALLOCATOR
            .force()
            .lock()
            .alloc(0)
            .expect("out of memory");

        let page_ptr = page as *mut SlubPage;

        unsafe {
            page_ptr.write_volatile(SlubPage {
                inuse: 0,
                next: null_mut(),
                free_list: IntrusiveList::new(),
                objects: 0,
                page_start: page as *mut u8,
            });
        }

        let ptr = unsafe { &mut *page_ptr };

        let header_size = size_of::<SlubPage>();
        let align = object_size;
        let start_offset = (header_size + align - 1) & !(align - 1);

        let mut count = 0;
        let mut pos = page + start_offset;
        while pos + object_size <= page + PAGE_SIZE {
            ptr.free_list.push(pos as *mut usize);
            pos += object_size;
            count += 1;
        }

        ptr.objects = count;
        ptr
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
            free_slubs: SlubPageList::new(),
            partial_slubs: SlubPageList::new(),
            full_slubs: SlubPageList::new(),
        }
    }

    pub fn alloc(&mut self) -> usize {
        if !self.partial_slubs.is_empty() {
            let page = self.partial_slubs.iter_mut().next().unwrap();

            let ptr = page.alloc_obj().unwrap();

            if page.is_full() {
                self.full_slubs.push(self.partial_slubs.pop().unwrap());
            }

            return ptr;
        }

        if !self.free_slubs.is_empty() {
            let page = self.free_slubs.pop().unwrap();

            let ptr = unsafe { &mut *page }.alloc_obj().unwrap();

            self.partial_slubs.push(page);

            return ptr;
        }

        let page = SlubPage::new(self.objects_size);

        let ptr = page.alloc_obj().unwrap();

        self.partial_slubs.push(page);

        ptr
    }

    pub fn dealloc(&mut self, ptr: usize) {
        let page_start = ptr & !(PAGE_SIZE - 1);

        let mut found = None;
        for i in self.full_slubs.iter_mut() {
            if i.page_start as usize == page_start {
                i.dealloc_obj(ptr);

                found = Some(i as *mut SlubPage);

                break;
            }
        }

        if let Some(i) = found {
            self.full_slubs.remove(i);
            self.partial_slubs.push(i);

            return;
        }

        let mut found = None;
        for i in self.partial_slubs.iter_mut() {
            if i.page_start as usize == page_start {
                i.dealloc_obj(ptr);

                if i.is_empty() {
                    found = Some(i as *mut SlubPage);
                    break;
                }

                return;
            }
        }

        if let Some(i) = found {
            self.partial_slubs.remove(i);
            FRAME_ALLOCATOR
                .force()
                .lock()
                .dealloc_frame(i as usize, PAGE_SIZE);
        }
    }
}

pub fn snapshot() {
    debug!("{:#?}", CACHES.force().lock().0);
}
