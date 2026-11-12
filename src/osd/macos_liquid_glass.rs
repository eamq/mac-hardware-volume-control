//! macOS 26/27 (Tahoe+) Liquid Glass capsule HUD backend.
//!
//! Dispatches native volume/mute HUD updates through `OSD.framework` ->
//! `com.apple.OSDUIHelper` using the full 64-step discrete grid so the capsule
//! bar aligns 1:1 with the hardware LED ring.

use super::osd_framework::{show_chiclet_hud, OSD_GRAPHIC_MUTE, OSD_GRAPHIC_VOLUME};
use super::{MacOsVersion, OsdBackend, OsdError};
use crate::volume::{VolumeState, MAX_STEP};

const MIN_SUPPORTED_VERSION: MacOsVersion = MacOsVersion::new(26, 0, 0);
const DEFAULT_FADE_MSEC: u32 = 1500;

#[derive(Debug, Default, Clone, Copy)]
pub struct LiquidGlassOsd;

impl LiquidGlassOsd {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Computes `(graphic_type, filled_chiclets, total_chiclets)` for the 64-step grid.
    #[must_use]
    pub const fn hud_params(state: VolumeState) -> (u64, u32, u32) {
        if state.is_muted() {
            (OSD_GRAPHIC_MUTE, 0, MAX_STEP as u32)
        } else {
            (
                OSD_GRAPHIC_VOLUME,
                state.effective_step() as u32,
                MAX_STEP as u32,
            )
        }
    }
}

impl OsdBackend for LiquidGlassOsd {
    fn name(&self) -> &'static str {
        "macos-liquid-glass"
    }

    fn supports_version(&self, version: MacOsVersion) -> bool {
        version >= MIN_SUPPORTED_VERSION
    }

    fn show_volume(&self, state: VolumeState) -> Result<(), OsdError> {
        let (graphic, filled, total) = Self::hud_params(state);
        show_chiclet_hud(graphic, filled, total, DEFAULT_FADE_MSEC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_macos_26_and_newer() {
        let osd = LiquidGlassOsd::new();
        assert!(!osd.supports_version(MacOsVersion::new(15, 5, 0)));
        assert!(osd.supports_version(MacOsVersion::new(26, 0, 0)));
        assert!(osd.supports_version(MacOsVersion::new(27, 0, 1)));
    }

    #[test]
    fn maps_volume_and_mute_to_64_step_hud_params() {
        assert_eq!(
            LiquidGlassOsd::hud_params(VolumeState::from_step(32, false)),
            (OSD_GRAPHIC_VOLUME, 32, 64)
        );
        assert_eq!(
            LiquidGlassOsd::hud_params(VolumeState::from_step(32, true)),
            (OSD_GRAPHIC_MUTE, 0, 64)
        );
    }
}
