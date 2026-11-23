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
    let parsed = match cli::parse() {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };

    match parsed.command {
        Some(cmd) => cli::execute(cmd, parsed.device_override.as_deref()),
        None => daemon::run(parsed.device_override.as_deref()),
    }
}
