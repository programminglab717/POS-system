//! Event hashes: SHA-256, the links of each device's hash chain.

use core::fmt;

use sha2::{Digest, Sha256};

/// SHA-256 of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Writes bytes as lowercase hexadecimal.
pub(crate) fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    bytes.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
}

/// The SHA-256 hash of an event's body: its identity in its device's hash chain.
///
/// Each event body records the hash of the previous event in the same device's log, so altering,
/// removing or reordering any event breaks every link after it. The hash covers the body only,
/// not the signature: an event's identity is its content.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventHash([u8; 32]);

impl EventHash {
    /// The previous-event hash of the first event in a log, which has no previous event.
    pub const ZERO: EventHash = EventHash([0; 32]);

    /// The hash of an encoded event body.
    pub fn of(body: &[u8]) -> EventHash {
        EventHash(sha256(body))
    }

    /// A hash from its 32 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> EventHash {
        EventHash(bytes)
    }

    /// The 32 bytes of the hash.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for EventHash {
    /// Lowercase hexadecimal.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Debug for EventHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EventHash({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answers() {
        // FIPS 180-2 examples, confirmed with Python's hashlib.
        assert_eq!(
            EventHash::of(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            EventHash::of(b"").to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(EventHash::ZERO.as_bytes(), &[0; 32]);
        assert_eq!(format!("{:?}", EventHash::ZERO), format!("EventHash({})", "0".repeat(64)));
    }
}
