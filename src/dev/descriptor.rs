use alloc::{
    borrow::ToOwned,
    collections::BTreeMap,
    string::{String, ToString},
};

use fdt::node::FdtNode;

use crate::dev::Vec;

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

#[derive(Debug)]
pub struct Region {
    pub base: usize,
    pub size: usize,
}
