#![allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Fdt,
    OutOfMemory,
    NotElf,
    Not64Bit,
    NotLsb,
    NotRiscv,
    MemoryProbeFailed,
    VirtioHandshakeFailed,
    VirtioNotSupported,
    VirtioFeaturesNotOk,
    NoUart,
    NoTidAvailable,
    AddressSpaceNotFound,
    PageTableMissing,
    NullPointer,
}

pub type Result<T> = core::result::Result<T, Error>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let message = match self {
            Error::Fdt => "invalid device tree",
            Error::OutOfMemory => "out of memory",
            Error::NotElf => "not an ELF file",
            Error::Not64Bit => "not a 64-bit ELF",
            Error::NotLsb => "ELF is not little-endian",
            Error::NotRiscv => "not a RISC-V ELF",
            Error::MemoryProbeFailed => "failed to probe system memory",
            Error::VirtioHandshakeFailed => "virtio handshake failed",
            Error::VirtioNotSupported => "virtio device not supported",
            Error::VirtioFeaturesNotOk => "virtio features negotiation failed",
            Error::NoUart => "no UART device found",
            Error::NoTidAvailable => "no thread id available",
            Error::AddressSpaceNotFound => "user address space not found",
            Error::PageTableMissing => "page table not found",
            Error::NullPointer => "null pointer",
        };
        f.write_str(message)
    }
}

impl core::error::Error for Error {}
