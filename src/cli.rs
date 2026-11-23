//! Command-line parsing and one-shot administrative subcommands.

use crate::config;
use crate::devices::{self, default_registry, detect_connected_devices};
use crate::osd::{detect_macos_version, select_backend};
use crate::platform::coreaudio;
use crate::volume::VolumeState;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const SERVICE_LABEL: &str = "com.user.mac-hardware-volume-control";

pub enum CliOutcome {
    Exit(ExitCode),
    RunDaemon { device_override: Option<String> },
}

enum CliAction {
    Help,
    Version,
    Start,
    Stop,
    Detect,
    ListDevices,
    Status,
    TestOsd(u8),
    ProbeFocusriteUsb,
    ProbeFcp,
    Daemon,
}

pub fn run() -> CliOutcome {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_args(&args) {
        Ok((CliAction::Help, _)) => {
            print_help();
            CliOutcome::Exit(ExitCode::SUCCESS)
        }
        Ok((CliAction::Version, _)) => {
            println!("mac-hardware-volume-control {}", env!("CARGO_PKG_VERSION"));
            CliOutcome::Exit(ExitCode::SUCCESS)
        }
        Ok((CliAction::Start, _)) => CliOutcome::Exit(run_start()),
        Ok((CliAction::Stop, _)) => CliOutcome::Exit(run_stop()),
        Ok((CliAction::Detect, _)) => CliOutcome::Exit(run_detect()),
        Ok((CliAction::ListDevices, _)) => CliOutcome::Exit(run_list_devices()),
        Ok((CliAction::Status, dev)) => CliOutcome::Exit(run_status(dev.as_deref())),
        Ok((CliAction::TestOsd(step), _)) => CliOutcome::Exit(run_test_osd(step)),
        Ok((CliAction::ProbeFocusriteUsb, _)) => CliOutcome::Exit(run_probe_focusrite_usb()),
        Ok((CliAction::ProbeFcp, _)) => CliOutcome::Exit(run_probe_fcp()),
        Ok((CliAction::Daemon, device_override)) => CliOutcome::RunDaemon { device_override },
        Err((msg, show_help)) => {
            eprintln!("{msg}");
            if show_help {
                print_help();
            }
            CliOutcome::Exit(ExitCode::from(2))
        }
    }
}

fn parse_args(args: &[String]) -> Result<(CliAction, Option<String>), (String, bool)> {
    let mut device_override: Option<String> = None;
    let mut action: Option<CliAction> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => return Ok((CliAction::Help, device_override)),
            "-v" | "--version" => return Ok((CliAction::Version, device_override)),
            "--start" => action = Some(CliAction::Start),
            "--stop" => action = Some(CliAction::Stop),
            "--detect" => action = Some(CliAction::Detect),
            "--list-devices" => action = Some(CliAction::ListDevices),
            "--status" => action = Some(CliAction::Status),
            "--test-osd" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    return Err((
                        "Error: --test-osd requires a step (0-64)".to_string(),
                        false,
                    ));
                };
                let Ok(step) = val.parse::<u8>() else {
                    return Err((
                        format!("Error: invalid step '{val}' (expected 0-64)"),
                        false,
                    ));
                };
                action = Some(CliAction::TestOsd(step));
            }
            "--probe-focusrite-usb" => action = Some(CliAction::ProbeFocusriteUsb),
            "--probe-fcp" => action = Some(CliAction::ProbeFcp),
            "--device" => {
                i += 1;
                if let Some(val) = args.get(i) {
                    device_override = Some(val.clone());
                } else {
                    return Err(("Error: --device requires an argument".to_string(), false));
                }
            }
            other => return Err((format!("Unknown option: {other}"), true)),
        }
        i += 1;
    }

    Ok((action.unwrap_or(CliAction::Daemon), device_override))
}

fn run_test_osd(step: u8) -> ExitCode {
    let osd = select_backend(detect_macos_version());
    let state = VolumeState::from_step(step, step == 0);
    if let Err(err) = osd.show_volume(state) {
        eprintln!("OSD error ({}): {err}", osd.name());
        return ExitCode::from(1);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    println!("Triggered {} HUD: {state}", osd.name());
    ExitCode::SUCCESS
}

fn run_probe_focusrite_usb() -> ExitCode {
    if let Err(err) =
        devices::iokit_usb::probe_vendor_usb_interfaces(devices::scarlett_gen4::FOCUSRITE_VENDOR_ID)
    {
        eprintln!("USB probe error: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run_probe_fcp() -> ExitCode {
    match devices::fcp::FcpSession::open(devices::scarlett_gen4::FOCUSRITE_VENDOR_ID) {
        Ok(mut session) => match session.read_monitor_state() {
            Ok((db, muted)) => {
                println!(
                    "FCP connected! masterVolume={db} dB, muteSwitch={muted} (offsets: vol={}, mute={})",
                    session.controls.master_volume.offset,
                    session.controls.mute_switch.offset
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("FCP read error: {err}");
                ExitCode::from(1)
            }
        },
        Err(err) => {
            eprintln!("FCP open error: {err}");
            ExitCode::from(1)
        }
    }
}

fn print_help() {
    println!(
        "mac-hardware-volume-control {}
Native hardware volume key & OSD bridge for macOS

USAGE:
  mac-hardware-volume-control [OPTIONS]

OPTIONS:
  --start                  Start the LaunchAgent daemon via launchctl bootstrap
  --stop                   Stop the LaunchAgent daemon via launchctl bootout
  --detect                 Detect connected supported USB hardware and exit (0 if found)
  --list-devices           List all registered hardware drivers
  --device <ID>            Override configured device ID (default from config.json)
  --status                 Print active CoreAudio output, macOS version, and OSD backend
  --test-osd <STEP>        Trigger the native OSD HUD at step 0-64 (0 = muted)
  --probe-focusrite-usb    Probe Focusrite vendor USB interface access via IOKit
  --probe-fcp              Initialize FCP and read current Scarlett monitor volume/mute
  -v, --version            Print version information
  -h, --help               Print this help message",
        env!("CARGO_PKG_VERSION")
    );
}

fn service_target(uid: u32) -> String {
    format!("gui/{uid}/{SERVICE_LABEL}")
}

fn gui_domain(uid: u32) -> String {
    format!("gui/{uid}")
}

fn launch_agent_plist_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist"))
}

fn is_service_loaded(target: &str) -> bool {
    Command::new("launchctl")
        .args(["print", target])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn run_start() -> ExitCode {
    let Some(home) = env::var_os("HOME") else {
        eprintln!("Error: HOME environment variable is not set.");
        return ExitCode::from(1);
    };
    let plist = launch_agent_plist_path(Path::new(&home));
    if !plist.exists() {
        eprintln!(
            "Error: LaunchAgent plist not found at {}. Run ./scripts/install.sh first.",
            plist.display()
        );
        return ExitCode::from(1);
    }

    let uid = unsafe { libc::getuid() };
    let target = service_target(uid);
    if is_service_loaded(&target) {
        println!("Service {SERVICE_LABEL} is already running.");
        return ExitCode::SUCCESS;
    }

    let domain = gui_domain(uid);
    match Command::new("launchctl")
        .arg("bootstrap")
        .arg(&domain)
        .arg(&plist)
        .status()
    {
        Ok(status) if status.success() => {
            println!("Started {SERVICE_LABEL}.");
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!("Error: launchctl bootstrap exited with {status}.");
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("Error running launchctl: {err}");
            ExitCode::from(1)
        }
    }
}

fn run_stop() -> ExitCode {
    let uid = unsafe { libc::getuid() };
    let target = service_target(uid);
    if !is_service_loaded(&target) {
        println!("Service {SERVICE_LABEL} is already stopped.");
        return ExitCode::SUCCESS;
    }

    match Command::new("launchctl")
        .args(["bootout", &target])
        .status()
    {
        Ok(status) if status.success() => {
            println!("Stopped {SERVICE_LABEL}.");
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!("Error: launchctl bootout exited with {status}.");
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("Error running launchctl: {err}");
            ExitCode::from(1)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_launchd_targets_and_plist_path() {
        assert_eq!(gui_domain(501), "gui/501");
        assert_eq!(
            service_target(501),
            "gui/501/com.user.mac-hardware-volume-control"
        );
        assert_eq!(
            launch_agent_plist_path(Path::new("/Users/test")),
            PathBuf::from(
                "/Users/test/Library/LaunchAgents/com.user.mac-hardware-volume-control.plist"
            )
        );
    }

    #[test]
    fn parses_device_override_regardless_of_flag_order() {
        let args = vec![
            "--status".to_string(),
            "--device".to_string(),
            "scarlett-gen4".to_string(),
        ];
        let (action, dev) = parse_args(&args).expect("should parse --status --device");
        assert!(matches!(action, CliAction::Status));
        assert_eq!(dev.as_deref(), Some("scarlett-gen4"));
    }
}
