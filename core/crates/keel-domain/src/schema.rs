//! Event schemas: every event payload Keel writes, by name and version.
//!
//! Each aggregate's events are one Rust enum implementing [`DomainEvent`]. An event is written as
//! the current version of its schema, and a kernel decodes every version it knows, strictly;
//! older versions are converted (upcast) to the current form as they are decoded. Together, the
//! aggregates' [`DomainEvent::SCHEMAS`] lists are the schema registry, and each listed schema has
//! a pinned example payload that tests decode, so no release can break a stored event.
//!
//! A kernel never writes a version that another kernel at its location doesn't know: the sync
//! protocol negotiates which versions a location's writers may use. An event whose schema a
//! kernel doesn't know anyway (from a newer kernel) or whose payload is malformed is kept in the
//! log, so its device's chain stays intact, but it isn't applied: the aggregate records that it
//! skipped it.

use keel_events::cbor::Value;
use keel_events::envelope::{Payload, SchemaName, SchemaRef};

use crate::codec::PayloadError;

/// A schema's name and version, such as `order.line_added` version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaId {
    /// The schema's name: dot-separated identifiers, starting with the stream kind.
    pub name: &'static str,
    /// The schema's version, from 1.
    pub version: u32,
}

impl SchemaId {
    /// Whether `schema` is this schema.
    pub fn matches(self, schema: &SchemaRef) -> bool {
        schema.name.as_str() == self.name && schema.version.get() == self.version
    }

    /// The schema as the event envelope records it.
    ///
    /// # Errors
    /// [`SchemaError::InvalidName`] if the name or version is invalid. The registry's tests check
    /// every schema, so this doesn't happen for Keel's own schemas.
    pub fn to_ref(self) -> Result<SchemaRef, SchemaError> {
        let name = SchemaName::new(self.name).map_err(|_| SchemaError::InvalidName(self))?;
        let version = self.version.try_into().map_err(|_| SchemaError::InvalidName(self))?;
        Ok(SchemaRef { name, version })
    }
}

/// The events of one kind of aggregate.
pub trait DomainEvent: Sized {
    /// The kind of stream these events belong to, such as `order`.
    const STREAM: &'static str;

    /// Every schema this kernel decodes for the stream, oldest first within each name.
    const SCHEMAS: &'static [SchemaId];

    /// The schema this event is written as: the current version of its name.
    fn schema(&self) -> SchemaId;

    /// The event's payload.
    fn to_value(&self) -> Value;

    /// Decodes a payload written as `schema`.
    ///
    /// # Errors
    /// [`DecodeError::UnknownSchema`] if this kernel doesn't know the schema, and
    /// [`DecodeError::Malformed`] if the payload isn't valid for it.
    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<Self, DecodeError>;

    /// The schema and payload the event is written with.
    ///
    /// # Errors
    /// [`SchemaError`] if the payload nests too deeply to be decoded (a deep tree of modifiers,
    /// say), or the schema is invalid.
    fn encode(&self) -> Result<(SchemaRef, Payload), SchemaError> {
        let schema = self.schema().to_ref()?;
        let payload = Payload::new(&self.to_value()).map_err(|_| SchemaError::TooDeep)?;
        Ok((schema, payload))
    }

    /// Decodes an event's payload.
    ///
    /// # Errors
    /// As [`DomainEvent::from_value`].
    fn decode(schema: &SchemaRef, payload: &Payload) -> Result<Self, DecodeError> {
        let value = payload.value().map_err(|_| DecodeError::Malformed(PayloadError::NotAMap))?;
        Self::from_value(schema, &value)
    }
}

/// Why an event payload couldn't be decoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// A schema this kernel doesn't know, written by a newer kernel.
    #[error("unknown schema")]
    UnknownSchema,
    /// A known schema, but the payload isn't valid for it.
    #[error("malformed payload: {0}")]
    Malformed(PayloadError),
    /// The event belongs to another stream than the aggregate it was folded into.
    #[error("the event belongs to another stream")]
    WrongStream,
}

impl From<PayloadError> for DecodeError {
    fn from(error: PayloadError) -> DecodeError {
        DecodeError::Malformed(error)
    }
}

/// Why an event couldn't be encoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SchemaError {
    /// A schema whose name or version isn't valid in the envelope.
    #[error("invalid schema {}/{}", .0.name, .0.version)]
    InvalidName(SchemaId),
    /// The payload nests arrays and maps too deeply to be decoded.
    #[error("the payload nests too deeply")]
    TooDeep,
}
