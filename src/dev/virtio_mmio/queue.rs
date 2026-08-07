#![allow(dead_code)]
//! virtio 虚拟队列。
//!
//! 包含描述符表（`VRingDesc`）、avail/used 环（`VirtqAvail` /
//! `VirtqUsed`）与对齐到页的 [`Queue`] 布局，以及队列初始化逻辑。
use core::{
    alloc::Layout,
    ptr::addr_of,
    sync::atomic::{Ordering, fence},
};

use crate::{
    bits,
    constants::RING_SIZE,
    error::{DevError, MemError},
    mem::{alloc_page::AllocPage, allocators::FRAME_ALLOCATOR},
};

/// 本模块统一结果类型。
pub type Result<T> = core::result::Result<T, DevError>;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VRingDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: Flags,
    pub next: u16,
}

bits! {
    pub type Flags : u16 {
        next: 0,
        write: 1,
        indirect: 2,
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: VirtqRing<u16>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: VirtqRing<VirtqUsedElem>,
}

#[repr(C, align(4096))]
pub struct Queue {
    pub desc: [VRingDesc; RING_SIZE],
    pub avail: VirtqAvail,
    // 把 used 环对齐到下一页：legacy virtio 要求 used 环落在页边界上
    // （avail 紧跟 desc 表，offset = 16 * RING_SIZE = 4096）。
    _pad: [u8; 4096 - size_of::<VirtqAvail>()],
    pub used: VirtqUsed,
}

impl Queue {
    pub fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

pub struct Virtq {
    _mem: AllocPage,
}

impl Virtq {
    pub fn new() -> Result<Self> {
        let layout = Layout::new::<Queue>();
        let start = FRAME_ALLOCATOR
            .force()
            .lock()
            .alloc_frame(layout.size())
            .ok_or(MemError::OutOfMemory)?;
        unsafe {
            core::ptr::write_bytes(start as *mut u8, 0, layout.size());
        }
        Ok(Virtq {
            _mem: AllocPage {
                start,
                size: layout.size(),
            },
        })
    }

    pub fn desc_addr(&self) -> u64 {
        self._mem.start as u64
    }

    pub fn avail_addr(&self) -> u64 {
        self._mem.start as u64 + core::mem::offset_of!(Queue, avail) as u64
    }

    pub fn used_addr(&self) -> u64 {
        self._mem.start as u64 + core::mem::offset_of!(Queue, used) as u64
    }

    pub fn queue_ptr(&self) -> u64 {
        self._mem.start as u64
    }

    pub fn as_mut(&mut self) -> &mut Queue {
        unsafe { &mut *(self._mem.start as *mut Queue) }
    }

    /// 把 `head` 指向的描述符链追加到 avail 环，返回追加前的 used 索引
    /// （作 [`Self::wait_used`] 的等待基准）。
    ///
    /// 调用前必须已把描述符表填好；写入后执行 `fence`，保证设备在收到
    /// notify 前能看到全部描述符与 avail 更新。
    pub fn post_avail(&mut self, head: u16) -> u16 {
        let queue = self.as_mut();
        let last_used = queue.used.idx;

        // 环索引按队列大小（RING_SIZE）取模，与设备视角一致。
        let slot = queue.avail.idx as usize % RING_SIZE;
        queue.avail.ring[slot] = head;
        queue.avail.idx = queue.avail.idx.wrapping_add(1);

        fence(Ordering::SeqCst);
        last_used
    }

    /// 轮询等待设备产生新的 used 元素（`last_used` 之后第一个），
    /// 返回被消费的描述符链信息。
    pub fn wait_used(&mut self, last_used: u16) -> Result<VirtqUsedElem> {
        loop {
            let queue = self.as_mut();
            let used_idx = unsafe { addr_of!(queue.used.idx).read_volatile() };
            if used_idx != last_used {
                let slot = used_idx.wrapping_sub(1) as usize % RING_SIZE;
                let elem = unsafe { addr_of!(queue.used.ring[slot]).read_volatile() };
                return Ok(elem);
            }
            fence(Ordering::SeqCst);
            core::hint::spin_loop();
        }
    }
}

pub type VirtqRing<T> = [T; RING_SIZE];
