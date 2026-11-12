//! Vendor-agnostic hardware device abstraction and registry.
//!
//! To add a new audio interface (e.g. Universal Audio Apollo x8):
//! 1. Add `src/devices/<device>.rs` implementing [`HardwareDevice`].
//! 2. Register its `DESCRIPTOR` and constructor in [`default_registry`].

pub mod fcp;
pub mod iokit_usb;
pub mod scarlett_gen4;
pub mod usb_probe;

use crate::volume::VolumeState;
use std::fmt;

/// USB Vendor/Product identifier pair for hotplug and device discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UsbId {
    pub vendor_id: u16,
    pub product_id: u16,
    pub model_name: &'static str,
}

/// Static metadata describing a supported hardware device family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// Stable CLI/config identifier (e.g., `"scarlett-gen4"`).
    pub id: &'static str,
    /// Human-readable display name (e.g., `"Scarlett (Gen 4)"`).
    pub display_name: &'static str,
    /// Substrings matched against CoreAudio device names when checking active output.
    pub coreaudio_name_hints: &'static [&'static str],
    /// Known USB VID/PID pairs for IOKit hotplug matching and `--detect`.
    pub usb_ids: &'static [UsbId],
}

/// Errors returned by hardware transport operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceError {
    NotConnected,
    #[allow(dead_code)]
    TransportUnavailable(String),
    #[allow(dead_code)]
    ProtocolError(String),
    #[allow(dead_code)]
    Io(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "target hardware device is not connected"),
            Self::TransportUnavailable(msg) => write!(f, "transport unavailable: {msg}"),
            Self::ProtocolError(msg) => write!(f, "hardware protocol error: {msg}"),
            Self::Io(msg) => write!(f, "hardware I/O error: {msg}"),
        }
    }
}

impl std::error::Error for DeviceError {}

/// Hardware volume control interface implemented per device family.
pub trait HardwareDevice: Send + Sync {
    /// Returns the static descriptor for this device family.
    fn descriptor(&self) -> DeviceDescriptor;

    /// Connects or refreshes the underlying transport session.
    fn connect(&mut self) -> Result<(), DeviceError>;

    /// Releases the underlying transport session when inactive or on error.
    fn disconnect(&mut self) {}

    /// Reads the current hardware monitor/line output volume so the daemon
    /// can synchronize without causing a volume jump on startup or reconnect.
    fn read_volume(&mut self) -> Result<VolumeState, DeviceError>;

    /// Writes the target 64-step volume state to the hardware.
    fn write_volume(&mut self, state: VolumeState) -> Result<(), DeviceError>;

    /// Returns `true` if a CoreAudio default output device name belongs to this hardware.
    fn matches_coreaudio_name(&self, device_name: &str) -> bool {
        let lower = device_name.to_ascii_lowercase();
        self.descriptor()
            .coreaudio_name_hints
            .iter()
            .any(|hint| lower.contains(&hint.to_ascii_lowercase()))
    }
}

/// Factory entry stored in [`DeviceRegistry`].
pub struct RegisteredDevice {
    pub descriptor: DeviceDescriptor,
    pub create: fn() -> Box<dyn HardwareDevice>,
}

/// Vendor-agnostic registry of supported hardware device drivers.
#[derive(Default)]
pub struct DeviceRegistry {
    entries: Vec<RegisteredDevice>,
}

impl DeviceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a hardware device driver.
    pub fn register(
        &mut self,
        descriptor: DeviceDescriptor,
        create: fn() -> Box<dyn HardwareDevice>,
    ) {
        self.entries.push(RegisteredDevice { descriptor, create });
    }

    /// Lists all registered device descriptors.
    #[must_use]
    pub fn list(&self) -> Vec<DeviceDescriptor> {
        self.entries.iter().map(|e| e.descriptor).collect()
    }

    /// Looks up and instantiates a driver by its stable `id` (e.g., `"scarlett-gen4"`).
    #[must_use]
    pub fn create_by_id(&self, id: &str) -> Option<Box<dyn HardwareDevice>> {
        self.entries
            .iter()
            .find(|e| e.descriptor.id.eq_ignore_ascii_case(id))
            .map(|e| (e.create)())
    }

    /// Matches a connected USB VID/PID against registered devices.
    #[must_use]
    pub fn match_usb(&self, vendor_id: u16, product_id: u16) -> Option<(DeviceDescriptor, UsbId)> {
        for entry in &self.entries {
            if let Some(usb_id) = entry
                .descriptor
                .usb_ids
                .iter()
                .find(|u| u.vendor_id == vendor_id && u.product_id == product_id)
            {
                return Some((entry.descriptor, *usb_id));
            }
        }
        None
    }
}

/// Builds the default registry containing all built-in hardware drivers.
#[must_use]
pub fn default_registry() -> DeviceRegistry {
    let mut registry = DeviceRegistry::new();
    registry.register(scarlett_gen4::DESCRIPTOR, || {
        Box::new(scarlett_gen4::ScarlettGen4Device::new())
    });
    registry
}

/// Detects all connected USB devices matching any registered hardware driver.
#[must_use]
pub fn detect_connected_devices(registry: &DeviceRegistry) -> Vec<(DeviceDescriptor, UsbId)> {
    usb_probe::connected_usb_devices()
        .into_iter()
        .filter_map(|(vid, pid)| registry.match_usb(vid, pid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDevice;

    const MOCK_DESC: DeviceDescriptor = DeviceDescriptor {
        id: "mock-audio",
        display_name: "Mock Audio",
        coreaudio_name_hints: &["Mock", "Audio"],
        usb_ids: &[UsbId {
            vendor_id: 0x1234,
            product_id: 0x5678,
            model_name: "Mock Mk1",
        }],
    };

    impl HardwareDevice for MockDevice {
        fn descriptor(&self) -> DeviceDescriptor {
            MOCK_DESC
        }
        fn connect(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }
        fn read_volume(&mut self) -> Result<VolumeState, DeviceError> {
            Ok(VolumeState::from_step(32, false))
        }
        fn write_volume(&mut self, _state: VolumeState) -> Result<(), DeviceError> {
            Ok(())
        }
    }

    #[test]
    fn registry_matches_id_and_usb() {
        let mut reg = DeviceRegistry::new();
        reg.register(MOCK_DESC, || Box::new(MockDevice));

        assert_eq!(reg.list().len(), 1);
        assert!(reg.create_by_id("mock-audio").is_some());
        assert!(reg.create_by_id("unknown").is_none());

        let (desc, usb) = reg.match_usb(0x1234, 0x5678).expect("USB match expected");
        assert_eq!(desc.display_name, "Mock Audio");
        assert_eq!(usb.model_name, "Mock Mk1");
    }

    #[test]
    fn default_registry_contains_scarlett_gen4() {
        let reg = default_registry();
        let list = reg.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "scarlett-gen4");
        assert_eq!(list[0].display_name, "Scarlett (Gen 4)");
    }
}
