//! Extensible macOS On-Screen Display (OSD) abstraction and version dispatch.
//!
//! Only the macOS 26/27 (Tahoe+) Liquid Glass backend is implemented up front.
//! Older or future macOS HUD APIs can be added as standalone modules in
//! `src/osd/` implementing [`OsdBackend`] and registered in [`select_backend`].

pub mod macos_liquid_glass;
pub mod noop;
mod osd_framework;

use crate::volume::VolumeState;
use std::fmt;
use std::process::Command;

/// Parsed macOS product version (`major.minor.patch`) for runtime OSD backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacOsVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl MacOsVersion {
    #[must_use]
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses a version string such as `"27.0.1"`, `"26.1"`, or `"15.4.0"`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut parts = trimmed.split('.');
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next().map_or(Some(0), |s| s.parse::<u32>().ok())?;
        let patch = parts.next().map_or(Some(0), |s| s.parse::<u32>().ok())?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for MacOsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Errors returned by an OSD backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsdError {
    Unavailable(String),
    #[allow(dead_code)]
    DispatchFailed(String),
}

impl fmt::Display for OsdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(msg) => write!(f, "OSD backend unavailable: {msg}"),
            Self::DispatchFailed(msg) => write!(f, "OSD dispatch failed: {msg}"),
        }
    }
}

impl std::error::Error for OsdError {}

/// Abstraction over macOS On-Screen Display (HUD) rendering backends.
pub trait OsdBackend: Send + Sync {
    /// Human-readable backend identifier (e.g., `"macos-liquid-glass"`).
    fn name(&self) -> &'static str;

    /// Returns `true` if this backend supports the given macOS version.
    fn supports_version(&self, version: MacOsVersion) -> bool;

    /// Displays the native volume HUD for the given 64-step [`VolumeState`].
    fn show_volume(&self, state: VolumeState) -> Result<(), OsdError>;
}

/// Detects the running macOS product version (e.g., `27.0.1`).
#[must_use]
pub fn detect_macos_version() -> MacOsVersion {
    if let Ok(output) = Command::new("sw_vers").arg("-productVersion").output() {
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout);
            if let Some(ver) = MacOsVersion::parse(&raw) {
                return ver;
            }
        }
    }
    MacOsVersion::new(26, 0, 0)
}

/// Selects the best [`OsdBackend`] for the given macOS version at runtime.
#[must_use]
pub fn select_backend(version: MacOsVersion) -> Box<dyn OsdBackend> {
    let liquid_glass = macos_liquid_glass::LiquidGlassOsd::new();
    if liquid_glass.supports_version(version) {
        return Box::new(liquid_glass);
    }

    Box::new(noop::NoopOsd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_versions() {
        assert_eq!(
            MacOsVersion::parse("27.0.1"),
            Some(MacOsVersion::new(27, 0, 1))
        );
        assert_eq!(
            MacOsVersion::parse("26.0\n"),
            Some(MacOsVersion::new(26, 0, 0))
        );
        assert_eq!(MacOsVersion::parse("15"), Some(MacOsVersion::new(15, 0, 0)));
        assert_eq!(MacOsVersion::parse(""), None);
        assert_eq!(MacOsVersion::parse("invalid"), None);
    }

    #[test]
    fn dispatches_liquid_glass_for_tahoe_and_newer() {
        assert_eq!(
            select_backend(MacOsVersion::new(26, 0, 0)).name(),
            "macos-liquid-glass"
        );
        assert_eq!(
            select_backend(MacOsVersion::new(27, 0, 1)).name(),
            "macos-liquid-glass"
        );
        assert_eq!(select_backend(MacOsVersion::new(15, 4, 0)).name(), "noop");
    }
}
