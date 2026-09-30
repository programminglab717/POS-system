//! Signed events: an event body, signed by its origin device.
//!
//! On the wire and at rest, an event is a COSE_Sign1 message whose payload is the encoded
//! [`EventBody`]. Bytes received from elsewhere become an [`UnverifiedEvent`]; only verifying
//! its signature with the origin device's key turns it into a [`SignedEvent`], so nothing can
//! use an event's content before its signature has been checked. The one exception, behind the
//! `stored` feature, reads back events that a device's store verified before storing them.

use keel_types::Id;

use crate::cose::{CoseError, CoseSign1};
use crate::envelope::{Device, EnvelopeError, EventBody};
use crate::hash::EventHash;
use crate::keys::{KeyId, PublicKey, SignError, Signer};

/// The largest event, as stored and transmitted, in bytes. Replicas exchange events in frames of
/// bounded size, and a device's log replicates in order, so an event too large to send would
/// hold back every later event of its device: the log writer refuses to make one, and the
/// registry refuses to verify one (ADR-0019).
pub const MAX_EVENT_BYTES: usize = 256 * 1024;

/// An event whose signature has been made or verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedEvent {
    body: EventBody,
    hash: EventHash,
    message: CoseSign1,
}

impl SignedEvent {
    /// Signs `body` with its origin device's `signer`.
    ///
    /// # Errors
    /// [`SignError`] if the signer fails.
    pub fn sign<S: Signer + ?Sized>(body: EventBody, signer: &S) -> Result<SignedEvent, SignError> {
        let encoded = body.encode();
        let hash = EventHash::of(&encoded);
        let message = CoseSign1::sign(encoded, signer)?;
        Ok(SignedEvent { body, hash, message })
    }

    /// The event's content.
    pub const fn body(&self) -> &EventBody {
        &self.body
    }

    /// The hash of the encoded body: the event's identity in its device's hash chain.
    pub const fn hash(&self) -> EventHash {
        self.hash
    }

    /// The identifier of the key that signed the event.
    pub const fn key_id(&self) -> KeyId {
        self.message.key_id()
    }

    /// The event as stored and transmitted: a COSE_Sign1 message.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.message.encode()
    }

    /// Decodes an event that a store verified before storing it, without checking its
    /// signature again. Use it only for bytes read back from that store: bytes from anywhere
    /// else go through [`UnverifiedEvent::verify`].
    ///
    /// # Errors
    /// [`EventError`] if the bytes aren't a well-formed event.
    #[cfg(feature = "stored")]
    pub fn from_stored(bytes: &[u8]) -> Result<SignedEvent, EventError> {
        let UnverifiedEvent { body, hash, message } = UnverifiedEvent::decode(bytes)?;
        Ok(SignedEvent { body, hash, message })
    }
}

/// An event decoded from bytes whose signature hasn't been checked yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnverifiedEvent {
    body: EventBody,
    hash: EventHash,
    message: CoseSign1,
}

impl UnverifiedEvent {
    /// Decodes an event, strictly: canonical CBOR, Keel's COSE_Sign1 form, a valid body.
    ///
    /// # Errors
    /// [`EventError`] if the bytes aren't a well-formed event.
    pub fn decode(bytes: &[u8]) -> Result<UnverifiedEvent, EventError> {
        let message = CoseSign1::decode(bytes)?;
        let body = EventBody::decode(message.payload())?;
        let hash = EventHash::of(message.payload());
        Ok(UnverifiedEvent { body, hash, message })
    }

    /// The device that claims to have signed the event: look up its key to verify.
    pub const fn origin_device(&self) -> Id<Device> {
        self.body.origin_device
    }

    /// The identifier of the key that claims to have signed the event.
    pub const fn key_id(&self) -> KeyId {
        self.message.key_id()
    }

    /// The claimed content, for inspection only: nothing here is trustworthy until verified.
    pub const fn unverified_body(&self) -> &EventBody {
        &self.body
    }

    /// Verifies the signature with `key`, the origin device's key named by [`Self::key_id`].
    ///
    /// # Errors
    /// [`CoseError::KeyMismatch`] if `key` isn't the key named by the event, and
    /// [`CoseError::Signature`] if the signature doesn't verify.
    pub fn verify(self, key: &PublicKey) -> Result<SignedEvent, CoseError> {
        self.message.verify(key)?;
        Ok(SignedEvent { body: self.body, hash: self.hash, message: self.message })
    }
}

/// Why event bytes were rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EventError {
    /// Not a well-formed COSE_Sign1 message, or its signature doesn't verify.
    #[error(transparent)]
    Cose(#[from] CoseError),
    /// Not a valid event body.
    #[error(transparent)]
    Envelope(#[from] EnvelopeError),
}

#[cfg(test)]
pub(crate) mod golden {
    //! A fixed event, used as a known-answer test of the whole format.

    use core::num::{NonZeroU32, NonZeroU64};

    use keel_types::{Hlc, Id};

    use crate::cbor::{Map, Value};
    use crate::envelope::{
        Actor, EventBody, Payload, SchemaName, SchemaRef, StreamKind, StreamRef,
    };
    use crate::hash::EventHash;
    use crate::keys::{SignatureAlgorithm, Signer as _, SoftwareSigner};

    pub(crate) fn body() -> EventBody {
        let payload = Map::from_entries([
            (Value::Unsigned(1), Value::from("espresso")),
            (Value::Unsigned(2), Value::Unsigned(350)),
        ])
        .unwrap();
        EventBody {
            event_id: Id::parse("0192f0c1-9c4a-7abc-8123-456789abcdef").unwrap(),
            location: Id::parse("0192f0c1-0000-7000-8000-000000000001").unwrap(),
            stream: StreamRef {
                kind: StreamKind::new("order").unwrap(),
                id: Id::parse("0192f0c1-9c4a-7000-8000-00000000000a").unwrap(),
            },
            schema: SchemaRef {
                name: SchemaName::new("order.line_added").unwrap(),
                version: NonZeroU32::new(1).unwrap(),
            },
            origin_device: Id::parse("0192f0c1-0000-7000-8000-00000000000d").unwrap(),
            origin_seq: NonZeroU64::new(1).unwrap(),
            hlc: Hlc::new(1_790_517_780_123, 0).unwrap(),
            business_date: "2026-09-27".parse().unwrap(),
            actor: Actor::TeamMember(Id::parse("0192f0c1-0000-7000-8000-00000000000e").unwrap()),
            approval: None,
            causation: None,
            correlation: None,
            payload: Payload::new(&Value::Map(payload)).unwrap(),
            prev_hash: EventHash::ZERO,
        }
    }

    pub(crate) fn signer() -> SoftwareSigner {
        SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap()
    }

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    /// The event format, pinned forever. Python's cbor2 decoded this body field by field and
    /// confirmed it is canonical, hashlib recomputed the hash, and pycose verified the signed
    /// event. If this test fails, the format changed: stored events would no longer verify.
    #[test]
    fn the_event_format_is_pinned() {
        let body = body();
        let expected_body = "ae000101500192f0c19c4a7abc8123456789abcdef02500192f0c100007000800000000000000103656f7264657204500192f0c19c4a7000800000000000000a05706f726465722e6c696e655f6164646564060107500192f0c100007000800000000000000d0801091b01a0e32d1e9b00000a6a323032362d30392d32370b8200500192f0c100007000800000000000000e0f4fa20168657370726573736f0219015e1058200000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(body.encode(), hex(expected_body));
        let event = super::SignedEvent::sign(body.clone(), &signer()).unwrap();
        assert_eq!(
            event.hash().to_string(),
            "44ae8bc4840551a468fd81cad2d2e404304f2870c9569894c979510d713e2899"
        );
        let expected_event = "845826a20127045820cbf65c8afcb5f69056927170cfe6acc226f1e3182e0aac3b83245e746beb0e6aa058c6ae000101500192f0c19c4a7abc8123456789abcdef02500192f0c100007000800000000000000103656f7264657204500192f0c19c4a7000800000000000000a05706f726465722e6c696e655f6164646564060107500192f0c100007000800000000000000d0801091b01a0e32d1e9b00000a6a323032362d30392d32370b8200500192f0c100007000800000000000000e0f4fa20168657370726573736f0219015e10582000000000000000000000000000000000000000000000000000000000000000005840a8d44b6717906a98628e3889acb70915a7ae6f41b51b40b1554b69a2496b2fe954131a91bfa6c794a4903fbd4cecfdbb4c6bf1ce90fcb188e6084ddc6d0a590b";
        assert_eq!(event.to_bytes(), hex(expected_event));
        let received = super::UnverifiedEvent::decode(&hex(expected_event)).unwrap();
        assert_eq!(received.verify(signer().public_key()), Ok(event));
    }
}

#[cfg(test)]
mod tests {
    use super::golden::{body, signer};
    use super::*;
    use crate::cose::CoseError;
    use crate::keys::{SignatureAlgorithm, SoftwareSigner};

    #[test]
    fn signed_events_verify_with_their_key_only() {
        let event = SignedEvent::sign(body(), &signer()).unwrap();
        let received = UnverifiedEvent::decode(&event.to_bytes()).unwrap();
        assert_eq!(received.origin_device(), body().origin_device);
        assert_eq!(received.key_id(), signer().public_key().key_id());
        assert_eq!(received.unverified_body(), &body());
        let other = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[8; 32]).unwrap();
        assert_eq!(received.clone().verify(other.public_key()), Err(CoseError::KeyMismatch));
        assert_eq!(received.verify(signer().public_key()), Ok(event));
    }

    #[cfg(feature = "stored")]
    #[test]
    fn stored_events_read_back_without_a_signature_check() {
        let event = SignedEvent::sign(body(), &signer()).unwrap();
        let bytes = event.to_bytes();
        assert_eq!(SignedEvent::from_stored(&bytes), Ok(event));
        // It trusts the store: a message signed by another key still reads back.
        let other = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[8; 32]).unwrap();
        let forged = SignedEvent::sign(body(), &other).unwrap();
        assert_eq!(SignedEvent::from_stored(&forged.to_bytes()), Ok(forged));
        // But not bytes that aren't an event.
        assert!(SignedEvent::from_stored(&bytes[1..]).is_err());
    }

    #[test]
    fn a_body_that_is_not_an_event_is_rejected_even_when_signed() {
        let message = CoseSign1::sign(vec![0x01], &signer()).unwrap();
        assert!(matches!(
            UnverifiedEvent::decode(&message.encode()),
            Err(EventError::Envelope(EnvelopeError::NotAMap))
        ));
    }
}
