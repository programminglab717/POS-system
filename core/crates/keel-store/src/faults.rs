//! Points where a write can be interrupted, for crash-safety tests and the simulator.
//!
//! The store asks its [`Faults`] whether to go on at each [`Point`] of a write. In production it
//! always goes on ([`NoFaults`]). A test can refuse, and the write must roll back and leave the
//! store usable; or it can end the process there, and the store must reopen with every write
//! either done or not done at all.

use crate::error::StoreError;

/// A point in a write where it can be interrupted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Point {
    /// A migration of the schema is about to commit.
    Migrating,
    /// A write has begun its transaction.
    Began,
    /// An event was stored or quarantined, and the write goes on.
    Stored,
    /// The write is about to commit.
    Committing,
    /// The write has committed, and the store is about to return. Only a crash can interrupt the
    /// store here: the write has happened, so refusing changes nothing.
    Committed,
}

impl Point {
    /// Every point, in the order a write passes them.
    pub const ALL: [Point; 5] =
        [Point::Migrating, Point::Began, Point::Stored, Point::Committing, Point::Committed];
}

/// Decides whether a write goes on at each point.
pub trait Faults {
    /// Whether the store goes on at `point`. `false` interrupts the write, which rolls back and
    /// fails with [`StoreError::Interrupted`], except at [`Point::Committed`].
    fn proceed(&mut self, point: Point) -> bool;
}

/// No faults: the store always goes on.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFaults;

impl Faults for NoFaults {
    fn proceed(&mut self, _point: Point) -> bool {
        true
    }
}

/// Asks `faults` whether to go on at `point`.
pub(crate) fn proceed(faults: &mut dyn Faults, point: Point) -> Result<(), StoreError> {
    if faults.proceed(point) { Ok(()) } else { Err(StoreError::Interrupted(point)) }
}
