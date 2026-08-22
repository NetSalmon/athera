//! 设备驱动与设备树（FDT）探测。
//!
//! 启动阶段解析设备树并提供设备管理器、系统内存和设备树副本。
use alloc::{sync::Arc, vec, vec::Vec};
use core::{arch::asm, slice};

use athera_macros::lazy;

use crate::{
    FDT_ADDR,
    constants::MEMORY_RANGE,
    driver::{memory::Memory, ramfb::Ramfb},
    error::{DevError, Error, Result},
    mm::allocator::FRAME_ALLOCATOR,
    sync::rwlock::RwLock,
    warn,
};

pub mod descriptor;
mod device;
pub(crate) mod fw_cfg;
mod memory;
mod ns16550a;
mod ramfb;
pub(crate) mod reboot;
pub(crate) mod traits;
pub mod tree;
mod virtio_blk;
mod virtio_mmio;
mod virtio_rng;

fn parse_fdt() -> Result<fdt::Fdt<'static>> {
    Ok(unsafe { fdt::Fdt::from_ptr(FDT_ADDR)? })
}

fn boot_fail(err: Error) -> ! {
    panic!("kernel cannot boot: {err}");
}

#[lazy(spin)]
pub static RAMFB: Option<Arc<RwLock<Ramfb>>> = {
    match parse_fdt() {
        Ok(fdt) => Ramfb::probe(&fdt).map(|ramfb| Arc::new(RwLock::new(ramfb))),
        Err(err) => {
            warn!("failed to init ramfb: {err}");
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
