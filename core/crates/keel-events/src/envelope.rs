//! The event envelope: what every event records, whatever it is about.
//!
//! An [`EventBody`] is the signed content of an event: its identity, where and when it happened,
//! who did it, what it is about, its payload, and the hash of the previous event in its device's
//! log. It is encoded as a canonical CBOR map with small integer keys (format 1):
//!
//! | Key | Field | Encoding |
//! |---|---|---|
//! | 0 | format | unsigned integer: 1 |
//! | 1 | event id | 16 bytes: a UUIDv7 |
//! | 2 | location | 16 bytes: a UUIDv7 |
//! | 3 | stream kind | text, such as `"order"` |
//! | 4 | stream id | 16 bytes: a UUIDv7 |
//! | 5 | schema name | text, such as `"order.line_added"` |
//! | 6 | schema version | unsigned integer, at least 1 |
//! | 7 | origin device | 16 bytes: a UUIDv7 |
//! | 8 | origin sequence | unsigned integer, at least 1 |
//! | 9 | hybrid logical clock | unsigned integer (packed) |
//! | 10 | business date | text, `YYYY-MM-DD` |
//! | 11 | actor | array: `[kind, id]`, or `[4, name]` for a system component |
//! | 12 | approval (optional) | 16 bytes: the approving event's id |
//! | 13 | causation (optional) | 16 bytes: the causing command's or event's id |
//! | 14 | correlation (optional) | 16 bytes |
//! | 15 | payload | byte string holding one canonical CBOR item |
//! | 16 | previous hash | 32 bytes |
//!
//! Optional fields are omitted when absent, never written as null, so each body has exactly one
//! encoding. Decoding is strict: unknown keys, missing fields, wrong types and invalid values are
//! all rejected. Format 1 is closed; new fields will need a new format number.

use core::fmt;
use core::num::NonZeroU64;

use keel_types::{BusinessDate, Hlc, Id};

use crate::cbor::{self, CborError, Map, Value};
use crate::hash::EventHash;

/// The envelope format written and accepted by this version of Keel.
pub const FORMAT: u64 = 1;

/// Marks identifiers of events.
#[derive(Debug)]
pub enum Event {}
/// Marks identifiers of the aggregates that event streams belong to (an order, a drawer session).
#[derive(Debug)]
pub enum Aggregate {}
/// Marks identifiers of locations: stores, restaurants, venues, and online channels.
#[derive(Debug)]
pub enum Location {}
/// Marks identifiers of enrolled devices: registers, handhelds, hubs, cloud nodes.
#[derive(Debug)]
pub enum Device {}
/// Marks identifiers of team members.
#[derive(Debug)]
pub enum TeamMember {}
/// Marks identifiers of customers.
#[derive(Debug)]
pub enum Customer {}
/// Marks identifiers of API integrations.
#[derive(Debug)]
pub enum Integration {}
/// Marks identifiers of extensions (sandboxed functions).
#[derive(Debug)]
pub enum Extension {}
/// Marks identifiers of what caused an event: a command, or another event.
#[derive(Debug)]
pub enum Cause {}
/// Marks correlation identifiers, which group the events of one user action across aggregates.
#[derive(Debug)]
pub enum Correlation {}

/// The signed content of an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventBody {
    /// The event's unique identifier, minted by the origin device.
    pub event_id: Id<Event>,
    /// Where the event happened.
    pub location: Id<Location>,
    /// The stream (aggregate) the event belongs to.
    pub stream: StreamRef,
    /// The payload's schema.
    pub schema: SchemaRef,
    /// The device whose log holds the event.
    pub origin_device: Id<Device>,
    /// The event's position in its device's log: 1 for the first event, with no gaps.
    pub origin_seq: NonZeroU64,
    /// The device's hybrid logical clock when the event was recorded.
    pub hlc: Hlc,
    /// The business date the event is reported under.
    pub business_date: BusinessDate,
    /// Who or what caused the event.
    pub actor: Actor,
    /// The event granting approval, when the action needed a manager's approval.
    pub approval: Option<Id<Event>>,
    /// The command or event that caused this event.
    pub causation: Option<Id<Cause>>,
    /// Groups the events of one user action across aggregates.
    pub correlation: Option<Id<Correlation>>,
    /// The event-specific data, as canonical CBOR.
    pub payload: Payload,
    /// The hash of the previous event in the device's log; [`EventHash::ZERO`] for the first.
    pub prev_hash: EventHash,
}

/// Envelope keys.
mod key {
    pub(super) const FORMAT: u64 = 0;
    pub(super) const EVENT_ID: u64 = 1;
    pub(super) const LOCATION: u64 = 2;
    pub(super) const STREAM_KIND: u64 = 3;
    pub(super) const STREAM_ID: u64 = 4;
    pub(super) const SCHEMA_NAME: u64 = 5;
    pub(super) const SCHEMA_VERSION: u64 = 6;
    pub(super) const ORIGIN_DEVICE: u64 = 7;
    pub(super) const ORIGIN_SEQ: u64 = 8;
    pub(super) const HLC: u64 = 9;
    pub(super) const BUSINESS_DATE: u64 = 10;
    pub(super) const ACTOR: u64 = 11;
    pub(super) const APPROVAL: u64 = 12;
    pub(super) const CAUSATION: u64 = 13;
    pub(super) const CORRELATION: u64 = 14;
    pub(super) const PAYLOAD: u64 = 15;
    pub(super) const PREV_HASH: u64 = 16;
    /// One more than the largest key.
    pub(super) const COUNT: usize = 17;
}

impl EventBody {
    /// The body's canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut entries = vec![
            (key::FORMAT, Value::Unsigned(FORMAT)),
            (key::EVENT_ID, id_value(self.event_id)),
            (key::LOCATION, id_value(self.location)),
            (key::STREAM_KIND, Value::from(self.stream.kind.as_str())),
            (key::STREAM_ID, id_value(self.stream.id)),
            (key::SCHEMA_NAME, Value::from(self.schema.name.as_str())),
            (key::SCHEMA_VERSION, Value::Unsigned(u64::from(self.schema.version.get()))),
            (key::ORIGIN_DEVICE, id_value(self.origin_device)),
            (key::ORIGIN_SEQ, Value::Unsigned(self.origin_seq.get())),
            (key::HLC, Value::Unsigned(self.hlc.to_u64())),
            (key::BUSINESS_DATE, Value::from(self.business_date.to_string())),
            (key::ACTOR, self.actor.to_value()),
        ];
        if let Some(approval) = self.approval {
            entries.push((key::APPROVAL, id_value(approval)));
        }
        if let Some(causation) = self.causation {
            entries.push((key::CAUSATION, id_value(causation)));
        }
        if let Some(correlation) = self.correlation {
            entries.push((key::CORRELATION, id_value(correlation)));
        }
        entries.push((key::PAYLOAD, Value::from(self.payload.as_bytes())));
        entries.push((key::PREV_HASH, Value::from(self.prev_hash.as_bytes().as_slice())));
        let entries = entries.into_iter().map(|(key, value)| (Value::Unsigned(key), value));
        // The keys are distinct constants, so this can't fail.
        Value::Map(Map::from_entries(entries).unwrap_or_default()).encode()
    }

    /// Decodes a body, strictly.
    ///
    /// # Errors
    /// [`EnvelopeError`] if the bytes aren't canonical CBOR, or aren't a valid format 1 body.
    pub fn decode(bytes: &[u8]) -> Result<EventBody, EnvelopeError> {
        let value = cbor::decode(bytes)?;
        let map = value.as_map().ok_or(EnvelopeError::NotAMap)?;
        let mut fields: [Option<&Value>; key::COUNT] = [None; key::COUNT];
        for (key, value) in map.iter() {
            let slot = key
                .as_u64()
                .and_then(|key| usize::try_from(key).ok())
                .and_then(|index| fields.get_mut(index))
                .ok_or_else(|| EnvelopeError::UnknownField(key.to_string()))?;
            *slot = Some(value);
        }
        let field = |key: u64| -> Option<&Value> {
            usize::try_from(key).ok().and_then(|index| fields.get(index).copied().flatten())
        };
        let required =
            |key: u64, name: &'static str| field(key).ok_or(EnvelopeError::Missing(name));

        let format = required(key::FORMAT, "format")?.as_u64();
        if format != Some(FORMAT) {
            return Err(EnvelopeError::UnsupportedFormat);
        }
        let stream_kind = text(required(key::STREAM_KIND, "stream kind")?, "stream kind")?;
        let schema_name = text(required(key::SCHEMA_NAME, "schema name")?, "schema name")?;
        let schema_version = required(key::SCHEMA_VERSION, "schema version")?
            .as_u64()
            .and_then(|version| u32::try_from(version).ok())
            .and_then(core::num::NonZeroU32::new)
            .ok_or(EnvelopeError::Invalid("schema version"))?;
        let business_date = text(required(key::BUSINESS_DATE, "business date")?, "business date")?
            .parse()
            .map_err(|_| EnvelopeError::Invalid("business date"))?;
        let payload = required(key::PAYLOAD, "payload")?
            .as_bytes()
            .ok_or(EnvelopeError::Invalid("payload"))
            .and_then(|bytes| Payload::from_bytes(bytes.to_vec()))?;
        let prev_hash = required(key::PREV_HASH, "previous hash")?
            .as_bytes()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .map(EventHash::from_bytes)
            .ok_or(EnvelopeError::Invalid("previous hash"))?;
        Ok(EventBody {
            event_id: id(required(key::EVENT_ID, "event id")?, "event id")?,
            location: id(required(key::LOCATION, "location")?, "location")?,
            stream: StreamRef {
                kind: StreamKind::new(stream_kind)?,
                id: id(required(key::STREAM_ID, "stream id")?, "stream id")?,
            },
            schema: SchemaRef { name: SchemaName::new(schema_name)?, version: schema_version },
            origin_device: id(required(key::ORIGIN_DEVICE, "origin device")?, "origin device")?,
            origin_seq: required(key::ORIGIN_SEQ, "origin sequence")?
                .as_u64()
                .and_then(NonZeroU64::new)
                .ok_or(EnvelopeError::Invalid("origin sequence"))?,
            hlc: Hlc::from_u64(
                required(key::HLC, "hybrid logical clock")?
                    .as_u64()
                    .ok_or(EnvelopeError::Invalid("hybrid logical clock"))?,
            ),
            business_date,
            actor: Actor::from_value(required(key::ACTOR, "actor")?)?,
            approval: field(key::APPROVAL).map(|value| id(value, "approval")).transpose()?,
            causation: field(key::CAUSATION).map(|value| id(value, "causation")).transpose()?,
            correlation: field(key::CORRELATION)
                .map(|value| id(value, "correlation"))
                .transpose()?,
            payload,
            prev_hash,
        })
    }
}

fn id_value<T>(id: Id<T>) -> Value {
    Value::from(id.to_bytes().as_slice())
}

fn id<T>(value: &Value, name: &'static str) -> Result<Id<T>, EnvelopeError> {
    value
        .as_bytes()
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
        .and_then(|bytes| Id::from_bytes(bytes).ok())
        .ok_or(EnvelopeError::Invalid(name))
}

fn text<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, EnvelopeError> {
    value.as_text().ok_or(EnvelopeError::Invalid(name))
}

/// The stream an event belongs to: an aggregate's kind and identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StreamRef {
    /// The kind of aggregate, such as `order`.
    pub kind: StreamKind,
    /// The aggregate's identifier.
    pub id: Id<Aggregate>,
}

/// The payload's schema: a name and a version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SchemaRef {
    /// The schema's name, such as `order.line_added`.
    pub name: SchemaName,
    /// The schema's version, from 1.
    pub version: core::num::NonZeroU32,
}

impl fmt::Display for SchemaRef {
    /// `name/version`, as in `order.line_added/3`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.version)
    }
}

/// A kind of aggregate: 1 to 32 characters, a lowercase letter then lowercase letters, digits
/// and underscores. For example `order`, `drawer_session`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamKind(String);

impl StreamKind {
    /// A validated stream kind.
    ///
    /// # Errors
    /// [`EnvelopeError::Invalid`] if the name breaks the rules above.
    pub fn new(kind: &str) -> Result<StreamKind, EnvelopeError> {
        if is_identifier(kind) && kind.len() <= 32 {
            Ok(StreamKind(kind.to_owned()))
        } else {
            Err(EnvelopeError::Invalid("stream kind"))
        }
    }

    /// The kind as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The name of a Keel component acting on its own, such as `scheduler` or `sync`: 1 to 32
/// characters, a lowercase letter then lowercase letters, digits and underscores.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Component(String);

impl Component {
    /// A validated component name.
    ///
    /// # Errors
    /// [`EnvelopeError::Invalid`] if the name breaks the rules above.
    pub fn new(name: &str) -> Result<Component, EnvelopeError> {
        if is_identifier(name) && name.len() <= 32 {
            Ok(Component(name.to_owned()))
        } else {
            Err(EnvelopeError::Invalid("actor"))
        }
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A schema name: dot-separated identifiers, each a lowercase letter then lowercase letters,
/// digits and underscores; at most 64 characters. For example `order.line_added`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaName(String);

impl SchemaName {
    /// A validated schema name.
    ///
    /// # Errors
    /// [`EnvelopeError::Invalid`] if the name breaks the rules above.
    pub fn new(name: &str) -> Result<SchemaName, EnvelopeError> {
        if name.len() <= 64 && name.split('.').all(is_identifier) {
            Ok(SchemaName(name.to_owned()))
        } else {
            Err(EnvelopeError::Invalid("schema name"))
        }
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SchemaName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A lowercase letter, then lowercase letters, digits and underscores.
fn is_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Who or what caused an event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Actor {
    /// A team member, signed in on the device.
    TeamMember(Id<TeamMember>),
    /// A customer, for example ordering at a kiosk or online.
    Customer(Id<Customer>),
    /// An API integration.
    Integration(Id<Integration>),
    /// An extension (a sandboxed function).
    Extension(Id<Extension>),
    /// A component of Keel itself, such as the scheduler closing a business day.
    System(Component),
}

impl Actor {
    const TEAM_MEMBER: u64 = 0;
    const CUSTOMER: u64 = 1;
    const INTEGRATION: u64 = 2;
    const EXTENSION: u64 = 3;
    const SYSTEM: u64 = 4;

    fn to_value(&self) -> Value {
        let (kind, id) = match self {
            Actor::TeamMember(id) => (Actor::TEAM_MEMBER, id_value(*id)),
            Actor::Customer(id) => (Actor::CUSTOMER, id_value(*id)),
            Actor::Integration(id) => (Actor::INTEGRATION, id_value(*id)),
            Actor::Extension(id) => (Actor::EXTENSION, id_value(*id)),
            Actor::System(name) => (Actor::SYSTEM, Value::from(name.as_str())),
        };
        Value::Array(vec![Value::Unsigned(kind), id])
    }

    fn from_value(value: &Value) -> Result<Actor, EnvelopeError> {
        let invalid = EnvelopeError::Invalid("actor");
        let Some([kind, id_or_name]) = value.as_array() else {
            return Err(invalid);
        };
        match kind.as_u64() {
            Some(Actor::TEAM_MEMBER) => Ok(Actor::TeamMember(id(id_or_name, "actor")?)),
            Some(Actor::CUSTOMER) => Ok(Actor::Customer(id(id_or_name, "actor")?)),
            Some(Actor::INTEGRATION) => Ok(Actor::Integration(id(id_or_name, "actor")?)),
            Some(Actor::EXTENSION) => Ok(Actor::Extension(id(id_or_name, "actor")?)),
            Some(Actor::SYSTEM) => {
                id_or_name.as_text().ok_or(invalid).and_then(Component::new).map(Actor::System)
            }
            _ => Err(invalid),
        }
    }
}

/// An event's payload: exactly one canonical CBOR data item.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Payload(Vec<u8>);

impl Payload {
    /// A payload holding `value`.
    ///
    /// # Errors
    /// [`EnvelopeError::Cbor`] if `value` nests arrays and maps deeper than [`cbor::MAX_DEPTH`]:
    /// no replica could decode it, so an event carrying it would stall its device's log.
    pub fn new(value: &Value) -> Result<Payload, EnvelopeError> {
        Payload::from_bytes(value.encode())
    }

    /// A payload from encoded bytes, which must be one canonical CBOR data item.
    ///
    /// # Errors
    /// [`EnvelopeError::Cbor`] otherwise.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Payload, EnvelopeError> {
        cbor::decode(&bytes)?;
        Ok(Payload(bytes))
    }

    /// The encoded payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The decoded payload.
    ///
    /// # Errors
    /// Never, in practice: the payload was checked when it was created.
    pub fn value(&self) -> Result<Value, CborError> {
        cbor::decode(&self.0)
    }
}

impl fmt::Debug for Payload {
    /// The payload in diagnostic notation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value() {
            Ok(value) => write!(f, "Payload({value})"),
            Err(_) => f.write_str("Payload(<invalid>)"),
        }
    }
}

/// Why an event body was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EnvelopeError {
    /// The bytes aren't canonical CBOR.
    #[error(transparent)]
    Cbor(#[from] CborError),
    /// The body isn't a map.
    #[error("an event body must be a map")]
    NotAMap,
    /// The body is in a format this version of Keel doesn't know.
    #[error("unsupported event body format")]
    UnsupportedFormat,
    /// A key that format 1 doesn't define.
    #[error("unknown field {0}")]
    UnknownField(String),
    /// A required field is missing.
    #[error("missing {0}")]
    Missing(&'static str),
    /// A field has the wrong type or an invalid value.
    #[error("invalid {0}")]
    Invalid(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::golden;

    /// The golden body as a map, with `change` applied to its entries, re-encoded canonically.
    fn altered(change: impl FnOnce(&mut Vec<(Value, Value)>)) -> Vec<u8> {
        let value = cbor::decode(&golden::body().encode()).unwrap();
        let Value::Map(map) = value else { panic!("the body is a map") };
        let mut entries = map.into_entries();
        change(&mut entries);
        Value::Map(Map::from_entries(entries).unwrap()).encode()
    }

    fn set(key: u64, value: Value) -> impl FnOnce(&mut Vec<(Value, Value)>) {
        move |entries| {
            entries.retain(|(k, _)| k.as_u64() != Some(key));
            entries.push((Value::Unsigned(key), value));
        }
    }

    fn remove(key: u64) -> impl FnOnce(&mut Vec<(Value, Value)>) {
        move |entries| entries.retain(|(k, _)| k.as_u64() != Some(key))
    }

    fn uuid_v7() -> Value {
        Value::Bytes(vec![0x01, 0x92, 0xf0, 0xc1, 0, 0, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, 1])
    }

    fn uuid_v4() -> Value {
        Value::Bytes(vec![0x01, 0x92, 0xf0, 0xc1, 0, 0, 0x40, 0, 0x80, 0, 0, 0, 0, 0, 0, 1])
    }

    #[test]
    fn round_trip_with_every_optional_field() {
        let mut body = golden::body();
        body.approval = Some(Id::parse("0192f0c1-0000-7000-8000-0000000000a1").unwrap());
        body.causation = Some(Id::parse("0192f0c1-0000-7000-8000-0000000000c1").unwrap());
        body.correlation = Some(Id::parse("0192f0c1-0000-7000-8000-0000000000c2").unwrap());
        body.actor = Actor::System(Component::new("scheduler").unwrap());
        assert_eq!(EventBody::decode(&body.encode()), Ok(body));
    }

    #[test]
    fn every_required_field_is_required() {
        let required = [
            (key::FORMAT, "format"),
            (key::EVENT_ID, "event id"),
            (key::LOCATION, "location"),
            (key::STREAM_KIND, "stream kind"),
            (key::STREAM_ID, "stream id"),
            (key::SCHEMA_NAME, "schema name"),
            (key::SCHEMA_VERSION, "schema version"),
            (key::ORIGIN_DEVICE, "origin device"),
            (key::ORIGIN_SEQ, "origin sequence"),
            (key::HLC, "hybrid logical clock"),
            (key::BUSINESS_DATE, "business date"),
            (key::ACTOR, "actor"),
            (key::PAYLOAD, "payload"),
            (key::PREV_HASH, "previous hash"),
        ];
        for (key, name) in required {
            assert_eq!(
                EventBody::decode(&altered(remove(key))),
                Err(EnvelopeError::Missing(name)),
                "{name}"
            );
        }
    }

    #[test]
    fn invalid_fields_are_rejected() {
        use EnvelopeError::{Invalid, UnknownField, UnsupportedFormat};
        let cases: Vec<(Vec<u8>, EnvelopeError)> = vec![
            (altered(set(key::FORMAT, Value::Unsigned(2))), UnsupportedFormat),
            (altered(set(17, Value::Null)), UnknownField("17".to_owned())),
            (
                altered(|entries| entries.push((Value::from("extra"), Value::Null))),
                UnknownField("\"extra\"".to_owned()),
            ),
            (
                altered(|entries| entries.push((Value::integer(-2), Value::Null))),
                UnknownField("-2".to_owned()),
            ),
            (altered(set(key::EVENT_ID, uuid_v4())), Invalid("event id")),
            (altered(set(key::LOCATION, Value::Bytes(vec![0; 15]))), Invalid("location")),
            (altered(set(key::STREAM_KIND, Value::from("Order"))), Invalid("stream kind")),
            (altered(set(key::STREAM_KIND, Value::from(""))), Invalid("stream kind")),
            (altered(set(key::SCHEMA_NAME, Value::from("order..added"))), Invalid("schema name")),
            (altered(set(key::SCHEMA_VERSION, Value::Unsigned(0))), Invalid("schema version")),
            (
                altered(set(key::SCHEMA_VERSION, Value::Unsigned(1 << 32))),
                Invalid("schema version"),
            ),
            (
                altered(set(key::SCHEMA_VERSION, Value::Unsigned((1 << 32) + 1))),
                Invalid("schema version"),
            ),
            (altered(set(key::ORIGIN_SEQ, Value::Unsigned(0))), Invalid("origin sequence")),
            (altered(set(key::ORIGIN_SEQ, Value::integer(-1))), Invalid("origin sequence")),
            (altered(set(key::HLC, Value::from("now"))), Invalid("hybrid logical clock")),
            (altered(set(key::BUSINESS_DATE, Value::from("2026-02-30"))), Invalid("business date")),
            (altered(set(key::BUSINESS_DATE, Value::from("2026-9-27"))), Invalid("business date")),
            (
                altered(set(key::BUSINESS_DATE, Value::from(" 2026-09-27"))),
                Invalid("business date"),
            ),
            (
                altered(set(key::BUSINESS_DATE, Value::from("+2026-09-27"))),
                Invalid("business date"),
            ),
            (altered(set(key::BUSINESS_DATE, Value::from("0000-01-01"))), Invalid("business date")),
            (
                altered(set(key::BUSINESS_DATE, Value::Unsigned(20_260_927))),
                Invalid("business date"),
            ),
            (
                altered(set(key::ACTOR, Value::Array(vec![Value::Unsigned(9), uuid_v4()]))),
                Invalid("actor"),
            ),
            (
                altered(set(
                    key::ACTOR,
                    Value::Array(vec![Value::Unsigned(4), Value::from("Bad")]),
                )),
                Invalid("actor"),
            ),
            (altered(set(key::ACTOR, Value::Array(vec![Value::Unsigned(0)]))), Invalid("actor")),
            (
                altered(set(
                    key::ACTOR,
                    Value::Array(vec![Value::Unsigned(0), uuid_v7(), Value::Null]),
                )),
                Invalid("actor"),
            ),
            (altered(set(key::APPROVAL, Value::Null)), Invalid("approval")),
            (altered(set(key::PREV_HASH, Value::Bytes(vec![0; 31]))), Invalid("previous hash")),
            (altered(set(key::PAYLOAD, Value::from("text"))), Invalid("payload")),
        ];
        for (bytes, expected) in cases {
            assert_eq!(EventBody::decode(&bytes), Err(expected.clone()), "{expected}");
        }
        // A payload that isn't canonical CBOR.
        let non_canonical = altered(set(key::PAYLOAD, Value::Bytes(vec![0x18, 0x17])));
        assert!(matches!(EventBody::decode(&non_canonical), Err(EnvelopeError::Cbor(_))));
        assert_eq!(EventBody::decode(&Value::Unsigned(1).encode()), Err(EnvelopeError::NotAMap));
    }

    #[test]
    fn payloads_must_be_decodable() {
        let nested = |depth| (0..depth).fold(Value::Null, |inner, _| Value::Array(vec![inner]));
        let deepest = Payload::new(&nested(cbor::MAX_DEPTH)).unwrap();
        assert_eq!(deepest.value(), Ok(nested(cbor::MAX_DEPTH)));
        let too_deep = nested(cbor::MAX_DEPTH.checked_add(1).unwrap());
        assert!(matches!(Payload::new(&too_deep), Err(EnvelopeError::Cbor(_))));
    }

    #[test]
    fn names_follow_their_rules() {
        for good in ["order", "drawer_session", "a", "x1", &"a".repeat(32)] {
            assert!(StreamKind::new(good).is_ok(), "{good}");
            assert!(Component::new(good).is_ok(), "{good}");
        }
        for bad in
            ["", "Order", "orDer", "1order", "_order", "order-line", "order.line", &"a".repeat(33)]
        {
            assert!(StreamKind::new(bad).is_err(), "{bad}");
            assert!(Component::new(bad).is_err(), "{bad}");
        }
        for good in ["order.line_added", "payment.refund.completed", "audit", &"a".repeat(64)] {
            assert!(SchemaName::new(good).is_ok(), "{good}");
        }
        for bad in ["", ".order", "order.", "order..x", "Order.x", "order.Line", &"a".repeat(65)] {
            assert!(SchemaName::new(bad).is_err(), "{bad}");
        }
        let schema = SchemaRef {
            name: SchemaName::new("order.line_added").unwrap(),
            version: core::num::NonZeroU32::new(3).unwrap(),
        };
        assert_eq!(schema.to_string(), "order.line_added/3");
    }
}
