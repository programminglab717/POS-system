//! `keel-cell <cloud kit>...`: the cloud cell (ADR-0028), serving each location whose cloud's kit
//! `keel-provision` wrote into a directory named here, each one's store in its kit's directory,
//! listening for each on every address at the port its roster names the cloud's.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports to its terminal"
)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use keel_cloud::Location;

fn main() -> ExitCode {
    let kits: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if kits.is_empty() {
        eprintln!("usage: keel-cell <cloud kit>...");
        return ExitCode::FAILURE;
    }
    match run(&kits) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("keel-cell: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A listener on every address at `port`: IPv6's, which takes IPv4's too where the system lets
/// it, else IPv4's alone.
fn listen(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(("::", port)).or_else(|_| TcpListener::bind(("0.0.0.0", port)))
}

fn run(kits: &[PathBuf]) -> Result<(), String> {
    let mut locations = Vec::new();
    for kit in kits {
        let roster = keel_provision::Kit::read(kit)
            .map_err(|error| format!("{}: {error}", kit.display()))
            .and_then(|kit| {
                keel_domain::roster::Roster::decode(&kit.roster).map_err(|error| error.to_string())
            })?;
        let port = roster.cloud().ok_or("the roster names no cloud")?.port.get();
        let listener = listen(port).map_err(|error| error.to_string())?;
        let location =
            Location::open(kit, listener).map_err(|error| format!("{}: {error}", kit.display()))?;
        println!("keel-cell: {} ({}) listening on {port}", location.name(), location.id());
        locations.push(location);
    }
    // Each location runs on its own thread until the process ends.
    loop {
        std::thread::park();
    }
}
