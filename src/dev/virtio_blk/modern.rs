use crate::{
    debug,
    dev::virtio_blk::{
        Status, VirtioBlk, VirtioBlkFeaturesHigh, VirtioBlkFeaturesLow,
        queue::{Queue, get_queue_ptr},
    },
    error::Error,
};

pub fn handshake_modern(blk: &VirtioBlk) -> Result<(), Error> {
    let mut status: Status = 0.into();
    blk.write_status(status.into());

    status.set_acknowledge(true);
    blk.write_status(status.into());

    status.set_driver(true);
    blk.write_status(status.into());

    blk.write_device_features_sel(0);
    let mut features_low: VirtioBlkFeaturesLow = blk.device_features().into();
    debug!("features_low : {:?}", features_low);
    blk.write_driver_features_sel(0);
    features_low.set_readonly(false);
    blk.write_driver_features(features_low.into());

    blk.write_device_features_sel(1);
    let mut features_high: VirtioBlkFeaturesHigh = blk.device_features().into();
    debug!("features_high: {:?}", features_high);

    if !features_high.version_1() {
        return Err(Error::VirtioNotSupported);
    }

    blk.write_driver_features_sel(1);
    features_high = VirtioBlkFeaturesHigh(0);
    features_high.set_version_1(true);
    blk.write_driver_features(features_high.into());

    status.set_features_ok(true);
    blk.write_status(status.into());

    let got_status: Status = blk.status().into();
    if !got_status.features_ok() {
        return Err(Error::VirtioFeaturesNotOk);
    }

    let queue_ptr = get_queue_ptr();
    let desc_addr = queue_ptr as u64;
    let avail_addr = queue_ptr as u64 + core::mem::offset_of!(Queue, avail) as u64;
    let used_addr = queue_ptr as u64 + core::mem::offset_of!(Queue, used) as u64;

    blk.write_queue_desc_low(desc_addr as u32);
    blk.write_queue_desc_high((desc_addr >> 32) as u32);

    blk.write_queue_driver_low(avail_addr as u32);
    blk.write_queue_driver_high((avail_addr >> 32) as u32);

    blk.write_queue_device_low(used_addr as u32);
    blk.write_queue_device_high((used_addr >> 32) as u32);

    blk.write_queue_ready(1);

    status.set_driver_ok(true);
    blk.write_status(status.into());

    debug!("modern handshake ok");
    Ok(())
}
