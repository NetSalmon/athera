#![allow(dead_code)]
//! virtio-rng 熵源设备驱动。
//!
//! 基于 virtio-mmio 传输层（见 [`super::virtio_mmio`]）完成设备探测、
//! 特征协商与队列配置，并通过 [`athera_rand::EntropySource`] trait 提供
//! 真随机字节（例如给 ChaCha CSPRNG 种子化使用）。
//!
//! 协议：virtio-rng 只有一个 virtqueue（索引 0），请求就是一个可写
//! 缓冲区——设备把随机字节写入缓冲区后放回 used 环，used 元素长度即
//! 实际写入的字节数。
use alloc::vec::Vec;
use core::{
    ptr::addr_of,
    sync::atomic::{Ordering, fence},
};

use athera_rand::{EntropyError, EntropySource};
use fdt::Fdt;

use crate::{
    constants::{RING_SIZE, VIRTIO_VERSION_LEGACY},
    dev::{
        device::{Device, Resource},
        virtio_mmio::{
            DeviceType, VirtqCfg,
            handshake::QueueConfig,
            queue::{Flags, VRingDesc, Virtq},
        },
    },
    error::{Error, Result},
};

/// `queue_notify` 寄存器偏移（见 [`VirtqCfg`]）。
const QUEUE_NOTIFY_OFFSET: usize = 0x050;

pub struct VirtioRng {
    pub device: Device,
    pub queues: Option<Vec<Virtq>>,
}

impl VirtioRng {
    /// 遍历设备树中所有 virtio,mmio 节点，逐个读取其 MMIO 寄存器中的
    /// `device_id`，返回第一个 virtio-rng（熵源）设备；空槽位或其它
    /// virtio 设备（如 blk）会被跳过。
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        fdt.all_nodes().find_map(|node| {
            let is_mmio = node
                .compatible()
                .map(|c| c.all().any(|c| c == "virtio,mmio"))
                .unwrap_or(false);
            if !is_mmio {
                return None;
            }

            let reg = node.reg()?.next()?;
            let start = reg.starting_address as usize;
            let size = reg.size.unwrap_or(0);
            let interrupts = node.interrupts()?.next()?;

            let device = Device {
                mmio: Resource::new(start, size),
                irq: Some(interrupts),
            };

            // 空槽位返回 0，blk 等其它设备返回各自的 device_id，均跳过。
            let cfg = VirtqCfg { device };
            if cfg.device_id() != DeviceType::ENTROPY_SOURCE.0 {
                return None;
            }

            Some(VirtioRng {
                device,
                queues: None,
            })
        })
    }

    pub fn handshake(&mut self) -> Result<()> {
        let mut cfg = VirtqCfg {
            device: self.device,
        };
        let is_modern = cfg.version() != VIRTIO_VERSION_LEGACY;

        let queues = if is_modern {
            // virtio-rng 暂无设备特性位，协商 (0, 0)。
            cfg.handshake()
                .modern(|_low, _high| (0, 0))?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_SIZE as u32,
                })?
                .finish()
        } else {
            cfg.handshake()
                .legacy(|_f| 0u32)?
                .setup_queue(QueueConfig {
                    index: 0,
                    size: RING_SIZE as u32,
                })?
                .finish()
        };

        self.queues = Some(queues);
        Ok(())
    }

    pub fn from(dev: Device) -> VirtioRng {
        VirtioRng {
            device: dev,
            queues: None,
        }
    }

    /// 通过 virtqueue 0 请求一次熵数据。
    ///
    /// 单个可写描述符（设备写入 `buf`），提交后轮询等待 used 元素，
    /// 返回设备实际写入的字节数（可能小于 `buf.len()`）。描述符 0 每次
    /// 复用，同一时刻最多一个在途请求。
    fn submit(&mut self, buf: &mut [u8]) -> Result<usize> {
        // 探测成功后队列尚未配置，先补一次握手，保证 EntropySource 可直接使用。
        if self.queues.is_none() {
            self.handshake()?;
        }

        let last_used = {
            let queue = self
                .queues
                .as_mut()
                .and_then(|queues| queues.first_mut())
                .ok_or(Error::VirtioHandshakeFailed)?
                .as_mut();

            let last_used = queue.used.idx;

            let mut flags = Flags::new();
            flags.set_write(true);
            queue.desc[0] = VRingDesc {
                addr: buf.as_ptr() as u64,
                len: buf.len() as u32,
                flags,
                next: 0,
            };

            // 环索引按队列大小（RING_SIZE）取模，与设备视角一致。
            let slot = queue.avail.idx as usize % RING_SIZE;
            queue.avail.ring[slot] = 0;
            queue.avail.idx = queue.avail.idx.wrapping_add(1);

            // 保证描述符表与 avail 环的写入先于 notify 被设备看到。
            fence(Ordering::SeqCst);
            last_used
        };

        // 通知设备处理队列 0。
        self.device.mmio.write::<u32>(QUEUE_NOTIFY_OFFSET, 0);
        fence(Ordering::SeqCst);

        // 等待设备产生 used 元素，校验被消费的描述符链头并返回写入长度。
        loop {
            let queue = self
                .queues
                .as_mut()
                .and_then(|queues| queues.first_mut())
                .ok_or(Error::VirtioHandshakeFailed)?
                .as_mut();

            let used_idx = unsafe { addr_of!(queue.used.idx).read_volatile() };
            if used_idx != last_used {
                let slot = used_idx.wrapping_sub(1) as usize % RING_SIZE;
                let elem = unsafe { addr_of!(queue.used.ring[slot]).read_volatile() };
                if elem.id != 0 {
                    return Err(Error::VirtioRngFailed);
                }
                return Ok(elem.len as usize);
            }
            fence(Ordering::SeqCst);
            core::hint::spin_loop();
        }
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
