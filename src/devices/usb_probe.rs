//! Lightweight USB VID/PID discovery via macOS `ioreg`.

use std::process::Command;

/// Queries `ioreg -r -c IOUSBHostDevice -l` and returns `(vendor_id, product_id)` pairs.
#[must_use]
pub fn connected_usb_devices() -> Vec<(u16, u16)> {
    let Ok(output) = Command::new("ioreg")
        .args(["-r", "-c", "IOUSBHostDevice", "-l"])
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_ioreg_usb_ids(&text)
}

/// Parses `idVendor` and `idProduct` properties from `ioreg` output.
#[must_use]
pub fn parse_ioreg_usb_ids(ioreg_output: &str) -> Vec<(u16, u16)> {
    let mut devices = Vec::new();
    let mut current_vid: Option<u16> = None;
    let mut current_pid: Option<u16> = None;

    for line in ioreg_output.lines() {
        let trimmed = line
            .trim_start_matches(|c: char| c.is_whitespace() || c == '|')
            .trim();
        if trimmed.starts_with("+-o ") {
            if let (Some(v), Some(p)) = (current_vid.take(), current_pid.take()) {
                let pair = (v, p);
                if !devices.contains(&pair) {
                    devices.push(pair);
                }
            }
            current_vid = None;
            current_pid = None;
            continue;
        }

        if let Some(val) = parse_ioreg_u16(trimmed, "\"idVendor\" = ") {
            current_vid = Some(val);
        } else if let Some(val) = parse_ioreg_u16(trimmed, "\"idProduct\" = ") {
            current_pid = Some(val);
        }
    }

    if let (Some(v), Some(p)) = (current_vid, current_pid) {
        let pair = (v, p);
        if !devices.contains(&pair) {
            devices.push(pair);
        }
    }

    devices
}

fn parse_ioreg_u16(line: &str, prefix: &str) -> Option<u16> {
    let rest = line.strip_prefix(prefix)?.trim();
    rest.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ioreg_vendor_and_product_ids() {
        let sample = r#"
  | | +-o Scarlett 18i20 4th Gen@20110000  <class IOUSBHostDevice, id 0x10000cc72>
  | |   | {
  | |   |   "idProduct" = 33309
  | |   |   "idVendor" = 4661
  | |   | }
+-o Apple Internal Keyboard@00000000  <class IOUSBHostDevice, id 0x100000111>
  {
    "idVendor" = 1452
    "idProduct" = 832
  }
"#;
        let parsed = parse_ioreg_usb_ids(sample);
        assert_eq!(parsed, vec![(0x1235, 0x821d), (0x05ac, 0x0340)]);
    }
}
