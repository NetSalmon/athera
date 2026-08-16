use crate::{
    info,
    proc::{CURRENT_TASK, CurrentTask, task::TASKS},
    trap::restore_context,
};
use crate::proc::task::TaskContext;

/// 保存被定时器中断抢占的当前任务现场。
pub fn save_current(trap_frame_sp: u64, sepc: u64, sstatus: u64, satp: u64) {
    let Some(current) = *CURRENT_TASK.current() else {
        return;
    };

    let frame = unsafe { &*(trap_frame_sp as *const [u64; 32]) };
    let mut tasks = TASKS.force().lock();
    let Some(task) = tasks.get_mut(&current.tid) else {
        return;
    };

    task.trap_context.context = *frame;
    task.trap_context.sepc = sepc;
    task.trap_context.sstatus = sstatus;
    task.trap_context.satp = satp;
}

#[unsafe(no_mangle)]
pub fn switch() {
    loop {
        let TaskContext{ tid, context: ctx } = {
            let mut tasks = TASKS.force().lock();
            if tasks.is_empty() {
                continue;
            }

            tasks.spawn_current().unwrap()
        };
        
        *CURRENT_TASK.current() = Some(CurrentTask { tid, exit_code: None});

        restore_context(&ctx);
    }
}
