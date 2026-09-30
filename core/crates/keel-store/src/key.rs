//! The key a store is encrypted with (ADR-0018).

use core::fmt::Write as _;

use zeroize::{Zeroize, Zeroizing};

/// The key a store is encrypted with: 32 random bytes that the platform keeps safe.
///
/// The platform shell makes the key once from the operating system's secure random generator,
/// wraps it with a key that never leaves the platform keystore, and stores the wrapped key before
/// it creates the store; it unwraps the key to open the store. The key never goes into the
/// database, a log or a backup. A lost key loses the store.
///
/// The bytes are zeroed when the key is dropped, and never printed. SQLCipher keeps its own copy
/// while the store is open.
pub struct StoreKey {
    // Boxed, so moving the key doesn't leave copies of it behind.
    bytes: Box<Zeroizing<[u8; 32]>>,
}

impl StoreKey {
    /// The key made of `bytes`, which are then zeroed.
    pub fn new(bytes: &mut [u8; 32]) -> StoreKey {
        let key = StoreKey { bytes: Box::new(Zeroizing::new(*bytes)) };
        bytes.zeroize();
        key
    }

    /// The statement `PRAGMA <pragma> = "x'…'"`, giving SQLCipher the key as a raw key: no
    /// passphrase to derive it from, since it is already random.
    pub(crate) fn pragma(&self, pragma: &str) -> Zeroizing<String> {
        // Long enough for the whole statement, so the string never moves and leaves a copy.
        let mut sql = Zeroizing::new(String::with_capacity(pragma.len().saturating_add(80)));
        sql.push_str("PRAGMA ");
        sql.push_str(pragma);
        sql.push_str(" = \"x'");
        for byte in self.bytes.iter() {
            // Writing to a String can't fail.
            let _ = write!(sql, "{byte:02X}");
        }
        sql.push_str("'\"");
        sql
    }
}

impl core::fmt::Debug for StoreKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("StoreKey(..)")
    }
}
