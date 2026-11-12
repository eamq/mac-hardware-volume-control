//! No-op OSD fallback for unsupported macOS versions or headless tests.

use super::{MacOsVersion, OsdBackend, OsdError};
use crate::volume::VolumeState;

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopOsd;

impl OsdBackend for NoopOsd {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn supports_version(&self, _version: MacOsVersion) -> bool {
        true
    }

    fn show_volume(&self, _state: VolumeState) -> Result<(), OsdError> {
        Ok(())
    }
}
