//! Availability events, version 1, and their payload (ADR-0033).
//!
//! Keys and encodings follow [`crate::codec`]; "optional" fields are omitted when absent. A time
//! is microseconds since the Unix epoch.
//!
//! | Schema | Keys |
//! |---|---|
//! | `availability.changed` | 1 kind (0 variant, 1 modifier), 2 available, 3 until (optional), 4 reason code (optional), 5 note (optional) |
//!
//! Beyond the types, the payload must satisfy these rules: an item put back on sale has no
//! `until`; a note comes with a reason.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_types::Timestamp;

use crate::codec::{Fields, PayloadError, Record, code_enum};
use crate::order::Reason;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

code_enum! {
    /// What kind of item an availability is of.
    pub enum ItemKind {
        /// A variant, rung as a line.
        Variant = 0,
        /// A modifier, chosen on a line.
        Modifier = 1,
    }
}

/// An item's availability changed: 86'd, until a time or until put back; or put back on sale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailabilityChanged {
    /// What kind of item it is.
    pub kind: ItemKind,
    /// Whether it is on sale.
    pub available: bool,
    /// For an 86, when it is back on sale; `None` until it is put back. Never for an item put
    /// back.
    pub until: Option<Timestamp>,
    /// Why, if the person said.
    pub reason: Option<Reason>,
}

/// Something that happened to an item's availability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AvailabilityEvent {
    /// It changed.
    Changed(AvailabilityChanged),
}

const CHANGED: SchemaId = SchemaId { name: "availability.changed", version: 1 };

impl AvailabilityEvent {
    /// Checks the payload's rules: see the module's.
    pub(crate) fn validate(&self) -> Result<(), PayloadError> {
        let AvailabilityEvent::Changed(changed) = self;
        if changed.available && changed.until.is_some() {
            return Err(PayloadError::Invalid("until"));
        }
        Ok(())
    }
}

impl DomainEvent for AvailabilityEvent {
    const STREAM: &'static str = "availability";

    const SCHEMAS: &'static [SchemaId] = &[CHANGED];

    fn schema(&self) -> SchemaId {
        match self {
            AvailabilityEvent::Changed(_) => CHANGED,
        }
    }

    fn to_value(&self) -> Value {
        let AvailabilityEvent::Changed(changed) = self;
        Record::default()
            .field(1, &changed.kind)
            .field(2, &changed.available)
            .optional(3, changed.until.as_ref())
            .optional(4, changed.reason.as_ref().map(|reason| &reason.code))
            .optional(5, changed.reason.as_ref().and_then(|reason| reason.note.as_ref()))
            .build()
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<AvailabilityEvent, DecodeError> {
        if !CHANGED.matches(schema) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let kind = fields.required(1, "kind")?;
        let available = fields.required(2, "available")?;
        let until = fields.optional(3, "until")?;
        let code = fields.optional(4, "reason")?;
        let note = fields.optional(5, "note")?;
        fields.finish()?;
        let reason = match (code, note) {
            (Some(code), note) => Some(Reason { code, note }),
            (None, None) => None,
            (None, Some(_)) => return Err(PayloadError::Invalid("note").into()),
        };
        let event =
            AvailabilityEvent::Changed(AvailabilityChanged { kind, available, until, reason });
        event.validate()?;
        Ok(event)
    }
}
