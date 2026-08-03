#![allow(unused)]

pub enum ScheduleRule {
    Batch,
    TimeSharing,
}

#[unsafe(no_mangle)]
pub fn schedule() {}
