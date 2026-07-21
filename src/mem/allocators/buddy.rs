use core::ops::Range;

use crate::{
    constants::{BUDDY_MAX_ORDER, PAGE_SIZE_LOG_2, align, ilog2_ceil},
    debug,
    mem::allocators::intrusive_list::IntrusiveList,
};

pub struct BuddyAllocator {
    pub free_list: [IntrusiveList; BUDDY_MAX_ORDER],
}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            free_list: [IntrusiveList::new(); BUDDY_MAX_ORDER],
        }
    }

    fn add_frame(&mut self, start: usize, end: usize) {
        let mut current = start;
        for order in (0..BUDDY_MAX_ORDER).rev() {
            debug!("order: {order}, current start: {current:#x}");
            if current >= end {
                break;
            }
            let order_size = order_size(order);

            let chunks = (end - current) / order_size;

            for _ in 0..chunks {
                debug!("order: {order}, push chunk: {current:#x}");
                self.free_list[order].push(current as *mut _);
                current += order_size;
            }
        }
    }

    pub fn add(&mut self, range: &Range<usize>) {
        let start = align(range.start, 1 << (BUDDY_MAX_ORDER + 11));
        self.add_frame(start, range.end);
    }

    pub fn alloc(&mut self, order: usize) -> Option<usize> {
        for i in order..self.free_list.len() {
            if self.free_list[i].is_empty() {
                continue;
            }

            let chunk = self.free_list[i].pop()? as usize;

            for idx in (order..i).rev() {
                let left = chunk + order_size(idx);
                self.free_list[idx].push(left as *mut _);
            }

            return Some(chunk);
        }

        None
    }

    pub fn dealloc(&mut self, start: usize, order: usize) {
        let mut current_ptr = start;
        let mut current_order = order;

        for i in order..BUDDY_MAX_ORDER {
            let buddy = get_buddy(current_ptr, current_order);
            if self.free_list[current_order].remove(buddy) {
                current_ptr = if buddy < current_ptr {
                    buddy
                } else {
                    current_ptr
                };
                current_order = i + 1;
                if current_order >= BUDDY_MAX_ORDER {
                    current_order = BUDDY_MAX_ORDER - 1;
                    break;
                }
            } else {
                break;
            }
        }

        self.free_list[current_order].push(current_ptr as *mut _);
    }

    pub fn alloc_frame(&mut self, size: usize) -> Option<usize> {
        let order = size_to_order(size);
        self.alloc(order)
    }

    pub fn dealloc_frame(&mut self, start: usize, size: usize) {
        let order = size_to_order(size);
        self.dealloc(start, order)
    }
}

#[inline]
pub fn size_to_order(size: usize) -> usize {
    let n = ilog2_ceil(size);
    if n <= PAGE_SIZE_LOG_2 {
        0
    } else {
        (n - PAGE_SIZE_LOG_2).min(BUDDY_MAX_ORDER - 1)
    }
}

#[inline]
pub fn get_buddy(base: usize, order: usize) -> usize {
    base ^ order_size(order)
}

#[inline]
pub fn order_size(order: usize) -> usize {
    1 << order << PAGE_SIZE_LOG_2
}
