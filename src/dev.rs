use alloc::{vec, vec::Vec};
use core::{arch::asm, slice};

use novus_const::lazy;

use crate::{
    FDT_ADDR,
    constants::MEMORY_RANGE,
    dev::{memory::Memory, ns16550a::Ns16550a, virtio_blk::VirtioBlk},
    error::{Error, Result},
    locks::SpinLock,
    mem::allocators::FRAME_ALLOCATOR,
    warn,
};

pub mod device;
pub mod memory;
pub mod ns16550a;
pub mod virtio_blk;
pub mod virtio_mmio;

pub use device::{Device, Resource};

fn parse_fdt() -> Result<fdt::Fdt<'static>> {
    unsafe { fdt::Fdt::from_ptr(FDT_ADDR) }.map_err(|_| Error::Fdt)
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

#[lazy]
pub static VIRTIO_BLK: Option<VirtioBlk> = {
    match parse_fdt() {
        Ok(fdt) => VirtioBlk::probe(&fdt),
        Err(err) => {
            warn!("failed to init virtio-blk: {err}");
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
        None => boot_fail(Error::MemoryProbeFailed),
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
