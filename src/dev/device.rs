#![allow(dead_code)]
#[derive(Copy, Clone)]
pub struct Resource {
    pub start: usize,
    pub size: usize,
}

impl Resource {
    pub const fn new(start: usize, size: usize) -> Self {
        Self { start, size }
    }

    #[inline]
    pub fn read<T>(&self, offset: usize) -> T {
        unsafe { ((self.start as *const u8).add(offset) as *const T).read_volatile() }
    }

    #[inline]
    pub fn write<T>(&self, offset: usize, val: T) {
        unsafe { ((self.start as *mut u8).add(offset) as *mut T).write_volatile(val) }
    }
}

#[derive(Copy, Clone)]
pub struct Device {
    pub mmio: Resource,
    pub irq: Option<usize>,
}

impl Device {
    pub const fn new(mmio: Resource, irq: Option<usize>) -> Self {
        Self { mmio, irq }
    }
}

#[macro_export]
macro_rules! mmio_regs {
    ($device:ident: [ $( $reg:ident $( : $t:ty )? => $offset:expr ),+ $(,)? ]) => {
        paste::paste! {
            $( #[allow(dead_code)] const [<$reg:upper _OFFSET>]: usize = $offset as usize; )+

            #[allow(dead_code)]
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
