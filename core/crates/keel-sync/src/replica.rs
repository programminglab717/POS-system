//! What the replicator needs of a replica's store, and `keel-store`'s store adapted to it.

use core::num::NonZeroU8;

use keel_domain::hub::Term;
use keel_events::envelope::{Device, Location};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::verify::DeviceRegistry;
use keel_store::{Received, Store, StoreError};
use keel_types::{Entropy, Id, Timestamp};

use crate::frame::VersionVector;

/// A replica's store, as the replicator uses it.
pub trait Replica {
    /// Why the store couldn't do what it was asked.
    type Error;

    /// The location whose events the replica holds.
    fn location(&self) -> Id<Location>;

    /// The replica's own device, whose log it appends to.
    fn device(&self) -> Id<Device>;

    /// How far into each device's log the replica holds.
    ///
    /// # Errors
    /// If the store can't be read.
    fn version_vector(&mut self) -> Result<VersionVector, Self::Error>;

    /// Up to `limit` events of `device`'s log after position `after`, in order, as stored.
    ///
    /// # Errors
    /// If the store can't be read.
    fn events_after(
        &mut self,
        device: Id<Device>,
        after: u64,
        limit: u32,
    ) -> Result<Vec<Vec<u8>>, Self::Error>;

    /// Receives `events`, in order and all together, and says what became of each: stored, a
    /// duplicate, after a gap, or quarantined.
    ///
    /// # Errors
    /// If the store can't be written, in which case none of them was stored.
    fn receive(&mut self, events: &[Vec<u8>], now: Timestamp)
    -> Result<Vec<Received>, Self::Error>;

    /// The chain of terms the replica works out from the claims it holds, from the winning
    /// claim's term back (ADR-0022).
    ///
    /// # Errors
    /// If the store can't be read.
    fn terms(&mut self) -> Result<Vec<Term>, Self::Error>;

    /// Claims the Store Hub's role for the replica's device, at `priority`, in a write of its own
    /// at physical time `now`, succeeding the winning claim (ADR-0022).
    ///
    /// # Errors
    /// If the store can't be read or written, in which case it appended nothing.
    fn claim(&mut self, priority: NonZeroU8, now: Timestamp) -> Result<Claiming, Self::Error>;

    /// Numbers, as the Store Hub, in its epoch, the events of each device's log after the last
    /// position a sequencing record that counts covers, except records, in a write of its own at
    /// physical time `now`, and returns the records it appended to its device's log: none when
    /// nothing is new (ADR-0020, ADR-0022). The replicator asks only while the replica holds the
    /// winning claim.
    ///
    /// # Errors
    /// If the store can't be written, in which case it appended nothing.
    fn sequence(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, Self::Error>;

    /// Answers, as the Store Hub, in its epoch, every request for an order the replica holds
    /// that no answer names yet, in a write of its own at physical time `now`, and returns the
    /// answers it appended to its device's log: none when no request waits (ADR-0021). The
    /// replicator asks only while the replica holds the winning claim.
    ///
    /// # Errors
    /// If the store can't be written, in which case it appended nothing.
    fn answer_requests(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, Self::Error>;
}

/// What became of a replica's claim of the Store Hub's role (ADR-0022).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claiming {
    /// The replica claimed it: the claim, appended to its device's log.
    Claimed(Box<SignedEvent>),
    /// The replica already holds the winning claim, and wrote nothing.
    Hub,
    /// The replica doesn't hold the whole chain of claims and every record that counts on it,
    /// and wrote nothing: it must catch up first.
    Behind,
}

/// A [`Replica`] that is `keel-store`'s store, verifying the events it receives with a device
/// registry. It borrows both, for as long as the replicator needs them.
pub struct StoreReplica<'a, S, E> {
    store: &'a mut Store<S, E>,
    registry: &'a DeviceRegistry,
}

impl<'a, S, E> StoreReplica<'a, S, E> {
    /// `store`, verifying received events with `registry`.
    pub const fn new(store: &'a mut Store<S, E>, registry: &'a DeviceRegistry) -> Self {
        StoreReplica { store, registry }
    }
}

impl<S, E> core::fmt::Debug for StoreReplica<'_, S, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoreReplica").field("store", &self.store).finish_non_exhaustive()
    }
}

impl<S: Signer, E: Entropy> Replica for StoreReplica<'_, S, E> {
    type Error = StoreError;

    fn location(&self) -> Id<Location> {
        self.store.location()
    }

    fn device(&self) -> Id<Device> {
        self.store.device()
    }

    fn version_vector(&mut self) -> Result<VersionVector, StoreError> {
        self.store.version_vector()
    }

    fn events_after(
        &mut self,
        device: Id<Device>,
        after: u64,
        limit: u32,
    ) -> Result<Vec<Vec<u8>>, StoreError> {
        Ok(self.store.log(device, after, limit)?.iter().map(SignedEvent::to_bytes).collect())
    }

    fn receive(&mut self, events: &[Vec<u8>], now: Timestamp) -> Result<Vec<Received>, StoreError> {
        let registry = self.registry;
        self.store.write(|w| events.iter().map(|bytes| w.receive(bytes, registry, now)).collect())
    }

    fn terms(&mut self) -> Result<Vec<Term>, StoreError> {
        self.store.terms()
    }

    fn claim(&mut self, priority: NonZeroU8, now: Timestamp) -> Result<Claiming, StoreError> {
        match self.store.claim(priority, now) {
            Ok(Some(claim)) => Ok(Claiming::Claimed(Box::new(claim))),
            Ok(None) => Ok(Claiming::Hub),
            Err(StoreError::Behind) => Ok(Claiming::Behind),
            Err(error) => Err(error),
        }
    }

    fn sequence(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, StoreError> {
        self.store.sequence(now)
    }

    fn answer_requests(&mut self, now: Timestamp) -> Result<Vec<SignedEvent>, StoreError> {
        self.store.answer_requests(now)
    }
}
