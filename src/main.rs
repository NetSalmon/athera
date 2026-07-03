#![no_std]
#![no_main]
mod arch;
mod dev;
mod elf;
mod error;
mod io;
mod locks;
mod log;
mod marco;
mod mem;
mod syscall;
mod trap;
mod usr;
mod proc;

use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::mem::page_table::{PageTable, ROOT_PAGE_TABLE, equal_mapping};
use core::arch::global_asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::elf::{EIdent, EMachine, Elf64Ehdr, Elf64Phdr};

global_asm!(include_str!("entry.asm"));

pub static FDT_ADDRESS: AtomicUsize = AtomicUsize::new(0);
pub static ROOT_PAGE_TABLE_ADDRESS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    pub fn _end();
    pub static PAGE_OFFSET: usize;
}

#[inline]
pub fn page_offset() -> usize {
    unsafe { PAGE_OFFSET }
}

#[unsafe(no_mangle)]
fn main(hart_id: usize, dev_tree_address: usize) -> ! {
    if hart_id != 0 {
        core::hint::spin_loop();
    }

    FDT_ADDRESS.swap(dev_tree_address, Ordering::Relaxed);

    debug!("kernel end: {:#x}", _end as *const () as usize);

    equal_mapping();

    debug!("page table setup ok");

    assert_eq!(size_of::<EIdent>(), 16);
    assert_eq!(size_of::<EMachine>(), 2);
    assert_eq!(size_of::<Elf64Ehdr>(), 64);
    assert_eq!(core::mem::offset_of!(Elf64Ehdr, e_machine), 18);

    debug!("read elf header");
    let header = unsafe { (&ELF as *const Elf as *const Elf64Ehdr).read() };

    debug!("endianness: {:?}", header.e_ident.data());
    debug!("elf os abi: {:?}", header.e_ident.os_abi());
    debug!("elf version: {:?}", header.e_ident.version());
    debug!("elf type: {:?}", header.e_type);
    debug!("elf version: {:?}", header.e_version);
    debug!("elf entry addr: {:#x}", header.e_entry);
    debug!("elf ph offset: {:#x}", header.e_phoff);
    debug!("elf ph sz: {:#x}", header.e_phnum);
    debug!("elf machine: {:#?}", header.e_machine);

    assert_eq!(size_of::<Elf64Phdr>(), 56);

    let ptr = ELF.0.as_ptr();

    for i in 0..header.e_phnum as usize {
        let ph1 = unsafe { (ptr.add(header.e_phoff as usize + i * size_of::<Elf64Phdr>()) as *const Elf64Phdr).read() };
        debug!("ph{i}: {:#x?}", ph1);
    }

    // save time
    system_reset(ResetType::Shutdown, ResetReason::None);

    kernel_do_no_thing()
}

#[unsafe(no_mangle)]
fn kernel_do_no_thing() -> ! {
    debug!("do no thing");
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic_handle(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        error!(
            "panic at => {}:{}:{} : {}",
            location.file(),
            location.line(),
            location.column(),
            info.message()
        );
    } else {
        error!("panic: {}", info.message());
    }

    let _ = system_reset(ResetType::Shutdown, ResetReason::SysFail);

    loop {
        core::hint::spin_loop();
    }
}

#[repr(align(8))]
struct Elf ([u8;include_bytes!("../applications/target/riscv64gc-unknown-none-elf/release/hello_world").len()]);
static ELF: Elf = Elf(*include_bytes!("../applications/target/riscv64gc-unknown-none-elf/release/hello_world"));