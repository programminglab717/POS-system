//! # keel-sim
//!
//! Keel's deterministic simulator (ADR-0019): devices, the Store Hub and the cloud replica in one
//! process, one thread and virtual time, each with a real `keel-store` on a RAM disk and a
//! `keel-sync` replicator, under a seeded scheduler.
//!
//! - **Nodes:** two or three devices, the hub and the cloud, in a star: devices to the hub, the
//!   hub to the cloud. Each has its own key, signer and clock, off virtual time by up to 25 s.
//!   The hub sequences, in epoch 1, and names the cloud its durable peer (ADR-0020).
//! - **The network** delivers frames after random delays, so they arrive out of order, and while
//!   it is faulty loses and duplicates some; partitions cut links for a while.
//! - **Faults:** nodes crash, between writes or in the middle of one, and restart; a device's
//!   store is restored from an older copy of itself; clocks jump.
//! - **Workload:** devices ring orders and take cash through `keel-domain`'s commands and
//!   checkout, each decided against the device's own view.
//! - **A run** works under faults for 30 virtual seconds, heals, and runs until the replicas
//!   agree and every watermark reaches what the cloud holds, then checks the invariants:
//!   convergence, no loss, causality, no forks or quarantine, stores that check clean, and the
//!   hub's numbering, every replica confirming alike, and no store-durable event lost (see
//!   [`simulate`]). All along, it checks each batch replicas send against the protocol's rules for
//!   batches, each `durable` frame against what the cloud holds, and that every replicator's
//!   watermark only rises. Its [`Config`] is drawn from a seed, and every
//!   choice the run makes comes from the seed too, so a failing seed replays exactly.
//!
//! This crate is for tests only, and is never shipped.

mod check;
mod config;
mod monitor;
mod node;
mod rng;
mod run;
mod workload;

pub use config::Config;
pub use node::SimError;
pub use run::{Failure, Report, simulate};
pub use workload::Move;
