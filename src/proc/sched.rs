use alloc::collections::VecDeque;

use athera_macros::lazy;

use crate::{
    constants::MAX_CPU,
    info,
    proc::{CURRENT_TASK, CurrentTask, Tid, task::TASKS},
    sync::per_cpu::PerCpu,
    trap::restore_context,
};

#[lazy]
pub static TASK_QUEUE: PerCpu<VecDeque<Tid>, MAX_CPU> =
    PerCpu::new([const { VecDeque::new() }; MAX_CPU]);

#[unsafe(no_mangle)]
pub fn switch() -> ! {
    match *CURRENT_TASK.current() {
        Some(CurrentTask { tid, exit_code }) => {
            info!("task: {tid:?}, exit with: {exit_code:?}");
            let tcb = TASKS.force().lock().remove(&tid);
            drop(tcb);
        }
        None => info!("no prev task"),
    }

    info!("delete prev task");

    loop {
        let ctx = {
            let mut tasks = TASKS.force().lock();
            if tasks.is_empty() {
                continue;
            }

            tasks.run_first().unwrap()
        };

        restore_context(&ctx);
    }
}
