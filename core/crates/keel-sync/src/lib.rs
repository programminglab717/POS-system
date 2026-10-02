//! # keel-sync
//!
//! Replication between Keel's replicas (ADR-0019, ADR-0020, ADR-0022). Devices, the Store Hub,
//! its standby and the cloud each hold the events of one location, and send each other what the
//! other lacks, over any transport.
//!
//! - **Frames** ([`Frame`]): `have`, a replica's version vector, which says for each device how
//!   far into its log the replica holds, and whether the replica asks for the peer's in return;
//!   `events`, a batch of signed events, each device's in order; `durable`, the durable-ack
//!   watermark; and `heartbeat`, a replica's priority as hub, its term, the epoch and hub of the
//!   winning claim it holds, whether it acts as that hub, the hub's beat, and the term's floor,
//!   the latest beat of it the replica knows. All are canonical CBOR, at most [`MAX_FRAME`]
//!   bytes.
//! - **The replicator** ([`Replicator`]): one replica's side of replication with its peers. It
//!   does no I/O: the caller hands it each frame received, and a tick when
//!   [`Replicator::next_tick`] asks, and sends the frames it returns, and gives it random bits
//!   as it starts, which it numbers its batches from. It assumes nothing of delivery or of the
//!   clock: frames may be lost, duplicated, reordered or delayed, and the clock may jump, and
//!   replicas still converge, since each `have` says exactly what its sender holds, and
//!   receiving an event twice changes nothing.
//! - **Replicas** ([`Replica`]): what the replicator needs of a store: its version vector, a
//!   device's log after a position, receiving a batch of events, the chain of hub terms, and,
//!   for the hub, claiming, answering and sequencing. [`StoreReplica`] adapts `keel-store`'s
//!   store, which verifies each event it receives, keeps each device's log in order, and
//!   quarantines what it refuses.
//! - **A device's own log** is settled once every peer that has answered holds no more of it than
//!   the device, and either all have answered or a heartbeat period has passed
//!   ([`Replicator::settled`]). A device must not write before, or it could fork its log after its
//!   store lost its latest writes.
//! - **Roles** ([`Roles`]): a replica that can be the Store Hub has a priority. Every replica
//!   sends its peers a heartbeat every second, and the most preferred candidate that hears no hub
//!   for three periods claims the next epoch in its log; the replica holding the winning claim
//!   is the hub, and while it serves, it answers requests for orders and numbers what its store
//!   holds, pushing its records like any new events (ADR-0022). A replica that hears no hub is
//!   an island ([`Replicator::hub_reachable`]). A replica may name a durable peer, the cloud,
//!   whose `have` is the durable-ack watermark ([`Replicator::durable`]); replicas relay it in
//!   `durable` frames and keep the most they are told. A device's own events are store-durable
//!   as far as a peer holds them ([`Replicator::store_durable`]).
//!
//! Like `keel-store`, this is a platform crate, not built for `wasm32`: browsers are clients,
//! never replicas.

mod election;
mod frame;
mod replica;
mod replicator;

#[cfg(test)]
mod tests;

pub use frame::{
    Durable, Events, Frame, FrameError, Have, Heartbeat, MAX_FRAME, PROTOCOL, VersionVector,
};
pub use replica::{Claiming, Replica, StoreReplica};
pub use replicator::{Outgoing, Replicator, Roles, Stats, SyncConfig};
