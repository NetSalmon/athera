use crate::mem::frame::AllocPage;

pub struct ProcessControlBlock {
    pub context: [u64; 512],
    pub satp: u64,
    pub sepc: u64,
    pub sstatus: u64,
    pub stvec: u64,
    pub stack: AllocPage,
}
