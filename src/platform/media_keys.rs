//! CGEventTap media-key interceptor (`F11` / `F12` / `Mute` + `Option+Shift`).
//!
//! Uses only Accessibility permission (`CGEventTap`) — zero microphone or audio
//! capture permissions required.

use crate::volume::VolumeAction;
use std::ffi::c_void;
use std::ptr;

/// `NX_SYSDEFINED` event type (`14`) carrying media-key `NSSystemDefined` payloads.
pub const K_CG_EVENT_SYS_DEFINED: u32 = 14;
/// Subtype `8` indicates an `NX_SUBTYPE_AUX_CONTROL_BUTTONS` media key event.
pub const NX_SUBTYPE_AUX_CONTROL_BUTTONS: i16 = 8;

pub const NX_KEYTYPE_SOUND_UP: u32 = 0;
pub const NX_KEYTYPE_SOUND_DOWN: u32 = 1;
pub const NX_KEYTYPE_MUTE: u32 = 7;

const K_CG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
const K_CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;

/// Decodes a macOS `NSSystemDefined` `data1` integer and event flags into a [`VolumeAction`].
///
/// Only triggers on key-down (`key_state == 0x0A`) so key-up does not double-step.
#[must_use]
pub fn decode_media_key_event(subtype: i16, data1: i64, flags: u64) -> Option<VolumeAction> {
    if subtype != NX_SUBTYPE_AUX_CONTROL_BUTTONS {
        return None;
    }

    let raw = data1 as u32;
    let key_code = (raw >> 16) & 0xFFFF;
    let key_flags = raw & 0xFFFF;
    let key_state = (key_flags & 0xFF00) >> 8;

    // 0x0A = key down, 0x0B = key up
    if key_state != 0x0A {
        return None;
    }

    let fine =
        (flags & K_CG_EVENT_FLAG_MASK_SHIFT) != 0 && (flags & K_CG_EVENT_FLAG_MASK_ALTERNATE) != 0;

    match key_code {
        NX_KEYTYPE_SOUND_UP => Some(VolumeAction::Up { fine }),
        NX_KEYTYPE_SOUND_DOWN => Some(VolumeAction::Down { fine }),
        NX_KEYTYPE_MUTE => Some(VolumeAction::ToggleMute),
        _ => None,
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: MediaKeyCallback,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    pub fn CGEventGetFlags(event: *mut c_void) -> u64;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
}

/// Checks whether Accessibility permission is granted, optionally showing the
/// native macOS system prompt.
#[must_use]
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        if !prompt {
            return AXIsProcessTrustedWithOptions(ptr::null());
        }
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let opts = CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const c_void,
        );
        let trusted = AXIsProcessTrustedWithOptions(opts);
        if !opts.is_null() {
            CFRelease(opts);
        }
        trusted
    }
}

pub type MediaKeyCallback =
    unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

pub struct MediaKeyTap {
    tap: *mut c_void,
    source: *mut c_void,
}

unsafe impl Send for MediaKeyTap {}

impl MediaKeyTap {
    /// Creates and attaches a `CGEventTap` for `NX_SYSDEFINED` events.
    pub fn install(callback: MediaKeyCallback, user_info: *mut c_void) -> Option<Self> {
        unsafe {
            let mask: u64 = 1 << K_CG_EVENT_SYS_DEFINED;
            // kCGHIDEventTap = 0, kCGHeadInsertEventTap = 0, kCGEventTapOptionDefault = 0
            let tap = CGEventTapCreate(0, 0, 0, mask, callback, user_info);
            if tap.is_null() {
                return None;
            }
            let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
            if source.is_null() {
                CFRelease(tap);
                return None;
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, false);
            Some(Self { tap, source })
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        unsafe {
            CGEventTapEnable(self.tap, enabled);
        }
    }
}

impl Drop for MediaKeyTap {
    fn drop(&mut self) {
        unsafe {
            CGEventTapEnable(self.tap, false);
            CFRelease(self.source);
            CFRelease(self.tap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_data1(key_code: u32, key_down: bool) -> i64 {
        let state: u32 = if key_down { 0x0A } else { 0x0B };
        i64::from((key_code << 16) | (state << 8))
    }

    #[test]
    fn decodes_standard_and_micro_step_keys() {
        let up = decode_media_key_event(
            NX_SUBTYPE_AUX_CONTROL_BUTTONS,
            make_data1(NX_KEYTYPE_SOUND_UP, true),
            0,
        );
        assert_eq!(up, Some(VolumeAction::Up { fine: false }));

        let fine_down = decode_media_key_event(
            NX_SUBTYPE_AUX_CONTROL_BUTTONS,
            make_data1(NX_KEYTYPE_SOUND_DOWN, true),
            K_CG_EVENT_FLAG_MASK_SHIFT | K_CG_EVENT_FLAG_MASK_ALTERNATE,
        );
        assert_eq!(fine_down, Some(VolumeAction::Down { fine: true }));

        let mute = decode_media_key_event(
            NX_SUBTYPE_AUX_CONTROL_BUTTONS,
            make_data1(NX_KEYTYPE_MUTE, true),
            0,
        );
        assert_eq!(mute, Some(VolumeAction::ToggleMute));
    }

    #[test]
    fn ignores_key_up_and_non_media_subtypes() {
        assert_eq!(
            decode_media_key_event(
                NX_SUBTYPE_AUX_CONTROL_BUTTONS,
                make_data1(NX_KEYTYPE_SOUND_UP, false),
                0
            ),
            None
        );
        assert_eq!(
            decode_media_key_event(0, make_data1(NX_KEYTYPE_SOUND_UP, true), 0),
            None
        );
    }
}
