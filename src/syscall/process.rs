//! 进程生命周期相关的系统调用。

use super::{RUsage, WaitOptions, WaitStatus};
use crate::proc::{
    CURRENT_TASK,
    task::{TASKS, TaskStatus, Tid},
};

pub(super) fn exit(code: i32) {
    let Some(tid) = *CURRENT_TASK.current() else {
        return;
    };
    if tid.0 == 1 {
        panic!("pid 1 exit")
    }

    let children = if let Some(task) = TASKS.force().lock().get_mut(&tid) {
        task.exit_code = code;
        task.status = TaskStatus::Zombie;
        let children = task.children.clone();
        task.children.clear();
        Some(children)
    } else {
        None
    };

    if let Some(children) = children
        && let Some(init) = TASKS.force().lock().get_mut(&Tid(1))
    {
        init.children.extend(children);
    }
}

pub(super) fn wait4(
    tid: isize,
    wait_status: *mut WaitStatus,
    options: WaitOptions,
    r_usage: *mut RUsage,
) -> Option<u64> {
    let Some(current) = *CURRENT_TASK.current() else {
        panic!()
    };

    let target_tid = match tid {
        x if x <= 0 => None,
        _ => Some(Tid(tid as usize)),
    };

    if options.nohang() {
        let result = get_zombie_children(current, target_tid);
        if let Some(tid) = result {
            let exit_code = TASKS
                .force()
                .lock()
                .get(&Tid(tid))
                .map(|task| task.exit_code)
                .unwrap_or_default();

            if !wait_status.is_null() {
                let status = WaitStatus::from(((exit_code as u32) & 0xff) << 8);
                unsafe { wait_status.write(status) };
            }
            if !r_usage.is_null() {
                unsafe { r_usage.write(RUsage::default()) };
            }
            Some(tid as u64)
        } else {
            Some(0)
        }
    } else {
        if let Some(task) = TASKS.force().lock().get_mut(&current) {
            task.status = TaskStatus::Waiting;
        }
        None
    }
}

fn get_zombie_children(parent: Tid, target: Option<Tid>) -> Option<usize> {
    let children = TASKS
        .force()
        .lock()
        .get(&parent)
        .map(|task| task.children.clone())?;

    if let Some(target) = target {
        children.iter().find(|child| **child == target)?;
        if TASKS.force().lock().get(&target)?.status == TaskStatus::Zombie {
            return Some(target.0);
        }
    } else {
        for child in children {
            if TASKS.force().lock().get(&child)?.status == TaskStatus::Zombie {
                return Some(child.0);
            }
        }
    }
    None
}
