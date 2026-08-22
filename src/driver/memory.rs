//! 系统内存探测。
//!
//! 从设备树 `/memory` 节点解析物理内存起始地址与大小，包装成
//! [`DeviceInfo`]。
use fdt::Fdt;

use crate::driver::device::{DeviceInfo, Resource};

pub struct Memory {
    pub device: DeviceInfo,
}

impl Memory {
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let node = fdt.find_node("/memory")?;
        let range = node.reg()?.next()?;
        let start = range.starting_address as usize;
        let size = range.size?;

        let result = Self {
            device: DeviceInfo {
                mmio: Resource { start, size },
                irq: None,
            },
        };

        Some(result)
    }
}
