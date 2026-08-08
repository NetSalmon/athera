#![allow(dead_code)]
//! 设备号与设备树。
//!
//! [`DeviceId`] 即设备号，由 [`bits!`](crate::bits) 生成：低 `MINOR_BITS`
//! 位为从设备号，高 `MAJOR_BITS` 位为主设备号。设备号空间按主设备号
//! 划分成整段区间，由 [`DeviceNumberAllocator`] 统一分配，支持：
//!
//! - [`DeviceNumberAllocator::alloc`]：分配任意空闲设备号；
//! - [`DeviceNumberAllocator::alloc_specific`]：分配指定主/从设备号；
//! - [`DeviceNumberAllocator::alloc_major`]：划出一整段主设备号，返回
//!   从设备号子分配器（drop 时自动归还）；
//! - [`DeviceNumberAllocator::dealloc`]：释放设备号。
//!
//! [`DeviceTree`] 以设备号为键登记设备。
use alloc::{collections::BTreeMap, sync::Arc};

use athera_id_alloc::{Id, IdAllocError, IdAllocator, SubAllocator};
use athera_macros::lazy;

use crate::{bits, dev::traits::Dev};

/// 设备号中主设备号占用的位数。
pub const MAJOR_BITS: u32 = 12;
/// 设备号中从设备号占用的位数。
pub const MINOR_BITS: u32 = 20;
/// 主设备号数量。
pub const MAJOR_COUNT: u32 = 1 << MAJOR_BITS;
/// 主设备号掩码。
pub const MAJOR_MASK: u32 = MAJOR_COUNT - 1;
/// 每个主设备号下从设备号的数量。
pub const MINOR_COUNT: u32 = 1 << MINOR_BITS;
/// 从设备号掩码。
pub const MINOR_MASK: u32 = MINOR_COUNT - 1;

bits! {
    pub type DeviceId : u32 {
        minor: 0 => 19,
        major: 20 => 31,
    }
}

impl DeviceId {
    /// 由主设备号与从设备号构建设备号（`MKDEV`）。
    pub const fn mkdev(major: u32, minor: u32) -> Self {
        Self::from(((major & MAJOR_MASK) << MINOR_BITS) | (minor & MINOR_MASK))
    }
}

impl Id for DeviceId {
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
}

/// 设备号分配器，支持主设备号 + 从设备号两级分配。
///
/// 内部用 [`IdAllocator<DeviceId>`] 管理整个设备号空间；主设备号通过
/// [`alloc_major`](Self::alloc_major) 作为整段区间划出，从设备号在
/// 返回的 [`SubAllocator`] 中独立分配。
pub struct DeviceNumberAllocator {
    inner: IdAllocator<DeviceId>,
}

impl DeviceNumberAllocator {
    pub fn new() -> Self {
        Self {
            inner: IdAllocator::from_range(DeviceId::MIN..DeviceId::MAX),
        }
    }

    /// 分配任意一个空闲设备号。
    pub fn alloc(&mut self) -> Option<DeviceId> {
        self.inner.alloc()
    }

    /// 分配指定主设备号 + 从设备号。
    pub fn alloc_specific(&mut self, major: u32, minor: u32) -> Result<DeviceId, IdAllocError> {
        let dev = DeviceId::mkdev(major, minor);
        self.inner.alloc_specific(dev)?;
        Ok(dev)
    }

    /// 分配一个新的主设备号，返回其从设备号子分配器。
    ///
    /// 从设备号 0 起逐个主设备号扫描，找第一段整段空闲的主设备号区间
    /// 并划出；从设备号通过返回的 [`SubAllocator`] 独立分配，子分配器
    /// 释放（drop）时整段区间自动归还。主设备号全部占用时返回 `None`。
    pub fn alloc_major(&mut self) -> Option<SubAllocator<'_, DeviceId>> {
        let mut range = None;
        for major in 0..MAJOR_COUNT {
            let start = DeviceId::mkdev(major, 0);
            // 最后一个主设备号的区间上界为整个设备号空间上界（`u32::MAX`）。
            let end = if major + 1 < MAJOR_COUNT {
                DeviceId::from((major + 1) << MINOR_BITS)
            } else {
                DeviceId::MAX
            };
            let candidate = start..end;
            if self.inner.is_range_free(candidate.clone()) {
                range = Some(candidate);
                break;
            }
        }
        self.inner.alloc_range(range?).ok()
    }

    /// 释放一个设备号。
    pub fn dealloc(&mut self, dev: DeviceId) -> Result<(), IdAllocError> {
        self.inner.dealloc(dev)
    }
}

impl Default for DeviceNumberAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局设备号分配器。
#[lazy(spin)]
pub static DEVICE_NUMBER_ALLOCATOR: DeviceNumberAllocator = DeviceNumberAllocator::new();

/// 设备树：设备号 → 设备。
pub struct DeviceTree {
    pub tree: BTreeMap<DeviceId, Arc<dyn Dev>>,
}

impl DeviceTree {
    pub fn new() -> Self {
        Self {
            tree: BTreeMap::new(),
        }
    }

    /// 分配一个设备号并登记设备。
    pub fn register(&mut self, dev: Arc<dyn Dev>) -> Option<DeviceId> {
        let id = DEVICE_NUMBER_ALLOCATOR.lock().alloc()?;
        self.tree.insert(id, dev);
        Some(id)
    }

    /// 按设备号查找设备。
    pub fn get(&self, id: DeviceId) -> Option<&Arc<dyn Dev>> {
        self.tree.get(&id)
    }

    /// 注销设备并释放设备号。
    pub fn unregister(&mut self, id: DeviceId) -> Option<Arc<dyn Dev>> {
        DEVICE_NUMBER_ALLOCATOR.lock().dealloc(id).ok()?;
        self.tree.remove(&id)
    }
}
