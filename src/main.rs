#![no_std]
#![no_main]
mod arch;
mod constants;
mod dev;
mod elf;
mod error;
mod fs;
mod io;
mod log;
mod macros;
mod mem;
mod proc;
mod rand;
mod sync;
mod syscall;
mod trap;

extern crate alloc;

use alloc::vec;
use core::{arch::global_asm, panic::PanicInfo};

use crate::{
    arch::sbi::srst::{ResetReason, ResetType, system_reset},
    constants::*,
    dev::VIRTIO_BLK,
    fs::minix_fs::{MinixFs, Path},
    log::Level,
    mem::page_table::identity_map,
    trap::set_next_timer,
};

global_asm!(include_str!("entry.asm"));

#[unsafe(no_mangle)]
/// 内核入口（由 `entry.asm` 的 `_start` 调用）。
///
/// 初始化日志级别、恒等映射页表，然后加载并执行内嵌的用户程序。
fn main(hart_id: usize, dev_tree_address: usize) -> ! {
    arch::registers::gpr::Tp::write(hart_id as u64);

    if hart_id != 0 {
        core::hint::spin_loop();
    }

    set_next_timer();

    // checking
    unsafe {
        if dev_tree_address != FDT_ADDR as usize {
            core::hint::spin_loop();
        }
    }

    #[cfg(debug_assertions)]
    log::set_level(Level::TRACE);

    #[cfg(not(debug_assertions))]
    log::set_level(Level::INFO);

    info!("system info: {SYS} {VERSION} {RELEASE} {ARCH}");
    info!(
        "memory config: page size = {PAGE_SIZE}, buddy max order = {BUDDY_MAX_ORDER}, slub = {SLUB_MIN_ORDER}..={SLUB_MAX_ORDER}"
    );
    info!("kernel end: {:#x}", _end as *const () as usize);

    if let Err(err) = identity_map() {
        error!("identity mapping failed: {err}");
        kernel_halt()
    }

    info!("page table setup ok");

    let path = Path::from_str("/bin/hello_world");
    if let Some(ref mut blk) = *VIRTIO_BLK.force().lock() {
        let fs = MinixFs::from_device(blk).unwrap().unwrap();
        let mut f = fs.open(&path, blk).unwrap().unwrap();

        let mut buf = vec![0u8; f.size() as usize];
        info!("{:#?}", f);

        f.read(&mut buf, blk).unwrap();

        if let Err(err) = proc::exec::execute_buffer(&buf, None) {
            error!("failed to execute user program: {err}");
            kernel_halt()
        }
    }

    if let Err(err) = proc::exec::execute_buffer(&ELF.0, None) {
        error!("failed to execute user program: {err}");
        kernel_halt()
    }

    kernel_halt()
}

#[unsafe(no_mangle)]
/// 停机：输出日志并通过 SBI 复位（关机）。
fn kernel_halt() -> ! {
    info!("kernel halted");

    #[cfg(feature = "halt_directly")]
    system_reset(ResetType::SHUTDOWN, ResetReason::NONE);

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic_handle(info: &PanicInfo) -> ! {
    error!("========= kernel panic =========");
    error!("{}", info.message());

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

    error!("================================");

    let _ = system_reset(ResetType::SHUTDOWN, ResetReason::SYS_FAIL);

    loop {
        core::hint::spin_loop();
    }
}
