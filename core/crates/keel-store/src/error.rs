//! Why the store failed.

use keel_events::log::AppendError;

use crate::faults::Point;

/// Why the store couldn't open, read or write.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// SQLite failed: the file couldn't be opened, read or written.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// SQLite won't run with a setting the store needs for durability.
    #[error("the database can't run with {0}")]
    Settings(&'static str),
    /// The database's schema is newer than this kernel knows: a newer kernel wrote it.
    #[error("the database has schema version {found}, and this kernel knows up to {known}")]
    NewerSchema {
        /// The database's schema version.
        found: i64,
        /// The latest version this kernel knows.
        known: i64,
    },
    /// The database belongs to another device, or to the device at another location.
    #[error("the database belongs to another device or location")]
    NotThisDevice,
    /// The signer's key isn't the one that signed the device's stored events.
    #[error("the signer's key didn't sign this device's events")]
    WrongKey,
    /// Stored data doesn't read back as it was stored.
    #[error("stored data is corrupt: {0}")]
    Corrupt(&'static str),
    /// A number too large for the database, such as a sequence number beyond 2^63 − 1.
    #[error("{0} is out of range")]
    OutOfRange(&'static str),
    /// The device's log writer couldn't make the next event.
    #[error(transparent)]
    Append(#[from] AppendError),
    /// A fault hook interrupted the write, which rolled back.
    #[error("the write was interrupted at {0:?}")]
    Interrupted(Point),
}
