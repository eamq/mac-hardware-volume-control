//! Read-only IOKit (`IOUSBLib`) probe for Focusrite vendor USB interfaces.

use std::ffi::{c_char, c_void, CString};
use std::ptr;

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

// kIOUSBInterfaceUserClientTypeID: 2d9786c6-9ef3-11d4-ad51-000a27052861
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

// kIOCFPlugInInterfaceID: c244e858-109c-11d4-91d4-0050e4c6426f
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

// kIOUSBInterfaceInterfaceID190: 8fdb8455-74a6-11d6-97b1-003065d3608e
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

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFUUIDCreateFromUUIDBytes(alloc: *const c_void, bytes: CFUUIDBytes) -> *const c_void;
    fn CFRelease(cf: *const c_void);
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
    get_interface_sub_class: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    get_interface_protocol: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    get_device_vendor: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u16) -> i32,
    get_device_product: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u16) -> i32,
    _get_device_release_number: *mut c_void,
    _get_configuration_value: *mut c_void,
    get_interface_number: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    _get_alternate_setting: *mut c_void,
    get_num_endpoints: unsafe extern "C" fn(*mut *mut IOUSBInterface190Vtbl, *mut u8) -> i32,
    _get_location_id: *mut c_void,
    _get_device: *mut c_void,
    _set_alternate_interface: *mut c_void,
    _get_bus_frame_number: *mut c_void,
    _control_request: *mut c_void,
    _control_request_async: *mut c_void,
    get_pipe_properties: unsafe extern "C" fn(
        *mut *mut IOUSBInterface190Vtbl,
        u8,
        *mut u8,
        *mut u8,
        *mut u8,
        *mut u16,
        *mut u8,
    ) -> i32,
}

/// Probes all `IOUSBHostInterface` services matching `vendor_id` (e.g. `0x1235` Focusrite)
/// and tests `USBInterfaceOpen` + pipe inspection on vendor-specific interfaces (`class == 255`).
pub fn probe_vendor_usb_interfaces(vendor_id: u16) -> Result<(), String> {
    unsafe {
        let class_name = CString::new("IOUSBHostInterface").map_err(|e| e.to_string())?;
        let matching = IOServiceMatching(class_name.as_ptr());
        if matching.is_null() {
            return Err("IOServiceMatching returned null".to_string());
        }

        let mut iter: u32 = 0;
        let kr = IOServiceGetMatchingServices(0, matching, &mut iter);
        if kr != 0 {
            return Err(format!("IOServiceGetMatchingServices failed: 0x{kr:08x}"));
        }

        let plugin_type =
            CFUUIDCreateFromUUIDBytes(ptr::null(), K_IO_USB_INTERFACE_USER_CLIENT_TYPE_ID);
        let iface_type = CFUUIDCreateFromUUIDBytes(ptr::null(), K_IO_CF_PLUGIN_INTERFACE_ID);
        let mut matched_count = 0;

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
            let mut pid: u16 = 0;
            let mut iface_num: u8 = 0;
            let mut iface_class: u8 = 0;
            let mut iface_sub: u8 = 0;
            let mut iface_proto: u8 = 0;

            let _ = ((**usb_if).get_device_vendor)(usb_if, &mut vid);
            let _ = ((**usb_if).get_device_product)(usb_if, &mut pid);
            if vid != vendor_id {
                ((**usb_if).release)(usb_if);
                continue;
            }

            let _ = ((**usb_if).get_interface_number)(usb_if, &mut iface_num);
            let _ = ((**usb_if).get_interface_class)(usb_if, &mut iface_class);
            let _ = ((**usb_if).get_interface_sub_class)(usb_if, &mut iface_sub);
            let _ = ((**usb_if).get_interface_protocol)(usb_if, &mut iface_proto);

            matched_count += 1;
            println!(
                "USB interface #{iface_num}: VID=0x{vid:04x} PID=0x{pid:04x} class={iface_class} sub={iface_sub} proto={iface_proto}"
            );

            if iface_class == 255 {
                let open_kr = ((**usb_if).usb_interface_open)(usb_if);
                println!(
                    "  USBInterfaceOpen -> 0x{open_kr:08x} ({})",
                    if open_kr == 0 { "OK" } else { "FAILED" }
                );
                if open_kr == 0 {
                    let mut num_ep: u8 = 0;
                    let _ = ((**usb_if).get_num_endpoints)(usb_if, &mut num_ep);
                    println!("  Endpoints: {num_ep}");
                    for pipe in 1..=num_ep {
                        let mut dir: u8 = 0;
                        let mut num: u8 = 0;
                        let mut xfer: u8 = 0;
                        let mut max_pkt: u16 = 0;
                        let mut interval: u8 = 0;
                        let pk = ((**usb_if).get_pipe_properties)(
                            usb_if,
                            pipe,
                            &mut dir,
                            &mut num,
                            &mut xfer,
                            &mut max_pkt,
                            &mut interval,
                        );
                        if pk == 0 {
                            println!("    pipe {pipe}: dir={dir} ep=0x{num:02x} type={xfer} max_pkt={max_pkt} interval={interval}");
                        }
                    }
                    let _ = ((**usb_if).usb_interface_close)(usb_if);
                }
            }

            ((**usb_if).release)(usb_if);
        }

        CFRelease(plugin_type);
        CFRelease(iface_type);
        IOObjectRelease(iter);

        if matched_count == 0 {
            println!("No USB interfaces found for VID=0x{vendor_id:04x}");
        }
        Ok(())
    }
}
