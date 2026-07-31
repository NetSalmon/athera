#![allow(dead_code)]
use crate::numeric;

numeric! {
    pub enum SbiError: i64 {
        SUCCESS = 0,
        FAILED = -1,
        NOT_SUPPORTED = -2,
        INVALID_PARAM = -3,
        DENIED = -4,
        INVALID_ADDRESS = -5,
        ALREADY_AVAILABLE = -6,
        ALREADY_STARTED = -7,
        ALREADY_STOPPED = -8,
        NO_SHMEM = -9,
    }
}

#[derive(Debug)]
pub struct SbiResult {
    pub error: i64,
    pub value: u64,
}

#[derive(Debug)]
pub enum Result {
    Err(SbiError),
    Ok(u64),
}

impl From<SbiResult> for Result {
    fn from(result: SbiResult) -> Self {
        if result.error != 0 {
            let err = match result.error {
                0 => SbiError::SUCCESS,
                -1 => SbiError::FAILED,
                -2 => SbiError::NOT_SUPPORTED,
                -3 => SbiError::INVALID_PARAM,
                -4 => SbiError::DENIED,
                -5 => SbiError::INVALID_ADDRESS,
                -6 => SbiError::ALREADY_AVAILABLE,
                -7 => SbiError::ALREADY_STARTED,
                -8 => SbiError::ALREADY_STOPPED,
                -9 => SbiError::NO_SHMEM,
                _ => unreachable!(),
            };
            Result::Err(err)
        } else {
            Result::Ok(result.value)
        }
    }
}

numeric! {
    pub enum Eid: u64 {
        // Legacy extensions (v0.1). Each occupies its own EID and uses the legacy ABI.
        LEGACY_SET_TIMER = 0x00,
        LEGACY_CONSOLE_PUTCHAR = 0x01,
        LEGACY_CONSOLE_GETCHAR = 0x02,
        LEGACY_CLEAR_IPI = 0x03,
        LEGACY_SEND_IPI = 0x04,
        LEGACY_REMOTE_FENCE_I = 0x05,
        LEGACY_REMOTE_SFENCE_VMA = 0x06,
        LEGACY_REMOTE_SFENCE_VMA_ASID = 0x07,
        LEGACY_SHUTDOWN = 0x08,

        BASE = 0x10,
        TIME = 0x54494D45,
        IPI = 0x735049,
        RFENCE = 0x52464E43,
        HSM = 0x48534D,
        SRST = 0x53525354,
        PMU = 0x504D55,
        DBCN = 0x4442434E,
        SUSP = 0x53555350,
        FWFT = 0x46574654,
    }
}

pub fn ecall(eid: Eid, fid: u64, args: [u64; 6]) -> Result {
    let (mut a0, mut a1) = (args[0], args[1]);

    unsafe {
        core::arch::asm!(
        "ecall",
        inout("a0") a0,
        inout("a1") a1,
        in("a2") args[2],
        in("a3") args[3],
        in("a4") args[4],
        in("a5") args[5],
        in("a6") fid,
        in("a7") eid.0,
        );
    }

    SbiResult {
        error: a0 as i64,
        value: a1,
    }
    .into()
}

/// Legacy SBI v0.1 ecall.
///
/// The legacy ABI differs from the v0.2+ one: there is no FID, the EID lives
/// in `a7` alone, and the only return value is `a0` (no separate error / value
/// split). Some calls return 0 on success and a negative error otherwise;
/// `console_getchar` returns the byte read or -1.
pub fn legacy_ecall(eid: Eid, args: [u64; 4]) -> i64 {
    let mut a0 = args[0];

    unsafe {
        core::arch::asm!(
        "ecall",
        inout("a0") a0,
        in("a1") args[1],
        in("a2") args[2],
        in("a3") args[3],
        in("a7") eid.0,
        );
    }

    a0 as i64
}

// ========================== BASE ==========================
pub mod base {
    use super::*;

    pub fn get_spec_version() -> Result {
        ecall(Eid::BASE, 0, [0; 6])
    }

    pub fn get_impl_id() -> Result {
        ecall(Eid::BASE, 1, [0; 6])
    }

    pub fn get_impl_version() -> Result {
        ecall(Eid::BASE, 2, [0; 6])
    }

    pub fn probe_extension(eid: Eid) -> Result {
        ecall(Eid::BASE, 3, [eid.0, 0, 0, 0, 0, 0])
    }

    pub fn get_mvendorid() -> Result {
        ecall(Eid::BASE, 4, [0; 6])
    }

    pub fn get_marchid() -> Result {
        ecall(Eid::BASE, 5, [0; 6])
    }

    pub fn get_mimpid() -> Result {
        ecall(Eid::BASE, 6, [0; 6])
    }
}

// ========================== TIME ==========================
pub mod time {
    use super::*;

    pub fn set_timer(stime_value: u64) -> Result {
        ecall(Eid::TIME, 0, [stime_value, 0, 0, 0, 0, 0])
    }
}

// ========================== IPI ==========================
pub mod ipi {
    use super::*;

    pub fn send_ipi(hart_mask: u64, hart_mask_base: u64) -> Result {
        ecall(Eid::IPI, 0, [hart_mask, hart_mask_base, 0, 0, 0, 0])
    }
}

// ========================== RFENCE ==========================
pub mod rfence {
    use super::*;

    pub fn remote_fence_i(mask: u64, base: u64) -> Result {
        ecall(Eid::RFENCE, 0, [mask, base, 0, 0, 0, 0])
    }

    pub fn remote_sfence_vma(mask: u64, base: u64, start: u64, size: u64) -> Result {
        ecall(Eid::RFENCE, 1, [mask, base, start, size, 0, 0])
    }

    pub fn remote_sfence_vma_asid(
        mask: u64,
        base: u64,
        start: u64,
        size: u64,
        asid: u64,
    ) -> Result {
        ecall(Eid::RFENCE, 2, [mask, base, start, size, asid, 0])
    }
}

// ========================== HSM ==========================
pub mod hsm {
    use super::*;

    pub fn hart_start(hart_id: u64, start_addr: u64, opaque: u64) -> Result {
        ecall(Eid::HSM, 0, [hart_id, start_addr, opaque, 0, 0, 0])
    }

    pub fn hart_stop() -> Result {
        ecall(Eid::HSM, 1, [0; 6])
    }

    pub fn hart_get_status(hart_id: u64) -> Result {
        ecall(Eid::HSM, 2, [hart_id, 0, 0, 0, 0, 0])
    }

    pub fn hart_suspend(suspend_type: u64, resume_addr: u64, opaque: u64) -> Result {
        ecall(Eid::HSM, 3, [suspend_type, resume_addr, opaque, 0, 0, 0])
    }
}

// ========================== SRST ==========================
pub mod srst {
    use super::*;

    numeric! {
        pub enum ResetType: u64 {
            SHUTDOWN = 0,
            COLD_REBOOT = 1,
            WARM_REBOOT = 2,
        }
    }

    numeric! {
        pub enum ResetReason: u64 {
            NONE = 0,
            SYS_FAIL = 1,
        }
    }

    pub fn system_reset(reset_type: ResetType, reset_reason: ResetReason) -> Result {
        ecall(
            Eid::SRST,
            0,
            [reset_type.0, reset_reason.0, 0, 0, 0, 0],
        )
    }
}

// ========================== Legacy console (v0.1) ==========================
//
// These calls predate the BASE/probe model and are always either implemented
// or stubbed out by the SBI firmware (OpenSBI keeps them as a thin wrapper
// over DBCN when DBCN is present). They are useful as an early-boot console
// before the device tree has been parsed and a real UART driver brought up.
pub mod legacy {
    use super::*;

    /// Write a single byte to the debug console. Always returns 0 in OpenSBI.
    pub fn console_putchar(c: u8) {
        legacy_ecall(Eid::LEGACY_CONSOLE_PUTCHAR, [c as u64, 0, 0, 0]);
    }

    /// Read a single byte from the debug console.
    /// Returns `Some(byte)` on success, `None` if no byte is available
    /// (legacy ABI uses -1 to signal "no input").
    pub fn console_getchar() -> Option<u8> {
        let r = legacy_ecall(Eid::LEGACY_CONSOLE_GETCHAR, [0; 4]);
        if r < 0 { None } else { Some(r as u8) }
    }

    pub fn shutdown() -> ! {
        legacy_ecall(Eid::LEGACY_SHUTDOWN, [0; 4]);
        // SBI never returns from shutdown; loop just to satisfy `!`.
        loop {
            unsafe { core::arch::asm!("wfi") }
        }
    }

    pub fn set_timer(stime_value: u64) {
        legacy_ecall(Eid::LEGACY_SET_TIMER, [stime_value, 0, 0, 0]);
    }

    pub fn clear_ipi() {
        legacy_ecall(Eid::LEGACY_CLEAR_IPI, [0; 4]);
    }
}

// ========================== DBCN (Debug Console) ==========================
//
// The modern replacement for the legacy console. Probe with
// `base::probe_extension(EID::DBCN)` before using; fall back to `legacy`
// otherwise.
pub mod dbcn {
    use super::*;

    /// Write `len` bytes starting at physical address `base_addr`.
    pub fn console_write(len: u64, base_addr_lo: u64, base_addr_hi: u64) -> Result {
        ecall(Eid::DBCN, 0, [len, base_addr_lo, base_addr_hi, 0, 0, 0])
    }

    /// Read up to `len` bytes into the buffer at physical address `base_addr`.
    pub fn console_read(len: u64, base_addr_lo: u64, base_addr_hi: u64) -> Result {
        ecall(Eid::DBCN, 1, [len, base_addr_lo, base_addr_hi, 0, 0, 0])
    }

    /// Write a single byte. Convenient for putchar-style output.
    pub fn console_write_byte(b: u8) -> Result {
        ecall(Eid::DBCN, 2, [b as u64, 0, 0, 0, 0, 0])
    }

    /// Helper: write a whole byte slice. The buffer must live in memory
    /// addressable by the firmware (identity-mapped at boot, so this is fine
    /// before paging is enabled).
    pub fn write_bytes(buf: &[u8]) -> Result {
        let ptr = buf.as_ptr() as u64;
        // 32-bit hi half is 0 on RV64 with sv39/sv48 user addresses below 2^64.
        console_write(buf.len() as u64, ptr, 0)
    }
}
