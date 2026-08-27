#![no_std]
#![no_main]
#![recursion_limit = "512"]
mod arch;
mod binfmt;
mod constants;
mod driver;
mod error;
mod fs;
mod io;
mod log;
mod macros;
mod mm;
mod rand;
mod sync;
mod syscall;
mod task;

extern crate alloc;

use core::{arch::global_asm, panic::PanicInfo};

use crate::{
    arch::riscv64::sbi::srst::{ResetReason, ResetType, system_reset},
    constants::*,
    log::Level,
    mm::page_table::identity_map,
};

#[cfg(target_arch = "riscv64")]
global_asm!(include_str!("entry.asm"));

#[unsafe(no_mangle)]
/// 内核入口（由 `entry.asm` 的 `_start` 调用）。
///
/// 初始化日志级别、恒等映射页表，然后从 MINIX 文件系统加载并执行
/// `/bin/hello_world`、`/bin/sort`，最后执行内嵌的用户程序 `add`。
fn main(hart_id: usize, dev_tree_address: usize) -> ! {
    if hart_id != 0 {
        core::hint::spin_loop();
    }

    #[cfg(feature = "smp")]
    {
        arch::riscv64::sbi::hsm::hart_start(1, hart_entry as *const () as u64, 0);
        let r = arch::riscv64::sbi::hsm::hart_get_status(1);
        info!("hart 1 status: {:#?}", r);
    }

    // 校验 entry.asm 写入的 FDT_ADDR 与 Rust 侧入口参数一致。
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

    if let Err(err) = fs::init() {
        error!("filesystem initialization failed: {err}");
        kernel_halt()
    }

    fs::enable_vfs_console();

    // 从 MINIX 文件系统加载并执行磁盘上的用户程序。
    arch::riscv64::boot::start_default_programs();
    binfmt::route("/bin/test.sh", &[&"/bin/test.sh"], &[]).unwrap(); // #!/bin/run 开头的/bin/test.sh文件需自备

    // 只有在初始任务创建完成后才启动时钟，避免定时器中断进入空调度器。
    arch::riscv64::trap::set_next_timer(None);
    arch::riscv64::registers::csr::Sstatus::set_bits(1 << 1);
    task::scheduler::switch();

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
fn panic(info: &PanicInfo) -> ! {
    error!("========= kernel panic =========");

    if let Some(location) = info.location() {
        error!(
            "panicked at {}:{}:{}:",
            location.file(),
            location.line(),
            location.column()
        );
    } else {
        error!("panicked at unknown location:");
    }

    error!("{}", info.message());

    error!("================================");

    let _ = system_reset(ResetType::SHUTDOWN, ResetReason::SYS_FAIL);

    kernel_halt()
}
