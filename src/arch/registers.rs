//! RISC-V 寄存器抽象。
//!
//! - [`csr`]：CSR 读写（`sie` / `sepc` / `satp` / `stimecmp` 等）；
//! - [`gpr`]：通用寄存器读写（含 ABI 别名，如 `tp`）；
//! - [`values`]：寄存器位域值类型（`SatpValue` / `SStatusBits` 等）。
pub(crate) mod csr;
pub(crate) mod gpr;
pub(crate) mod values;
