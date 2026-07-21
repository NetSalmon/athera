use crate::{
    dev::virtio_mmio::{DeviceStatus, VirtqCfg},
    error::Error,
};

const VIRTIO_VERSION_LEGACY: u32 = 1;

impl VirtqCfg {
    pub fn handshake<T, F>(
        &mut self,
        legacy_negotiate: T,
        modern_negotiate: F,
    ) -> Result<(u32, Option<u32>), Error>
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

        let features = match self.version() {
            VIRTIO_VERSION_LEGACY => {
                self.write_device_features_sel(0);
                let features: u32 = self.device_features();

                let negotiated = legacy_negotiate(features);

                self.write_driver_features_sel(0);
                self.write_driver_features(negotiated);

                (negotiated, None)
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

                (negotiated_low, Some(features_high))
            }
        };

        status.set_features_ok(true);
        self.write_status(status.into());

        let got_status: DeviceStatus = self.status().into();
        if !got_status.features_ok() {
            return Err(Error::VirtioFeaturesNotOk);
        }

        Ok(features)
    }

    pub fn finish_handshake(&mut self) {
        let mut status: DeviceStatus = self.status().into();
        status.set_driver_ok(true);
        self.write_status(status.into());
    }
}
