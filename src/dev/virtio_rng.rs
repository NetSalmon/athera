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
    dev::{
        device::Device,
        virtio_mmio::{
            DeviceType, VirtioDevice,
            queue::{Flags, VRingDesc, Virtq},
        },
    },
    error::{Error, Result},
};

pub struct VirtioRng {
    pub device: Device,
    pub queues: Option<Vec<Virtq>>,
}

impl VirtioDevice for VirtioRng {
    const DEVICE_TYPE: DeviceType = DeviceType::ENTROPY_SOURCE;

    fn device(&self) -> Device {
        self.device
    }

    fn queues_mut(&mut self) -> &mut Option<Vec<Virtq>> {
        &mut self.queues
    }
}

impl From<Device> for VirtioRng {
    fn from(dev: Device) -> Self {
        VirtioRng {
            device: dev,
            queues: None,
        }
    }
}

impl VirtioRng {
    /// 通过 virtqueue 0 请求一次熵数据。
    ///
    /// 单个可写描述符（设备写入 `buf`），提交后轮询等待 used 元素，
    /// 返回设备实际写入的字节数（可能小于 `buf.len()`）。描述符 0 每次
    /// 复用，同一时刻最多一个在途请求。
    fn submit(&mut self, buf: &mut [u8]) -> Result<usize> {
        let last_used = {
            let queue = self.queue()?;
            {
                let q = queue.as_mut();

                let mut flags = Flags::new();
                flags.set_write(true);
                q.desc[0] = VRingDesc {
                    addr: buf.as_ptr() as u64,
                    len: buf.len() as u32,
                    flags,
                    next: 0,
                };
            }
            queue.post_avail(0)
        };

        // 通知设备处理队列 0。
        self.notify();

        // 等待设备产生 used 元素，校验被消费的描述符链头并返回写入长度。
        let elem = self.queue()?.wait_used(last_used)?;
        if elem.id != 0 {
            return Err(Error::VirtioRngFailed);
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
