//! `keel-hub <kit>`: the Store Hub's daemon, on the kit `keel-provision` wrote into `<kit>`, its
//! store in `<kit>/store.db`, listening on every address at the roster's port.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a command-line tool reports to its terminal"
)]

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;

use keel_domain::profile::Profile;
use keel_domain::roster::Roster;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::Kit;
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Locale, OsEntropy, SystemClock};

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: keel-hub <kit>");
        return ExitCode::FAILURE;
    };
    match run(&dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("keel-hub: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(dir: &std::path::Path) -> Result<(), String> {
    let kit = Kit::read(dir).map_err(|error| error.to_string())?;
    let roster = Roster::decode(&kit.roster).map_err(|error| error.to_string())?;
    let profile = Profile::decode(&kit.profile).map_err(|error| error.to_string())?;
    let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &kit.signing_key)
        .map_err(|error| error.to_string())?;
    let mut store_key = kit.store_key;
    let mut runtime = Runtime::open(
        dir.join("store.db"),
        StoreKey::new(&mut store_key),
        Identity { device: kit.device, signer },
        profile,
        SystemClock,
        OsEntropy,
        OsEntropy,
        Locale::EN_US,
    )
    .map_err(|error| error.to_string())?;
    let listener = TcpListener::bind(("0.0.0.0", roster.data().hub.port.get()))
        .map_err(|error| error.to_string())?;
    let first = runtime.start_sync(roster.clone()).map_err(|error| error.to_string())?;
    let net = Net::hub(listener, roster, kit.credentials()).map_err(|error| error.to_string())?;
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    println!("keel-hub: {} listening", kit.name);
    let mut shown = None;
    loop {
        if let Err(error) = keel_hub::step(&mut runtime, &net, keel_hub::MAX_WAIT) {
            eprintln!("keel-hub: {error}");
        }
        let status = runtime.sync_status();
        if status != shown {
            println!("keel-hub: {status:?}");
            shown = status;
        }
    }
}
