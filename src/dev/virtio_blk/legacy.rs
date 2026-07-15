use crate::debug;
use crate::dev::virtio_blk::queue::get_queue_ptr;
use crate::dev::virtio_blk::{RING_MAX_SIZE, Status, VirtioBlk, VirtioBlkFeaturesLow};
use crate::error::Error;
use crate::mem::addr::PhysicalAddr;

pub fn handshake_legacy(blk: &VirtioBlk) -> Result<(), Error> {
    let mut status: Status = Status::from(0);
    blk.write_status(status.into());

    status.set_acknowledge(true);
    blk.write_status(status.into());

    status.set_driver(true);
    blk.write_status(status.into());

    blk.write_device_features_sel(0);
    let features_low: VirtioBlkFeaturesLow = blk.device_features().into();
    debug!("features_low : {:?}", features_low);

    blk.write_device_features_sel(1);
    let features_high: u32 = blk.device_features();
    debug!("features_high : {:?}", features_high);

    let new_feat = VirtioBlkFeaturesLow::from(0);
    blk.write_driver_features_sel(0);
    blk.write_driver_features(new_feat.into());

    status.set_features_ok(true);
    blk.write_status(status.into());

    let got_status: Status = blk.status().into();
    if !got_status.features_ok() {
        return Err(Error::VirtioFeaturesNotOk);
    }

    let queue_addr = get_queue_ptr() as usize;
    blk.write_queue_sel(0);
    blk.write_queue_num(RING_MAX_SIZE as u32);
    blk.write_guest_page_size(4096);

    let pa: PhysicalAddr = queue_addr.into();
    let ppn = pa.ppn();
    debug!("queue_addr: {:#x}, ppn: {:#x}", queue_addr, ppn);
    blk.write_queue_pfn(ppn as u32);

    status.set_driver_ok(true);
    blk.write_status(status.into());

    debug!("legacy handshake ok");
    Ok(())
}
