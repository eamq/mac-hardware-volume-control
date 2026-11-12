//! CoreAudio default output device query and zero-polling property listener.

use coreaudio_sys::{
    kAudioDevicePropertyDeviceNameCFString, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectRemovePropertyListener, OSStatus,
};
use std::ffi::{c_char, c_void};
use std::ptr;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(cf: *const c_void);
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

/// Returns the current macOS CoreAudio Default Output Device name, if available.
#[must_use]
pub fn default_output_device_name() -> Option<String> {
    let mut device_id: AudioObjectID = 0;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &addr,
            0,
            ptr::null(),
            &mut size,
            (&mut device_id as *mut AudioObjectID).cast::<c_void>(),
        )
    };
    if status != 0 || device_id == 0 {
        return None;
    }

    let mut cf_name: *const c_void = ptr::null();
    let mut name_size = std::mem::size_of::<*const c_void>() as u32;
    let name_addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceNameCFString,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };

    let name_status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &name_addr,
            0,
            ptr::null(),
            &mut name_size,
            (&mut cf_name as *mut *const c_void).cast::<c_void>(),
        )
    };
    if name_status != 0 || cf_name.is_null() {
        return None;
    }

    let mut buf = [0i8; 256];
    let ok = unsafe {
        CFStringGetCString(
            cf_name,
            buf.as_mut_ptr(),
            buf.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    unsafe { CFRelease(cf_name) };

    if ok == 0 {
        return None;
    }

    let c_str = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
    Some(c_str.to_string_lossy().into_owned())
}

pub type DefaultDeviceCallback = unsafe extern "C" fn(
    AudioObjectID,
    u32,
    *const AudioObjectPropertyAddress,
    *mut c_void,
) -> OSStatus;

/// Registers a CoreAudio property listener for default output device changes.
pub struct DefaultDeviceListener {
    callback: DefaultDeviceCallback,
    context: *mut c_void,
}

impl DefaultDeviceListener {
    /// Installs the listener on `kAudioObjectSystemObject`.
    pub fn register(
        callback: DefaultDeviceCallback,
        context: *mut c_void,
    ) -> Result<Self, OSStatus> {
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let status = unsafe {
            AudioObjectAddPropertyListener(kAudioObjectSystemObject, &addr, Some(callback), context)
        };
        if status == 0 {
            Ok(Self { callback, context })
        } else {
            Err(status)
        }
    }
}

impl Drop for DefaultDeviceListener {
    fn drop(&mut self) {
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        unsafe {
            let _ = AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject,
                &addr,
                Some(self.callback),
                self.context,
            );
        }
    }
}
