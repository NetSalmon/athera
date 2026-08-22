use crate::{
    driver::tree::DEVICE_MANAGER,
    fs::{FsError, FsResult, vfs::file_ops::FileOps},
};

pub struct VirtioBlk;

impl FileOps for VirtioBlk {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_block().ok_or(FsError::NotFound)?;
        Ok(manager.read_at(did, buf, offset as usize)?)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        let manager = DEVICE_MANAGER.force().read();
        let did = manager.first_block().ok_or(FsError::NotFound)?;
        Ok(manager.write_at(did, buf, offset as usize)?)
    }
}
