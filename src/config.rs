//! User configuration loader for `~/.config/mac-hardware-volume-control/config.json`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_DEVICE_ID: &str = "scarlett-gen4";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub device_id: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            device_id: DEFAULT_DEVICE_ID.to_string(),
        }
    }
}

/// Returns the default configuration file path (`~/.config/mac-hardware-volume-control/config.json`).
#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(
                PathBuf::from(xdg)
                    .join("mac-hardware-volume-control")
                    .join("config.json"),
            );
        }
    }
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("mac-hardware-volume-control")
            .join("config.json"),
    )
}

/// Loads `config.json` if present, otherwise returns [`AppConfig::default`].
#[must_use]
pub fn load_config(path: &Path) -> AppConfig {
    let Ok(raw) = fs::read_to_string(path) else {
        return AppConfig::default();
    };
    parse_config_json(&raw).unwrap_or_default()
}

/// Parses a minimal `{"device_id": "..."}` JSON payload without external dependencies.
#[must_use]
pub fn parse_config_json(raw: &str) -> Option<AppConfig> {
    let key_idx = raw.find("\"device_id\"")?;
    let after_key = &raw[key_idx + "\"device_id\"".len()..];
    let colon_idx = after_key.find(':')?;
    let after_colon = after_key[colon_idx + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;
    let end_quote = rest.find('"')?;
    let device_id = rest[..end_quote].trim();
    if device_id.is_empty() {
        return None;
    }
    Some(AppConfig {
        device_id: device_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_config_json() {
        let json = r#"{
  "device_id": "scarlett-gen4"
}"#;
        let cfg = parse_config_json(json).expect("should parse device_id");
        assert_eq!(cfg.device_id, "scarlett-gen4");
    }

    #[test]
    fn returns_none_on_invalid_or_empty_device_id() {
        assert_eq!(parse_config_json("{}"), None);
        assert_eq!(parse_config_json(r#"{"device_id": ""}"#), None);
    }
}
