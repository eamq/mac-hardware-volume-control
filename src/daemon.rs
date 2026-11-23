//! Background `CFRunLoop` daemon coordinating CoreAudio, USB hotplug,
//! conflicting-app handoff, and media-key events.

use crate::config;
use crate::devices::{default_registry, HardwareDevice};
use crate::osd::{detect_macos_version, select_backend, OsdBackend};
use crate::platform;
use crate::platform::app_watcher::{self, AppEventKind};
use crate::platform::coreaudio;
use crate::platform::iokit_hotplug;
use crate::platform::media_keys::{
    decode_media_key_event, reenable_tap, CGEventGetFlags, MediaKeyTap, K_CG_EVENT_SYS_DEFINED,
    K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT,
};
use crate::volume::{VolumeAction, VolumeState};
use coreaudio_sys::{AudioObjectID, AudioObjectPropertyAddress, OSStatus};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::ffi::c_void;
use std::process::ExitCode;
use std::ptr;
use std::sync::{mpsc, Arc, Mutex};

#[repr(C)]
struct CFRunLoopSourceContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
    hash: *const c_void,
    schedule: *const c_void,
    cancel: *const c_void,
    perform: extern "C" fn(*mut c_void),
}

#[repr(C)]
struct CFRunLoopTimerContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: *mut c_void);
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopWakeUp(rl: *mut c_void);
    fn CFRunLoopSourceCreate(
        allocator: *const c_void,
        order: isize,
        context: *mut CFRunLoopSourceContext,
    ) -> *mut c_void;
    fn CFRunLoopSourceSignal(source: *mut c_void);
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        fire_date: f64,
        interval: f64,
        flags: u64,
        order: isize,
        callout: extern "C" fn(*mut c_void, *mut c_void),
        context: *mut CFRunLoopTimerContext,
    ) -> *mut c_void;
    fn CFRunLoopAddTimer(rl: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> *mut c_void;
    fn sel_registerName(name: *const std::ffi::c_char) -> *mut c_void;
    fn objc_msgSend();
}

struct PendingUiUpdate {
    osd: Arc<dyn OsdBackend>,
    pending: Mutex<Option<(VolumeState, bool)>>,
}

extern "C" fn perform_ui_update(info: *mut c_void) {
    if info.is_null() {
        return;
    }
    let ui = unsafe { &*(info as *const PendingUiUpdate) };
    let item = ui.pending.lock().ok().and_then(|mut g| g.take());
    if let Some((state, play_sound)) = item {
        if let Err(err) = ui.osd.show_volume(state) {
            eprintln!("OSD error ({}): {err}", ui.osd.name());
        }
        if play_sound {
            platform::feedback_sound::play_if_enabled();
        }
    }
}

struct UiNotifier {
    main_rl: usize,
    source: usize,
    ui: Arc<PendingUiUpdate>,
}

impl UiNotifier {
    fn notify(&self, state: VolumeState, play_sound: bool) {
        if let Ok(mut g) = self.ui.pending.lock() {
            *g = Some((state, play_sound));
        }
        if self.source != 0 && self.main_rl != 0 {
            unsafe {
                CFRunLoopSourceSignal(self.source as *mut c_void);
                CFRunLoopWakeUp(self.main_rl as *mut c_void);
            }
        }
    }
}

struct DaemonState {
    device: Box<dyn HardwareDevice>,
    ui: UiNotifier,
    volume: VolumeState,
    last_hw: VolumeState,
    active: bool,
    conflicting_app_running: bool,
    check_conflicting_apps: fn(&[&str]) -> bool,
    tap: Option<MediaKeyTap>,
}

impl DaemonState {
    fn refresh_activation(&mut self) {
        let prefixes = self.device.conflicting_app_prefixes();
        if !prefixes.is_empty() && (self.check_conflicting_apps)(prefixes) {
            if self.active {
                self.device.disconnect();
            }
            self.conflicting_app_running = true;
            self.active = false;
            if let Some(tap) = &self.tap {
                tap.set_enabled(false);
            }
            return;
        }
        self.conflicting_app_running = false;

        let output_name = coreaudio::default_output_device_name();
        let is_target_output = output_name
            .as_deref()
            .is_some_and(|name| self.device.matches_coreaudio_name(name));

        if is_target_output {
            if !self.active {
                if self.device.connect().is_ok() {
                    if let Ok(hw_vol) = self.device.read_volume() {
                        self.volume = hw_vol;
                        self.last_hw = hw_vol;
                    }
                    self.active = true;
                } else {
                    self.device.disconnect();
                    self.active = false;
                }
            }
        } else {
            if self.active {
                self.device.disconnect();
            }
            self.active = false;
        }

        if let Some(tap) = &self.tap {
            tap.set_enabled(self.active);
        }
    }

    fn on_conflicting_app_launched(&mut self, bundle_id: &str) {
        if self.active {
            self.device.disconnect();
        }
        self.conflicting_app_running = true;
        self.active = false;
        if let Some(tap) = &self.tap {
            tap.set_enabled(false);
        }
        eprintln!("Released hardware interface for conflicting app: {bundle_id}");
    }

    fn on_conflicting_app_terminated(&mut self) {
        let prefixes = self.device.conflicting_app_prefixes();
        if !prefixes.is_empty() && (self.check_conflicting_apps)(prefixes) {
            return;
        }
        self.conflicting_app_running = false;
        self.refresh_activation();
        if self.active {
            eprintln!("Reclaimed hardware interface after conflicting app closed.");
        }
    }

    fn handle_action(&mut self, action: VolumeAction) {
        if !self.active {
            return;
        }
        match self.device.read_volume() {
            Ok(hw_vol) => {
                if hw_vol != self.last_hw {
                    self.volume = hw_vol;
                    self.last_hw = hw_vol;
                }
            }
            Err(err) => {
                eprintln!("Hardware read error: {err}");
                self.device.disconnect();
                self.active = false;
                if let Some(tap) = &self.tap {
                    tap.set_enabled(false);
                }
                return;
            }
        }
        let next = self.volume.apply(action);
        match self.device.write_volume(next) {
            Ok(()) => {
                self.volume = next;
                self.last_hw = self.device.read_volume().unwrap_or(next);
                self.ui.notify(next, !next.is_muted());
            }
            Err(err) => {
                eprintln!("Hardware write error: {err}");
                self.device.disconnect();
                self.active = false;
                if let Some(tap) = &self.tap {
                    tap.set_enabled(false);
                }
            }
        }
    }
}

struct SharedContext {
    state: Arc<Mutex<DaemonState>>,
    tx: mpsc::Sender<VolumeAction>,
}

unsafe extern "C" fn on_default_device_changed(
    _obj_id: AudioObjectID,
    _num_addresses: u32,
    _addresses: *const AudioObjectPropertyAddress,
    context: *mut c_void,
) -> OSStatus {
    if !context.is_null() {
        let ctx = &*(context as *const SharedContext);
        if let Ok(mut guard) = ctx.state.lock() {
            guard.refresh_activation();
        }
    }
    0
}

extern "C" fn on_usb_hotplug(context: *mut c_void, iterator: u32) {
    unsafe {
        iokit_hotplug::drain_iterator(iterator);
        if !context.is_null() {
            let ctx = &*(context as *const SharedContext);
            if let Ok(mut guard) = ctx.state.lock() {
                guard.refresh_activation();
            }
        }
    }
}

fn on_app_event(context: *mut c_void, kind: AppEventKind, bundle_id: &str) {
    if context.is_null() {
        return;
    }
    let ctx = unsafe { &*(context as *const SharedContext) };
    if let Ok(mut guard) = ctx.state.lock() {
        match kind {
            AppEventKind::WillLaunch => guard.on_conflicting_app_launched(bundle_id),
            AppEventKind::DidTerminate => guard.on_conflicting_app_terminated(),
        }
    }
}

/// Extracts `(subtype, data1)` from a `CGEventRef` via `[NSEvent eventWithCGEvent:]`.
unsafe fn extract_ns_event_fields(cg_event: *mut c_void) -> Option<(i16, i64)> {
    let cls_name = std::ffi::CString::new("NSEvent").ok()?;
    let cls = objc_getClass(cls_name.as_ptr());
    if cls.is_null() {
        return None;
    }

    let sel_event = sel_registerName(c"eventWithCGEvent:".as_ptr());
    let make_event: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const c_void);
    let ns_event = make_event(cls, sel_event, cg_event);
    if ns_event.is_null() {
        return None;
    }

    let sel_subtype = sel_registerName(c"subtype".as_ptr());
    let get_subtype: extern "C" fn(*mut c_void, *mut c_void) -> i16 =
        std::mem::transmute(objc_msgSend as *const c_void);
    let subtype = get_subtype(ns_event, sel_subtype);

    let sel_data1 = sel_registerName(c"data1".as_ptr());
    let get_data1: extern "C" fn(*mut c_void, *mut c_void) -> i64 =
        std::mem::transmute(objc_msgSend as *const c_void);
    let data1 = get_data1(ns_event, sel_data1);

    Some((subtype, data1))
}

unsafe extern "C" fn on_media_key_event(
    proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
    if event_type == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT {
        if !user_info.is_null() {
            let ctx = &*(user_info as *const SharedContext);
            if ctx.state.lock().is_ok_and(|g| g.active) {
                reenable_tap(proxy);
            }
        }
        return event;
    }

    if event_type != K_CG_EVENT_SYS_DEFINED || user_info.is_null() || event.is_null() {
        return event;
    }

    let Some((subtype, data1)) = extract_ns_event_fields(event) else {
        return event;
    };
    let flags = CGEventGetFlags(event);
    let Some(action) = decode_media_key_event(subtype, data1, flags) else {
        return event;
    };

    let ctx = &*(user_info as *const SharedContext);
    let is_active = ctx.state.lock().is_ok_and(|g| g.active);
    if is_active {
        let _ = ctx.tx.send(action);
        return ptr::null_mut(); // Consume event immediately without blocking WindowServer
    }

    event
}

pub fn run(device_override: Option<&str>) -> ExitCode {
    let registry = default_registry();
    let cfg = config::default_config_path()
        .map(|p| config::load_config(&p))
        .unwrap_or_default();
    let target_id = device_override.unwrap_or(&cfg.device_id);

    let Some(device) = registry.create_by_id(target_id) else {
        eprintln!("Error: unknown hardware device '{target_id}'");
        return ExitCode::from(1);
    };

    let os_ver = detect_macos_version();
    let osd: Arc<dyn OsdBackend> = Arc::from(select_backend(os_ver));
    let osd_name = osd.name();

    let main_rl = unsafe { CFRunLoopGetCurrent() };
    let pending_ui = Arc::new(PendingUiUpdate {
        osd,
        pending: Mutex::new(None),
    });
    let mut source_ctx = CFRunLoopSourceContext {
        version: 0,
        info: Arc::as_ptr(&pending_ui) as *mut c_void,
        retain: ptr::null(),
        release: ptr::null(),
        copy_description: ptr::null(),
        equal: ptr::null(),
        hash: ptr::null(),
        schedule: ptr::null(),
        cancel: ptr::null(),
        perform: perform_ui_update,
    };
    let ui_source = unsafe { CFRunLoopSourceCreate(ptr::null(), 0, &mut source_ctx) };
    unsafe {
        CFRunLoopAddSource(main_rl, ui_source, kCFRunLoopCommonModes);
    }

    let (tx, rx) = mpsc::channel::<VolumeAction>();
    let state = Arc::new(Mutex::new(DaemonState {
        device,
        ui: UiNotifier {
            main_rl: main_rl as usize,
            source: ui_source as usize,
            ui: Arc::clone(&pending_ui),
        },
        volume: VolumeState::default(),
        last_hw: VolumeState::default(),
        active: false,
        conflicting_app_running: false,
        check_conflicting_apps: app_watcher::is_any_app_running,
        tap: None,
    }));

    let worker_state = Arc::clone(&state);
    std::thread::spawn(move || {
        while let Ok(action) = rx.recv() {
            if let Ok(mut guard) = worker_state.lock() {
                guard.handle_action(action);
            }
        }
    });

    let ctx = Box::new(SharedContext {
        state: Arc::clone(&state),
        tx,
    });
    let ctx_ptr = Box::into_raw(ctx);

    let tap = MediaKeyTap::install(on_media_key_event, ctx_ptr.cast());
    let mut perm_timer: *mut c_void = ptr::null_mut();
    if tap.is_none() {
        let _ = platform::media_keys::is_accessibility_trusted(true);
        eprintln!(
            "Waiting for Accessibility permission for mac-hardware-volume-control (no restart required once granted)..."
        );
        extern "C" fn on_perm_timer(timer: *mut c_void, info: *mut c_void) {
            if info.is_null() {
                return;
            }
            if !platform::media_keys::is_accessibility_trusted(false) {
                return;
            }
            if let Some(new_tap) = MediaKeyTap::install(on_media_key_event, info) {
                let ctx = unsafe { &*(info as *const SharedContext) };
                if let Ok(mut guard) = ctx.state.lock() {
                    guard.tap = Some(new_tap);
                    guard.refresh_activation();
                }
                eprintln!("Accessibility granted; media-key tap active.");
                unsafe {
                    CFRunLoopTimerInvalidate(timer);
                    CFRelease(timer);
                }
            }
        }
        let mut timer_ctx = CFRunLoopTimerContext {
            version: 0,
            info: ctx_ptr.cast(),
            retain: ptr::null(),
            release: ptr::null(),
            copy_description: ptr::null(),
        };
        perm_timer = unsafe {
            CFRunLoopTimerCreate(
                ptr::null(),
                CFAbsoluteTimeGetCurrent() + 2.0,
                2.0,
                0,
                0,
                on_perm_timer,
                &mut timer_ctx,
            )
        };
        if !perm_timer.is_null() {
            unsafe {
                CFRunLoopAddTimer(main_rl, perm_timer, kCFRunLoopCommonModes);
            }
        }
    }
    let _ = perm_timer;

    {
        if let Ok(mut guard) = state.lock() {
            guard.tap = tap;
            guard.refresh_activation();
            eprintln!(
                "Daemon ready: active={}, volume={}, osd={}, feedback={}",
                guard.active,
                guard.volume,
                osd_name,
                platform::feedback_sound::is_feedback_enabled()
            );
        }
    }

    let app_prefixes = state
        .lock()
        .map(|g| g.device.conflicting_app_prefixes())
        .unwrap_or(&[]);
    let _app_watcher =
        app_watcher::AppWatcher::register(app_prefixes, on_app_event, ctx_ptr.cast());

    let _ca_listener =
        coreaudio::DefaultDeviceListener::register(on_default_device_changed, ctx_ptr.cast());
    let _usb_watcher =
        iokit_hotplug::UsbHotplugWatcher::register(on_usb_hotplug, on_usb_hotplug, ctx_ptr.cast());

    let run_loop = unsafe { CFRunLoopGetCurrent() } as usize;
    if let Ok(mut signals) = Signals::new([SIGINT, SIGTERM]) {
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                unsafe { CFRunLoopStop(run_loop as *mut c_void) };
            }
        });
    }

    unsafe { CFRunLoopRun() };
    drop(_app_watcher);
    drop(_ca_listener);
    drop(_usb_watcher);
    unsafe { drop(Box::from_raw(ctx_ptr)) };
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{DeviceDescriptor, DeviceError};
    use crate::osd::MacOsVersion;

    struct NoopOsd;
    impl OsdBackend for NoopOsd {
        fn name(&self) -> &'static str {
            "noop-test"
        }
        fn supports_version(&self, _version: MacOsVersion) -> bool {
            true
        }
        fn show_volume(&self, _state: VolumeState) -> Result<(), crate::osd::OsdError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct MockDevice {
        hw_vol: Arc<Mutex<VolumeState>>,
        quantize_top: bool,
        fail_write: Arc<Mutex<bool>>,
        disconnected: Arc<Mutex<bool>>,
    }

    impl HardwareDevice for MockDevice {
        fn descriptor(&self) -> DeviceDescriptor {
            DeviceDescriptor {
                id: "mock",
                display_name: "Mock",
                coreaudio_name_hints: &["Mock"],
                usb_ids: &[],
            }
        }

        fn connect(&mut self) -> Result<(), DeviceError> {
            Ok(())
        }

        fn disconnect(&mut self) {
            *self.disconnected.lock().unwrap() = true;
        }

        fn read_volume(&mut self) -> Result<VolumeState, DeviceError> {
            let v = *self.hw_vol.lock().unwrap();
            if self.quantize_top && v.step() >= 61 {
                Ok(VolumeState::from_step(64, v.is_muted()))
            } else {
                Ok(v)
            }
        }

        fn write_volume(&mut self, state: VolumeState) -> Result<(), DeviceError> {
            if *self.fail_write.lock().unwrap() {
                return Err(DeviceError::Io("mock write failure".to_string()));
            }
            *self.hw_vol.lock().unwrap() = state;
            Ok(())
        }

        fn conflicting_app_prefixes(&self) -> &'static [&'static str] {
            &["com.focusrite."]
        }
    }

    fn make_test_state(initial: VolumeState, quantize_top: bool) -> (DaemonState, MockDevice) {
        let mock = MockDevice {
            hw_vol: Arc::new(Mutex::new(initial)),
            quantize_top,
            fail_write: Arc::new(Mutex::new(false)),
            disconnected: Arc::new(Mutex::new(false)),
        };
        let pending_ui = Arc::new(PendingUiUpdate {
            osd: Arc::new(NoopOsd),
            pending: Mutex::new(None),
        });
        let state = DaemonState {
            device: Box::new(mock.clone()),
            ui: UiNotifier {
                main_rl: 0,
                source: 0,
                ui: pending_ui,
            },
            volume: initial,
            last_hw: initial,
            active: true,
            conflicting_app_running: false,
            check_conflicting_apps: |_| false,
            tap: None,
        };
        (state, mock)
    }

    #[test]
    fn syncs_external_knob_turn_before_stepping() {
        let (mut state, mock) = make_test_state(VolumeState::from_step(32, false), false);
        *mock.hw_vol.lock().unwrap() = VolumeState::from_step(16, false);

        state.handle_action(VolumeAction::Up { fine: false });
        assert_eq!(state.volume.step(), 20);
        assert_eq!(mock.hw_vol.lock().unwrap().step(), 20);
    }

    #[test]
    fn preserves_micro_step_when_hardware_quantizes_top_range() {
        let (mut state, _mock) = make_test_state(VolumeState::from_step(64, false), true);

        state.handle_action(VolumeAction::Down { fine: true });
        assert_eq!(state.volume.step(), 63);
        assert_eq!(state.last_hw.step(), 64);

        state.handle_action(VolumeAction::Down { fine: true });
        assert_eq!(state.volume.step(), 62);
    }

    #[test]
    fn disconnects_and_deactivates_on_write_error() {
        let (mut state, mock) = make_test_state(VolumeState::from_step(32, false), false);
        *mock.fail_write.lock().unwrap() = true;

        state.handle_action(VolumeAction::Up { fine: false });
        assert!(!state.active);
        assert!(*mock.disconnected.lock().unwrap());
    }

    #[test]
    fn releases_usb_when_conflicting_app_launches_and_tracks_state() {
        let (mut state, mock) = make_test_state(VolumeState::from_step(32, false), false);
        assert!(state.active);

        state.on_conflicting_app_launched("com.focusrite.FocusriteControl2");
        assert!(!state.active);
        assert!(state.conflicting_app_running);
        assert!(*mock.disconnected.lock().unwrap());

        state.handle_action(VolumeAction::Up { fine: false });
        assert_eq!(mock.hw_vol.lock().unwrap().step(), 32);
    }

    #[test]
    fn stays_inactive_while_conflicting_app_is_running() {
        let (mut state, _mock) = make_test_state(VolumeState::from_step(32, false), false);
        state.check_conflicting_apps = |_| true;
        state.refresh_activation();
        assert!(!state.active);
        assert!(state.conflicting_app_running);

        state.on_conflicting_app_terminated();
        assert!(!state.active);
        assert!(state.conflicting_app_running);
    }
}
