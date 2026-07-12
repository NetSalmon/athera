#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SbiError {
    Success = 0,
    Failed = -1,
    NotSupported = -2,
    InvalidParam = -3,
    Denied = -4,
    InvalidAddress = -5,
    AlreadyAvailable = -6,
    AlreadyStarted = -7,
    AlreadyStopped = -8,
    NoShmem = -9,
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
                0 => SbiError::Success,
                -1 => SbiError::Failed,
                -2 => SbiError::NotSupported,
                -3 => SbiError::InvalidParam,
                -4 => SbiError::Denied,
                -5 => SbiError::InvalidAddress,
                -6 => SbiError::AlreadyAvailable,
                -7 => SbiError::AlreadyStarted,
                -8 => SbiError::AlreadyStopped,
                -9 => SbiError::NoShmem,
                _ => unreachable!(),
            };
            Result::Err(err)
        } else {
            Result::Ok(result.value)
        }
    }
}

#[repr(u64)]
#[derive(Clone, Copy)]
pub enum Eid {
    // Legacy extensions (v0.1). Each occupies its own EID and uses the legacy ABI.
    LegacySetTimer = 0x00,
    LegacyConsolePutchar = 0x01,
    LegacyConsoleGetchar = 0x02,
    LegacyClearIpi = 0x03,
    LegacySendIpi = 0x04,
    LegacyRemoteFenceI = 0x05,
    LegacyRemoteSfenceVma = 0x06,
    LegacyRemoteSfenceVmaAsid = 0x07,
    LegacyShutdown = 0x08,

    Base = 0x10,
    Time = 0x54494D45,
    Ipi = 0x735049,
    Rfence = 0x52464E43,
    Hsm = 0x48534D,
    Srst = 0x53525354,
    Pmu = 0x504D55,
    Dbcn = 0x4442434E,
    Susp = 0x53555350,
    Fwft = 0x46574654,
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
        in("a7") eid as u64,
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
        in("a7") eid as u64,
        );
    }

    a0 as i64
}

// ========================== BASE ==========================
pub mod base {
    use super::*;

    pub fn get_spec_version() -> Result {
        ecall(Eid::Base, 0, [0; 6])
    }

    pub fn get_impl_id() -> Result {
        ecall(Eid::Base, 1, [0; 6])
    }

    pub fn get_impl_version() -> Result {
        ecall(Eid::Base, 2, [0; 6])
    }

    pub fn probe_extension(eid: Eid) -> Result {
        ecall(Eid::Base, 3, [eid as u64, 0, 0, 0, 0, 0])
    }

    pub fn get_mvendorid() -> Result {
        ecall(Eid::Base, 4, [0; 6])
    }

    pub fn get_marchid() -> Result {
        ecall(Eid::Base, 5, [0; 6])
    }

    pub fn get_mimpid() -> Result {
        ecall(Eid::Base, 6, [0; 6])
    }
}

// ========================== TIME ==========================
pub mod time {
    use super::*;

    pub fn set_timer(stime_value: u64) -> Result {
        ecall(Eid::Time, 0, [stime_value, 0, 0, 0, 0, 0])
    }
}

// ========================== IPI ==========================
pub mod ipi {
    use super::*;

    pub fn send_ipi(hart_mask: u64, hart_mask_base: u64) -> Result {
        ecall(Eid::Ipi, 0, [hart_mask, hart_mask_base, 0, 0, 0, 0])
    }
}

// ========================== RFENCE ==========================
pub mod rfence {
    use super::*;

    pub fn remote_fence_i(mask: u64, base: u64) -> Result {
        ecall(Eid::Rfence, 0, [mask, base, 0, 0, 0, 0])
    }

    pub fn remote_sfence_vma(mask: u64, base: u64, start: u64, size: u64) -> Result {
        ecall(Eid::Rfence, 1, [mask, base, start, size, 0, 0])
    }

    pub fn remote_sfence_vma_asid(
        mask: u64,
        base: u64,
        start: u64,
        size: u64,
        asid: u64,
    ) -> Result {
        ecall(Eid::Rfence, 2, [mask, base, start, size, asid, 0])
    }
}

// ========================== HSM ==========================
pub mod hsm {
    use super::*;

    pub fn hart_start(hart_id: u64, start_addr: u64, opaque: u64) -> Result {
        ecall(Eid::Hsm, 0, [hart_id, start_addr, opaque, 0, 0, 0])
    }

    pub fn hart_stop() -> Result {
        ecall(Eid::Hsm, 1, [0; 6])
    }

    pub fn hart_get_status(hart_id: u64) -> Result {
        ecall(Eid::Hsm, 2, [hart_id, 0, 0, 0, 0, 0])
    }

    pub fn hart_suspend(suspend_type: u64, resume_addr: u64, opaque: u64) -> Result {
        ecall(Eid::Hsm, 3, [suspend_type, resume_addr, opaque, 0, 0, 0])
    }
}

// ========================== SRST ==========================
pub mod srst {
    use super::*;

    #[repr(u64)]
    pub enum ResetType {
        Shutdown = 0,
        ColdReboot = 1,
        WarmReboot = 2,
    }

    #[repr(u64)]
    pub enum ResetReason {
        None = 0,
        SysFail = 1,
    }

    pub fn system_reset(reset_type: ResetType, reset_reason: ResetReason) -> Result {
        ecall(
            Eid::Srst,
            0,
            [reset_type as u64, reset_reason as u64, 0, 0, 0, 0],
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
        legacy_ecall(Eid::LegacyConsolePutchar, [c as u64, 0, 0, 0]);
    }

    /// Read a single byte from the debug console.
    /// Returns `Some(byte)` on success, `None` if no byte is available
    /// (legacy ABI uses -1 to signal "no input").
    pub fn console_getchar() -> Option<u8> {
        let r = legacy_ecall(Eid::LegacyConsoleGetchar, [0; 4]);
        if r < 0 { None } else { Some(r as u8) }
    }

    pub fn shutdown() -> ! {
        legacy_ecall(Eid::LegacyShutdown, [0; 4]);
        // SBI never returns from shutdown; loop just to satisfy `!`.
        loop {
            unsafe { core::arch::asm!("wfi") }
        }
    }

    pub fn set_timer(stime_value: u64) {
        legacy_ecall(Eid::LegacySetTimer, [stime_value, 0, 0, 0]);
    }

    pub fn clear_ipi() {
        legacy_ecall(Eid::LegacyClearIpi, [0; 4]);
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
        ecall(Eid::Dbcn, 0, [len, base_addr_lo, base_addr_hi, 0, 0, 0])
    }

    /// Read up to `len` bytes into the buffer at physical address `base_addr`.
    pub fn console_read(len: u64, base_addr_lo: u64, base_addr_hi: u64) -> Result {
        ecall(Eid::Dbcn, 1, [len, base_addr_lo, base_addr_hi, 0, 0, 0])
    }

    /// Write a single byte. Convenient for putchar-style output.
    pub fn console_write_byte(b: u8) -> Result {
        ecall(Eid::Dbcn, 2, [b as u64, 0, 0, 0, 0, 0])
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
