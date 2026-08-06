#![no_std]

//! athera-rand：no_std 随机数库。
//!
//! 提供两种随机数生成器：
//!
//! - [`Rng`]（即 [`SecureRng`]）：**密码学安全**随机数，基于 ChaCha20（CSPRNG），
//!   默认推荐使用；用 [`EntropySource`]（例如 virtio-rng）提供真随机种子。
//! - [`FastRng`]（即 [`XoShiro256StarStar`]）：快速但**非密码学安全**的确定性 PRNG，
//!   适合对安全性无要求、只需要高性能的场景（如模拟、采样）。
//!
//! 两种生成器都实现了 `rand_core` 的 [`Rng`](rand_core::Rng) trait（通过
//! [`TryRng`](rand_core::TryRng) 的 blanket impl），可以通用传参；
//! 其中 [`SecureRng`] 额外实现了 `CryptoRng` 标记。

pub use rand_chacha::{self, rand_core};
use rand_core::{Rng as _, SeedableRng};

// ---------------------------------------------------------------------------
// 熵源
// ---------------------------------------------------------------------------

/// 熵源：提供不可预测的真随机字节。
///
/// 典型实现：virtio-rng、硬件 TRNG、RDRAND 等。
///
/// 契约：成功时（`Ok`）必须**填满** `dest`；任何失败返回 [`EntropyError`]，
/// 此时 `dest` 内容未定义，调用方不应使用。
pub trait EntropySource {
    /// 用真随机字节填满 `dest`。
    fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), EntropyError>;
}

/// 熵源读取失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntropyError;

// ---------------------------------------------------------------------------
// 密码学安全随机数（默认）
// ---------------------------------------------------------------------------

/// 基于 ChaCha20 的密码学安全伪随机数生成器（CSPRNG）。
///
/// 内部是 RustCrypto `rand_chacha` 的 [`ChaCha20Rng`](rand_chacha::ChaCha20Rng)。
/// 密码学场景必须通过 [`Self::from_entropy`] 用真随机种子初始化，
/// 不要用固定种子（[`Self::from_seed`] / [`Default`] 仅供测试、调试）。
///
/// 本类型实现了 `rand_core` 的 [`Rng`](rand_core::Rng) 与
/// [`CryptoRng`](rand_core::CryptoRng)，可直接用于依赖 `rand_core` 的代码。
pub struct ChaChaRng {
    inner: rand_chacha::ChaCha20Rng,
}

impl ChaChaRng {
    /// 从熵源读取 32 字节真随机种子，构造 CSPRNG。
    pub fn from_entropy<S: EntropySource>(source: &mut S) -> Result<Self, EntropyError> {
        let mut seed = [0u8; 32];
        source.fill_bytes(&mut seed)?;
        Ok(Self::from_seed(seed))
    }

    /// 用固定种子构造。
    ///
    /// 确定性输出，**不是密码学安全的**，仅供测试 / 调试。
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            inner: SeedableRng::from_seed(seed),
        }
    }

    /// 生成下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// 生成下一个 `u32`。
    pub fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    /// 用随机字节填充 `dest`。
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.inner.fill_bytes(dest)
    }
}

impl Default for ChaChaRng {
    /// 全零固定种子，仅供测试 / 调试，**不是密码学安全的**。
    fn default() -> Self {
        Self::from_seed([0u8; 32])
    }
}

impl rand_core::TryRng for ChaChaRng {
    type Error = rand_core::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        self.inner.try_next_u32()
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        self.inner.try_next_u64()
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        self.inner.try_fill_bytes(dest)
    }
}

/// 正确种子化后是密码学安全的（ChaCha20 属于 CSPRNG）。
impl rand_core::TryCryptoRng for ChaChaRng {}

impl rand_core::SeedableRng for ChaChaRng {
    type Seed = [u8; 32];

    fn from_seed(seed: Self::Seed) -> Self {
        Self {
            inner: SeedableRng::from_seed(seed),
        }
    }
}

// ---------------------------------------------------------------------------
// 快速随机数（非密码学安全）
// ---------------------------------------------------------------------------

/// xoshiro256** 生成器。
pub struct XoShiro256StarStar {
    s: [u64; 4],
}

impl XoShiro256StarStar {
    /// 创建一个状态全零的生成器。
    ///
    /// 全零状态会一直输出 0，使用前必须先调用 [`Self::seed`] 或
    /// [`Self::seed_from_raw`] 设置非零状态。
    pub fn new() -> Self {
        Self { s: [0; 4] }
    }

    /// 直接用 4 个 u64 原始状态字设置状态。
    ///
    /// `seed` 必须非全零（全零是退化不动点）。
    pub fn seed_from_raw(&mut self, seed: [u64; 4]) {
        self.s = seed;
    }

    /// 用一个 u64 种子初始化状态。
    ///
    /// 按官方建议用 splitmix64 把单一 64 位种子扩展成 4 个状态字，
    /// 避免调用者手动构造全零状态。等价于 `Self::from(seed)`。
    pub fn seed(&mut self, seed: u64) {
        let mut state = seed;
        for s in &mut self.s {
            *s = splitmix64(&mut state);
        }
    }

    /// 生成下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.step()
    }

    /// 生成下一个 `u32`（取高 32 位）。
    pub fn next_u32(&mut self) -> u32 {
        (self.step() >> 32) as u32
    }

    /// 用随机字节填充 `dest`。
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let bytes = self.step().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }

    /// 跳跃：等价于连续调用 2^128 次 [`Self::next_u64`]。
    ///
    /// 可用来切出 2^128 个互不重叠的子序列（并行计算）。
    pub fn jump(&mut self) {
        const JUMP: [u64; 4] = [
            0x180ec6d33cfd0aba,
            0xd5a61266f0c9392c,
            0xa9582618e03fc9aa,
            0x39abdc4529b1661c,
        ];
        self.jump_with(&JUMP);
    }

    /// 长跳跃：等价于连续调用 2^192 次 [`Self::next_u64`]。
    ///
    /// 可用来生成 2^64 个起始点，每个起始点再配合 [`Self::jump`]
    /// 切出 2^128 个互不重叠的子序列。
    pub fn long_jump(&mut self) {
        const LONG_JUMP: [u64; 4] = [
            0x76e15d3efefdcbbf,
            0xc5004e441c522fb3,
            0x77710069854ee241,
            0x39109bb02acbe635,
        ];
        self.jump_with(&LONG_JUMP);
    }

    /// 单步核心：输出 `rotl(s[1] * 5, 7) * 9` 并推进状态。
    ///
    /// 与参考 C 实现逐条对应；乘法用 `wrapping_mul` 避免 debug 构建
    /// 下溢出 panic。
    fn step(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);

        let t = self.s[1] << 17;

        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];

        self.s[2] ^= t;

        self.s[3] = self.s[3].rotate_left(45);

        result
    }

    /// `jump` / `long_jump` 的公共实现。
    ///
    /// 与参考 C 一致：先累加所有跳变字对应的状态异或，最后一次性写回。
    fn jump_with(&mut self, jump: &[u64; 4]) {
        let mut s0 = 0;
        let mut s1 = 0;
        let mut s2 = 0;
        let mut s3 = 0;

        for item in jump {
            for b in 0..64 {
                if item & 1u64 << b != 0 {
                    s0 ^= self.s[0];
                    s1 ^= self.s[1];
                    s2 ^= self.s[2];
                    s3 ^= self.s[3];
                }
                self.step();
            }
        }

        self.s[0] = s0;
        self.s[1] = s1;
        self.s[2] = s2;
        self.s[3] = s3;
    }
}

impl Iterator for XoShiro256StarStar {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.step())
    }
}

impl rand_core::TryRng for XoShiro256StarStar {
    type Error = rand_core::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.next_u32())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.next_u64())
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl Default for XoShiro256StarStar {
    fn default() -> Self {
        Self::new()
    }
}

impl From<[u64; 4]> for XoShiro256StarStar {
    fn from(s: [u64; 4]) -> Self {
        Self { s }
    }
}

impl From<u64> for XoShiro256StarStar {
    fn from(seed: u64) -> Self {
        let mut rng = Self::new();
        rng.seed(seed);
        rng
    }
}

// ---------------------------------------------------------------------------
// 公开别名
// ---------------------------------------------------------------------------

/// 密码学安全随机数生成器（ChaCha20 CSPRNG）。
pub type SecureRng = ChaChaRng;

/// 默认随机数生成器：**密码学安全**（ChaCha20 CSPRNG）。
pub type Rng = SecureRng;

/// 快速随机数生成器（xoshiro256**，**非密码学安全**）。
pub type FastRng = XoShiro256StarStar;

/// splitmix64：官方建议的单一 64 位种子扩展函数。
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}
