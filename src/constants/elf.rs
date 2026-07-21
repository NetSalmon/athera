#[repr(align(8))]
pub struct Elf(
    pub [u8; include_bytes!("../../target/riscv64gc-unknown-none-elf/release/hello_world").len()],
);

pub static ELF: Elf = Elf(*include_bytes!(
    "../../target/riscv64gc-unknown-none-elf/release/hello_world"
));
