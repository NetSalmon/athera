use crate::{locks::PerCPU, proc::task::Tid};

pub mod exec;
pub mod task;

pub static CURRENT_TASK: PerCPU<Option<Tid>, 16> = PerCPU::new([None; 16]);
