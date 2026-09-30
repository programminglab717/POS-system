//! Why the store failed.

use keel_events::log::AppendError;
use rusqlite::ErrorCode;

use crate::faults::Point;
use crate::outbox::EffectError;

/// Why the store couldn't open, read or write.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// SQLite failed: the file couldn't be opened, read or written.
    #[error("database error: {0}")]
    Database(rusqlite::Error),
    /// The key doesn't open the store: it isn't the store's key, the file isn't a store, or the
    /// file's first page is damaged. Encryption can't tell these apart.
    #[error("the key doesn't open the store, or the store's first page is damaged")]
    KeyRejected,
    /// The store's file is damaged: a page failed its authentication, or SQLite found the file
    /// malformed. The store refuses everything after it until it is reopened, and never returns
    /// data from a damaged page.
    #[error("the store's file is damaged")]
    Damaged,
    /// The store couldn't be re-encrypted with the new key, and is still encrypted with its old
    /// one.
    #[error("the store wasn't re-encrypted, and still has its old key")]
    NotRekeyed,
    /// A rekey left the store without a connection to its file that it can trust, and it refuses
    /// everything until it is opened again.
    #[error("the store lost its connection in a rekey: open it again")]
    Closed,
    /// Another connection is using the store's file. A store belongs to one process at a time.
    #[error("another connection is using the store")]
    Busy,
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
    WrongSigner,
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
    /// The outbox refused a change.
    #[error(transparent)]
    Effect(#[from] EffectError),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> StoreError {
        match error.sqlite_error_code() {
            // SQLCipher reports a page that fails its authentication as either, and SQLite a
            // malformed file as the first.
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => StoreError::Damaged,
            _ => StoreError::Database(error),
        }
    }
}
