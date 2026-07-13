use core::fmt;
use core::fmt::{Debug, Formatter};
use core::marker::PhantomData;

#[derive(Copy, Clone)]
pub struct FreeList {
    next: *mut usize,
}

impl FreeList {
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

    pub fn iter(&self) -> LinkedListIter<'_> {
        LinkedListIter {
            current: self.next,
            _marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> LinkedListIterMut<'_> {
        LinkedListIterMut {
            current: self.next,
            _marker: PhantomData,
        }
    }
}

impl Debug for FreeList {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        fmt.write_str("[ ")?;
        for i in self.iter() {
            fmt.write_fmt(format_args!("{:#x} ", i as usize))?;
        }
        fmt.write_str("]")
    }
}

impl PartialEq for FreeList {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next
    }
}

pub struct LinkedListIter<'a> {
    current: *mut usize,
    _marker: PhantomData<&'a mut FreeList>,
}

impl<'a> Iterator for LinkedListIter<'a> {
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

pub struct LinkedListIterMut<'a> {
    current: *mut usize,
    _marker: PhantomData<&'a mut FreeList>,
}

impl<'a> Iterator for LinkedListIterMut<'a> {
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
