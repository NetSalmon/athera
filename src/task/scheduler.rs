//! 任务调度器。
//!
//! 提供基于轮转的简单调度：[`save_current`] 保存被定时器中断抢占的当前任务现场，
//! [`switch`] 从任务表中选择下一个可运行任务并切换执行。

use crate::{
    arch::riscv64::trap::restore_context,
    task::{CURRENT_TASK, TASKS, TaskContext},
};

/// 保存被定时器中断抢占的当前任务现场。
pub fn save_current(trap_frame_sp: u64, sepc: u64, sstatus: u64, satp: u64) {
    let Some(current) = *CURRENT_TASK.current() else {
        return;
    };

    let frame = unsafe { &*(trap_frame_sp as *const [u64; 32]) };
    let mut tasks = TASKS.force().lock();
    let Some(task) = tasks.get_mut(&current) else {
        return;
    };

    task.trap_context.context = *frame;
    task.trap_context.sepc = sepc;
    task.trap_context.sstatus = sstatus;
    task.trap_context.satp = satp;
}

/// 调度器主循环：从任务表中选择下一个可运行任务并恢复其上下文。
///
/// 无限循环，每次迭代从 [`TASKS`] 中选择下一个任务，设置 [`CURRENT_TASK`]
/// 后通过 [`restore_context`] 恢复其寄存器现场并跳转执行。若任务表为空
/// 则继续自旋等待。
#[unsafe(no_mangle)]
pub fn switch() {
    loop {
        let TaskContext { tid, context: ctx } = {
            let mut tasks = TASKS.force().lock();
            if tasks.is_empty() {
                continue;
            }

            tasks.select_next().unwrap()
        };

        *CURRENT_TASK.current() = Some(tid);

        restore_context(&ctx);
    }
}
