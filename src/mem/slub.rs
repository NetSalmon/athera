use crate::locks::{LazyLock, SpinLock};
use crate::mem::PAGE_SIZE;
use crate::mem::frame_allocator::FRAME_ALLOCATOR;
use crate::mem::linked_list::LinkedList;

#[const_val::const_val]
pub const SLUB_MAX_ORDER: usize = 11;

#[const_val::const_val]
pub const SLUB_MIN_ORDER: usize = 4;

pub const CACHES_MAX: usize = SLUB_MAX_ORDER - SLUB_MIN_ORDER;
pub static CACHES: LazyLock<SpinLock<[Option<Cache>; CACHES_MAX]>> = LazyLock::new(
    || SpinLock::new([None; CACHES_MAX])
);

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

pub fn create_objects(object_size: usize) -> LinkedList {
    let page = FRAME_ALLOCATOR
        .force()
        .lock()
        .alloc(0)
        .expect("out of memory");

    let mut list = LinkedList::new();

    for i in (page..(page + PAGE_SIZE)).step_by(object_size) {
        list.push(i as *mut usize);
    }

    list
}
