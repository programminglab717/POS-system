//! Importing a location's events (ADR-0037, decision 4): an export's events, as their devices
//! signed them, taken into this device's store as sync takes them, each verified and its
//! device's log checked link by link. An export imported into a fresh store gives the same
//! events, so the same projections and reports.

use keel_events::keys::Signer;
use keel_events::verify::DeviceRegistry;
use keel_store::Received;
use keel_types::{Clock, Entropy};

use crate::error::RuntimeError;
use crate::runtime::Runtime;

/// What an import took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    /// Events stored.
    pub stored: u64,
    /// Events the store held already.
    pub duplicates: u64,
    /// Events after a gap in their device's log, so not stored.
    pub gaps: u64,
    /// Events refused, as one not signed by its device's key, and quarantined.
    pub quarantined: u64,
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Takes in `events`, each as its device signed it, verified with `registry`: in the order of
    /// each device's log, as an export has them.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be written. A refused event isn't an error: it
    /// is quarantined, and counted.
    pub fn import<'a>(
        &mut self,
        registry: &DeviceRegistry,
        events: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<Imported, RuntimeError> {
        let mut imported = Imported::default();
        let now = self.clock.now();
        for bytes in events {
            let received = self.store.write(|w| w.receive(bytes, registry, now))?;
            let count = match received {
                Received::Stored(_) => &mut imported.stored,
                Received::Duplicate => &mut imported.duplicates,
                Received::Gap { .. } => &mut imported.gaps,
                Received::Quarantined(_) => &mut imported.quarantined,
            };
            *count = count.saturating_add(1);
        }
        Ok(imported)
    }
}
