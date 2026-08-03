#![allow(dead_code)]
//! 内核全局随机源。
//!
//! 首次访问（`RNG.force()` / `random_u64()` 等）时用 virtio-rng 熵源
//! 种子化一个 ChaCha20 CSPRNG（密码学安全），之后内核各处直接获取
//! 随机数即可；没有 virtio-rng 时回退到固定种子（此时**不是**密码学
//! 安全的，会打印警告）。
//!
//! 启动流程在 `identity_map()` 之后主动触发一次（见 `main`），保证
//! 种子化发生在设备 MMIO 映射完成之后。

use athera_const::lazy;
use athera_rand::SecureRng;

use crate::{dev::VIRTIO_RNG, warn};

/// 全局密码学安全随机数生成器（ChaCha20 CSPRNG）。
#[lazy(spin)]
pub static RNG: SecureRng = {
    let mut rng = SecureRng::default();
    if let Some(source) = VIRTIO_RNG.lock().as_mut() {
        match SecureRng::from_entropy(source) {
            Ok(r) => rng = r,
            Err(err) => warn!("failed to seed global RNG from virtio-rng: {err:?}"),
        }
    } else {
        warn!("no virtio-rng found, global RNG uses a fixed seed (not crypto-secure)");
    }
    rng
};

/// 生成一个密码学安全 `u64`。
pub fn random_u64() -> u64 {
    RNG.lock().next_u64()
}

/// 用密码学安全随机字节填充 `buf`。
pub fn random_bytes(buf: &mut [u8]) {
    RNG.lock().fill_bytes(buf)
}
