//! Linux 兼容的设备号分配与设备登记表。

use crate::dev::{Arc, FDT};
use crate::dev::Vec;
use athera_id_alloc::{Id as IdTrait, IdAlloc};

use crate::bits;
use crate::constants::FDT_ADDR;
use crate::dev::descriptor::Descriptor;

/// Linux `dev_t` 中主设备号占用的位数。
pub const MAJOR_BITS: u32 = 12;
/// Linux `dev_t` 中从设备号占用的位数。
pub const MINOR_BITS: u32 = 20;
pub const MAJOR_COUNT: u32 = 1 << MAJOR_BITS;
pub const MINOR_COUNT: u32 = 1 << MINOR_BITS;
pub const MAJOR_MASK: u32 = MAJOR_COUNT - 1;
pub const MINOR_MASK: u32 = MINOR_COUNT - 1;

// Linux 兼容的设备号：`major << MINOR_BITS | minor`
bits! {
    pub type Did: u32 {
        minor: 0 => 19,
        major: 20 => 31,
    }
}

impl Did {
    /// Linux `MKDEV(major, minor)`。
    pub const fn mkdev(major: u32, minor: u32) -> Self {
        Self::from(((major & MAJOR_MASK) << MINOR_BITS) | (minor & MINOR_MASK))
    }
}

impl IdTrait for Did {
    const BITS: u32 = 32;
    const MAX: Self = Self::from(u32::MAX);
    const MIN: Self = Self::from(0);

    fn next(&self) -> Option<Self> {
        u32::from(*self).checked_add(1).map(Self::from)
    }

    fn prev(&self) -> Option<Self> {
        u32::from(*self).checked_sub(1).map(Self::from)
    }

    fn distance_to(&self, other: &Self) -> usize {
        (u32::from(*other) - u32::from(*self)) as usize
    }

    fn to_bits(&self) -> u128 {
        u32::from(*self) as u128
    }

    fn from_bits(bits: u128) -> Self {
        Self::from(bits as u32)
    }
}

/// 设备号分配器的 minor 位宽（const 泛型用）。
const MINOR_BITS_USIZE: usize = MINOR_BITS as usize;

/// 设备号分配器类型。
pub type DidAlloc = IdAlloc<Did, MINOR_BITS_USIZE>;
/// Linux 设备驱动常用的静态主号，初始化时预先划入主号表。
pub const PRESET_MAJORS: &[u32] = &[1, 4, 8, 10, 252];

pub struct DeviceManager {
    pub id_alloc: DidAlloc,
}

#[derive(Debug)]
pub struct DeviceDescriptors {
    pub descriptors: Vec<Arc<Descriptor>>
}

impl DeviceDescriptors {
    pub fn probe() -> DeviceDescriptors {
        let fdt = unsafe { fdt::Fdt::from_ptr(FDT.force().as_ptr()).unwrap() };
        let mut decs = Vec::new();
        for i in fdt.all_nodes() {
            let descriptor = i.into();
            decs.push(Arc::new(descriptor));
        }
        
        DeviceDescriptors { descriptors: decs }
    }
}