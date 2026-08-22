#![allow(dead_code)]
//! 设备抽象。
//!
//! [`Resource`] 描述一段 MMIO 区间（易失读写），[`DeviceInfo`] 附加可选
//! 中断号；`mmio_regs!` 宏根据偏移量为设备生成寄存器读写方法。
#[derive(Copy, Clone)]
pub struct Resource {
    pub start: usize,
    pub size: usize,
}

impl Resource {
    pub const fn new(start: usize, size: usize) -> Self {
        Self { start, size }
    }

    /// 从 `start + offset` 处读取一个 `T`（易失读）。
    ///
    /// # Safety
    ///
    /// 要求 `start..start+offset+size_of::<T>()` 落在合法 MMIO 区间内。
    #[inline]
    pub fn read<T>(&self, offset: usize) -> T {
        unsafe { ((self.start as *const u8).add(offset) as *const T).read_volatile() }
    }

    /// 向 `start + offset` 处写入一个 `T`（易失写）。
    ///
    /// # Safety
    ///
    /// 要求 `start..start+offset+size_of::<T>()` 落在合法 MMIO 区间内。
    #[inline]
    pub fn write<T>(&self, offset: usize, val: T) {
        unsafe { ((self.start as *mut u8).add(offset) as *mut T).write_volatile(val) }
    }
}

#[derive(Copy, Clone)]
pub struct DeviceInfo {
    pub mmio: Resource,
    pub irq: Option<usize>,
}

impl DeviceInfo {
    pub const fn new(mmio: Resource, irq: Option<usize>) -> Self {
        Self { mmio, irq }
    }

    pub fn from_descriptor(desc: &crate::driver::descriptor::Descriptor) -> Option<Self> {
        let region = desc.resource.first()?;
        Some(Self::new(
            Resource::new(region.base, region.size),
            desc.irq.first().copied(),
        ))
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
