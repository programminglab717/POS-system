//! `keel-cell --api <address> --token-file <file> [--web <dir>] <cloud kit>...`: the cloud cell
//! (ADR-0028), serving each location whose cloud's kit `keel-provision` wrote into a directory
//! named here, each one's store in its kit's directory, listening for each on every address at
//! the port its roster names the cloud's; and the back office's API on `<address>`, for the
//! operator whose bearer token `<file>` holds, with the back office's built files from `<dir>`.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports to its terminal"
)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use keel_cloud::Location;
use keel_cloud::api::{Cell, router};

fn usage() -> ExitCode {
    eprintln!("usage: keel-cell --api <address> --token-file <file> [--web <dir>] <cloud kit>...");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut api, mut token, mut web, mut kits) = (None, None, None, Vec::new());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--api" => api = args.next(),
            "--token-file" => token = args.next().map(PathBuf::from),
            "--web" => web = args.next().map(PathBuf::from),
            _ if arg.starts_with("--") => return usage(),
            _ => kits.push(PathBuf::from(arg)),
        }
    }
    let (Some(api), Some(token)) = (api, token) else { return usage() };
    if kits.is_empty() {
        return usage();
    }
    match run(&api, &token, web, &kits) {
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

fn run(api: &str, token: &PathBuf, web: Option<PathBuf>, kits: &[PathBuf]) -> Result<(), String> {
    let token = std::fs::read_to_string(token).map_err(|error| format!("the token: {error}"))?;
    let token = token.trim();
    // A token too short to guess is no token.
    if token.len() < 32 {
        return Err("the token must be at least 32 characters".into());
    }
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
    let cell = Arc::new(Cell::new(locations, token, web));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        let listener =
            tokio::net::TcpListener::bind(api).await.map_err(|error| format!("{api}: {error}"))?;
        println!("keel-cell: the back office's API on {api}");
        axum::serve(listener, router(cell)).await.map_err(|error| error.to_string())
    })
}
