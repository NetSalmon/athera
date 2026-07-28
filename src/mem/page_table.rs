use core::arch::asm;

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

#[inline]
fn get_or_create_table(pte: &mut PageTableEntry) -> &mut PageTable {
    if !pte.v() {
        let addr = alloc_frame().expect("out of memory");
        unsafe { (addr as *mut PageTable).write(PageTable::new()) };
        pte.set_ppn((addr >> 12) as u64);
        pte.set_v(true);
    }
    let ppn = pte.ppn() as usize;
    unsafe { &mut *((ppn << 12) as *mut PageTable) }
}

#[inline]
pub fn map(va: VirtualAddr, pa: PhysicalAddr, flush: bool, u: bool) {
    let root_pa = *ROOT_PAGE_TABLE.force();
    let root = unsafe { &mut *(root_pa as *mut PageTable) };

    let l1_table = get_or_create_table(&mut root.entries[va.vpn2()]);
    let l0_table = get_or_create_table(&mut l1_table.entries[va.vpn1()]);

    let vpn0 = va.vpn0();
    let mut pte = PageTableEntry::new();
    pte.set_v(true);
    pte.set_r(true);
    pte.set_w(true);
    pte.set_x(true);
    pte.set_u(u);
    pte.set_ppn(pa.ppn() as u64);
    l0_table.entries[vpn0] = pte;

    if flush {
        unsafe { asm!("sfence.vma") }
    }
}

#[inline]
pub fn unmap(va: VirtualAddr, flush: bool) {
    let root_pa = *ROOT_PAGE_TABLE.force();
    let root = unsafe { &mut *(root_pa as *mut PageTable) };

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
        unsafe { asm!("sfence.vma") }
    }
}

pub fn identity_map() {
    debug!("start identity mapping memory");
    let start = SYSTEM_MEMORY.device.mmio.start;
    let end = start + SYSTEM_MEMORY.device.mmio.size;

    for i in (start..end).step_by(PAGE_SIZE) {
        map(VirtualAddr::from(i), PhysicalAddr::from(i), false, false);
    }

    if let Some(ref uart) = *UART {
        let start = uart.lock().device.mmio.start;
        map(
            VirtualAddr::from(start),
            PhysicalAddr::from(start),
            false,
            false,
        );
    }

    if let Some(ref blk) = *VIRTIO_BLK {
        let start = blk.device.mmio.start;
        let end = start + blk.device.mmio.size;

        for i in (start..end).step_by(PAGE_SIZE) {
            map(VirtualAddr::from(i), PhysicalAddr::from(i), false, false);
        }
    }

    debug!("map ok");

    let root_pt_addr = PhysicalAddr::from(*ROOT_PAGE_TABLE.force());
    debug!("root pt addr at: {:?}", root_pt_addr);

    let ppn = root_pt_addr.ppn() as u64;

    let mut value = SatpValue::new();
    value.set_ppn(ppn);
    value.set_mode(SatpMode::SV39.into());

    debug!("value: {:?}", value);

    Satp::write(value.into());

    debug!("storage satp ok");
    flush();
    debug!("identity mapping memory end");
}

#[inline]
pub fn flush() {
    unsafe { asm!("sfence.vma") }
}
