//! RISC-V 寄存器抽象。
//!
//! - [`csr`]：CSR 读写（`sie` / `sepc` / `satp` / `stimecmp` 等）；
//! - [`gpr`]：通用寄存器读写（含 ABI 别名，如 `tp`）；
//! - [`values`]：寄存器位域值类型（`SatpValue` / `SstatusBits` 等）。
//!
//! 寄存器类型使用零大小标记结构体表示，例如 [`csr::Sie`]。这避免了把
//! CSR 编号或 GPR 名称作为运行时数据传递，同时让读写权限在 API 层可见。
pub(crate) mod csr;
pub(crate) mod gpr;
pub(crate) mod values;
