#![allow(dead_code)]
//! virtio-rng 熵源设备驱动。
//!
//! 基于 virtio-mmio 传输层（见 [`super::virtio_mmio`]）完成设备探测、
//! 特征协商与队列配置，并通过 [`athera_rand::EntropySource`] trait 提供
//! 真随机字节（例如给 ChaCha CSPRNG 种子化使用）。探测、握手与队列
//! 收发原语复用 [`super::virtio_mmio::VirtioDevice`] 通用抽象。
//!
//! 协议：virtio-rng 只有一个 virtqueue（索引 0），请求就是一个可写
//! 缓冲区——设备把随机字节写入缓冲区后放回 used 环，used 元素长度即
//! 实际写入的字节数。
use alloc::vec::Vec;

use athera_rand::{EntropyError, EntropySource};

use crate::{
    driver::{
        descriptor::Descriptor,
        device::DeviceInfo,
        virtio_mmio::{
            DeviceType, VirtioDevice,
            queue::{Flags, VirtQueue, VirtQueueDescriptor},
        },
    },
    error::DevError,
    sync::spin::SpinLock,
};

/// 本模块统一结果类型。
pub type DevResult<T> = core::result::Result<T, DevError>;

pub struct VirtioRng {
    pub device: DeviceInfo,
    pub queues: SpinLock<Option<Vec<VirtQueue>>>,
}

impl VirtioDevice for VirtioRng {
    const DEVICE_TYPE: DeviceType = DeviceType::ENTROPY_SOURCE;

    fn device(&self) -> DeviceInfo {
        self.device
    }

    fn queues(&self) -> &SpinLock<Option<Vec<VirtQueue>>> {
        &self.queues
    }
}

impl From<DeviceInfo> for VirtioRng {
    fn from(dev: DeviceInfo) -> Self {
        VirtioRng {
            device: dev,
            queues: SpinLock::new(None),
        }
    }
}

impl VirtioRng {
    pub fn from_desc(desc: &Descriptor) -> Option<Self> {
        let device = DeviceInfo::from_descriptor(desc)?;
        crate::driver::virtio_mmio::is_device(device, Self::DEVICE_TYPE).then(|| device.into())
    }

    /// 通过 virtqueue 0 请求一次熵数据。
    ///
    /// 单个可写描述符（设备写入 `buf`），提交后轮询等待 used 元素，
    /// 返回设备实际写入的字节数（可能小于 `buf.len()`）。描述符 0 每次
    /// 复用，同一时刻最多一个在途请求。
    fn submit(&self, buf: &mut [u8]) -> DevResult<usize> {
        let elem = VirtioDevice::submit(self, |q| {
            let mut flags = Flags::new();
            flags.set_write(true);
            q.desc[0] = VirtQueueDescriptor {
                addr: buf.as_mut_ptr() as u64,
                len: buf.len() as u32,
                flags,
                next: 0,
            };
        })?;
        if elem.id != 0 {
            return Err(DevError::VirtioRngFailed);
        }
        Ok(elem.len as usize)
    }
}

impl EntropySource for VirtioRng {
    fn fill_bytes(&mut self, dest: &mut [u8]) -> core::result::Result<(), EntropyError> {
        let mut done = 0;
        while done < dest.len() {
            match self.submit(&mut dest[done..]) {
                Ok(written) => {
                    // 设备写入量不应超过请求长度，防御性截断。
                    let written = written.min(dest.len() - done);
                    if written == 0 {
                        crate::error!("virtio-rng returned 0 bytes");
                        return Err(EntropyError);
                    }
                    done += written;
                }
                Err(err) => {
                    crate::error!("virtio-rng request failed: {err}");
                    return Err(EntropyError);
                }
            }
        }
        Ok(())
    }
}
