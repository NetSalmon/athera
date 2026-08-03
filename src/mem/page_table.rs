#![allow(dead_code)]
//! Sv39 页表与地址空间管理。
//!
//! [`PageTable`] 是一页 512 项的页表；[`PageTableManager`] 维护内核
//! 地址空间与按 TID 索引的用户地址空间，提供恒等映射、映射/解映射、
//! 激活（写 `satp`）与 `sfence.vma` 刷新。
pub mod handle;

use alloc::collections::BTreeMap;
use core::arch::asm;

use athera_const::lazy;

use crate::{
    arch::registers::{
        csr::Satp,
        values::{SatpMode, SatpValue},
    },
    bits,
    constants::{PAGE_SIZE, PTE_NUMBER},
    debug,
    dev::{SYSTEM_MEMORY, UART, VIRTIO_BLK, VIRTIO_RNG},
    error::{Error, Result},
    mem::{
        addr::{PhysicalAddr, VirtualAddr},
        page_table::handle::PageTableHandle,
    },
    proc::task::Tid,
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

#[repr(align(4096))]
#[derive(Debug)]
pub struct PageTable {
    entries: [PageTableEntry; PTE_NUMBER],
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

#[lazy(spin)]
pub static PAGE_TABLE_MANAGER: PageTableManager = PageTableManager::new();

/// 单次持锁映射的块大小。`SpinLock` 持锁期间会关闭中断，把整段物理
/// 内存一次性映射完会让定时器中断（s_timer）长时间得不到响应；分块
/// 映射、块间释放锁并恢复中断，保证长启动映射期间定时器仍能按时触发。
const IDENTITY_MAP_CHUNK: usize = 8 * 1024 * 1024; // 8 MiB

pub fn identity_map() -> Result<()> {
    debug!("start identity mapping memory");

    // 先在各自锁内短暂取出设备 MMIO 区间，避免把设备锁嵌套在页表锁里。
    let uart_start = UART.as_ref().map(|uart| uart.lock().device.mmio.start);
    let blk_range = VIRTIO_BLK.lock().as_ref().map(|blk| {
        blk.device.mmio.start..blk.device.mmio.start + blk.device.mmio.size
    });
    let rng_range = VIRTIO_RNG.lock().as_ref().map(|rng| {
        rng.device.mmio.start..rng.device.mmio.start + rng.device.mmio.size
    });

    let mem_start = SYSTEM_MEMORY.device.mmio.start;
    let mem_end = mem_start + SYSTEM_MEMORY.device.mmio.size;

    // 物理内存分块映射：每映射一块就释放页表锁，块间中断恢复使能。
    let mut cursor = mem_start;
    while cursor < mem_end {
        let chunk_end = (cursor + IDENTITY_MAP_CHUNK).min(mem_end);
        PAGE_TABLE_MANAGER.lock().identity_map(cursor, chunk_end)?;
        cursor = chunk_end;
    }

    let mut flags = PageTableEntryFlags::new();
    flags.set_r(true);
    flags.set_w(true);
    flags.set_x(true);

    if let Some(start) = uart_start {
        PAGE_TABLE_MANAGER.lock().map(
            AddressSpaceId::Kernel,
            VirtualAddr::from(start),
            PhysicalAddr::from(start),
            flags,
            false,
        )?;
    }

    if let Some(range) = blk_range {
        PAGE_TABLE_MANAGER.lock().identity_map(range.start, range.end)?;
    }

    if let Some(range) = rng_range {
        PAGE_TABLE_MANAGER.lock().identity_map(range.start, range.end)?;
    }

    let root_addr = {
        let mgr = PAGE_TABLE_MANAGER.lock();
        let root = mgr.kernel_root_addr();
        mgr.activate(AddressSpaceId::Kernel)?;
        root
    };

    debug!("map ok");
    debug!("root pt addr at: {:?}", root_addr);

    debug!("identity mapping memory end");
    Ok(())
}

pub struct AddressSpace {
    root: PageTableHandle,
    tables: BTreeMap<PhysicalAddr, PageTableHandle>,
}

pub enum AddressSpaceId {
    Kernel,
    User(Tid),
}

pub struct PageTableManager {
    kernel: AddressSpace,
    user: BTreeMap<Tid, AddressSpace>,
}

impl AddressSpace {
    pub fn map(
        &mut self,
        va: VirtualAddr,
        pa: PhysicalAddr,
        flags: PageTableEntryFlags,
    ) -> Result<()> {
        let vpn2 = va.vpn2();
        let vpn1 = va.vpn1();
        let vpn0 = va.vpn0();

        let pt1_addr = {
            let pte2 = &mut self.root.entries[vpn2];
            if !pte2.v() {
                let new_pt = PageTableHandle::create()?;
                let addr = new_pt.as_phys_addr();
                pte2.set_ppn(addr.ppn() as u64);
                pte2.set_v(true);
                self.tables.insert(addr, new_pt);
            }
            let mut addr = PhysicalAddr::new();
            addr.set_ppn(pte2.ppn() as usize);
            addr
        };

        let pt0_addr = {
            let need_create = {
                let pt1 = self.tables.get(&pt1_addr).ok_or(Error::PageTableMissing)?;
                !pt1.entries[vpn1].v()
            };
            if need_create {
                let new_pt = PageTableHandle::create()?;
                let addr = new_pt.as_phys_addr();
                {
                    let pt1 = self
                        .tables
                        .get_mut(&pt1_addr)
                        .ok_or(Error::PageTableMissing)?;
                    let pte1 = &mut pt1.entries[vpn1];
                    pte1.set_ppn(addr.ppn() as u64);
                    pte1.set_v(true);
                }
                self.tables.insert(addr, new_pt);
            }
            let pt1 = self.tables.get(&pt1_addr).ok_or(Error::PageTableMissing)?;
            let mut addr = PhysicalAddr::new();
            addr.set_ppn(pt1.entries[vpn1].ppn() as usize);
            addr
        };

        let pt0 = self
            .tables
            .get_mut(&pt0_addr)
            .ok_or(Error::PageTableMissing)?;

        let pte0 = &mut pt0.entries[vpn0];
        pte0.set_ppn(pa.ppn() as u64);
        pte0.set_v(true);
        pte0.set_r(flags.r());
        pte0.set_w(flags.w());
        pte0.set_x(flags.x());
        pte0.set_u(flags.u());
        pte0.set_g(flags.g());
        pte0.set_a(flags.a());
        pte0.set_d(flags.d());

        Ok(())
    }

    pub fn unmap(&mut self, va: VirtualAddr) {
        let vpn2 = va.vpn2();
        let vpn1 = va.vpn1();
        let vpn0 = va.vpn0();

        let pt1_addr = {
            let pte2 = self.root.entries[vpn2];
            if !pte2.v() {
                return;
            }
            let mut addr = PhysicalAddr::new();
            addr.set_ppn(pte2.ppn() as usize);
            addr
        };

        let pt0_addr = {
            let pt1 = match self.tables.get(&pt1_addr) {
                Some(pt) => pt,
                None => return,
            };
            let pte1 = pt1.entries[vpn1];
            if !pte1.v() {
                return;
            }
            let mut addr = PhysicalAddr::new();
            addr.set_ppn(pte1.ppn() as usize);
            addr
        };

        let pt0 = match self.tables.get_mut(&pt0_addr) {
            Some(pt) => pt,
            None => return,
        };

        let pte0 = &mut pt0.entries[vpn0];
        *pte0 = PageTableEntry::new();
    }
}

impl PageTableManager {
    pub fn create_user_address_space(&mut self, tid: Tid) -> Result<()> {
        let mut page_table = PageTableHandle::create()?;
        self.kernel.root.copy_low_half(&mut page_table);

        let address = AddressSpace {
            root: page_table,
            tables: BTreeMap::new(),
        };

        self.user.insert(tid, address);
        Ok(())
    }

    pub fn new() -> Self {
        let root = PageTableHandle::create().expect("kernel page table allocation failed");
        Self {
            kernel: AddressSpace {
                root,
                tables: BTreeMap::new(),
            },
            user: BTreeMap::new(),
        }
    }

    pub fn kernel_root_addr(&self) -> PhysicalAddr {
        self.kernel.root.as_phys_addr()
    }

    pub fn user_root_addr(&self, tid: Tid) -> Result<PhysicalAddr> {
        self.user
            .get(&tid)
            .map(|space| space.root.as_phys_addr())
            .ok_or(Error::AddressSpaceNotFound)
    }

    pub fn map(
        &mut self,
        id: AddressSpaceId,
        va: VirtualAddr,
        pa: PhysicalAddr,
        flags: PageTableEntryFlags,
        flush: bool,
    ) -> Result<()> {
        match id {
            AddressSpaceId::Kernel => self.kernel_map(va, pa, flags, flush),
            AddressSpaceId::User(tid) => self.user_map(tid, va, pa, flags, flush),
        }
    }

    pub fn unmap(&mut self, id: AddressSpaceId, va: VirtualAddr, flush: bool) -> Result<()> {
        match id {
            AddressSpaceId::Kernel => self.kernel_unmap(va, flush),
            AddressSpaceId::User(tid) => self.user_unmap(tid, va, flush),
        }
    }

    pub fn kernel_map(
        &mut self,
        va: VirtualAddr,
        pa: PhysicalAddr,
        flags: PageTableEntryFlags,
        flush: bool,
    ) -> Result<()> {
        self.kernel.map(va, pa, flags)?;
        if flush {
            Self::flush();
        }
        Ok(())
    }

    pub fn kernel_unmap(&mut self, va: VirtualAddr, flush: bool) -> Result<()> {
        self.kernel.unmap(va);
        if flush {
            Self::flush();
        }
        Ok(())
    }

    pub fn user_map(
        &mut self,
        tid: Tid,
        va: VirtualAddr,
        pa: PhysicalAddr,
        flags: PageTableEntryFlags,
        flush: bool,
    ) -> Result<()> {
        self.user
            .get_mut(&tid)
            .ok_or(Error::AddressSpaceNotFound)?
            .map(va, pa, flags)?;
        if flush {
            Self::flush();
        }
        Ok(())
    }

    pub fn user_unmap(&mut self, tid: Tid, va: VirtualAddr, flush: bool) -> Result<()> {
        self.user
            .get_mut(&tid)
            .ok_or(Error::AddressSpaceNotFound)?
            .unmap(va);
        if flush {
            Self::flush();
        }
        Ok(())
    }

    pub fn identity_map(&mut self, start: usize, end: usize) -> Result<()> {
        let mut flags = PageTableEntryFlags::new();
        flags.set_r(true);
        flags.set_w(true);
        flags.set_x(true);
        for i in (start..end).step_by(PAGE_SIZE) {
            self.kernel
                .map(VirtualAddr::from(i), PhysicalAddr::from(i), flags)?;
        }
        Ok(())
    }

    pub fn activate(&self, id: AddressSpaceId) -> Result<()> {
        let root = match id {
            AddressSpaceId::Kernel => self.kernel.root.as_phys_addr(),
            AddressSpaceId::User(tid) => self
                .user
                .get(&tid)
                .ok_or(Error::AddressSpaceNotFound)?
                .root
                .as_phys_addr(),
        };
        let ppn = root.ppn() as u64;
        let mut value = SatpValue::new();
        value.set_ppn(ppn);
        value.set_mode(SatpMode::SV39.into());
        Satp::write(value.into());
        Self::flush();
        Ok(())
    }

    #[inline]
    pub fn flush() {
        unsafe { asm!("sfence.vma") }
    }
}
