//! Focusrite Control Protocol (FCP) over macOS `IOKit` (`IOUSBLib`).
//!
//! Implements the vendor-specific USB control + interrupt-ACK protocol used by
//! big Scarlett 4th Gen interfaces (`16i16`, `18i16`, `18i20`) on Interface #3
//! (`bInterfaceClass = 255`, `bInterfaceSubClass = 1`, `bInterfaceProtocol = 32`).

use crate::devices::DeviceError;
use std::ffi::{c_char, c_void, CString};
use std::ptr;

const FCP_USB_REQ_STEP0: u8 = 0;
const FCP_USB_REQ_CMD_TX: u8 = 2;
const FCP_USB_REQ_CMD_RX: u8 = 3;

const USB_DIR_OUT: u8 = 0x00;
const USB_DIR_IN: u8 = 0x80;
const USB_TYPE_CLASS: u8 = 0x20;
const USB_RECIP_INTERFACE: u8 = 0x01;

const FCP_OPCODE_INIT_1: u32 = 0x0000_0000;
const FCP_OPCODE_INIT_2: u32 = 0x0000_0002;
const FCP_OPCODE_DATA_READ: u32 = 0x0080_0000;
const FCP_OPCODE_DATA_WRITE: u32 = 0x0080_0001;
const FCP_OPCODE_DATA_NOTIFY: u32 = 0x0080_0002;
const FCP_OPCODE_DEVMAP_INFO: u32 = 0x0080_000c;
const FCP_OPCODE_DEVMAP_READ: u32 = 0x0080_000d;

const FCP_STEP0_SIZE: usize = 24;
const FCP_STEP2_SIZE: usize = 84;
const FCP_DEVMAP_BLOCK_SIZE: usize = 1024;
const FCP_NOTIFY_ACK: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct CFUUIDBytes {
    byte0: u8,
    byte1: u8,
    byte2: u8,
    byte3: u8,
    byte4: u8,
    byte5: u8,
    byte6: u8,
    byte7: u8,
    byte8: u8,
    byte9: u8,
    byte10: u8,
    byte11: u8,
    byte12: u8,
    byte13: u8,
    byte14: u8,
    byte15: u8,
}

const K_IO_USB_INTERFACE_USER_CLIENT_TYPE_ID: CFUUIDBytes = CFUUIDBytes {
    byte0: 0x2d,
    byte1: 0x97,
    byte2: 0x86,
    byte3: 0xc6,
    byte4: 0x9e,
    byte5: 0xf3,
    byte6: 0x11,
    byte7: 0xd4,
    byte8: 0xad,
    byte9: 0x51,
    byte10: 0x00,
    byte11: 0x0a,
    byte12: 0x27,
    byte13: 0x05,
    byte14: 0x28,
    byte15: 0x61,
};

const K_IO_CF_PLUGIN_INTERFACE_ID: CFUUIDBytes = CFUUIDBytes {
    byte0: 0xc2,
    byte1: 0x44,
    byte2: 0xe8,
    byte3: 0x58,
    byte4: 0x10,
    byte5: 0x9c,
    byte6: 0x11,
    byte7: 0xd4,
    byte8: 0x91,
    byte9: 0xd4,
    byte10: 0x00,
    byte11: 0x50,
    byte12: 0xe4,
    byte13: 0xc6,
    byte14: 0x42,
    byte15: 0x6f,
};

const K_IO_USB_INTERFACE_INTERFACE_ID_190: CFUUIDBytes = CFUUIDBytes {
    byte0: 0x8f,
    byte1: 0xdb,
    byte2: 0x84,
    byte3: 0x55,
    byte4: 0x74,
    byte5: 0xa6,
    byte6: 0x11,
    byte7: 0xd6,
    byte8: 0x97,
    byte9: 0xb1,
    byte10: 0x00,
    byte11: 0x30,
    byte12: 0x65,
    byte13: 0xd3,
    byte14: 0x60,
    byte15: 0x8e,
};

#[repr(C)]
#[allow(non_snake_case)]
struct IOUSBDevRequest {
    bmRequestType: u8,
    bRequest: u8,
    wValue: u16,
    wIndex: u16,
    wLength: u16,
    pData: *mut c_void,
    wLenDone: u32,
}

#[repr(C)]
struct IUnknownVtbl {
    _reserved: *mut c_void,
    query_interface:
        unsafe extern "C" fn(*mut *mut IUnknownVtbl, CFUUIDBytes, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(*mut *mut IUnknownVtbl) -> u32,
    release: unsafe extern "C" fn(*mut *mut IUnknownVtbl) -> u32,
}

#[repr(C)]
struct IOUSBInterface190Vtbl {
    _reserved: *mut c_void,
    query_interface:
        unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, CFUUIDBytes, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl) -> u32,
    release: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl) -> u32,
    _create_async_event_source: *mut c_void,
    _get_async_event_source: *mut c_void,
    _create_async_port: *mut c_void,
    _get_async_port: *mut c_void,
    usb_interface_open: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl) -> i32,
    usb_interface_close: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl) -> i32,
    get_interface_class: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    _get_interface_sub_class: *mut c_void,
    _get_interface_protocol: *mut c_void,
    get_device_vendor: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u16) -> i32,
    get_device_product: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u16) -> i32,
    _get_device_release_number: *mut c_void,
    _get_configuration_value: *mut c_void,
    get_interface_number: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    _get_alternate_setting: *mut c_void,
    _get_num_endpoints: *mut c_void,
    _get_location_id: *mut c_void,
    _get_device: *mut c_void,
    _set_alternate_interface: *mut c_void,
    _get_bus_frame_number: *mut c_void,
    control_request:
        unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, u8, *mut IOUSBDevRequest) -> i32,
    _control_request_async: *mut c_void,
    _get_pipe_properties: *mut c_void,
    _get_pipe_status: *mut c_void,
    _abort_pipe: *mut c_void,
    _reset_pipe: *mut c_void,
    _clear_pipe_stall: *mut c_void,
    read_pipe:
        unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, u8, *mut c_void, *mut u32) -> i32,
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: *mut c_void,
        existing: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOCreatePlugInInterfaceForService(
        service: u32,
        plugin_type: *const c_void,
        interface_type: *const c_void,
        the_interface: *mut *mut *mut c_void,
        the_score: *mut i32,
    ) -> i32;
    fn IODestroyPlugInInterface(interface: *mut *mut c_void) -> i32;
}

#[link(name = "z")]
extern "C" {
    fn uncompress(
        dest: *mut u8,
        dest_len: *mut libc::c_ulong,
        source: *const u8,
        source_len: libc::c_ulong,
    ) -> libc::c_int;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFUUIDCreateFromUUIDBytes(alloc: *const c_void, bytes: CFUUIDBytes) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcpMemberLayout {
    pub offset: u32,
    pub size: u32,
    pub is_signed: bool,
    pub notify_device: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FcpMonitorControls {
    pub master_volume: FcpMemberLayout,
    pub mute_switch: FcpMemberLayout,
}

/// Active FCP USB session to a Focusrite Scarlett 4th Gen device.
pub struct FcpSession {
    usb_if: *mut *mut IOUSBInterface190Vtbl,
    iface_num: u8,
    seq: u16,
    pub controls: FcpMonitorControls,
}

unsafe impl Send for FcpSession {}
unsafe impl Sync for FcpSession {}

impl Drop for FcpSession {
    fn drop(&mut self) {
        unsafe {
            let _ = ((**self.usb_if).usb_interface_close)(self.usb_if);
            ((**self.usb_if).release)(self.usb_if);
        }
    }
}

impl FcpSession {
    /// Opens the vendor-specific FCP USB interface (`class == 255`) for `vendor_id`,
    /// initializes FCP, reads the firmware devmap JSON, and locates `masterVolume` & `muteSwitch`.
    pub fn open(vendor_id: u16) -> Result<Self, DeviceError> {
        let (usb_if, iface_num) = open_vendor_interface(vendor_id)?;
        let mut session = Self {
            usb_if,
            iface_num,
            seq: 0,
            controls: FcpMonitorControls {
                master_volume: FcpMemberLayout {
                    offset: 0,
                    size: 1,
                    is_signed: true,
                    notify_device: 0,
                },
                mute_switch: FcpMemberLayout {
                    offset: 0,
                    size: 1,
                    is_signed: false,
                    notify_device: 0,
                },
            },
        };

        session.init_protocol()?;
        let devmap_json = session.read_devmap_json()?;
        session.controls = parse_monitor_controls(&devmap_json)?;
        Ok(session)
    }

    fn control_in(&self, request: u8, buf: &mut [u8]) -> Result<usize, DeviceError> {
        let mut req = IOUSBDevRequest {
            bmRequestType: USB_DIR_IN | USB_TYPE_CLASS | USB_RECIP_INTERFACE,
            bRequest: request,
            wValue: 0,
            wIndex: u16::from(self.iface_num),
            wLength: buf.len() as u16,
            pData: buf.as_mut_ptr().cast(),
            wLenDone: 0,
        };
        let kr = unsafe { ((**self.usb_if).control_request)(self.usb_if, 0, &mut req) };
        if kr != 0 {
            return Err(DeviceError::Io(format!("ControlRequest IN 0x{kr:08x}")));
        }
        Ok(req.wLenDone as usize)
    }

    fn control_out(&self, request: u8, buf: &[u8]) -> Result<(), DeviceError> {
        let mut req = IOUSBDevRequest {
            bmRequestType: USB_DIR_OUT | USB_TYPE_CLASS | USB_RECIP_INTERFACE,
            bRequest: request,
            wValue: 0,
            wIndex: u16::from(self.iface_num),
            wLength: buf.len() as u16,
            pData: buf.as_ptr() as *mut c_void,
            wLenDone: 0,
        };
        let kr = unsafe { ((**self.usb_if).control_request)(self.usb_if, 0, &mut req) };
        if kr != 0 {
            return Err(DeviceError::Io(format!("ControlRequest OUT 0x{kr:08x}")));
        }
        Ok(())
    }

    fn wait_for_ack(&self) -> Result<(), DeviceError> {
        let mut buf = [0u8; 64];
        for _ in 0..32 {
            let mut len: u32 = 8;
            let kr = unsafe {
                ((**self.usb_if).read_pipe)(self.usb_if, 1, buf.as_mut_ptr().cast(), &mut len)
            };
            if kr != 0 {
                return Err(DeviceError::Io(format!(
                    "ReadPipe notify failed: 0x{kr:08x}"
                )));
            }
            if len >= 4 {
                let word = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                if (word & FCP_NOTIFY_ACK) != 0 {
                    return Ok(());
                }
            }
        }
        Err(DeviceError::ProtocolError(
            "timed out waiting for FCP ACK".to_string(),
        ))
    }

    fn fcp_cmd(
        &mut self,
        opcode: u32,
        req_data: &[u8],
        resp_len: usize,
    ) -> Result<Vec<u8>, DeviceError> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);

        let mut tx = Vec::with_capacity(16 + req_data.len());
        tx.extend_from_slice(&opcode.to_le_bytes());
        tx.extend_from_slice(&(req_data.len() as u16).to_le_bytes());
        tx.extend_from_slice(&seq.to_le_bytes());
        tx.extend_from_slice(&0u32.to_le_bytes()); // error
        tx.extend_from_slice(&0u32.to_le_bytes()); // pad
        tx.extend_from_slice(req_data);

        self.control_out(FCP_USB_REQ_CMD_TX, &tx)?;
        self.wait_for_ack()?;

        let mut rx = vec![0u8; 16 + resp_len];
        let got = self.control_in(FCP_USB_REQ_CMD_RX, &mut rx)?;
        if got < 16 {
            return Err(DeviceError::ProtocolError(format!(
                "FCP RX too short: {got}"
            )));
        }

        let rx_opcode = u32::from_le_bytes([rx[0], rx[1], rx[2], rx[3]]);
        let rx_size = u16::from_le_bytes([rx[4], rx[5]]) as usize;
        let rx_seq = u16::from_le_bytes([rx[6], rx[7]]);
        let rx_err = u32::from_le_bytes([rx[8], rx[9], rx[10], rx[11]]);

        if rx_seq != seq || rx_opcode != opcode || rx_err != 0 {
            return Err(DeviceError::ProtocolError(format!(
                "FCP mismatch/error: opcode=0x{rx_opcode:08x} seq={rx_seq}/{seq} err={rx_err}"
            )));
        }

        let end = (16 + rx_size).min(got);
        Ok(rx[16..end].to_vec())
    }

    fn init_protocol(&mut self) -> Result<(), DeviceError> {
        let mut step0 = [0u8; FCP_STEP0_SIZE];
        let got = self.control_in(FCP_USB_REQ_STEP0, &mut step0)?;
        if got != FCP_STEP0_SIZE {
            return Err(DeviceError::ProtocolError(format!(
                "STEP0 expected {FCP_STEP0_SIZE} bytes, got {got}"
            )));
        }

        self.seq = 0;
        let _ = self.fcp_cmd(FCP_OPCODE_INIT_1, &[], 0)?;
        let _ = self.fcp_cmd(FCP_OPCODE_INIT_2, &[], FCP_STEP2_SIZE)?;
        Ok(())
    }

    fn read_devmap_json(&mut self) -> Result<String, DeviceError> {
        let info = self.fcp_cmd(FCP_OPCODE_DEVMAP_INFO, &[], 4)?;
        if info.len() < 4 {
            return Err(DeviceError::ProtocolError(
                "DEVMAP_INFO too short".to_string(),
            ));
        }
        let total_size = u16::from_le_bytes([info[2], info[3]]) as usize;
        let mut encoded = Vec::with_capacity(total_size);
        let mut offset = 0usize;
        while offset < total_size {
            let block_num = (offset / FCP_DEVMAP_BLOCK_SIZE) as u32;
            let chunk_len = (total_size - offset).min(FCP_DEVMAP_BLOCK_SIZE);
            let chunk =
                self.fcp_cmd(FCP_OPCODE_DEVMAP_READ, &block_num.to_le_bytes(), chunk_len)?;
            encoded.extend_from_slice(&chunk[..chunk_len.min(chunk.len())]);
            offset += chunk_len;
        }

        while encoded.last() == Some(&0) {
            encoded.pop();
        }
        let compressed = decode_base64(&encoded)?;
        let json_bytes = zlib_decompress(&compressed)?;
        Ok(String::from_utf8_lossy(&json_bytes).into_owned())
    }

    pub fn data_read(&mut self, layout: FcpMemberLayout) -> Result<i32, DeviceError> {
        let mut req = [0u8; 8];
        req[0..4].copy_from_slice(&layout.offset.to_le_bytes());
        req[4..8].copy_from_slice(&layout.size.to_le_bytes());
        let resp = self.fcp_cmd(FCP_OPCODE_DATA_READ, &req, layout.size as usize)?;
        let mut raw = [0u8; 4];
        for (i, b) in resp.iter().take(4).enumerate() {
            raw[i] = *b;
        }
        let val = u32::from_le_bytes(raw);
        if layout.is_signed {
            match layout.size {
                1 => Ok(i32::from(val as i8)),
                2 => Ok(i32::from(val as i16)),
                4 => Ok(val as i32),
                _ => Err(DeviceError::ProtocolError(
                    "unsupported signed size".to_string(),
                )),
            }
        } else {
            Ok(val as i32)
        }
    }

    pub fn data_write(&mut self, layout: FcpMemberLayout, value: i32) -> Result<(), DeviceError> {
        let mut req = [0u8; 12];
        req[0..4].copy_from_slice(&layout.offset.to_le_bytes());
        req[4..8].copy_from_slice(&layout.size.to_le_bytes());
        req[8..12].copy_from_slice(&(value as u32).to_le_bytes());
        let req_len = 8 + (layout.size as usize).min(4);
        let _ = self.fcp_cmd(FCP_OPCODE_DATA_WRITE, &req[..req_len], 0)?;
        if layout.notify_device != 0 {
            let _ = self.fcp_cmd(
                FCP_OPCODE_DATA_NOTIFY,
                &layout.notify_device.to_le_bytes(),
                0,
            )?;
        }
        Ok(())
    }

    /// Reads `(master_volume_db, muted)` directly from the hardware.
    pub fn read_monitor_state(&mut self) -> Result<(i32, bool), DeviceError> {
        let vol = self.data_read(self.controls.master_volume)?;
        let mute = self.data_read(self.controls.mute_switch)?;
        Ok((vol, mute != 0))
    }

    /// Writes `(master_volume_db, muted)` to the hardware.
    pub fn write_monitor_state(&mut self, volume_db: i32, muted: bool) -> Result<(), DeviceError> {
        self.data_write(self.controls.mute_switch, i32::from(muted))?;
        self.data_write(self.controls.master_volume, volume_db)?;
        Ok(())
    }
}

fn open_vendor_interface(
    vendor_id: u16,
) -> Result<(*mut *mut IOUSBInterface190Vtbl, u8), DeviceError> {
    unsafe {
        let class_name =
            CString::new("IOUSBHostInterface").map_err(|e| DeviceError::Io(e.to_string()))?;
        let matching = IOServiceMatching(class_name.as_ptr());
        if matching.is_null() {
            return Err(DeviceError::TransportUnavailable(
                "IOServiceMatching null".to_string(),
            ));
        }

        let mut iter: u32 = 0;
        if IOServiceGetMatchingServices(0, matching, &mut iter) != 0 {
            return Err(DeviceError::TransportUnavailable(
                "IOServiceGetMatchingServices failed".to_string(),
            ));
        }

        let plugin_type =
            CFUUIDCreateFromUUIDBytes(ptr::null(), K_IO_USB_INTERFACE_USER_CLIENT_TYPE_ID);
        let iface_type = CFUUIDCreateFromUUIDBytes(ptr::null(), K_IO_CF_PLUGIN_INTERFACE_ID);
        let mut last_err = DeviceError::NotConnected;

        loop {
            let service = IOIteratorNext(iter);
            if service == 0 {
                break;
            }

            let mut plugin_raw: *mut *mut c_void = ptr::null_mut();
            let mut score: i32 = 0;
            let pkr = IOCreatePlugInInterfaceForService(
                service,
                plugin_type,
                iface_type,
                &mut plugin_raw,
                &mut score,
            );
            IOObjectRelease(service);
            if pkr != 0 || plugin_raw.is_null() {
                continue;
            }

            let plugin = plugin_raw.cast::<*mut IUnknownVtbl>();
            let mut usb_if_raw: *mut c_void = ptr::null_mut();
            let qkr = ((**plugin).query_interface)(
                plugin,
                K_IO_USB_INTERFACE_INTERFACE_ID_190,
                &mut usb_if_raw,
            );
            IODestroyPlugInInterface(plugin_raw);
            if qkr != 0 || usb_if_raw.is_null() {
                continue;
            }

            let usb_if = usb_if_raw.cast::<*mut IOUSBInterface190Vtbl>();
            let mut vid: u16 = 0;
            let mut class: u8 = 0;
            let mut iface_num: u8 = 0;
            let _ = ((**usb_if).get_device_vendor)(usb_if, &mut vid);
            let _ = ((**usb_if).get_interface_class)(usb_if, &mut class);

            if vid == vendor_id && class == 255 {
                let _ = ((**usb_if).get_interface_number)(usb_if, &mut iface_num);
                let open_kr = ((**usb_if).usb_interface_open)(usb_if);
                if open_kr == 0 {
                    CFRelease(plugin_type);
                    CFRelease(iface_type);
                    IOObjectRelease(iter);
                    return Ok((usb_if, iface_num));
                }
                last_err = DeviceError::TransportUnavailable(format!(
                    "USBInterfaceOpen failed: 0x{open_kr:08x} (close Focusrite Control 2 if open)"
                ));
            }

            ((**usb_if).release)(usb_if);
        }

        CFRelease(plugin_type);
        CFRelease(iface_type);
        IOObjectRelease(iter);
        Err(last_err)
    }
}

/// Parses `masterVolume` and `muteSwitch` offsets/types from the device's firmware devmap JSON.
pub fn parse_monitor_controls(devmap_json: &str) -> Result<FcpMonitorControls, DeviceError> {
    let app_idx = devmap_json
        .find("\"APP_SPACE\"")
        .ok_or_else(|| DeviceError::ProtocolError("APP_SPACE missing in devmap".to_string()))?;
    let app_slice = &devmap_json[app_idx..];

    let master_volume = parse_member(app_slice, "masterVolume")?;
    let mute_switch = parse_member(app_slice, "muteSwitch")?;
    Ok(FcpMonitorControls {
        master_volume,
        mute_switch,
    })
}

fn parse_member(scope: &str, name: &str) -> Result<FcpMemberLayout, DeviceError> {
    let needle = format!("\"{name}\"");
    let start = scope
        .find(&needle)
        .ok_or_else(|| DeviceError::ProtocolError(format!("devmap member '{name}' not found")))?;
    let rest = &scope[start..];
    let brace_start = rest
        .find('{')
        .ok_or_else(|| DeviceError::ProtocolError(format!("malformed member '{name}'")))?;
    let brace_end = rest[brace_start..]
        .find('}')
        .ok_or_else(|| DeviceError::ProtocolError(format!("unclosed member '{name}'")))?;
    let block = &rest[brace_start..=brace_start + brace_end];

    let offset = extract_json_u32(block, "offset")
        .ok_or_else(|| DeviceError::ProtocolError(format!("missing offset for '{name}'")))?;
    let notify_device = extract_json_u32(block, "notify-device").unwrap_or(0);
    let type_str = extract_json_str(block, "type").unwrap_or("int8");
    let (size, is_signed) = match type_str {
        "int8" => (1, true),
        "uint8" | "bool" => (1, false),
        "int16" => (2, true),
        "uint16" => (2, false),
        "int32" => (4, true),
        "uint32" => (4, false),
        _ => (1, true),
    };

    Ok(FcpMemberLayout {
        offset,
        size,
        is_signed,
        notify_device,
    })
}

fn extract_json_u32(block: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{key}\"");
    let idx = block.find(&needle)?;
    let after = &block[idx + needle.len()..];
    let colon = after.find(':')?;
    let digits: String = after[colon + 1..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().ok()
}

fn extract_json_str<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let idx = block.find(&needle)?;
    let after = &block[idx + needle.len()..];
    let colon = after.find(':')?;
    let q1 = after[colon + 1..].find('"')?;
    let rest = &after[colon + 1 + q1 + 1..];
    let q2 = rest.find('"')?;
    Some(&rest[..q2])
}

fn decode_base64(input: &[u8]) -> Result<Vec<u8>, DeviceError> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let filtered: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity((filtered.len() / 4) * 3);

    for chunk in filtered.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        let a = val(chunk[0]).ok_or_else(|| DeviceError::ProtocolError("invalid base64".into()))?;
        let b = val(chunk[1]).ok_or_else(|| DeviceError::ProtocolError("invalid base64".into()))?;
        out.push((a << 2) | (b >> 4));

        if chunk.len() > 2 && chunk[2] != b'=' {
            let c =
                val(chunk[2]).ok_or_else(|| DeviceError::ProtocolError("invalid base64".into()))?;
            out.push((b << 4) | (c >> 2));
            if chunk.len() > 3 && chunk[3] != b'=' {
                let d = val(chunk[3])
                    .ok_or_else(|| DeviceError::ProtocolError("invalid base64".into()))?;
                out.push((c << 6) | d);
            }
        }
    }
    Ok(out)
}

fn zlib_decompress(compressed: &[u8]) -> Result<Vec<u8>, DeviceError> {
    let mut cap = compressed.len().saturating_mul(16).max(4096);
    for _ in 0..4 {
        let mut out = vec![0u8; cap];
        let mut out_len = out.len() as libc::c_ulong;
        let rc = unsafe {
            uncompress(
                out.as_mut_ptr(),
                &mut out_len,
                compressed.as_ptr(),
                compressed.len() as libc::c_ulong,
            )
        };
        if rc == 0 {
            out.truncate(out_len as usize);
            return Ok(out);
        }
        if rc == -5 {
            cap *= 2;
            continue;
        }
        return Err(DeviceError::ProtocolError(format!(
            "zlib uncompress failed: {rc}"
        )));
    }
    Err(DeviceError::ProtocolError(
        "zlib output buffer exceeded".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_base64_payload() {
        assert_eq!(decode_base64(b"SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(decode_base64(b"Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn parses_devmap_master_volume_and_mute_switch() {
        let sample = r#"{
          "structs": {
            "APP_SPACE": {
              "members": {
                "masterVolume": { "offset": 412, "type": "int8", "notify-device": 8 },
                "muteSwitch": { "offset": 416, "type": "bool", "notify-device": 8 }
              }
            }
          }
        }"#;
        let parsed = parse_monitor_controls(sample).expect("should parse devmap");
        assert_eq!(
            parsed.master_volume,
            FcpMemberLayout {
                offset: 412,
                size: 1,
                is_signed: true,
                notify_device: 8,
            }
        );
        assert_eq!(
            parsed.mute_switch,
            FcpMemberLayout {
                offset: 416,
                size: 1,
                is_signed: false,
                notify_device: 8,
            }
        );
    }
}
