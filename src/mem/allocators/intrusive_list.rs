#![allow(dead_code)]
//! 侵入式单链表。
//!
//! “下一个节点指针”存放在节点自身的首字处，不额外分配内存，供伙伴
//! 系统与 SLUB 的空闲链表复用。
use core::{
    fmt,
    fmt::{Debug, Formatter},
    marker::PhantomData,
};

#[derive(Copy, Clone)]
pub struct IntrusiveList {
    next: *mut usize,
}

impl IntrusiveList {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.next.is_null()
    }

    pub fn push(&mut self, item: *mut usize) {
        unsafe { *item = self.next as usize };
        self.next = item;
    }

    pub fn pop(&mut self) -> Option<*mut usize> {
        match self.is_empty() {
            true => None,
            false => {
                let item = self.next;
                self.next = unsafe { *item as *mut usize };
                Some(item)
            }
        }
    }

    pub fn remove(&mut self, item: usize) -> bool {
        if self.next as usize == item {
            self.next = unsafe { *self.next as *mut usize };
            return true;
        }

        let mut current = self.next;
        while !current.is_null() {
            let next = unsafe { *current };
            if next == item {
                unsafe { *current = *(next as *mut usize) }
                return true;
            }
            current = unsafe { *current as *mut usize };
        }

        false
    }

    pub fn iter(&self) -> IntrusiveListIter<'_> {
        IntrusiveListIter {
            current: self.next,
            _marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IntrusiveListIterMut<'_> {
        IntrusiveListIterMut {
            current: self.next,
            _marker: PhantomData,
        }
    }
}

impl Debug for IntrusiveList {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.write_str("[ ")?;
        for i in self.iter() {
            fmt.write_fmt(format_args!("{:#x} ", i as usize))?;
        }
        fmt.write_str("]")
    }
}

impl PartialEq for IntrusiveList {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next
    }
}

pub struct IntrusiveListIter<'a> {
    current: *mut usize,
    _marker: PhantomData<&'a mut IntrusiveList>,
}

impl<'a> Iterator for IntrusiveListIter<'a> {
    type Item = *mut usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            let item = self.current;
            self.current = unsafe { *item as *mut usize };
            Some(item)
        }
    }
}

pub struct IntrusiveListIterMut<'a> {
    current: *mut usize,
    _marker: PhantomData<&'a mut IntrusiveList>,
}

impl<'a> Iterator for IntrusiveListIterMut<'a> {
    type Item = *mut usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            let item = self.current;
            self.current = unsafe { *item as *mut usize };
            Some(item)
        }
    }
}
