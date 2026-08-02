//! 内嵌用户程序 ELF。
//!
//! 编译期通过 `include_bytes!` 把 `athera-userland` 构建产物内嵌进内核，
//! 由 [`crate::proc::exec`] 加载执行。
#[repr(align(8))]
pub struct Elf(
    pub [u8; include_bytes!("../../target/riscv64gc-unknown-none-elf/release/add").len()],
);

pub static ELF: Elf = Elf(*include_bytes!(
    "../../target/riscv64gc-unknown-none-elf/release/add"
));
