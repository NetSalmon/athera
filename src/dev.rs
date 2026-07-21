use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use core::{arch::asm, ptr::slice_from_raw_parts, slice};

use novus_const::lazy;
use novus_id_alloc::IdAllocator;

use crate::{
    FDT_ADDR,
    constants::MEMORY_RANGE,
    debug,
    dev::{memory::Memory, ns16550a::Ns16550a, virtio_blk::VirtioBlk},
    error::Error,
    locks::SpinLock,
    mem::allocators::FRAME_ALLOCATOR,
};

#[lazy]
pub static DEV_TREE: DeviceTree = { DeviceTree::probe().expect("device tree probe failed") };

pub mod memory;
pub mod ns16550a;
pub mod virtio_blk;
pub mod virtio_mmio;

#[derive(Copy, Clone)]
pub struct Resource {
    pub start: usize,
    pub size: usize,
}

impl Resource {
    pub const fn new(start: usize, size: usize) -> Self {
        Self { start, size }
    }
}

pub struct Device {
    pub mmio: Resource,
    pub irq: Option<usize>,
}

impl Device {
    pub const fn new(mmio: Resource, irq: Option<usize>) -> Self {
        Self { mmio, irq }
    }
}

pub struct DeviceTree {
    pub memory: Memory,
    pub ns16550a: Option<SpinLock<Ns16550a>>,
    pub virtio_blk: Option<VirtioBlk>,
}

impl DeviceTree {
    pub fn probe() -> Result<Self, Error> {
        let fdt = unsafe { fdt::Fdt::from_ptr(FDT_ADDR) }.map_err(|_| Error::Fdt)?;

        Ok(Self {
            ns16550a: Ns16550a::probe(&fdt).map(|uart| {
                uart.init();
                SpinLock::new(uart)
            }),
            virtio_blk: VirtioBlk::probe(&fdt),
            memory: Memory::probe(&fdt).ok_or(Error::MemoryProbeFailed)?,
        })
    }
}

impl Resource {
    #[inline]
    pub fn read<T>(&self, offset: usize) -> T {
        unsafe { ((self.start as *const u8).add(offset) as *const T).read_volatile() }
    }

    #[inline]
    pub fn write<T>(&self, offset: usize, val: T) {
        unsafe { ((self.start as *mut u8).add(offset) as *mut T).write_volatile(val) }
    }
}

#[macro_export]
macro_rules! mmio_regs {
    ($device:ident: [ $( $reg:ident $( : $t:ty )? => $offset:expr ),+ $(,)? ]) => {
        paste::paste! {
            $( const [<$reg:upper _OFFSET>]: usize = $offset as usize; )+

            impl $device {
                $(
                    $crate::mmio_regs!(@helper &self, $reg, $($t)?, $offset);
                )+
            }
        }
    };

    (@helper &self, $reg:ident, $t:ty, $offset:expr) => {
        paste::paste! {
            #[inline]
            pub fn [< $reg:snake >](&self) -> $t {
                self.device.mmio.read::<$t>($offset)
            }

            #[inline]
            pub fn [< write_ $reg:snake >](&self, val: $t) {
                self.device.mmio.write::<$t>($offset, val);
            }
        }
    };

    (@helper &self, $reg:ident, , $offset:expr) => {
        paste::paste! {
            #[inline]
            pub fn [< $reg:snake >]<T>(&self) -> T {
                self.device.mmio.read::<T>($offset)
            }

            #[inline]
            pub fn [< write_ $reg:snake >]<T>(&self, val: T) {
                self.device.mmio.write::<T>($offset, val);
            }
        }
    };
}

#[lazy]
pub static FDT: Vec<u8> = { backup_fdt() };

fn backup_fdt() -> Vec<u8> {
    unsafe {
        let fdt = fdt::Fdt::from_ptr(FDT_ADDR).unwrap();

        let total_size = fdt.total_size();

        let src_slice = slice::from_raw_parts(FDT_ADDR, total_size);

        let mut fdt_copy = vec![0u8; total_size];
        fdt_copy.copy_from_slice(src_slice);

        // 恢复
        let old_addr = FDT_ADDR;

        asm!(
            r#"la t0, FDT_ADDR
            sd {}, 0(t0)"#,
            in(reg) fdt_copy.as_ptr(),
        );

        assert_eq!(FDT_ADDR, fdt_copy.as_ptr());

        FRAME_ALLOCATOR
            .force()
            .lock()
            .add(&(old_addr as usize..MEMORY_RANGE.end));

        fdt_copy
    }
}
