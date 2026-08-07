#![allow(unused)]
pub trait CharDevice {
    type Error;

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error>;
}

pub trait BlockDevice {
    type Error;

    // offset 按字节算
    fn read_at(&mut self, buf: &mut [u8], offset: usize) -> Result<usize, Self::Error>;
    fn write_at(&mut self, buf: &[u8], offset: usize) -> Result<usize, Self::Error>;
}
