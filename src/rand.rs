#![allow(dead_code)]
//! 内核每 hart 随机源。
//!
//! 首次访问（`random_u64()` 等）时用设备管理器中的 virtio-rng 熵源
//! 种子化一个 ChaCha20 CSPRNG（密码学安全），之后内核各处直接获取
//! 随机数即可；没有 virtio-rng 时回退到固定种子（此时**不是**密码学
//! 安全的，会打印警告）。
//!
//! 启动流程在 `identity_map()` 之后主动触发一次（见 `main`），保证
//! 种子化发生在设备 MMIO 映射完成之后。

use athera_macros::lazy;
use athera_rand::SecureRng;

use crate::{constants::MAX_CPU, driver::tree::DEVICE_MANAGER, sync::per_cpu::PerCpu, warn};

/// 每 hart 独立的密码学安全随机数生成器（ChaCha20 CSPRNG）。
#[lazy]
pub static RNG: PerCpu<SecureRng, MAX_CPU> = {
    let mut master_seed = [0u8; 32];
    let seeded = match DEVICE_MANAGER.force().read().fill_entropy(&mut master_seed) {
        Ok(()) => true,
        Err(err) => {
            warn!("failed to seed per-CPU RNGs from virtio-rng: {err}");
            false
        }
    };

    if !seeded {
        warn!("no virtio-rng found, per-CPU RNGs use fixed seeds (not crypto-secure)");
    }

    let rngs = core::array::from_fn(|cpu| {
        let mut seed = master_seed;
        let cpu = (cpu as u64).to_le_bytes();
        for (dst, src) in seed[..cpu.len()].iter_mut().zip(cpu) {
            *dst ^= src;
        }
        SecureRng::from_seed(seed)
    });
    PerCpu::new(rngs)
};

/// 生成一个密码学安全 `u64`。
pub fn random_u64() -> u64 {
    RNG.force().current().next_u64()
}

/// 用密码学安全随机字节填充 `buf`。
pub fn random_bytes(buf: &mut [u8]) {
    RNG.force().current().fill_bytes(buf)
}
