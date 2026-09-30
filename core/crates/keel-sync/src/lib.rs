//! # keel-sync
//!
//! Replication between Keel's replicas (ADR-0019, ADR-0020). Devices, the Store Hub, its standby
//! and the cloud each hold the events of one location, and send each other what the other lacks,
//! over any transport.
//!
//! - **Frames** ([`Frame`]): `have`, a replica's version vector, which says for each device how
//!   far into its log the replica holds, and whether the replica asks for the peer's in return;
//!   `events`, a batch of signed events, each device's in order; and `durable`, the durable-ack
//!   watermark. All are canonical CBOR, at most [`MAX_FRAME`] bytes.
//! - **The replicator** ([`Replicator`]): one replica's side of replication with its peers. It
//!   does no I/O: the caller hands it each frame received, and a tick when
//!   [`Replicator::next_tick`] asks, and sends the frames it returns. It assumes nothing of
//!   delivery: frames may be lost, duplicated, reordered or delayed, and replicas still
//!   converge, since each `have` says exactly what its sender holds, and receiving an event twice
//!   changes nothing.
//! - **Replicas** ([`Replica`]): what the replicator needs of a store: its version vector, a
//!   device's log after a position, receiving a batch of events, and, for the hub, sequencing.
//!   [`StoreReplica`] adapts `keel-store`'s store, which verifies each event it receives, keeps
//!   each device's log in order, and quarantines what it refuses.
//! - **A device's own log** is settled once a peer has shown it holds no more of it than the
//!   device ([`Replicator::settled`]). A device must not write before, or it could fork its log
//!   after its store lost its latest writes.
//! - **Roles** ([`Roles`], ADR-0020): the Store Hub sequences, once its log is settled, after
//!   every write that stores events, and pushes its records like any new events. A replica may
//!   name a durable peer, the cloud, whose `have` is the durable-ack watermark
//!   ([`Replicator::durable`]); replicas relay it in `durable` frames and keep the most they are
//!   told. A device's own events are store-durable as far as a peer holds them
//!   ([`Replicator::store_durable`]).
//!
//! Like `keel-store`, this is a platform crate, not built for `wasm32`: browsers are clients,
//! never replicas. Ownership leases and hub election come in later slices of step 6.

mod frame;
mod replica;
mod replicator;

#[cfg(test)]
mod tests;

pub use frame::{Durable, Events, Frame, FrameError, Have, MAX_FRAME, PROTOCOL, VersionVector};
pub use replica::{Replica, StoreReplica};
pub use replicator::{Outgoing, Replicator, Roles, Stats, SyncConfig};
