#![no_std]
#![no_main]
mod arch;
mod constants;
mod dev;
mod elf;
mod error;
mod io;
mod locks;
mod log;
mod macros;
mod mem;
mod proc;
mod syscall;
mod trap;
mod usr;

extern crate alloc;

use core::{arch::global_asm, panic::PanicInfo};

use crate::{
    arch::sbi::srst::{ResetReason, ResetType, system_reset},
    constants::*,
    dev::FDT,
    mem::page_table::identity_map,
};

global_asm!(include_str!("entry.asm"));

#[unsafe(no_mangle)]
fn main(hart_id: usize, dev_tree_address: usize) -> ! {
    if hart_id != 0 {
        core::hint::spin_loop();
    }

    // checking
    unsafe {
        if dev_tree_address != FDT_ADDR as usize {
            core::hint::spin_loop();
        }
    }

    debug!("system info: {SYS} {VERSION} {RELEASE} {ARCH}");

    debug!("page size: {}", PAGE_SIZE);
    debug!("buddy max order: {}", BUDDY_MAX_ORDER);
    debug!("slub min order: {}", SLUB_MIN_ORDER);
    debug!("slub max order: {}", SLUB_MAX_ORDER);

    debug!("kernel end: {:#x}", _end as *const () as usize);

    identity_map();

    debug!("page table setup ok");

    debug!("fdt len: {:x?}", FDT.len());

    usr::exec(&ELF.0).expect("failed to load ELF");

    debug!("read elf ok");

    system_reset(ResetType::Shutdown, ResetReason::None);

    kernel_halt()
}

#[unsafe(no_mangle)]
fn kernel_halt() -> ! {
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
