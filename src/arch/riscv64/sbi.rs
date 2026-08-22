#![allow(dead_code)]
//! SBI 调用封装。
//!
//! 本模块统一处理 SBI v0.2+ 的 `a0`/`a1` 返回约定，并保留 SBI v0.1
//! legacy ABI 的独立入口。各扩展模块只负责组织参数，不重复编写 `ecall`
//! 汇编。
//!
//! 定义错误码 [`SbiError`]、调用结果与各扩展的调用入口（legacy、
//! srst 复位/关机、hsm 停止等）。
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

/// SBI 调用的原始返回值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbiResult {
    /// SBI error code returned in `a0`.
    pub error: i64,
    /// SBI value returned in `a1`.
    pub value: u64,
}

/// SBI 调用结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Result {
    /// The call failed and carries the SBI error code.
    Err(SbiError),
    /// The call succeeded and carries the SBI `a1` value.
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
                // Preserve unknown SBI error codes instead of panicking.
                _ => SbiError::from(result.error),
            };
            Result::Err(err)
        } else {
            Result::Ok(result.value)
        }
    }
}

impl Result {
    /// 返回调用是否成功。
    #[inline]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// 返回调用是否失败。
    #[inline]
    pub const fn is_err(&self) -> bool {
        !self.is_ok()
    }

    /// 获取成功返回值；失败时返回 `None`。
    #[inline]
    pub const fn value(&self) -> Option<u64> {
        match self {
            Self::Ok(value) => Some(*value),
            Self::Err(_) => None,
        }
    }

    /// 获取 SBI 错误码；成功时返回 `None`。
    #[inline]
    pub const fn error(&self) -> Option<SbiError> {
        match self {
            Self::Ok(_) => None,
            Self::Err(error) => Some(*error),
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

/// 按 SBI v0.2+ 标准 ABI 执行一次扩展调用。
///
/// `a6` 保存 function ID，`a7` 保存 extension ID；返回的 `a0` 被解释为
/// 有符号错误码，`a1` 作为成功时的返回值。
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
/// 执行一次 SBI v0.1 legacy ABI 调用。
///
/// legacy ABI 不携带 function ID，extension ID 位于 `a7`，返回值位于
/// `a0`。调用者负责解释不同 legacy 调用的返回约定。
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
/// SBI Base 扩展。
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
/// SBI Timer 扩展。
pub mod time {
    use super::*;

    pub fn set_timer(stime_value: u64) -> Result {
        ecall(Eid::TIME, 0, [stime_value, 0, 0, 0, 0, 0])
    }
}

// ========================== IPI ==========================
/// SBI IPI 扩展。
pub mod ipi {
    use super::*;

    pub fn send_ipi(hart_mask: u64, hart_mask_base: u64) -> Result {
        ecall(Eid::IPI, 0, [hart_mask, hart_mask_base, 0, 0, 0, 0])
    }
}

// ========================== RFENCE ==========================
/// SBI RFENCE 扩展。
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
/// SBI Hart State Management 扩展。
///
/// HSM 用 hart 编号管理其他硬件线程的生命周期。除 `hart_stop()` 外，
/// 这些调用通常由启动 hart 发起；目标 hart 的启动入口必须符合 SBI
/// 约定，并从 `a0`/`tp` 等启动状态中获取自身上下文。
pub mod hsm {
    use super::*;

    /// 启动指定 hart。
    ///
    /// `hart_id` 是目标 hart 编号，`start_addr` 是目标 hart 进入 S 模式
    /// 后跳转的物理地址，`opaque` 会原样传递给启动入口的 `a1`。
    /// 成功时返回 SBI 规定的零值。
    pub fn hart_start(hart_id: u64, start_addr: u64, opaque: u64) -> Result {
        ecall(Eid::HSM, 0, [hart_id, start_addr, opaque, 0, 0, 0])
    }

    /// 停止当前 hart。
    ///
    /// SBI 成功处理后通常不会返回；保留返回值是为了处理固件不支持或
    /// 拒绝该操作的情况。
    pub fn hart_stop() -> Result {
        ecall(Eid::HSM, 1, [0; 6])
    }

    /// 查询指定 hart 的当前状态。
    ///
    /// 成功时 `Result::Ok` 中的值是 SBI HSM 状态编码，失败时返回固件错误。
    pub fn hart_get_status(hart_id: u64) -> Result {
        ecall(Eid::HSM, 2, [hart_id, 0, 0, 0, 0, 0])
    }

    /// 挂起当前 hart，并指定恢复入口。
    ///
    /// `suspend_type`、`resume_addr` 和 `opaque` 的具体取值由 SBI HSM
    /// 挂起类型定义；当前内核通常不需要直接调用此接口。
    pub fn hart_suspend(suspend_type: u64, resume_addr: u64, opaque: u64) -> Result {
        ecall(Eid::HSM, 3, [suspend_type, resume_addr, opaque, 0, 0, 0])
    }
}

// ========================== SRST ==========================
/// SBI System Reset 扩展。
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
        ecall(Eid::SRST, 0, [reset_type.0, reset_reason.0, 0, 0, 0, 0])
    }
}

// ========================== Legacy console (v0.1) ==========================
//
// These calls predate the BASE/probe model and are always either implemented
// or stubbed out by the SBI firmware (OpenSBI keeps them as a thin wrapper
// over DBCN when DBCN is present). They are useful as an early-boot console
// before the device tree has been parsed and a real UART driver brought up.
/// SBI v0.1 legacy 扩展。
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
            crate::arch::riscv64::wait_for_interrupt();
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
/// SBI Debug Console extension.
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
