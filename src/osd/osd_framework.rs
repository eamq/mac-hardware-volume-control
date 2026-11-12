//! Thin FFI bridge to macOS `OSDUIHelper` (`com.apple.OSDUIHelper`) via
//! `OSDUIHelperProtocol`, with fallback to `OSD.framework` (`OSDManager`).
//!
//! Never renders custom UI windows; strictly delegates to the native system OSD.

use super::OsdError;
use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

/// Graphic IDs recognized by `OSDUIHelper` (`OSDGraphicType`).
/// `1` is display brightness, `3` is volume, `4` is mute.
pub const OSD_GRAPHIC_VOLUME: u64 = 3;
pub const OSD_GRAPHIC_MUTE: u64 = 4;

const OSD_FRAMEWORK_PATH: &str = "/System/Library/PrivateFrameworks/OSD.framework/OSD";
const OSD_MACH_SERVICE: &str = "com.apple.OSDUIHelper";

#[link(name = "Foundation", kind = "framework")]
extern "C" {}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn objc_getProtocol(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

static OSD_LOADED: OnceLock<Result<(), String>> = OnceLock::new();
static XPC_CONN: OnceLock<Option<usize>> = OnceLock::new();

fn ensure_osd_loaded() -> Result<(), OsdError> {
    let status = OSD_LOADED.get_or_init(|| {
        let Ok(path) = CString::new(OSD_FRAMEWORK_PATH) else {
            return Err("invalid framework path".to_string());
        };
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
        if handle.is_null() {
            Err("failed to dlopen OSD.framework".to_string())
        } else {
            Ok(())
        }
    });

    match status {
        Ok(()) => Ok(()),
        Err(msg) => Err(OsdError::Unavailable(msg.clone())),
    }
}

fn get_osd_xpc_connection() -> Option<*mut c_void> {
    let ptr = *XPC_CONN.get_or_init(|| unsafe {
        let pool = objc_autoreleasePoolPush();
        let proto = objc_getProtocol(c"OSDUIHelperProtocol".as_ptr());
        let xpc_cls = objc_getClass(c"NSXPCConnection".as_ptr());
        let iface_cls = objc_getClass(c"NSXPCInterface".as_ptr());
        if proto.is_null() || xpc_cls.is_null() || iface_cls.is_null() {
            objc_autoreleasePoolPop(pool);
            return None;
        }

        let Ok(svc_c) = CString::new(OSD_MACH_SERVICE) else {
            objc_autoreleasePoolPop(pool);
            return None;
        };
        let svc_cf =
            CFStringCreateWithCString(std::ptr::null(), svc_c.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if svc_cf.is_null() {
            objc_autoreleasePoolPop(pool);
            return None;
        }

        let alloc_sel = sel_registerName(c"alloc".as_ptr());
        let alloc_fn: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let raw_conn = alloc_fn(xpc_cls, alloc_sel);

        let init_sel = sel_registerName(c"initWithMachServiceName:options:".as_ptr());
        let init_fn: extern "C" fn(*mut c_void, *mut c_void, *const c_void, u64) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let conn = init_fn(raw_conn, init_sel, svc_cf, 0);
        CFRelease(svc_cf);
        if conn.is_null() {
            objc_autoreleasePoolPop(pool);
            return None;
        }

        let iface_sel = sel_registerName(c"interfaceWithProtocol:".as_ptr());
        let iface_fn: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let iface = iface_fn(iface_cls, iface_sel, proto);

        let set_iface_sel = sel_registerName(c"setRemoteObjectInterface:".as_ptr());
        let set_iface_fn: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend as *const c_void);
        set_iface_fn(conn, set_iface_sel, iface);

        let resume_sel = sel_registerName(c"resume".as_ptr());
        let resume_fn: extern "C" fn(*mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend as *const c_void);
        resume_fn(conn, resume_sel);

        objc_autoreleasePoolPop(pool);
        Some(conn as usize)
    });
    ptr.map(|p| p as *mut c_void)
}

/// Invokes `showImage:onDisplayID:priority:msecUntilFade:filledChiclets:totalChiclets:locked:`
/// on `com.apple.OSDUIHelper`, falling back to `OSDManager`.
pub fn show_chiclet_hud(
    graphic_type: u64,
    filled_chiclets: u32,
    total_chiclets: u32,
    msec_until_fade: u32,
) -> Result<(), OsdError> {
    ensure_osd_loaded()?;

    unsafe {
        let pool = objc_autoreleasePoolPush();
        let display_id = CGMainDisplayID();
        let show_sel = sel_registerName(
            c"showImage:onDisplayID:priority:msecUntilFade:filledChiclets:totalChiclets:locked:"
                .as_ptr(),
        );
        let show_fn: extern "C" fn(*mut c_void, *mut c_void, u64, u32, u32, u32, u32, u32, bool) =
            std::mem::transmute(objc_msgSend as *const c_void);

        if let Some(conn) = get_osd_xpc_connection() {
            let proxy_sel = sel_registerName(c"remoteObjectProxy".as_ptr());
            let get_proxy: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let proxy = get_proxy(conn, proxy_sel);
            if !proxy.is_null() {
                show_fn(
                    proxy,
                    show_sel,
                    graphic_type,
                    display_id,
                    0x1f4,
                    msec_until_fade,
                    filled_chiclets,
                    total_chiclets,
                    false,
                );
                objc_autoreleasePoolPop(pool);
                return Ok(());
            }
        }

        let cls = objc_getClass(c"OSDManager".as_ptr());
        if cls.is_null() {
            objc_autoreleasePoolPop(pool);
            return Err(OsdError::Unavailable(
                "OSDManager class not found".to_string(),
            ));
        }
        let shared_sel = sel_registerName(c"sharedManager".as_ptr());
        let get_shared: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let manager = get_shared(cls, shared_sel);
        if manager.is_null() {
            objc_autoreleasePoolPop(pool);
            return Err(OsdError::Unavailable(
                "OSDManager sharedManager returned nil".to_string(),
            ));
        }

        show_fn(
            manager,
            show_sel,
            graphic_type,
            display_id,
            0x1f4,
            msec_until_fade,
            filled_chiclets,
            total_chiclets,
            false,
        );
        objc_autoreleasePoolPop(pool);
    }

    Ok(())
}
