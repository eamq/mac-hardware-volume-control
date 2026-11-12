//! IOKit USB hotplug notification listener (0.0% CPU when idle).

use std::ffi::{c_char, c_void, CString};

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
    fn IONotificationPortGetRunLoopSource(notify: *mut c_void) -> *mut c_void;
    fn IONotificationPortDestroy(notify: *mut c_void);
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceAddMatchingNotification(
        notify_port: *mut c_void,
        notification_type: *const c_char,
        matching: *mut c_void,
        callback: extern "C" fn(*mut c_void, u32),
        ref_con: *mut c_void,
        notification: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    static kCFRunLoopDefaultMode: *const c_void;
}

const K_IO_FIRST_MATCH: &[u8] = b"IOServiceFirstMatch\0";
const K_IO_TERMINATED: &[u8] = b"IOServiceTerminate\0";

/// Drains an `io_iterator_t` so IOKit arms future notifications.
pub unsafe fn drain_iterator(iterator: u32) {
    loop {
        let service = IOIteratorNext(iterator);
        if service == 0 {
            break;
        }
        IOObjectRelease(service);
    }
}

/// Keeps the IOKit notification port and iterators alive for the run loop lifetime.
pub struct UsbHotplugWatcher {
    port: *mut c_void,
    matched_iter: u32,
    terminated_iter: u32,
}

impl UsbHotplugWatcher {
    /// Attaches USB attach/detach notifications to the current `CFRunLoop`.
    pub fn register(
        on_attach: extern "C" fn(*mut c_void, u32),
        on_detach: extern "C" fn(*mut c_void, u32),
        context: *mut c_void,
    ) -> Option<Self> {
        unsafe {
            let port = IONotificationPortCreate(0);
            if port.is_null() {
                return None;
            }
            let source = IONotificationPortGetRunLoopSource(port);
            if source.is_null() {
                IONotificationPortDestroy(port);
                return None;
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);

            let class_name = CString::new("IOUSBHostDevice").ok()?;
            let mut matched_iter: u32 = 0;
            let mut terminated_iter: u32 = 0;

            let matching_add = IOServiceMatching(class_name.as_ptr());
            if !matching_add.is_null() {
                let kr = IOServiceAddMatchingNotification(
                    port,
                    K_IO_FIRST_MATCH.as_ptr().cast::<c_char>(),
                    matching_add,
                    on_attach,
                    context,
                    &mut matched_iter,
                );
                if kr == 0 && matched_iter != 0 {
                    drain_iterator(matched_iter);
                }
            }

            let matching_rem = IOServiceMatching(class_name.as_ptr());
            if !matching_rem.is_null() {
                let kr = IOServiceAddMatchingNotification(
                    port,
                    K_IO_TERMINATED.as_ptr().cast::<c_char>(),
                    matching_rem,
                    on_detach,
                    context,
                    &mut terminated_iter,
                );
                if kr == 0 && terminated_iter != 0 {
                    drain_iterator(terminated_iter);
                }
            }

            Some(Self {
                port,
                matched_iter,
                terminated_iter,
            })
        }
    }
}

impl Drop for UsbHotplugWatcher {
    fn drop(&mut self) {
        unsafe {
            if self.matched_iter != 0 {
                IOObjectRelease(self.matched_iter);
            }
            if self.terminated_iter != 0 {
                IOObjectRelease(self.terminated_iter);
            }
            if !self.port.is_null() {
                IONotificationPortDestroy(self.port);
            }
        }
    }
}
