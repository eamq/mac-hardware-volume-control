//! `mac-hardware-volume-control` background daemon and CLI.

mod config;
mod devices;
mod osd;
mod platform;
mod volume;

use coreaudio_sys::{AudioObjectID, AudioObjectPropertyAddress, OSStatus};
use devices::{default_registry, detect_connected_devices, HardwareDevice};
use osd::{detect_macos_version, select_backend, OsdBackend};
use platform::coreaudio;
use platform::iokit_hotplug;
use platform::media_keys::{
    decode_media_key_event, CGEventGetFlags, MediaKeyTap, K_CG_EVENT_SYS_DEFINED,
};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::env;
use std::ffi::c_void;
use std::process::ExitCode;
use std::ptr;
use std::sync::{mpsc, Arc, Mutex};
use volume::{VolumeAction, VolumeState};

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
    tap: Option<MediaKeyTap>,
}

impl DaemonState {
    fn refresh_activation(&mut self) {
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
    _proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    user_info: *mut c_void,
) -> *mut c_void {
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

fn print_help() {
    println!(
        "mac-hardware-volume-control {}
Native hardware volume key & OSD bridge for macOS

USAGE:
  mac-hardware-volume-control [OPTIONS]

OPTIONS:
  --detect           Detect connected supported USB hardware and exit (0 if found)
  --list-devices     List all registered hardware drivers
  --device <ID>      Override configured device ID (default from config.json)
  --status           Print active CoreAudio output, macOS version, and OSD backend
  --test-osd <STEP>  Trigger the native OSD HUD at step 0..=64 (0 = muted)
  --probe-usb        Probe Focusrite vendor USB interface access via IOKit
  --probe-fcp        Initialize FCP and read current Scarlett monitor volume/mute
  -v, --version      Print version information
  -h, --help         Print this help message",
        env!("CARGO_PKG_VERSION")
    );
}

fn run_detect() -> ExitCode {
    let registry = default_registry();
    let detected = detect_connected_devices(&registry);
    if detected.is_empty() {
        eprintln!("No supported USB hardware interfaces detected.");
        return ExitCode::from(1);
    }
    for (desc, usb) in detected {
        println!("{}\t{}\t{}", desc.id, desc.display_name, usb.model_name);
    }
    ExitCode::SUCCESS
}

fn run_list_devices() -> ExitCode {
    let registry = default_registry();
    for desc in registry.list() {
        println!("{}\t{}", desc.id, desc.display_name);
    }
    ExitCode::SUCCESS
}

fn run_status(device_override: Option<&str>) -> ExitCode {
    let registry = default_registry();
    let cfg = config::default_config_path()
        .map(|p| config::load_config(&p))
        .unwrap_or_default();
    let device_id = device_override.unwrap_or(&cfg.device_id);
    let os_ver = detect_macos_version();
    let osd = select_backend(os_ver);
    let active_out =
        coreaudio::default_output_device_name().unwrap_or_else(|| "<unknown>".to_string());
    let detected = detect_connected_devices(&registry);

    println!("Configured device: {device_id}");
    println!("macOS version:     {os_ver}");
    println!("OSD backend:       {}", osd.name());
    println!("Default output:    {active_out}");
    if detected.is_empty() {
        println!("USB hardware:      none detected");
    } else {
        for (desc, usb) in detected {
            println!(
                "USB hardware:      {} ({})",
                desc.display_name, usb.model_name
            );
        }
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut device_override: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "-v" | "--version" => {
                println!("mac-hardware-volume-control {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--detect" => return run_detect(),
            "--list-devices" => return run_list_devices(),
            "--status" => return run_status(device_override.as_deref()),
            "--test-osd" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    eprintln!("Error: --test-osd requires a step (0..=64)");
                    return ExitCode::from(2);
                };
                let Ok(step) = val.parse::<u8>() else {
                    eprintln!("Error: invalid step '{val}' (expected 0..=64)");
                    return ExitCode::from(2);
                };
                let osd = select_backend(detect_macos_version());
                let state = VolumeState::from_step(step, step == 0);
                if let Err(err) = osd.show_volume(state) {
                    eprintln!("OSD error ({}): {err}", osd.name());
                    return ExitCode::from(1);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                println!("Triggered {} HUD: {state}", osd.name());
                return ExitCode::SUCCESS;
            }
            "--probe-usb" => {
                if let Err(err) = devices::iokit_usb::probe_vendor_usb_interfaces(
                    devices::scarlett_gen4::FOCUSRITE_VENDOR_ID,
                ) {
                    eprintln!("USB probe error: {err}");
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }
            "--probe-fcp" => {
                match devices::fcp::FcpSession::open(devices::scarlett_gen4::FOCUSRITE_VENDOR_ID) {
                    Ok(mut session) => match session.read_monitor_state() {
                        Ok((db, muted)) => {
                            println!(
                                "FCP connected! masterVolume={db} dB, muteSwitch={muted} (offsets: vol={}, mute={})",
                                session.controls.master_volume.offset,
                                session.controls.mute_switch.offset
                            );
                            return ExitCode::SUCCESS;
                        }
                        Err(err) => {
                            eprintln!("FCP read error: {err}");
                            return ExitCode::from(1);
                        }
                    },
                    Err(err) => {
                        eprintln!("FCP open error: {err}");
                        return ExitCode::from(1);
                    }
                }
            }
            "--device" => {
                i += 1;
                if let Some(val) = args.get(i) {
                    device_override = Some(val.clone());
                } else {
                    eprintln!("Error: --device requires an argument");
                    return ExitCode::from(2);
                }
            }
            other => {
                eprintln!("Unknown option: {other}");
                print_help();
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let registry = default_registry();
    let cfg = config::default_config_path()
        .map(|p| config::load_config(&p))
        .unwrap_or_default();
    let target_id = device_override.as_deref().unwrap_or(&cfg.device_id);

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
    drop(_ca_listener);
    drop(_usb_watcher);
    unsafe { drop(Box::from_raw(ctx_ptr)) };
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use devices::{DeviceDescriptor, DeviceError};

    struct NoopOsd;
    impl OsdBackend for NoopOsd {
        fn name(&self) -> &'static str {
            "noop-test"
        }
        fn supports_version(&self, _version: osd::MacOsVersion) -> bool {
            true
        }
        fn show_volume(&self, _state: VolumeState) -> Result<(), osd::OsdError> {
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
}
