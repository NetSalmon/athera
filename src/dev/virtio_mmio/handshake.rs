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

#[must_use]
pub struct HandshakeBegin<'a> {
    cfg: &'a mut VirtqCfg,
}

#[must_use]
pub struct FeaturesNegotiated<'a> {
    cfg: &'a mut VirtqCfg,
}

#[must_use]
pub struct QueuesReady<'a> {
    cfg: &'a mut VirtqCfg,
    queues: Vec<Virtq>,
}

impl VirtqCfg {
    pub fn handshake(&mut self) -> HandshakeBegin<'_> {
        let mut status: DeviceStatus = 0.into();
        self.write_status(status.into());

        status.set_acknowledge(true);
        self.write_status(status.into());

        status.set_driver(true);
        self.write_status(status.into());

        HandshakeBegin { cfg: self }
    }
}

impl<'a> HandshakeBegin<'a> {
    pub fn legacy(
        self,
        negotiate: impl FnOnce(u32) -> u32,
    ) -> Result<FeaturesNegotiated<'a>, Error> {
        self.cfg.write_device_features_sel(0);
        let features: u32 = self.cfg.device_features();

        let negotiated = negotiate(features);

        self.cfg.write_driver_features_sel(0);
        self.cfg.write_driver_features(negotiated);

        Self::commit_features(self.cfg)
    }

    pub fn modern(
        self,
        negotiate: impl FnOnce(u32, u32) -> (u32, u32),
    ) -> Result<FeaturesNegotiated<'a>, Error> {
        self.cfg.write_device_features_sel(0);
        let features_low: u32 = self.cfg.device_features();

        self.cfg.write_device_features_sel(1);
        let features_high: u32 = self.cfg.device_features();

        let (negotiated_low, negotiated_high) = negotiate(features_low, features_high);

        self.cfg.write_driver_features_sel(0);
        self.cfg.write_driver_features(negotiated_low);

        self.cfg.write_driver_features_sel(1);
        self.cfg.write_driver_features(negotiated_high);

        Self::commit_features(self.cfg)
    }

    fn commit_features(cfg: &mut VirtqCfg) -> Result<FeaturesNegotiated<'_>, Error> {
        let mut status: DeviceStatus = cfg.status().into();
        status.set_features_ok(true);
        cfg.write_status(status.into());

        let mut got_status: DeviceStatus = cfg.status().into();
        if !got_status.features_ok() {
            got_status.set_failed(true);
            cfg.write_status(got_status.into());
            return Err(Error::VirtioFeaturesNotOk);
        }

        Ok(FeaturesNegotiated { cfg })
    }
}

impl<'a> FeaturesNegotiated<'a> {
    pub fn setup_queue(mut self, config: QueueConfig) -> Result<QueuesReady<'a>, Error> {
        self.setup_queues(&[config])
    }

    pub fn setup_queues(mut self, configs: &[QueueConfig]) -> Result<QueuesReady<'a>, Error> {
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

        Ok(QueuesReady {
            cfg: self.cfg,
            queues,
        })
    }
}

impl<'a> QueuesReady<'a> {
    pub fn setup_queue(mut self, config: QueueConfig) -> Result<Self, Error> {
        let configs = [config];
        self.add_queues(&configs)?;
        Ok(self)
    }

    pub fn setup_queues(mut self, configs: &[QueueConfig]) -> Result<Self, Error> {
        self.add_queues(configs)?;
        Ok(self)
    }

    fn add_queues(&mut self, configs: &[QueueConfig]) -> Result<(), Error> {
        let version = self.cfg.version();

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

            self.queues.push(virtq);
        }

        Ok(())
    }

    pub fn finish(self) -> Vec<Virtq> {
        let mut status: DeviceStatus = self.cfg.status().into();
        status.set_driver_ok(true);
        self.cfg.write_status(status.into());
        self.queues
    }
}
