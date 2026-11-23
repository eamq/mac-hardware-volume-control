//! Watches macOS `NSWorkspace` application launch/termination notifications
//! so the daemon can release exclusive USB access when a vendor control app
//! (e.g., Focusrite Control 2) opens and reclaim it once the app quits.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicUsize, Ordering};

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn objc_allocateClassPair(
        superclass: *mut c_void,
        name: *const c_char,
        extra_bytes: usize,
    ) -> *mut c_void;
    fn objc_registerClassPair(cls: *mut c_void);
    fn class_addMethod(
        cls: *mut c_void,
        name: *mut c_void,
        imp: *const c_void,
        types: *const c_char,
    ) -> bool;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEventKind {
    WillLaunch,
    DidTerminate,
}

type AppEventCallback = fn(*mut c_void, AppEventKind, &str);

static CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static CONTEXT_PTR: AtomicUsize = AtomicUsize::new(0);
static PREFIXES_PTR: AtomicUsize = AtomicUsize::new(0);
static PREFIXES_LEN: AtomicUsize = AtomicUsize::new(0);

/// Returns `true` if `bundle_id` matches any prefix case-insensitively.
#[must_use]
pub fn matches_any_prefix(bundle_id: &str, prefixes: &[&str]) -> bool {
    let lower = bundle_id.to_ascii_lowercase();
    prefixes
        .iter()
        .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
}

unsafe fn nsstring_to_string(ns_str: *mut c_void) -> Option<String> {
    if ns_str.is_null() {
        return None;
    }
    let sel_utf8 = sel_registerName(c"UTF8String".as_ptr());
    let utf8_fn: extern "C" fn(*mut c_void, *mut c_void) -> *const c_char =
        std::mem::transmute(objc_msgSend as *const c_void);
    let ptr = utf8_fn(ns_str, sel_utf8);
    if ptr.is_null() {
        return None;
    }
    Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
}

unsafe fn running_app_bundle_id(app: *mut c_void) -> Option<String> {
    if app.is_null() {
        return None;
    }
    let sel_bid = sel_registerName(c"bundleIdentifier".as_ptr());
    let bid_fn: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    let bid_ns = bid_fn(app, sel_bid);
    nsstring_to_string(bid_ns)
}

/// Returns `true` if any currently running GUI application matches `prefixes`.
#[must_use]
pub fn is_any_app_running(prefixes: &[&str]) -> bool {
    if prefixes.is_empty() {
        return false;
    }
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let ws_cls = objc_getClass(c"NSWorkspace".as_ptr());
        if ws_cls.is_null() {
            objc_autoreleasePoolPop(pool);
            return false;
        }
        let sel_shared = sel_registerName(c"sharedWorkspace".as_ptr());
        let shared_fn: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let ws = shared_fn(ws_cls, sel_shared);
        if ws.is_null() {
            objc_autoreleasePoolPop(pool);
            return false;
        }

        let sel_apps = sel_registerName(c"runningApplications".as_ptr());
        let apps_fn: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);
        let apps = apps_fn(ws, sel_apps);
        if apps.is_null() {
            objc_autoreleasePoolPop(pool);
            return false;
        }

        let sel_count = sel_registerName(c"count".as_ptr());
        let count_fn: extern "C" fn(*mut c_void, *mut c_void) -> usize =
            std::mem::transmute(objc_msgSend as *const c_void);
        let count = count_fn(apps, sel_count);

        let sel_obj_at = sel_registerName(c"objectAtIndex:".as_ptr());
        let obj_fn: extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void =
            std::mem::transmute(objc_msgSend as *const c_void);

        let mut matched = false;
        for idx in 0..count {
            let app = obj_fn(apps, sel_obj_at, idx);
            if let Some(bid) = running_app_bundle_id(app) {
                if matches_any_prefix(&bid, prefixes) {
                    matched = true;
                    break;
                }
            }
        }
        objc_autoreleasePoolPop(pool);
        matched
    }
}

unsafe fn extract_notification_bundle_id(note: *mut c_void) -> Option<String> {
    if note.is_null() {
        return None;
    }
    let sel_info = sel_registerName(c"userInfo".as_ptr());
    let info_fn: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    let user_info = info_fn(note, sel_info);
    if user_info.is_null() {
        return None;
    }

    let key = CFStringCreateWithCString(
        std::ptr::null(),
        c"NSWorkspaceApplicationKey".as_ptr(),
        K_CF_STRING_ENCODING_UTF8,
    );
    if key.is_null() {
        return None;
    }
    let sel_obj_for_key = sel_registerName(c"objectForKey:".as_ptr());
    let obj_fn: extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    let app = obj_fn(user_info, sel_obj_for_key, key);
    CFRelease(key);

    running_app_bundle_id(app)
}

unsafe fn dispatch_event(note: *mut c_void, kind: AppEventKind) {
    let cb_raw = CALLBACK_PTR.load(Ordering::Acquire);
    let p_ptr = PREFIXES_PTR.load(Ordering::Acquire) as *const &'static str;
    let p_len = PREFIXES_LEN.load(Ordering::Acquire);
    if cb_raw == 0 || p_ptr.is_null() || p_len == 0 {
        return;
    }

    let prefixes = std::slice::from_raw_parts(p_ptr, p_len);
    let Some(bundle_id) = extract_notification_bundle_id(note) else {
        return;
    };
    if !matches_any_prefix(&bundle_id, prefixes) {
        return;
    }

    let cb: AppEventCallback = std::mem::transmute(cb_raw);
    let ctx = CONTEXT_PTR.load(Ordering::Acquire) as *mut c_void;
    cb(ctx, kind, &bundle_id);
}

extern "C" fn on_will_launch(_self: *mut c_void, _cmd: *mut c_void, note: *mut c_void) {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        dispatch_event(note, AppEventKind::WillLaunch);
        objc_autoreleasePoolPop(pool);
    }
}

extern "C" fn on_did_terminate(_self: *mut c_void, _cmd: *mut c_void, note: *mut c_void) {
    unsafe {
        let pool = objc_autoreleasePoolPush();
        dispatch_event(note, AppEventKind::DidTerminate);
        objc_autoreleasePoolPop(pool);
    }
}

pub struct AppWatcher {
    observer: *mut c_void,
    workspace_nc: *mut c_void,
}

impl AppWatcher {
    /// Registers `NSWorkspace` launch/terminate observers on the current run loop.
    #[must_use]
    pub fn register(
        prefixes: &'static [&'static str],
        callback: AppEventCallback,
        context: *mut c_void,
    ) -> Option<Self> {
        if prefixes.is_empty() {
            return None;
        }
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let ws_cls = objc_getClass(c"NSWorkspace".as_ptr());
            let ns_obj_cls = objc_getClass(c"NSObject".as_ptr());
            if ws_cls.is_null() || ns_obj_cls.is_null() {
                objc_autoreleasePoolPop(pool);
                return None;
            }

            let cls_name = c"MHVCAppObserver";
            let mut obs_cls = objc_getClass(cls_name.as_ptr());
            if obs_cls.is_null() {
                obs_cls = objc_allocateClassPair(ns_obj_cls, cls_name.as_ptr(), 0);
                if obs_cls.is_null() {
                    objc_autoreleasePoolPop(pool);
                    return None;
                }
                let sel_launch = sel_registerName(c"onAppWillLaunch:".as_ptr());
                let sel_term = sel_registerName(c"onAppDidTerminate:".as_ptr());
                let _ = class_addMethod(
                    obs_cls,
                    sel_launch,
                    on_will_launch as *const c_void,
                    c"v@:@".as_ptr(),
                );
                let _ = class_addMethod(
                    obs_cls,
                    sel_term,
                    on_did_terminate as *const c_void,
                    c"v@:@".as_ptr(),
                );
                objc_registerClassPair(obs_cls);
            }

            let sel_alloc = sel_registerName(c"alloc".as_ptr());
            let sel_init = sel_registerName(c"init".as_ptr());
            let msg0: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(objc_msgSend as *const c_void);
            let observer = msg0(msg0(obs_cls, sel_alloc), sel_init);
            if observer.is_null() {
                objc_autoreleasePoolPop(pool);
                return None;
            }

            let sel_shared = sel_registerName(c"sharedWorkspace".as_ptr());
            let sel_nc = sel_registerName(c"notificationCenter".as_ptr());
            let ws = msg0(ws_cls, sel_shared);
            let nc = msg0(ws, sel_nc);
            if nc.is_null() {
                let sel_release = sel_registerName(c"release".as_ptr());
                let rel_fn: extern "C" fn(*mut c_void, *mut c_void) =
                    std::mem::transmute(objc_msgSend as *const c_void);
                rel_fn(observer, sel_release);
                objc_autoreleasePoolPop(pool);
                return None;
            }

            PREFIXES_PTR.store(prefixes.as_ptr() as usize, Ordering::Release);
            PREFIXES_LEN.store(prefixes.len(), Ordering::Release);
            CONTEXT_PTR.store(context as usize, Ordering::Release);
            CALLBACK_PTR.store(callback as usize, Ordering::Release);

            let sel_add = sel_registerName(c"addObserver:selector:name:object:".as_ptr());
            let add_fn: extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *const c_void,
                *mut c_void,
            ) = std::mem::transmute(objc_msgSend as *const c_void);

            let will_launch_name = CFStringCreateWithCString(
                std::ptr::null(),
                c"NSWorkspaceWillLaunchApplicationNotification".as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );
            let did_launch_name = CFStringCreateWithCString(
                std::ptr::null(),
                c"NSWorkspaceDidLaunchApplicationNotification".as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );
            let did_term_name = CFStringCreateWithCString(
                std::ptr::null(),
                c"NSWorkspaceDidTerminateApplicationNotification".as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            );

            let sel_launch = sel_registerName(c"onAppWillLaunch:".as_ptr());
            let sel_term = sel_registerName(c"onAppDidTerminate:".as_ptr());

            if !will_launch_name.is_null() {
                add_fn(
                    nc,
                    sel_add,
                    observer,
                    sel_launch,
                    will_launch_name,
                    std::ptr::null_mut(),
                );
                CFRelease(will_launch_name);
            }
            if !did_launch_name.is_null() {
                add_fn(
                    nc,
                    sel_add,
                    observer,
                    sel_launch,
                    did_launch_name,
                    std::ptr::null_mut(),
                );
                CFRelease(did_launch_name);
            }
            if !did_term_name.is_null() {
                add_fn(
                    nc,
                    sel_add,
                    observer,
                    sel_term,
                    did_term_name,
                    std::ptr::null_mut(),
                );
                CFRelease(did_term_name);
            }

            objc_autoreleasePoolPop(pool);
            Some(Self {
                observer,
                workspace_nc: nc,
            })
        }
    }
}

impl Drop for AppWatcher {
    fn drop(&mut self) {
        CALLBACK_PTR.store(0, Ordering::Release);
        CONTEXT_PTR.store(0, Ordering::Release);
        unsafe {
            let pool = objc_autoreleasePoolPush();
            let sel_remove = sel_registerName(c"removeObserver:".as_ptr());
            let rem_fn: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);
            rem_fn(self.workspace_nc, sel_remove, self.observer);

            let sel_release = sel_registerName(c"release".as_ptr());
            let rel_fn: extern "C" fn(*mut c_void, *mut c_void) =
                std::mem::transmute(objc_msgSend as *const c_void);
            rel_fn(self.observer, sel_release);
            objc_autoreleasePoolPop(pool);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_conflicting_bundle_id_prefixes_case_insensitively() {
        let prefixes = &["com.focusrite."];
        assert!(matches_any_prefix(
            "com.focusrite.FocusriteControl2",
            prefixes
        ));
        assert!(matches_any_prefix(
            "COM.FOCUSRITE.FocusriteControl",
            prefixes
        ));
        assert!(!matches_any_prefix("com.apple.logic10", prefixes));
        assert!(!matches_any_prefix("com.focusrite.FocusriteControl2", &[]));
    }
}
