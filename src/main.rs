#![no_std]
#![no_main]
mod arch;
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

use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::dev::DEV_TREE;
use crate::error::Error;
use crate::mem::page_table::identity_map;
use alloc::string::String;
use core::arch::global_asm;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

global_asm!(include_str!("entry.asm"));

pub static FDT_ADDRESS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" {
    pub fn _end();
}

#[const_val::const_val]
pub const VERSION: &str = "0.1.0";

#[const_val::const_val]
pub const RELEASE: &str = "none";

#[const_val::const_val]
pub const SYS: &str = "Novus";

#[const_val::const_val]
pub const ARCH: &str = "riscv64gc";

#[unsafe(no_mangle)]
fn main(hart_id: usize, dev_tree_address: usize) -> ! {
    if hart_id != 0 {
        core::hint::spin_loop();
    }

    FDT_ADDRESS.swap(dev_tree_address, Ordering::Relaxed);

    debug!("{SYS} {VERSION} {RELEASE} {ARCH}");

    debug!("PAGE_SIZE: {}", mem::PAGE_SIZE);
    debug!("BUDDY_MAX_ORDER: {}", mem::buddy::BUDDY_MAX_ORDER);
    debug!("SLUB_MIN_ORDER: {}", mem::slub::SLUB_MIN_ORDER);
    debug!("SLUB_MAX_ORDER: {}", mem::slub::SLUB_MAX_ORDER);

    debug!("kernel end: {:#x}", _end as *const () as usize);

    identity_map();

    debug!("page table setup ok");

    arch::breakpoint();
    arch::breakpoint();

    let mut a = String::new();

    loop {
        mem::slub::snapshot();

        let byte = DEV_TREE
            .force()
            .ns16550a
            .as_ref()
            .ok_or(Error::NoUart)
            .expect("UART not initialized")
            .lock()
            .block_getchar();

        debug!("byte: {}", byte);

        if byte == b'q' {
            break;
        }

        let ch = byte as char;
        a.push(ch);
        debug!("string: {}, at: {:#x}", a, &a as *const String as usize);
    }

    drop(a);

    mem::slub::snapshot();

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

#[repr(align(8))]
struct Elf(
    [u8; include_bytes!("../applications/target/riscv64gc-unknown-none-elf/release/hello_world")
        .len()],
);

static ELF: Elf = Elf(*include_bytes!(
    "../applications/target/riscv64gc-unknown-none-elf/release/hello_world"
));
