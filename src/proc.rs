use crate::locks::SpinLock;

#[const_val::const_val]
pub const PID_MAX: usize = 1024;

static PID_POOL: SpinLock<[u64; PID_MAX / 64]> = SpinLock::new([0; PID_MAX / 64]);

pub fn alloc_pid() -> u64 {
    let mut pool = PID_POOL.lock();
    for (i, word) in pool.iter_mut().enumerate() {
        if *word != u64::MAX {
            let bit = word.trailing_ones();
            *word |= 1 << bit;
            return (i as u64) * 64 + bit as u64;
        }
    }
    panic!("out of pids");
}
pub fn free_pid(pid: u64) {
    let mut pool = PID_POOL.lock();
    let idx = (pid / 64) as usize;
    let bit = pid % 64;
    pool[idx] &= !(1 << bit);
}

pub struct ProcessControlBlock {
    pub context: [u64; 512],
    pub satp: u64,
    pub sepc: u64,
    pub sstatus: u64,
    pub stvec: u64,
}
