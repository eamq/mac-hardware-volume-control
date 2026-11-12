//! Focusrite Scarlett (4th Gen) hardware volume driver.
//!
//! Maps the 64-step 1:1 linear grid (`0..=64`) to the Scarlett Gen 4 monitor
//! output level and mute state, reading the current hardware level on connect
//! to prevent sudden volume jumps.

use super::{DeviceDescriptor, DeviceError, HardwareDevice, UsbId};
use crate::volume::{VolumeState, MAX_STEP};

/// Focusrite Audio Engineering Ltd. USB Vendor ID.
pub const FOCUSRITE_VENDOR_ID: u16 = 0x1235;

/// Known Focusrite Scarlett 4th Gen USB Product IDs.
pub const SCARLETT_GEN4_USB_IDS: &[UsbId] = &[
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x8218,
        model_name: "Scarlett Solo 4th Gen",
    },
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x8219,
        model_name: "Scarlett 2i2 4th Gen",
    },
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x821a,
        model_name: "Scarlett 4i4 4th Gen",
    },
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x821b,
        model_name: "Scarlett 16i16 4th Gen",
    },
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x821c,
        model_name: "Scarlett 18i16 4th Gen",
    },
    UsbId {
        vendor_id: FOCUSRITE_VENDOR_ID,
        product_id: 0x821d,
        model_name: "Scarlett 18i20 4th Gen",
    },
];

pub const DESCRIPTOR: DeviceDescriptor = DeviceDescriptor {
    id: "scarlett-gen4",
    display_name: "Scarlett (Gen 4)",
    coreaudio_name_hints: &["Scarlett", "Focusrite"],
    usb_ids: SCARLETT_GEN4_USB_IDS,
};

/// Transport abstraction for communicating with Scarlett Gen 4 hardware.
pub trait ScarlettTransport: Send + Sync {
    fn open(&mut self) -> Result<(), DeviceError>;
    fn close(&mut self) {}
    fn get_monitor_level(&mut self) -> Result<(u16, bool), DeviceError>;
    fn set_monitor_level(&mut self, raw_level: u16, muted: bool) -> Result<(), DeviceError>;
}

/// Default transport targeting Focusrite Scarlett Gen 4 via FCP over IOKit.
#[derive(Default)]
pub struct MacOsScarlettTransport {
    session: Option<super::fcp::FcpSession>,
}

impl ScarlettTransport for MacOsScarlettTransport {
    fn open(&mut self) -> Result<(), DeviceError> {
        let session = super::fcp::FcpSession::open(FOCUSRITE_VENDOR_ID)?;
        self.session = Some(session);
        Ok(())
    }

    fn close(&mut self) {
        self.session = None;
    }

    fn get_monitor_level(&mut self) -> Result<(u16, bool), DeviceError> {
        let session = self.session.as_mut().ok_or(DeviceError::NotConnected)?;
        let (db, muted) = session.read_monitor_state()?;
        let ratio = db_to_linear_ratio(db);
        let raw = (ratio * f32::from(u16::MAX)).round() as u16;
        Ok((raw, muted))
    }

    fn set_monitor_level(&mut self, raw_level: u16, muted: bool) -> Result<(), DeviceError> {
        let session = self.session.as_mut().ok_or(DeviceError::NotConnected)?;
        let ratio = f32::from(raw_level) / f32::from(u16::MAX);
        let db = linear_ratio_to_db(ratio);
        session.write_monitor_state(db, muted)
    }
}

/// Perceptual taper exponent matching the Scarlett Gen 4 LED ring (`50%` ≈ `-28 dB`).
const TAPER_EXPONENT: f32 = 2.2;

fn linear_ratio_to_db(ratio: f32) -> i32 {
    let r = ratio.clamp(0.0, 1.0);
    let db = -127.0 * (1.0 - r).powf(TAPER_EXPONENT);
    (db.round() as i32).clamp(-127, 0)
}

fn db_to_linear_ratio(db: i32) -> f32 {
    let clamped = db.clamp(-127, 0) as f32;
    let norm = (-clamped / 127.0).clamp(0.0, 1.0);
    (1.0 - norm.powf(1.0 / TAPER_EXPONENT)).clamp(0.0, 1.0)
}

/// Driver for Focusrite Scarlett (Gen 4) interfaces.
pub struct ScarlettGen4Device {
    transport: Box<dyn ScarlettTransport>,
}

impl ScarlettGen4Device {
    #[must_use]
    pub fn new() -> Self {
        Self {
            transport: Box::new(MacOsScarlettTransport::default()),
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_transport(transport: Box<dyn ScarlettTransport>) -> Self {
        Self { transport }
    }

    /// Converts a 64-step linear volume state to a 16-bit normalized raw value (`0..=0xFFFF`).
    /// Preserves the underlying volume level when muted so hardware `muteSwitch` handles muting.
    #[must_use]
    pub fn state_to_raw(state: VolumeState) -> u16 {
        if state.step() == 0 {
            return 0;
        }
        let ratio = f32::from(state.step()) / f32::from(MAX_STEP);
        (ratio * f32::from(u16::MAX)).round() as u16
    }

    /// Converts a 16-bit normalized raw value (`0..=0xFFFF`) back to the nearest 64-step state.
    #[must_use]
    pub fn raw_to_state(raw: u16, muted: bool) -> VolumeState {
        let ratio = f32::from(raw) / f32::from(u16::MAX);
        VolumeState::from_linear(ratio, muted)
    }
}

impl Default for ScarlettGen4Device {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareDevice for ScarlettGen4Device {
    fn descriptor(&self) -> DeviceDescriptor {
        DESCRIPTOR
    }

    fn connect(&mut self) -> Result<(), DeviceError> {
        self.transport.open()
    }

    fn disconnect(&mut self) {
        self.transport.close();
    }

    fn read_volume(&mut self) -> Result<VolumeState, DeviceError> {
        let (raw, muted) = self.transport.get_monitor_level()?;
        Ok(Self::raw_to_state(raw, muted))
    }

    fn write_volume(&mut self, state: VolumeState) -> Result<(), DeviceError> {
        let raw = Self::state_to_raw(state);
        self.transport.set_monitor_level(raw, state.is_muted())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MockTransport {
        raw: Arc<Mutex<u16>>,
        muted: Arc<Mutex<bool>>,
        open_called: Arc<Mutex<bool>>,
    }

    impl ScarlettTransport for MockTransport {
        fn open(&mut self) -> Result<(), DeviceError> {
            *self.open_called.lock().unwrap() = true;
            Ok(())
        }

        fn get_monitor_level(&mut self) -> Result<(u16, bool), DeviceError> {
            Ok((*self.raw.lock().unwrap(), *self.muted.lock().unwrap()))
        }

        fn set_monitor_level(&mut self, raw_level: u16, muted: bool) -> Result<(), DeviceError> {
            *self.raw.lock().unwrap() = raw_level;
            *self.muted.lock().unwrap() = muted;
            Ok(())
        }
    }

    #[test]
    fn round_trips_64_step_grid_linearly() {
        for step in 0..=MAX_STEP {
            let state = VolumeState::from_step(step, false);
            let raw = ScarlettGen4Device::state_to_raw(state);
            let decoded = ScarlettGen4Device::raw_to_state(raw, state.is_muted());
            assert_eq!(decoded.effective_step(), state.effective_step());
        }
    }

    #[test]
    fn reads_hardware_volume_on_connect_without_jump() {
        let transport = MockTransport {
            raw: Arc::new(Mutex::new(32768)), // ~50%
            muted: Arc::new(Mutex::new(false)),
            open_called: Arc::new(Mutex::new(false)),
        };
        let mut dev = ScarlettGen4Device::with_transport(Box::new(transport.clone()));
        dev.connect().unwrap();
        assert!(*transport.open_called.lock().unwrap());

        let state = dev.read_volume().unwrap();
        assert_eq!(state.step(), 32);
        assert_eq!(state.as_percent(), 50.0);

        dev.write_volume(VolumeState::from_step(48, false)).unwrap();
        let updated = dev.read_volume().unwrap();
        assert_eq!(updated.step(), 48);
        assert_eq!(updated.as_percent(), 75.0);
    }

    #[test]
    fn state_to_raw_preserves_level_when_muted() {
        let muted = VolumeState::from_step(32, true);
        let unmuted = VolumeState::from_step(32, false);
        assert_eq!(
            ScarlettGen4Device::state_to_raw(muted),
            ScarlettGen4Device::state_to_raw(unmuted)
        );
        assert!(ScarlettGen4Device::state_to_raw(muted) > 0);
    }

    #[test]
    fn perceptual_taper_maps_50_percent_and_bounds() {
        assert_eq!(linear_ratio_to_db(0.0), -127);
        assert_eq!(linear_ratio_to_db(1.0), 0);
        assert_eq!(linear_ratio_to_db(0.5), -28);

        assert!((db_to_linear_ratio(-28) - 0.5).abs() < 0.02);
        assert_eq!(db_to_linear_ratio(0), 1.0);
        assert_eq!(db_to_linear_ratio(-127), 0.0);
    }
}
