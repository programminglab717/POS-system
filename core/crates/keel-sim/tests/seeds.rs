//! The simulator's seeds (ADR-0019): every run keeps every invariant, and a seed replays exactly.
//!
//! `KEEL_SIM_SEEDS` sets how many seeds run (32 by default), and `KEEL_SIM_FIRST_SEED` the first;
//! a failing seed prints the command that runs it alone.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

use keel_sim::{Config, simulate};

fn setting(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|value| value.parse().ok()).unwrap_or(default)
}

fn seeds() -> core::ops::Range<u64> {
    let first = setting("KEEL_SIM_FIRST_SEED", 0);
    first..first + setting("KEEL_SIM_SEEDS", 32)
}

fn rerun(seed: u64) -> String {
    format!("KEEL_SIM_FIRST_SEED={seed} KEEL_SIM_SEEDS=1 cargo test -p keel-sim --test seeds")
}

#[test]
fn every_seed_keeps_every_invariant() {
    for seed in seeds() {
        if let Err(failure) = simulate(&Config::from_seed(seed)) {
            panic!("{failure}\nrun it alone: {}", rerun(seed));
        }
    }
}

#[test]
fn without_faults_every_event_reaches_every_replica_within_a_second() {
    for seed in seeds().take(8) {
        let report = simulate(&Config::calm(seed)).unwrap_or_else(|failure| panic!("{failure}"));
        assert!(report.events > 0, "seed {seed}: nothing was appended");
        assert!(report.max_lag <= 1_000, "seed {seed}: an event took {} ms", report.max_lag);
    }
}

/// The hub crashes halfway through a run without other faults, and stays down 10 s: its standby
/// claims the role within 5 s (ADR-0022).
#[test]
fn without_faults_a_crashed_hub_is_succeeded_within_five_seconds() {
    for seed in seeds().take(8) {
        let report =
            simulate(&Config::calm_failover(seed)).unwrap_or_else(|failure| panic!("{failure}"));
        let failover = report.failover.unwrap_or_else(|| panic!("seed {seed}: no failover"));
        assert!(failover <= 5_000, "seed {seed}: the standby claimed {failover} ms after");
        assert_eq!(report.claims, [2, 2], "seed {seed}: the hub's claim, then the standby's");
    }
}

#[test]
fn a_seed_replays_exactly() {
    for seed in seeds().take(2) {
        let config = Config::from_seed(seed);
        let first = simulate(&config).unwrap();
        let second = simulate(&config).unwrap();
        assert_eq!(first, second, "seed {seed}");
    }
    let (a, b) =
        (simulate(&Config::from_seed(1)).unwrap(), simulate(&Config::from_seed(2)).unwrap());
    assert_ne!(a.trace, b.trace);
}
