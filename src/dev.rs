//! 设备驱动与设备树（FDT）探测。
//!
//! 启动阶段解析设备树并初始化 ns16550a UART、virtio-blk / virtio-rng（MMIO）
//! 与系统内存，提供 `UART` / `VIRTIO_BLK` / `VIRTIO_RNG` / `SYSTEM_MEMORY` /
//! `FDT` 等懒加载
//! 静态。`FDT` 在初始化完成后会把设备树搬进堆中，并把 `FDT_ADDR` 指向
//! 新副本，同时把原设备树所占物理内存归还给伙伴系统。
use alloc::{sync::Arc, vec, vec::Vec};
use core::{arch::asm, slice};

use athera_macros::lazy;

use crate::{
    FDT_ADDR,
    constants::MEMORY_RANGE,
    dev::{
        memory::Memory, ns16550a::Ns16550a, virtio_blk::VirtioBlk, virtio_mmio::VirtioDevice,
        virtio_rng::VirtioRng,
    },
    error::{DevError, Error, Result},
    mem::allocators::FRAME_ALLOCATOR,
    sync::spin::SpinLock,
    warn,
};

mod device;
mod memory;
mod ns16550a;
pub(crate) mod traits;
mod tree;
mod virtio_blk;
mod virtio_mmio;
mod virtio_rng;

fn parse_fdt() -> Result<fdt::Fdt<'static>> {
    Ok(unsafe { fdt::Fdt::from_ptr(FDT_ADDR)? })
}

fn boot_fail(err: Error) -> ! {
    panic!("kernel cannot boot: {err}");
}

#[lazy]
pub static UART: Option<SpinLock<Ns16550a>> = {
    match parse_fdt() {
        Ok(fdt) => Ns16550a::probe(&fdt).map(|uart| {
            uart.init();
            SpinLock::new(uart)
        }),
        Err(err) => {
            warn!("failed to init UART: {err}");
            None
        }
    }
};

#[lazy(spin)]
pub static VIRTIO_BLK: Option<Arc<SpinLock<VirtioBlk>>> = {
    match parse_fdt() {
        Ok(fdt) => VirtioBlk::probe(&fdt).map(|blk| Arc::new(SpinLock::new(blk))),
        Err(err) => {
            warn!("failed to init virtio-blk: {err}");
            None
        }
    }
};

#[lazy(spin)]
pub static VIRTIO_RNG: Option<VirtioRng> = {
    match parse_fdt() {
        Ok(fdt) => VirtioRng::probe(&fdt),
        Err(err) => {
            warn!("failed to init virtio-rng: {err}");
            None
        }
    }
};

#[lazy]
pub static SYSTEM_MEMORY: Memory = {
    let fdt = match parse_fdt() {
        Ok(fdt) => fdt,
        Err(err) => boot_fail(err),
    };
    match Memory::probe(&fdt) {
        Some(memory) => memory,
        None => boot_fail(DevError::MemoryProbeFailed.into()),
    }
};

#[lazy]
pub static FDT: Vec<u8> = {
    let fdt = match parse_fdt() {
        Ok(fdt) => fdt,
        Err(err) => boot_fail(err),
    };

    let total_size = fdt.total_size();

    let src_slice = unsafe { slice::from_raw_parts(FDT_ADDR, total_size) };

    let mut fdt_copy = vec![0u8; total_size];
    fdt_copy.copy_from_slice(src_slice);

    // 恢复
    let old_addr = unsafe { FDT_ADDR };

    unsafe {
        asm!(
            r#"la t0, FDT_ADDR
            sd {}, 0(t0)"#,
            in(reg) fdt_copy.as_ptr(),
        );
    }

    assert_eq!(unsafe { FDT_ADDR }, fdt_copy.as_ptr());

    FRAME_ALLOCATOR
        .force()
        .lock()
        .add(&(old_addr as usize..MEMORY_RANGE.end));

    fdt_copy
};
