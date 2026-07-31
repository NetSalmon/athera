use novus_const::const_val;

use crate::{
    bits,
    constants::{PHY_PAGE_SIZE, align},
    debug,
    elf::{Class, EMachine, Elf64Ehdr, Elf64Phdr, Endianness, PType},
    error::{Error, Result},
    info,
    mem::{
        addr::{PhysicalAddr, VirtualAddr},
        alloc_page::AllocPage,
        allocators::FRAME_ALLOCATOR,
        page_table::{PAGE_TABLE_MANAGER, PageTableEntryFlags},
    },
    proc::{MemorySet, TASKS, TaskControlBlock, TaskStatus, Tid, alloc_tid},
    trace,
    trap::{TrapContext, restore_context},
};

bits! {
    pub type SStatusBits: u64 {
        spp: 8,
        spie: 5,
        sie: 1
    }
}

#[const_val(multiple_of = PHY_PAGE_SIZE)]
pub const USER_STACK_SIZE: usize = PHY_PAGE_SIZE * 8;

#[const_val(multiple_of = PHY_PAGE_SIZE)]
pub const USER_STACK_TOP: usize = 0x1_0000_0000;

pub const USER_STACK_LOWER_BOUND: usize = USER_STACK_TOP - USER_STACK_SIZE;
