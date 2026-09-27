//! # keel-events
//!
//! Keel's event log: the signed, hash-chained record of everything that happens.
//!
//! Every change in Keel is an event. The device where it happens appends it to its own log, and
//! the log replicates to the hub, the cloud and other devices. This crate fixes what an event is,
//! byte for byte, and the rules that make every log tamper-evident:
//!
//! - [`cbor`]: a strict subset of canonical CBOR (RFC 8949 §4.2.1). Every value has exactly one
//!   accepted encoding, so an event has the same bytes, hash and signature everywhere.
//! - [`envelope`]: the event body, what every event records: its identity, location, stream,
//!   schema, origin device and sequence number, hybrid logical clock, business date, actor,
//!   links to other events, payload, and the hash of the previous event in its device's log.
//! - [`keys`] and [`cose`]: device keys (ES256, from secure hardware where the device has it, or
//!   Ed25519) and signatures as COSE_Sign1 messages (RFC 9052), with one accepted form per
//!   signature.
//! - [`hash`] and [`event`]: event hashes, and signed events. Bytes from elsewhere can't be used
//!   as an event until their signature is verified.
//! - [`log`]: writing a device's log, where each event is numbered, chained to the one before and
//!   timestamped, and checking where a received event fits in its device's log.
//! - [`verify`]: the registry of enrolled devices, and revocation by position in a device's log.
//!
//! Like the rest of the kernel, this crate is deterministic (time and randomness are injected),
//! never panics, and builds for `wasm32`. The wire format and the reasons for it are recorded in
//! `docs/adr/0012-event-wire-format.md`, and a known-answer test pins it.
//!
//! ```
//! use core::num::NonZeroU32;
//! use core::time::Duration;
//!
//! use keel_events::cbor::Value;
//! use keel_events::envelope::{Actor, Payload, SchemaName, SchemaRef, StreamKind, StreamRef};
//! use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
//! use keel_events::log::{EventDraft, Link, LogConfig, LogHead, LogWriter};
//! use keel_events::verify::DeviceRegistry;
//! use keel_types::{Hlc, IdGenerator, SeededEntropy, Timestamp};
//!
//! // A register enrolled at a store, with its key (in production, held in secure hardware).
//! let now = Timestamp::from_millis(1_790_517_780_000)?;
//! let mut ids = IdGenerator::new(SeededEntropy::new(1));
//! let (device, location) = (ids.generate(now)?, ids.generate(now)?);
//! let key = SoftwareSigner::generate(SignatureAlgorithm::EdDsa, &mut SeededEntropy::new(2))?;
//! let mut registry = DeviceRegistry::new();
//! registry.enroll(device, location, key.public_key().clone())?;
//!
//! // The register records the first line of an order.
//! let config = LogConfig {
//!     device,
//!     location,
//!     head: LogHead::EMPTY,
//!     latest_hlc: Hlc::ZERO,
//!     max_forward_drift: Duration::from_secs(60),
//! };
//! let mut writer = LogWriter::new(config, key, SeededEntropy::new(3));
//! let draft = EventDraft {
//!     stream: StreamRef { kind: StreamKind::new("order")?, id: ids.generate(now)? },
//!     schema: SchemaRef { name: SchemaName::new("order.line_added")?, version: NonZeroU32::MIN },
//!     business_date: "2026-09-27".parse()?,
//!     actor: Actor::TeamMember(ids.generate(now)?),
//!     approval: None,
//!     causation: None,
//!     correlation: None,
//!     payload: Payload::new(&Value::from("espresso"))?,
//! };
//! let pending = writer.prepare(draft, now)?;
//! // ... stores `pending.event()` durably, then commits it.
//! let event = pending.commit();
//!
//! // The hub receives it: it checks the signature, then where the event fits in the register's
//! // log.
//! let received = registry.verify(&event.to_bytes())?;
//! assert_eq!(LogHead::EMPTY.link(&received)?, Link::Next);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod cbor;
pub mod cose;
pub mod envelope;
pub mod event;
pub mod hash;
pub mod keys;
pub mod log;
pub mod verify;
