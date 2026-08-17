//! 重启与关机系统调用。

use super::abi::{ErrorCode, RebootCmd};
use crate::{
    arch::sbi::{
        self,
        srst::{ResetReason, ResetType, system_reset},
    },
    debug, info,
};

pub(super) fn reboot(magic: u64, magic2: u64, cmd: u64) -> isize {
    if magic != 0xfee1dead || magic2 != 0x28121969 {
        debug!("reboot: invalid magic (magic = {magic:#x}, magic2 = {magic2:#x})");
        return ErrorCode::EINVAL.0;
    }

    match RebootCmd::from(cmd) {
        RebootCmd::POWER_OFF => {
            info!("reboot: power off requested");
            system_reset(ResetType::SHUTDOWN, ResetReason::NONE);
        }
        RebootCmd::RESTART => {
            info!("reboot: restart requested");
            system_reset(ResetType::COLD_REBOOT, ResetReason::NONE);
        }
        RebootCmd::HALT => {
            info!("reboot: halt requested");
            sbi::hsm::hart_stop();
        }
        _ => {
            debug!("reboot: unsupported command {cmd:#x}");
            return ErrorCode::EINVAL.0;
        }
    }
    ErrorCode::EINVAL.0
}
