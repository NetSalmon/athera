use crate::{
    dev::{
        VIRTIO_BLK,
        traits::{ReadAt, WriteAt},
    },
    fs::{FsError, FsResult, vfs::file_ops::FileOps},
};

pub struct VirtioBlk;

impl FileOps for VirtioBlk {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let blk = VIRTIO_BLK
            .force()
            .lock()
            .as_ref()
            .cloned()
            .ok_or(FsError::NotFound)?;
        let dev = blk.read();
        dev.read_at(buf, offset as usize).map_err(FsError::from)
    }

    fn write_at(&self, buf: &[u8], offset: u64) -> FsResult<usize> {
        let blk = VIRTIO_BLK
            .force()
            .lock()
            .as_ref()
            .cloned()
            .ok_or(FsError::NotFound)?;
        let dev = blk.read();
        dev.write_at(buf, offset as usize).map_err(FsError::from)
    }
}
