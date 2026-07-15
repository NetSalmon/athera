#[derive(Debug)]
pub enum Error {
    Fdt,
    OutOfMemory,
    NotElf,
    Not64Bit,
    NotLsb,
    MemoryProbeFailed,
    VirtioHandshakeFailed,
    VirtioNotSupported,
    VirtioFeaturesNotOk,
    NoUart,
}