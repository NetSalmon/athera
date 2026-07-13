use crate::debug;
use crate::locks::{LazyLock, SpinLock};
use crate::mem::PAGE_SIZE;
use crate::mem::frame_allocator::FRAME_ALLOCATOR;
use crate::mem::linked_list::LinkedList;
use core::alloc::{GlobalAlloc, Layout};
use core::fmt;
use core::ptr::null_mut;

#[const_val::const_val]
pub const SLUB_MAX_ORDER: usize = 11;

#[const_val::const_val]
pub const SLUB_MIN_ORDER: usize = 4;

pub const CACHES_MAX: usize = SLUB_MAX_ORDER - SLUB_MIN_ORDER;

pub struct Caches(LazyLock<SpinLock<[Option<Cache>; CACHES_MAX]>>);

impl Caches {
    pub const fn new() -> Self {
        Self(LazyLock::new(|| SpinLock::new([None; CACHES_MAX])))
    }
}

unsafe impl GlobalAlloc for Caches {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let order = size.ilog2() as usize;

        if order > SLUB_MAX_ORDER {
            return FRAME_ALLOCATOR.force().lock().alloc_frame(size).unwrap() as *mut _;
        }

        let index = order - SLUB_MIN_ORDER;

        self.0.force().lock()[index].unwrap().alloc() as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let order = size.ilog2() as usize;

        if order > SLUB_MAX_ORDER {
            return FRAME_ALLOCATOR
                .force()
                .lock()
                .dealloc_frame(ptr as usize, size);
        }

        let index = order - SLUB_MIN_ORDER;

        self.0.force().lock()[index].unwrap().dealloc(ptr as usize);
    }
}

pub static CACHES: Caches = Caches::new();

pub struct SlubPage {
    next: *mut SlubPage,
    free_list: LinkedList,
    inuse: usize,
    objects: usize,
    page_start: *mut u8,
}

#[derive(Clone, Copy)]
pub struct Cache {
    objects_size: usize,
    free_slubs: *mut SlubPage,
    partial_slubs: *mut SlubPage,
    full_slubs: *mut SlubPage,
}

impl SlubPage {
    pub fn new<'a>(object_size: usize) -> &'a SlubPage {
        let page = FRAME_ALLOCATOR
            .force()
            .lock()
            .alloc(0)
            .expect("out of memory");

        debug!("page allocated: {:#x}", page);

        unsafe {
            (page as *mut SlubPage).write_volatile(SlubPage {
                inuse: 0,
                next: null_mut(),
                free_list: LinkedList::new(),
                objects: (PAGE_SIZE - size_of::<SlubPage>()) / object_size,
                page_start: page as *mut u8,
            })
        }

        let ptr = unsafe { &mut *(page as *mut SlubPage) };

        let padding_end = (page + size_of::<SlubPage>() + object_size - 1) & !(object_size - 1);

        for pos in (padding_end..(page + PAGE_SIZE)).step_by(object_size) {
            ptr.free_list.push(pos as *mut usize)
        }

        ptr
    }
}

impl Cache {
    pub fn alloc(&mut self) -> usize {
        todo!()
    }

    pub fn dealloc(&mut self, ptr: usize) {
        todo!()
    }
}

impl fmt::Debug for SlubPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("SlubPage");
        f.field("next", &self.next);
        f.field("inuse", &self.inuse);
        f.field("objects", &self.objects);
        f.field("page_start", &self.page_start);

        f.field(
            "free_list",
            &DebugFreeList {
                list: &self.free_list,
                base: self.page_start as usize,
            },
        );

        f.finish()
    }
}

struct DebugFreeList<'a> {
    list: &'a LinkedList,
    base: usize,
}

impl fmt::Debug for DebugFreeList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_list();
        for p in self.list.iter() {
            f.entry(&((p as usize) - self.base));
        }
        f.finish()
    }
}
