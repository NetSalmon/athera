//! Linux 兼容的设备号分配与设备登记表。

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
};

use athera_id_alloc::{Id as IdTrait, IdAlloc};
use athera_macros::lazy;
use athera_rand::EntropySource;

use crate::{
    bits,
    driver::{
        Arc, FDT, Vec,
        descriptor::Descriptor,
        ns16550a::Ns16550a,
        traits::{BlockDevice, CharDevice, IoError, IoResult, MajorAlloc},
        virtio_blk::VirtioBlk,
        virtio_rng::VirtioRng,
    },
    fs::{FileType, Mode},
    sync::rwlock::RwLock,
};

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
    pub alloc_majors: Vec<Did>,

    pub by_id: BTreeMap<Did, Arc<DeviceNode>>,
    pub by_name: BTreeMap<String, Arc<DeviceNode>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        // `IdAlloc::new()` 没有可分配范围，必须先初始化完整 dev_t 空间。
        let mut id_alloc =
            DidAlloc::from_range(Did::MIN..Did::MAX).expect("dev_t allocator range must be valid");
        let mut alloc_majors = Vec::new();

        for i in PRESET_MAJORS.iter() {
            if let Ok(major) = id_alloc.alloc_major_at(Did::builder().set_major(*i).build()) {
                alloc_majors.push(major);
            }
        }

        Self {
            id_alloc,
            alloc_majors,
            by_id: BTreeMap::new(),
            by_name: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, desc: Arc<Descriptor>, ops: DeviceOps, mode: Mode) {
        let major = match &ops {
            DeviceOps::Block(ops) => ops.major(),
            DeviceOps::Char(ops) => ops.major(),
            DeviceOps::Entropy(_) => MajorAlloc::DynamicAlloc,
        };

        let major = match major {
            MajorAlloc::DynamicAlloc => {
                let major = self.id_alloc.alloc_major().unwrap();
                self.alloc_majors.push(major);
                major
            }
            MajorAlloc::StaticAlloc(did) => did,
        };

        let did = self.id_alloc.alloc_minor(major).unwrap();

        let node = Arc::new(DeviceNode {
            did,
            name: desc.name.clone(),
            ops,
            mode,
            descriptors: desc.clone(),
        });

        self.by_id.insert(did, Arc::clone(&node));
        self.by_name
            .insert(node.name.to_string(), Arc::clone(&node));
    }

    fn register_char(&mut self, desc: &Arc<Descriptor>, driver: impl CharDevice + 'static) {
        self.register(
            Arc::clone(desc),
            DeviceOps::Char(Arc::new(driver)),
            Mode::from((FileType::CHR.0 << 12) | 0o666),
        );
    }

    fn register_block(&mut self, desc: &Arc<Descriptor>, driver: impl BlockDevice + 'static) {
        self.register(
            Arc::clone(desc),
            DeviceOps::Block(Arc::new(driver)),
            Mode::from((FileType::BLK.0 << 12) | 0o660),
        );
    }

    fn register_rng(&mut self, desc: &Arc<Descriptor>, driver: VirtioRng) {
        self.register(
            Arc::clone(desc),
            DeviceOps::Entropy(Arc::new(RwLock::new(driver))),
            Mode::from((FileType::CHR.0 << 12) | 0o444),
        );
    }

    pub fn remove(&mut self, did: Did) -> Option<Arc<DeviceNode>> {
        let node = self.by_id.remove(&did)?;
        self.by_name.remove(&node.name)
    }

    pub fn find_by_name(&self, name: &str) -> Option<Arc<DeviceNode>> {
        self.by_name.get(name).cloned()
    }

    pub fn find_by_id(&self, id: Did) -> Option<Arc<DeviceNode>> {
        self.by_id.get(&id).cloned()
    }

    pub fn first_char(&self) -> Option<Did> {
        self.by_id.iter().find_map(|(did, node)| match &node.ops {
            DeviceOps::Char(_) => Some(*did),
            DeviceOps::Block(_) => None,
            DeviceOps::Entropy(_) => None,
        })
    }

    pub fn first_block(&self) -> Option<Did> {
        self.by_id.iter().find_map(|(did, node)| match &node.ops {
            DeviceOps::Block(_) => Some(*did),
            DeviceOps::Char(_) => None,
            DeviceOps::Entropy(_) => None,
        })
    }

    pub fn block_handle(&self) -> Option<ManagedBlockDevice> {
        self.first_block().map(ManagedBlockDevice::new)
    }

    pub fn read(&self, did: Did, buf: &mut [u8]) -> IoResult<usize> {
        let node = self.find_by_id(did).ok_or(IoError::NotReady)?;
        match &node.ops {
            DeviceOps::Char(device) => device.read(buf),
            DeviceOps::Block(_) => Err(IoError::Request),
            DeviceOps::Entropy(_) => Err(IoError::Request),
        }
    }

    pub fn write(&self, did: Did, buf: &[u8]) -> IoResult<usize> {
        let node = self.find_by_id(did).ok_or(IoError::NotReady)?;
        match &node.ops {
            DeviceOps::Char(device) => device.write(buf),
            DeviceOps::Block(_) => Err(IoError::Request),
            DeviceOps::Entropy(_) => Err(IoError::Request),
        }
    }

    pub fn read_at(&self, did: Did, buf: &mut [u8], offset: usize) -> IoResult<usize> {
        let node = self.find_by_id(did).ok_or(IoError::NotReady)?;
        match &node.ops {
            DeviceOps::Block(device) => device.read_at(buf, offset),
            DeviceOps::Char(_) => Err(IoError::Request),
            DeviceOps::Entropy(_) => Err(IoError::Request),
        }
    }

    pub fn write_at(&self, did: Did, buf: &[u8], offset: usize) -> IoResult<usize> {
        let node = self.find_by_id(did).ok_or(IoError::NotReady)?;
        match &node.ops {
            DeviceOps::Block(device) => device.write_at(buf, offset),
            DeviceOps::Char(_) => Err(IoError::Request),
            DeviceOps::Entropy(_) => Err(IoError::Request),
        }
    }

    pub fn fill_entropy(&self, dest: &mut [u8]) -> Result<(), athera_rand::EntropyError> {
        let did = self
            .by_id
            .iter()
            .find_map(|(did, node)| match &node.ops {
                DeviceOps::Entropy(_) => Some(*did),
                _ => None,
            })
            .ok_or(athera_rand::EntropyError)?;
        let node = self.find_by_id(did).ok_or(athera_rand::EntropyError)?;
        match &node.ops {
            DeviceOps::Entropy(source) => source.write().fill_bytes(dest),
            _ => Err(athera_rand::EntropyError),
        }
    }
}

/// 通过设备管理器访问块设备的稳定句柄，供文件系统层使用。
pub struct ManagedBlockDevice {
    did: Did,
}

impl ManagedBlockDevice {
    fn new(did: Did) -> Self {
        Self { did }
    }
}

impl crate::driver::traits::Device for ManagedBlockDevice {
    fn name(&self) -> &'static str {
        "managed-block"
    }

    fn irq(&self) -> Option<usize> {
        None
    }
}

impl crate::driver::traits::ReadAt for ManagedBlockDevice {
    fn read_at(&self, buf: &mut [u8], offset: usize) -> IoResult<usize> {
        DEVICE_MANAGER.force().read().read_at(self.did, buf, offset)
    }
}

impl crate::driver::traits::WriteAt for ManagedBlockDevice {
    fn write_at(&self, buf: &[u8], offset: usize) -> IoResult<usize> {
        DEVICE_MANAGER
            .force()
            .read()
            .write_at(self.did, buf, offset)
    }
}

#[derive(Debug)]
pub struct DeviceDescriptors {
    pub descriptors: Vec<Arc<Descriptor>>,
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

#[lazy]
pub static DEVICE_DESCRIPTORS: DeviceDescriptors = DeviceDescriptors::probe();

#[lazy]
pub static DEVICE_MANAGER: RwLock<DeviceManager> = {
    let mut manager = DeviceManager::new();

    for desc in &DEVICE_DESCRIPTORS.descriptors {
        if desc.compatible.iter().any(|value| value == "ns16550a") {
            if let Some(driver) = Ns16550a::from_desc(desc) {
                manager.register_char(desc, driver);
            }
        }

        if desc.compatible.iter().any(|value| value == "virtio,mmio") {
            if let Some(driver) = VirtioBlk::from_desc(desc) {
                manager.register_block(desc, driver);
            } else if let Some(driver) = VirtioRng::from_desc(desc) {
                manager.register_rng(desc, driver);
            }
        }
    }

    RwLock::new(manager)
};

pub enum DeviceOps {
    Block(Arc<dyn BlockDevice>),
    Char(Arc<dyn CharDevice>),
    Entropy(Arc<RwLock<VirtioRng>>),
}
pub struct DeviceNode {
    pub did: Did,

    pub mode: Mode,
    pub name: String,

    pub ops: DeviceOps,
    pub descriptors: Arc<Descriptor>,
}
