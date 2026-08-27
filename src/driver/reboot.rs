//! Platform reboot and power-control operations.

use crate::{arch::riscv64::sbi, info};

pub(crate) fn reboot(command: u64) -> bool {
    match command {
        0x4321_fedc => {
            info!("reboot: power off requested");
            let _ = sbi::srst::system_reset(
                sbi::srst::ResetType::SHUTDOWN,
                sbi::srst::ResetReason::NONE,
            );
        }
        0x0123_4567 => {
            info!("reboot: restart requested");
            let _ = sbi::srst::system_reset(
                sbi::srst::ResetType::COLD_REBOOT,
                sbi::srst::ResetReason::NONE,
            );
        }
        0xcdef_0123 => {
            info!("reboot: halt requested");
            let _ = sbi::hsm::hart_stop();
        }
        _ => return false,
    }
    true
}
