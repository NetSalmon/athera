//! 页表项与单页页表容器。

use crate::{bits, constants::PTE_NUMBER, mm::address::PhysicalAddr};

bits! {
    pub type PageTableEntry: u64 {
        v: 0,
        r: 1,
        w: 2,
        x: 3,
        u: 4,
        g: 5,
        a: 6,
        d: 7,
        flags: 0 => 7,
        rwx: 1 => 3,
        ppn0: 10 => 18,
        ppn1: 19 => 27,
        ppn2: 28 => 53,
        ppn: 10 => 53,
    }
}

bits! {
    pub type PageTableEntryFlags : usize {
        v: 0,
        r: 1,
        w: 2,
        x: 3,
        u: 4,
        g: 5,
        a: 6,
        d: 7,
        rwx: 1 => 3,
    }
}

#[repr(align(4096))]
#[derive(Debug)]
pub struct PageTable {
    pub(super) entries: [PageTableEntry; PTE_NUMBER],
}

impl PageTable {
    pub const fn new() -> PageTable {
        PageTable {
            entries: [PageTableEntry::new(); PTE_NUMBER],
        }
    }

    pub fn insert(&mut self, entry: PageTableEntry, index: usize) {
        self.entries[index] = entry;
    }

    #[inline]
    pub fn nth(&self, index: usize) -> Option<&PageTableEntry> {
        self.entries.get(index)
    }

    #[inline]
    pub fn nth_as_addr(&self, index: usize) -> Option<PhysicalAddr> {
        let pte = self.nth(index)?;
        let mut pa = PhysicalAddr::new();
        pa.set_ppn(pte.ppn() as usize);
        Some(pa)
    }

    #[inline]
    pub fn as_ptr(&self) -> *const PageTable {
        self as *const PageTable
    }

    #[inline]
    pub fn as_phys_addr(&self) -> PhysicalAddr {
        PhysicalAddr::from(self.as_ptr() as usize)
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut PageTable {
        self as *mut PageTable
    }
}
