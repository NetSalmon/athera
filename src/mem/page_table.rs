pub mod handle;

use alloc::collections::BTreeMap;
use core::{arch::asm, cell::UnsafeCell};

use novus_const::lazy;

use crate::{
    arch::registers::{
        csr::Satp,
        values::{SatpMode, SatpValue},
    },
    bits,
    constants::PAGE_SIZE,
    debug,
    dev::{SYSTEM_MEMORY, UART, VIRTIO_BLK},
    mem::{
        addr::{PhysicalAddr, VirtualAddr},
        allocators::alloc_frame,
    },
};

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

#[lazy]
pub static ROOT_PAGE_TABLE: usize = {
    let root_addr = alloc_frame().expect("out of memory");

    unsafe { (root_addr as *mut PageTable).write(PageTable::new()) };

    root_addr
};

#[repr(align(4096))]
#[derive(Debug)]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> PageTable {
        PageTable {
            entries: [PageTableEntry::new(); 512],
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

#[lazy(spin)]
pub static PAGE_TABLE_MANAGER: PageTableManager<'static> = PageTableManager::new();

pub fn identity_map() {
    debug!("start identity mapping memory");

    let root_pa = *ROOT_PAGE_TABLE.force();
    let root_addr = PhysicalAddr::from(root_pa);
    let root_cell: &'static UnsafeCell<PageTable> =
        unsafe { &*(root_pa as *const UnsafeCell<PageTable>) };

    let mut mgr = PAGE_TABLE_MANAGER.lock();
    mgr.insert(root_addr, root_cell);

    let start = SYSTEM_MEMORY.device.mmio.start;
    let end = start + SYSTEM_MEMORY.device.mmio.size;
    mgr.identity_map(root_addr, start, end);

    let mut flags = PageTableEntryFlags::new();
    flags.set_r(true);
    flags.set_w(true);
    flags.set_x(true);

    if let Some(ref uart) = *UART {
        let start = uart.lock().device.mmio.start;
        mgr.map(
            root_addr,
            VirtualAddr::from(start),
            PhysicalAddr::from(start),
            flags,
            false,
        );
    }

    if let Some(ref blk) = *VIRTIO_BLK {
        let start = blk.device.mmio.start;
        let end = start + blk.device.mmio.size;
        mgr.identity_map(root_addr, start, end);
    }

    debug!("map ok");
    debug!("root pt addr at: {:?}", root_addr);

    mgr.activate(root_addr);

    debug!("identity mapping memory end");
}

pub struct PageTableManager<'a> {
    page_tables: BTreeMap<PhysicalAddr, &'a UnsafeCell<PageTable>>,
}

impl<'a> PageTableManager<'a> {
    pub fn new() -> Self {
        Self {
            page_tables: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, pa: PhysicalAddr, pt: &'a UnsafeCell<PageTable>) {
        self.page_tables.insert(pa, pt);
    }

    pub fn get(&self, pa: PhysicalAddr) -> Option<&'a UnsafeCell<PageTable>> {
        self.page_tables.get(&pa).copied()
    }

    pub fn contains(&self, pa: PhysicalAddr) -> bool {
        self.page_tables.contains_key(&pa)
    }

    pub fn remove(&mut self, pa: PhysicalAddr) -> Option<&'a UnsafeCell<PageTable>> {
        self.page_tables.remove(&pa)
    }

    fn allocate_table(&mut self) -> *mut PageTable {
        let addr = alloc_frame().expect("out of memory");
        unsafe { (addr as *mut PageTable).write(PageTable::new()) };
        let cell_ref: &'a UnsafeCell<PageTable> =
            unsafe { &*(addr as *const UnsafeCell<PageTable>) };
        let pa = PhysicalAddr::from(addr);
        self.page_tables.insert(pa, cell_ref);
        addr as *mut PageTable
    }

    pub fn map(
        &mut self,
        root: PhysicalAddr,
        va: VirtualAddr,
        pa: PhysicalAddr,
        flags: PageTableEntryFlags,
        flush: bool,
    ) {
        let root_ptr = self
            .page_tables
            .get(&root)
            .expect("root page table not found")
            .get();
        let root = unsafe { &mut *root_ptr };

        let l1_ptr = if !root.entries[va.vpn2()].v() {
            let table = self.allocate_table();
            root.entries[va.vpn2()].set_ppn((table as usize >> 12) as u64);
            root.entries[va.vpn2()].set_v(true);
            table
        } else {
            ((root.entries[va.vpn2()].ppn() as usize) << 12) as *mut PageTable
        };

        let l1_table = unsafe { &mut *l1_ptr };
        let l0_ptr = if !l1_table.entries[va.vpn1()].v() {
            let table = self.allocate_table();
            l1_table.entries[va.vpn1()].set_ppn((table as usize >> 12) as u64);
            l1_table.entries[va.vpn1()].set_v(true);
            table
        } else {
            ((l1_table.entries[va.vpn1()].ppn() as usize) << 12) as *mut PageTable
        };

        let l0_table = unsafe { &mut *l0_ptr };
        let mut pte = PageTableEntry::new();
        pte.set_v(true);
        pte.set_r(flags.r());
        pte.set_w(flags.w());
        pte.set_x(flags.x());
        pte.set_u(flags.u());
        pte.set_ppn(pa.ppn() as u64);
        l0_table.entries[va.vpn0()] = pte;

        if flush {
            Self::flush();
        }
    }

    pub fn unmap(&self, root: PhysicalAddr, va: VirtualAddr, flush: bool) {
        let root_ptr = self
            .page_tables
            .get(&root)
            .expect("root page table not found")
            .get();
        let root = unsafe { &mut *root_ptr };

        let pte2 = &root.entries[va.vpn2()];
        if !pte2.v() {
            return;
        }
        let l1_table = unsafe { &mut *(((pte2.ppn() as usize) << 12) as *mut PageTable) };

        let pte1 = &l1_table.entries[va.vpn1()];
        if !pte1.v() {
            return;
        }
        let l0_table = unsafe { &mut *(((pte1.ppn() as usize) << 12) as *mut PageTable) };

        l0_table.entries[va.vpn0()] = PageTableEntry::new();

        if flush {
            Self::flush();
        }
    }

    pub fn translate(&self, root: PhysicalAddr, va: VirtualAddr) -> Option<PhysicalAddr> {
        let root_ptr = self.page_tables.get(&root)?.get();
        let root = unsafe { &*root_ptr };

        let pte2 = root.nth(va.vpn2())?;
        if !pte2.v() {
            return None;
        }
        let l1_table = unsafe { &*(((pte2.ppn() as usize) << 12) as *const PageTable) };

        let pte1 = l1_table.nth(va.vpn1())?;
        if !pte1.v() {
            return None;
        }
        let l0_table = unsafe { &*(((pte1.ppn() as usize) << 12) as *const PageTable) };

        let pte0 = l0_table.nth(va.vpn0())?;
        if !pte0.v() {
            return None;
        }

        let mut pa = PhysicalAddr::new();
        pa.set_ppn(pte0.ppn() as usize);
        pa.set_page_offset(va.page_offset());
        Some(pa)
    }

    pub fn activate(&self, root: PhysicalAddr) {
        let ppn = root.ppn() as u64;
        let mut value = SatpValue::new();
        value.set_ppn(ppn);
        value.set_mode(SatpMode::SV39.into());
        Satp::write(value.into());
        Self::flush();
    }

    pub fn identity_map(&mut self, root: PhysicalAddr, start: usize, end: usize) {
        let mut flags = PageTableEntryFlags::new();
        flags.set_r(true);
        flags.set_w(true);
        flags.set_x(true);
        for i in (start..end).step_by(PAGE_SIZE) {
            self.map(
                root,
                VirtualAddr::from(i),
                PhysicalAddr::from(i),
                flags,
                false,
            );
        }
    }

    #[inline]
    pub fn flush() {
        unsafe { asm!("sfence.vma") }
    }
}
