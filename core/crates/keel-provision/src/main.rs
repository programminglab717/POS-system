//! `keel-provision <out> --hub <host>:<port> --hub-name <name> --device <name>[=<hub priority>]...`
//!
//! Provisions a location with the demo café's profile: writes its CA's certificate and key into
//! `<out>`, and each device's kit into a directory of its own there, named by its identifier.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports to its terminal"
)]

use core::num::{NonZeroU8, NonZeroU16};
use std::path::PathBuf;
use std::process::ExitCode;

use keel_provision::{DeviceSpec, Spec, provision};

fn usage() -> ExitCode {
    eprintln!(
        "usage: keel-provision <out> --hub <host>:<port> --hub-name <name> --device <name>[=<priority>]..."
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next().map(PathBuf::from) else { return usage() };
    let (mut hub, mut hub_name, mut devices) = (None, None, Vec::new());
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else { return usage() };
        match flag.as_str() {
            "--hub" => {
                hub = value.rsplit_once(':').and_then(|(host, port)| {
                    Some((host.to_owned(), port.parse::<NonZeroU16>().ok()?))
                });
            }
            "--hub-name" => hub_name = Some(value),
            "--device" => {
                let (name, priority) = match value.split_once('=') {
                    Some((name, priority)) => (name.to_owned(), priority.parse::<NonZeroU8>().ok()),
                    None => (value, None),
                };
                devices.push(DeviceSpec { name, hub: priority });
            }
            _ => return usage(),
        }
    }
    let (Some((hub_host, hub_port)), Some(hub_name)) = (hub, hub_name) else { return usage() };
    let Ok(profile) = keel_domain::profile::demo().map(|demo| demo.encode()) else {
        return ExitCode::FAILURE;
    };
    let spec = Spec { profile, hub_host, hub_port, hub_name, devices };
    let result = provision(&spec).and_then(|provisioned| {
        std::fs::create_dir_all(&out)?;
        std::fs::write(out.join("ca.crt"), &provisioned.ca)?;
        std::fs::write(out.join("ca.key"), &provisioned.ca_key)?;
        for kit in &provisioned.kits {
            kit.write(&out.join(kit.device.to_string()))?;
            println!("{}  {}", kit.device, kit.name);
        }
        Ok(())
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("keel-provision: {error}");
            ExitCode::FAILURE
        }
    }
}
