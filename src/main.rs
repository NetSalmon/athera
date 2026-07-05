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
mod proc;
mod syscall;
mod trap;
mod usr;

use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::mem::page_table::equal_mapping;
use core::arch::global_asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

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

    structs_layout_tests();
    usr::exec(&ELF.0);

    debug!("read elf ok");

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
struct Elf(
    [u8; include_bytes!("../applications/target/riscv64gc-unknown-none-elf/release/hello_world")
        .len()],
);

static ELF: Elf = Elf(*include_bytes!(
    "../applications/target/riscv64gc-unknown-none-elf/release/hello_world"
));

fn structs_layout_tests() {
    use crate::dev::virtio_blk::queue::{
        Queue, VirtioAvail, VirtioDesc, VirtioDescTable, VirtioUsed, VirtioUsedElem,
    };
    use crate::elf::{Elf32Ehdr, Elf32Phdr, Elf64Ehdr, Elf64Phdr};
    use crate::mem::page_table::PageTable;
    use core::mem::{align_of, offset_of, size_of};

    // Elf32Ehdr: repr(C), size=52, align=4
    assert_eq!(size_of::<Elf32Ehdr>(), 52);
    assert_eq!(align_of::<Elf32Ehdr>(), 4);
    assert_eq!(offset_of!(Elf32Ehdr, e_ident), 0);
    assert_eq!(offset_of!(Elf32Ehdr, e_type), 16);
    assert_eq!(offset_of!(Elf32Ehdr, e_machine), 18);
    assert_eq!(offset_of!(Elf32Ehdr, e_version), 20);
    assert_eq!(offset_of!(Elf32Ehdr, e_entry), 24);
    assert_eq!(offset_of!(Elf32Ehdr, e_phoff), 28);
    assert_eq!(offset_of!(Elf32Ehdr, e_shoff), 32);
    assert_eq!(offset_of!(Elf32Ehdr, e_flags), 36);
    assert_eq!(offset_of!(Elf32Ehdr, e_ehsize), 40);
    assert_eq!(offset_of!(Elf32Ehdr, e_phentsize), 42);
    assert_eq!(offset_of!(Elf32Ehdr, e_phnum), 44);
    assert_eq!(offset_of!(Elf32Ehdr, e_shentsize), 46);
    assert_eq!(offset_of!(Elf32Ehdr, e_shnum), 48);
    assert_eq!(offset_of!(Elf32Ehdr, e_shstrndx), 50);

    // Elf64Ehdr: repr(C), size=64, align=8
    assert_eq!(size_of::<Elf64Ehdr>(), 64);
    assert_eq!(align_of::<Elf64Ehdr>(), 8);
    assert_eq!(offset_of!(Elf64Ehdr, e_ident), 0);
    assert_eq!(offset_of!(Elf64Ehdr, e_type), 16);
    assert_eq!(offset_of!(Elf64Ehdr, e_machine), 18);
    assert_eq!(offset_of!(Elf64Ehdr, e_version), 20);
    assert_eq!(offset_of!(Elf64Ehdr, e_entry), 24);
    assert_eq!(offset_of!(Elf64Ehdr, e_phoff), 32);
    assert_eq!(offset_of!(Elf64Ehdr, e_shoff), 40);
    assert_eq!(offset_of!(Elf64Ehdr, e_flags), 48);
    assert_eq!(offset_of!(Elf64Ehdr, e_ehsize), 52);
    assert_eq!(offset_of!(Elf64Ehdr, e_phentsize), 54);
    assert_eq!(offset_of!(Elf64Ehdr, e_phnum), 56);
    assert_eq!(offset_of!(Elf64Ehdr, e_shentsize), 58);
    assert_eq!(offset_of!(Elf64Ehdr, e_shnum), 60);
    assert_eq!(offset_of!(Elf64Ehdr, e_shstrndx), 62);

    // Elf32Phdr: repr(C), size=32, align=4
    assert_eq!(size_of::<Elf32Phdr>(), 32);
    assert_eq!(align_of::<Elf32Phdr>(), 4);
    assert_eq!(offset_of!(Elf32Phdr, p_type), 0);
    assert_eq!(offset_of!(Elf32Phdr, p_offset), 4);
    assert_eq!(offset_of!(Elf32Phdr, p_vaddr), 8);
    assert_eq!(offset_of!(Elf32Phdr, p_paddr), 12);
    assert_eq!(offset_of!(Elf32Phdr, p_filesz), 16);
    assert_eq!(offset_of!(Elf32Phdr, p_memsz), 20);
    assert_eq!(offset_of!(Elf32Phdr, p_flags), 24);
    assert_eq!(offset_of!(Elf32Phdr, p_align), 28);

    // Elf64Phdr: repr(C), size=56, align=8
    assert_eq!(size_of::<Elf64Phdr>(), 56);
    assert_eq!(align_of::<Elf64Phdr>(), 8);
    assert_eq!(offset_of!(Elf64Phdr, p_type), 0);
    assert_eq!(offset_of!(Elf64Phdr, p_flags), 4);
    assert_eq!(offset_of!(Elf64Phdr, p_offset), 8);
    assert_eq!(offset_of!(Elf64Phdr, p_vaddr), 16);
    assert_eq!(offset_of!(Elf64Phdr, p_paddr), 24);
    assert_eq!(offset_of!(Elf64Phdr, p_filesz), 32);
    assert_eq!(offset_of!(Elf64Phdr, p_memsz), 40);
    assert_eq!(offset_of!(Elf64Phdr, p_align), 48);

    // PageTable: repr(align(4096)), size=4096
    assert_eq!(size_of::<PageTable>(), 4096);
    assert_eq!(align_of::<PageTable>(), 4096);

    // VirtioDesc: repr(C), size=16, align=8
    assert_eq!(size_of::<VirtioDesc>(), 16);
    assert_eq!(align_of::<VirtioDesc>(), 8);
    assert_eq!(offset_of!(VirtioDesc, addr), 0);
    assert_eq!(offset_of!(VirtioDesc, len), 8);
    assert_eq!(offset_of!(VirtioDesc, flags), 12);
    assert_eq!(offset_of!(VirtioDesc, next), 14);

    // VirtioDescTable: repr(C), size=512, align=8
    assert_eq!(size_of::<VirtioDescTable>(), 512);
    assert_eq!(align_of::<VirtioDescTable>(), 8);

    // VirtioAvail: repr(C), size=68, align=2
    assert_eq!(size_of::<VirtioAvail>(), 68);
    assert_eq!(align_of::<VirtioAvail>(), 2);
    assert_eq!(offset_of!(VirtioAvail, flags), 0);
    assert_eq!(offset_of!(VirtioAvail, idx), 2);
    assert_eq!(offset_of!(VirtioAvail, ring), 4);

    // VirtioUsedElem: repr(C), size=8, align=4
    assert_eq!(size_of::<VirtioUsedElem>(), 8);
    assert_eq!(align_of::<VirtioUsedElem>(), 4);
    assert_eq!(offset_of!(VirtioUsedElem, id), 0);
    assert_eq!(offset_of!(VirtioUsedElem, len), 4);

    // VirtioUsed: repr(C, align(4096)), size=4096
    assert_eq!(size_of::<VirtioUsed>(), 4096);
    assert_eq!(align_of::<VirtioUsed>(), 4096);
    assert_eq!(offset_of!(VirtioUsed, flags), 0);
    assert_eq!(offset_of!(VirtioUsed, idx), 2);
    assert_eq!(offset_of!(VirtioUsed, ring), 4);

    // Queue: repr(C, align(4096)), size=8192
    assert_eq!(size_of::<Queue>(), 8192);
    assert_eq!(align_of::<Queue>(), 4096);
    assert_eq!(offset_of!(Queue, desc), 0);
    assert_eq!(offset_of!(Queue, avail), 512);
    assert_eq!(offset_of!(Queue, used), 4096);
}
