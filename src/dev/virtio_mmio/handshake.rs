use alloc::vec::Vec;

use crate::{
    dev::virtio_mmio::{DeviceStatus, VirtqCfg, queue::Virtq},
    error::Error,
    mem::addr::PhysicalAddr,
};

const VIRTIO_VERSION_LEGACY: u32 = 1;

pub struct QueueConfig {
    pub index: u32,
    pub size: u32,
}

pub struct Handshaking<'a> {
    cfg: &'a mut VirtqCfg,
}

pub struct Ready<'a> {
    cfg: &'a mut VirtqCfg,
    pub queues: Vec<Virtq>,
}

impl VirtqCfg {
    pub fn handshake<T, F>(
        &mut self,
        legacy_negotiate: T,
        modern_negotiate: F,
    ) -> Result<Handshaking<'_>, Error>
    where
        T: FnOnce(u32) -> u32,
        F: FnOnce(u32, u32) -> (u32, u32),
    {
        let mut status: DeviceStatus = 0.into();
        self.write_status(status.into());

        status.set_acknowledge(true);
        self.write_status(status.into());

        status.set_driver(true);
        self.write_status(status.into());

        match self.version() {
            VIRTIO_VERSION_LEGACY => {
                self.write_device_features_sel(0);
                let features: u32 = self.device_features();

                let negotiated = legacy_negotiate(features);

                self.write_driver_features_sel(0);
                self.write_driver_features(negotiated);
            }
            _ => {
                self.write_device_features_sel(0);
                let features_low: u32 = self.device_features();

                self.write_device_features_sel(1);
                let features_high: u32 = self.device_features();

                let (negotiated_low, negotiated_high) =
                    modern_negotiate(features_low, features_high);

                self.write_driver_features_sel(0);
                self.write_driver_features(negotiated_low);

                self.write_driver_features_sel(1);
                self.write_driver_features(negotiated_high);
            }
        };

        status.set_features_ok(true);
        self.write_status(status.into());

        let mut got_status: DeviceStatus = self.status().into();
        if !got_status.features_ok() {
            got_status.set_failed(true);
            self.write_status(got_status.into());
            return Err(Error::VirtioFeaturesNotOk);
        }

        Ok(Handshaking { cfg: self })
    }
}

impl<'a> Handshaking<'a> {
    pub fn setup_queues(self, configs: &[QueueConfig]) -> Result<Ready<'a>, Error> {
        let version = self.cfg.version();
        let mut queues = Vec::new();

        for qcfg in configs {
            self.cfg.write_queue_sel(qcfg.index);

            let num_max = self.cfg.queue_num_max();
            if qcfg.size > num_max {
                return Err(Error::VirtioHandshakeFailed);
            }
            self.cfg.write_queue_num(qcfg.size);

            let virtq = Virtq::new()?;

            if version == VIRTIO_VERSION_LEGACY {
                self.cfg.write_guest_page_size(4096);

                let pa: PhysicalAddr = (virtq.queue_ptr() as usize).into();
                self.cfg.write_queue_pfn(pa.ppn() as u32);
            } else {
                self.cfg.write_queue_desc_low(virtq.desc_addr() as u32);
                self.cfg
                    .write_queue_desc_high((virtq.desc_addr() >> 32) as u32);
                self.cfg.write_queue_driver_low(virtq.avail_addr() as u32);
                self.cfg
                    .write_queue_driver_high((virtq.avail_addr() >> 32) as u32);
                self.cfg.write_queue_device_low(virtq.used_addr() as u32);
                self.cfg
                    .write_queue_device_high((virtq.used_addr() >> 32) as u32);
                self.cfg.write_queue_ready(1);
            }

            queues.push(virtq);
        }

        Ok(Ready {
            cfg: self.cfg,
            queues,
        })
    }
}

impl<'a> Ready<'a> {
    pub fn cfg(&self) -> &VirtqCfg {
        self.cfg
    }

    pub fn finish(self) -> Vec<Virtq> {
        let mut status: DeviceStatus = self.cfg.status().into();
        status.set_driver_ok(true);
        self.cfg.write_status(status.into());
        self.queues
    }
}
