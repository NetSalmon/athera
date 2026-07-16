use alloc::collections::BTreeMap;
use const_val::lazy;
use crate::locks::{SpinLock};
use crate::trap::TrapContext;

#[const_val::const_val]
pub const PID_MAX: usize = 1024;

#[lazy(spin)]
pub static TASKS: BTreeMap<usize, TrapContext> = BTreeMap::new();

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Running,
    Waiting,
    Sleeping,
    Zombie,
    Stopped,
    Dead,
}
