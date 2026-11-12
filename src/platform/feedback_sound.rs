//! Native macOS volume-change feedback sound (`volume.aiff`), respecting
//! System Settings -> Sound -> "Play feedback when volume is changed"
//! (`NSGlobalDomain` / `com.apple.sound.beep.feedback`).
//!
//! Uses `AudioToolbox` System Sound Services (`AudioServicesPlaySystemSound`)
//! rather than `NSSound` / `AVAudioEngine` so it never triggers microphone
//! privacy prompts.

use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

const VOLUME_AIFF_PATH: &str =
    "/System/Library/LoginPlugins/BezelServices.loginPlugin/Contents/Resources/volume.aiff";

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioServicesCreateSystemSoundID(
        in_file_url: *const c_void,
        out_system_sound_id: *mut u32,
    ) -> i32;
    fn AudioServicesPlaySystemSound(in_system_sound_id: u32);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFURLCreateFromFileSystemRepresentation(
        alloc: *const c_void,
        buffer: *const u8,
        buf_len: isize,
        is_directory: bool,
    ) -> *const c_void;
    fn CFPreferencesCopyValue(
        key: *const c_void,
        application_id: *const c_void,
        user_name: *const c_void,
        host_name: *const c_void,
    ) -> *const c_void;
    fn CFGetTypeID(cf: *const c_void) -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(boolean: *const c_void) -> bool;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: *const c_void, the_type: isize, value_ptr: *mut i32) -> bool;
    fn CFRelease(cf: *const c_void);
    static kCFPreferencesAnyApplication: *const c_void;
    static kCFPreferencesCurrentUser: *const c_void;
    static kCFPreferencesAnyHost: *const c_void;
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32_TYPE: isize = 3;

static VOLUME_SOUND_ID: OnceLock<Option<u32>> = OnceLock::new();

fn get_volume_sound_id() -> Option<u32> {
    *VOLUME_SOUND_ID.get_or_init(|| unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            VOLUME_AIFF_PATH.as_ptr(),
            VOLUME_AIFF_PATH.len() as isize,
            false,
        );
        if url.is_null() {
            return None;
        }
        let mut sound_id: u32 = 0;
        let status = AudioServicesCreateSystemSoundID(url, &mut sound_id);
        CFRelease(url);
        if status == 0 {
            Some(sound_id)
        } else {
            eprintln!("AudioServicesCreateSystemSoundID failed: {status}");
            None
        }
    })
}

/// Returns `true` if `com.apple.sound.beep.feedback` is enabled (`1`/`true`) in `.GlobalPreferences`.
#[must_use]
pub fn is_feedback_enabled() -> bool {
    unsafe {
        let Ok(key_c) = CString::new("com.apple.sound.beep.feedback") else {
            return false;
        };
        let key =
            CFStringCreateWithCString(std::ptr::null(), key_c.as_ptr(), K_CF_STRING_ENCODING_UTF8);
        if key.is_null() {
            return false;
        }
        let val = CFPreferencesCopyValue(
            key,
            kCFPreferencesAnyApplication,
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        );
        CFRelease(key);
        if val.is_null() {
            return false;
        }

        let tid = CFGetTypeID(val);
        let enabled = if tid == CFBooleanGetTypeID() {
            CFBooleanGetValue(val)
        } else if tid == CFNumberGetTypeID() {
            let mut num: i32 = 0;
            CFNumberGetValue(val, K_CF_NUMBER_SINT32_TYPE, &mut num) && num != 0
        } else {
            false
        };
        CFRelease(val);
        enabled
    }
}

/// Plays the native volume pop sound if feedback is enabled in System Settings.
pub fn play_if_enabled() {
    if !is_feedback_enabled() {
        return;
    }
    if let Some(sound_id) = get_volume_sound_id() {
        unsafe {
            AudioServicesPlaySystemSound(sound_id);
        }
    }
}
