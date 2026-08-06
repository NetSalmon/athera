#![no_std]
#![no_main]
mod arch;
mod constants;
mod dev;
mod elf;
mod error;
pub mod fs;
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

use alloc::{string::ToString, vec};
use core::{arch::global_asm, panic::PanicInfo};

use crate::{
    arch::sbi::srst::{ResetReason, ResetType, system_reset},
    constants::*,
    dev::{VIRTIO_BLK, abstracts::BlockDevice},
    fs::{
        minix_fs::{DINode, DirEntry, DirEntryV1_14, DirEntryV1_30, MinixFsMagic, SuperBlock},
        record::Index,
    },
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

    let mut buffer = vec![0u8; 1024];
    if let Some(ref mut guard) = *VIRTIO_BLK.force().lock() {
        guard.read_at(&mut buffer, 1024).unwrap();
    }

    let super_block = unsafe { &*(buffer.as_ptr() as *const SuperBlock) };
    info!("{:#?}", super_block);

    let magic = super_block.magic;
    let zone_size = super_block.block_size();

    let root_inode_offset = super_block.d_inode_start();

    if let Some(ref mut guard) = *VIRTIO_BLK.force().lock() {
        guard.read_at(&mut buffer, root_inode_offset).unwrap();
    }

    let root_inode = unsafe { &*(buffer.as_ptr() as *const DINode) }.clone();

    info!("{:#?}", root_inode);

    if let Some(ref mut guard) = *VIRTIO_BLK.force().lock() {
        guard
            .read_at(&mut buffer, zone_size * root_inode.zone[0] as usize)
            .unwrap();
    }

    let ptr = buffer.as_ptr();

    let dist_file = "hello_world";
    let mut inode_num = None;

    if magic == MinixFsMagic::MAGIC_2 {
        for offset in (0..(root_inode.size as usize)).step_by(size_of::<DirEntryV1_30>()) {
            let entry = unsafe { &*(ptr.add(offset) as *const DirEntryV1_30) };

            info!("{:#?}", entry.name.to_string());

            if entry.name.to_string().as_str() == dist_file {
                inode_num = Some(entry.ino);
            }

            info!("{:?}", entry);
        }
    } else {
        for offset in (0..root_inode.size as usize).step_by(size_of::<DirEntryV1_14>()) {
            let entry = unsafe { &*(ptr.add(offset) as *const DirEntryV1_14) };

            if entry.name.to_string().as_str() == dist_file {
                inode_num = Some(entry.ino);
            }

            info!("{:?}", entry);
        }
    }

    info!("{} at: {:?}", dist_file, inode_num);
    let inode_num = inode_num.unwrap();

    if let Some(ref mut guard) = *VIRTIO_BLK.force().lock() {
        guard
            .read_at(
                &mut buffer,
                root_inode_offset + size_of::<DINode>() * (inode_num as usize - 1),
            )
            .unwrap();
    }

    let hello_world = unsafe { &*(buffer.as_ptr() as *const DINode) };

    info!("{:#?}", hello_world);

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
