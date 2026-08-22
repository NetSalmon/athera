//! 进程（任务）管理。
//!
//! - [`task`]：任务控制块、任务状态、TID 分配器与全局任务表 `TASKS`；
//! - [`exec`]：从内嵌 ELF 加载用户程序，建立用户地址空间并切换进用户态。
use athera_macros::lazy;

use crate::{constants::MAX_CPU, sync::per_cpu::PerCpu, task::task::Tid};

pub(crate) mod exec;
pub(crate) mod process;
pub(crate) mod scheduler;
pub(crate) mod task;

#[lazy]
pub static CURRENT_TASK: PerCpu<Option<Tid>, MAX_CPU> = PerCpu::new([None; MAX_CPU]);
