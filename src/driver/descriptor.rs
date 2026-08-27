//! FDT 节点描述符。
//!
//! 将设备树（FDT）节点解析为统一的 [`Descriptor`] 描述符，包含设备名称、
//! compatible 字符串、内存区域、中断号和自定义属性。设备管理器（[`crate::driver::tree`]
//! 的 `DEVICE_MANAGER`）据此登记驱动并分配 `dev_t` 设备号。

use alloc::{
    borrow::ToOwned,
    collections::BTreeMap,
    string::{String, ToString},
};

use fdt::node::FdtNode;

use crate::driver::Vec;

/// 设备树节点的统一描述符。
///
/// 从 FDT 节点（[`FdtNode`]）解析而来，包含设备名称、compatible 字符串列表、
/// 内存区域（`reg`）、中断号（`interrupts`）以及其他自定义属性。设备管理器
/// 据此匹配驱动并分配 `dev_t` 设备号。
#[derive(Debug)]
pub struct Descriptor {
    pub name: String,
    pub compatible: Vec<String>,

    pub resource: Vec<Region>,

    pub irq: Vec<usize>,

    pub props: BTreeMap<String, Vec<u8>>,
}

impl From<FdtNode<'_, '_>> for Descriptor {
    fn from(node: FdtNode) -> Descriptor {
        let name = node.name.to_string();

        let compatible = node
            .compatible()
            .iter()
            .flat_map(|compatible| compatible.all())
            .map(|comp| comp.to_string())
            .collect();

        let resource = node
            .reg()
            .into_iter()
            .flat_map(|reg| {
                reg.map(|r| Region {
                    base: r.starting_address as usize,
                    size: r.size.unwrap_or(0),
                })
            })
            .collect();

        let irq = node
            .interrupts()
            .map(|irq| irq.collect())
            .unwrap_or_default();

        let props = node
            .properties()
            .map(|prop| (prop.name.to_string(), prop.value.to_owned()))
            .collect();

        Descriptor {
            name,
            compatible,
            resource,
            irq,
            props,
        }
    }
}

/// 设备内存区域（对应 FDT 的 `reg` 属性）。
#[derive(Debug)]
pub struct Region {
    pub base: usize,
    pub size: usize,
}
