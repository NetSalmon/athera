use crate::FDT_ADDRESS;
use crate::dev::memory::Memory;
use crate::dev::ns16550a::Ns16550a;
use crate::dev::virtio_blk::VirtioBlk;
use crate::error::Error;
use crate::locks::{LazyLock, SpinLock};
use core::sync::atomic::Ordering;

pub static DEV_TREE: LazyLock<DeviceTree> = LazyLock::new(|| {
    let fdt_addr = FDT_ADDRESS.load(Ordering::Acquire);

    DeviceTree::probe(fdt_addr as *const u8)
});

pub mod memory;
pub mod ns16550a;
pub mod virtio_blk;

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
    pub fn probe(fdt_addr: *const u8) -> Self {
        let fdt = unsafe { fdt::Fdt::from_ptr(fdt_addr) }
            .map_err(|_| Error::Fdt)
            .expect("fdt parse error");

        Self {
            ns16550a: Ns16550a::probe(&fdt).map(|uart| {
                uart.init();
                SpinLock::new(uart)
            }),
            virtio_blk: VirtioBlk::probe(&fdt),
            memory: Memory::probe(&fdt).expect("memory probe error"),
        }
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
