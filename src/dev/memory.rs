//! 系统内存探测。
//!
//! 从设备树 `/memory` 节点解析物理内存起始地址与大小，包装成
//! [`Device`]。
use fdt::Fdt;

use crate::dev::device::{Device, Resource};

pub struct Memory {
    pub device: Device,
}

impl Memory {
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let node = fdt.find_node("/memory")?;
        let range = node.reg()?.next()?;
        let start = range.starting_address as usize;
        let size = range.size?;

        let result = Self {
            device: Device {
                mmio: Resource { start, size },
                irq: None,
            },
        };

        Some(result)
    }
}
