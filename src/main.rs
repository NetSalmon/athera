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

use core::{arch::global_asm, panic::PanicInfo, sync::atomic::AtomicBool};

use crate::{
    arch::sbi::srst::{ResetReason, ResetType, system_reset},
    constants::*,
    log::Level,
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

    #[cfg(debug_assertions)]
    log::set_level(Level::Trace);
    #[cfg(not(debug_assertions))]
    log::set_level(Level::Info);

    info!("system info: {SYS} {VERSION} {RELEASE} {ARCH}");

    info!("page size: {}", PAGE_SIZE);
    info!("buddy max order: {}", BUDDY_MAX_ORDER);
    info!("slub min order: {}", SLUB_MIN_ORDER);
    info!("slub max order: {}", SLUB_MAX_ORDER);

    info!("kernel end: {:#x}", _end as *const () as usize);

    identity_map();

    info!("page table setup ok");

    proc::execute_buffer(&ELF.0);

    kernel_halt()
}

#[unsafe(no_mangle)]
fn kernel_halt() -> ! {
    info!("kernel halted");
    system_reset(ResetType::Shutdown, ResetReason::None);
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic_handle(info: &PanicInfo) -> ! {
    error!("========= kernel panic =========");

    match info.location() {
        Some(location) => {
            error!(
                "at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }
        None => {
            error!("location unknown");
        }
    }

    error!("{}", info.message());

    error!("================================");

    let _ = system_reset(ResetType::Shutdown, ResetReason::SysFail);

    loop {
        core::hint::spin_loop();
    }
}
