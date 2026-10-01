//! # keel-sim
//!
//! Keel's deterministic simulator (ADR-0019): devices, the Store Hub, its standby and the cloud
//! replica in one process, one thread and virtual time, each with a real `keel-store` on a RAM
//! disk and a `keel-sync` replicator, under a seeded scheduler.
//!
//! - **Nodes:** two or three devices; the hub, of priority 2, and its standby, of priority 1; and
//!   the cloud, which can never be the hub. Each device links to the hub and the standby, and
//!   each of those two to the other and to the cloud. Each node has its own key, signer and
//!   clock, off virtual time by up to 25 s. No one holds a term at the start: the hub claims
//!   epoch 1, and whichever candidate holds the winning claim answers requests for orders
//!   (ADR-0021) and sequences, naming the cloud its durable peer (ADR-0020, ADR-0022).
//! - **The network** delivers frames after random delays, so they arrive out of order, and while
//!   it is faulty loses and duplicates some; partitions cut links for a while, and splits cut the
//!   store's network in two, the hub on one side and the standby on the other, each device on
//!   one side, and the cloud reaching one side, both or neither.
//! - **Faults:** nodes crash, the hub and the standby too, between writes or in the middle of
//!   one, and restart; a device's store is restored from an older copy of itself; clocks jump.
//! - **Workload:** devices ring orders and take cash through `keel-domain`'s commands and
//!   checkout, each decided against the device's own view, half their moves on other orders
//!   aimed at the latest few. A move that needs an order another device owns becomes a request
//!   for it, which waits for the hub, or, from a device that hears no hub
//!   ([`keel_sync::Replicator::hub_reachable`]), a manager's override.
//! - **A run** works under faults for 30 virtual seconds, heals, and runs until the replicas
//!   agree, on the winning claim among the rest, and every watermark reaches what the cloud
//!   holds, then checks the invariants: convergence, no loss, causality, no forks or quarantine,
//!   stores that check clean, one hub that every replica agrees on, numbering by the records
//!   that count, every replica confirming alike, no store-durable event lost, and the answers to
//!   requests for orders: at least one each and none twice from a term, each term's grants'
//!   leases rising, a stale grant only after an override or another term's grant, and every
//!   replica agreeing on each order's owner (see [`simulate`]). All along, it checks each batch
//!   replicas send against the protocol's rules for batches, each `durable` frame against what
//!   the cloud holds, that every replicator's watermark only rises, that a replica's log settles
//!   only when no peer it has heard from holds more of it, and that a candidate for the hub
//!   writes records and answers only while it serves, in the term it holds. Its [`Config`] is
//!   drawn from a seed, and every choice the run makes comes from the seed too, so a failing
//!   seed replays exactly.
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
