use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::locks::{LazyLock, SpinLock};
use crate::mem::frame::AllocPage;

#[const_val::const_val]
pub const PID_MAX: usize = 1024;

pub static TASKS: LazyLock<SpinLock<BTreeMap<usize, TrapContext>>> = LazyLock::new(|| todo!());

static TID_POOL: SpinLock<[u64; PID_MAX / 64]> = SpinLock::new([0; PID_MAX / 64]);

pub fn alloc_tid() -> u64 {
    let mut pool = TID_POOL.lock();
    for (i, word) in pool.iter_mut().enumerate() {
        if *word != u64::MAX {
            let bit = word.trailing_ones();
            *word |= 1 << bit;
            return (i as u64) * 64 + bit as u64;
        }
    }
    panic!("out of pids");
}
pub fn free_tid(pid: u64) {
    let mut pool = TID_POOL.lock();
    let idx = (pid / 64) as usize;
    let bit = pid % 64;
    pool[idx] &= !(1 << bit);
}

pub struct TrapContext {
    pub context: [u64; 512],
    pub satp: u64,
    pub sepc: u64,
    pub sstatus: u64,
    pub stval: u64,
    pub stvec: u64,
}
