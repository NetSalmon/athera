use crate::mem::linked_list::LinkedList;

pub struct SlubPage {
    next: *mut SlubPage,
    free_list: LinkedList,
    inuse: usize,
    objects: usize,
    page_start: *mut u8,
}

pub struct Cache {
    page_size: usize,
    objects_size: usize,
    free_slubs: *mut SlubPage,
    partial_slubs: *mut SlubPage,
    full_slubs: *mut SlubPage,
}
