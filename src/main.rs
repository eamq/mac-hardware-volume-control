//! `mac-hardware-volume-control` entrypoint.

mod cli;
mod config;
mod daemon;
mod devices;
mod osd;
mod platform;
mod volume;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        cli::CliOutcome::Exit(code) => code,
        cli::CliOutcome::RunDaemon { device_override } => daemon::run(device_override.as_deref()),
    }
}
