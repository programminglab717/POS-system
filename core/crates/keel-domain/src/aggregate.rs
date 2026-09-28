//! Aggregates: state folded from events.
//!
//! An aggregate's state is a fold over its stream's events in canonical order (see the offline
//! and sync design, §4). Folds are **total**: every event applies in every state. An event that
//! doesn't fit the state, because its device validated it against a different, concurrent view,
//! still applies with a well-defined result, and the aggregate records a conflict for a person to
//! resolve. No event is ever rejected or discarded.
//!
//! Replicas fold the same events in the same order, so they reach the same state.

use core::cmp::Ordering;
use core::num::NonZeroU64;

use keel_events::envelope::{self, Actor, Device, Event, EventBody, Location, SchemaRef};
use keel_events::event::SignedEvent;
use keel_types::{BusinessDate, Hlc, Id};

use crate::schema::{DecodeError, DomainEvent};

/// What the envelope records about an event, as folds use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventMeta {
    /// The event's identifier.
    pub event_id: Id<Event>,
    /// Where it happened.
    pub location: Id<Location>,
    /// The device that recorded it.
    pub origin_device: Id<Device>,
    /// Its position in the device's log.
    pub origin_seq: NonZeroU64,
    /// When, by the device's hybrid logical clock.
    pub hlc: Hlc,
    /// The business date it is reported under.
    pub business_date: BusinessDate,
    /// Who or what caused it.
    pub actor: Actor,
    /// The event granting approval, if the action needed one.
    pub approval: Option<Id<Event>>,
}

impl EventMeta {
    /// The metadata of an event body.
    pub fn of(body: &EventBody) -> EventMeta {
        EventMeta {
            event_id: body.event_id,
            location: body.location,
            origin_device: body.origin_device,
            origin_seq: body.origin_seq,
            hlc: body.hlc,
            business_date: body.business_date,
            actor: body.actor.clone(),
            approval: body.approval,
        }
    }
}

/// The state of an aggregate, folded from its stream's events.
pub trait Aggregate {
    /// The aggregate's events.
    type Event: DomainEvent;

    /// The identifier of the aggregate's stream.
    fn stream_id(&self) -> Id<envelope::Aggregate>;

    /// Applies an event. Every event applies in every state: an event that doesn't fit is
    /// recorded as a conflict, never rejected.
    fn apply(&mut self, meta: &EventMeta, event: &Self::Event);

    /// Records an event that couldn't be applied, because this kernel doesn't know its schema,
    /// its payload is malformed, or it belongs to another stream.
    fn skip(&mut self, meta: &EventMeta, schema: &SchemaRef, reason: &DecodeError);
}

/// Folds a verified event into `aggregate`: decodes its payload, then applies it, or records why
/// it was skipped.
pub fn fold<A: Aggregate>(aggregate: &mut A, event: &SignedEvent) {
    let body = event.body();
    let meta = EventMeta::of(body);
    let belongs =
        body.stream.kind.as_str() == A::Event::STREAM && body.stream.id == aggregate.stream_id();
    let decoded = if belongs {
        A::Event::decode(&body.schema, &body.payload)
    } else {
        Err(DecodeError::WrongStream)
    };
    match decoded {
        Ok(decoded) => aggregate.apply(&meta, &decoded),
        Err(reason) => aggregate.skip(&meta, &body.schema, &reason),
    }
}

/// The canonical order of provisional events, which the hub hasn't sequenced yet: by hybrid
/// logical clock, then by device, then by position in the device's log. Sequenced events come
/// first, in the hub's order; `keel-sync` handles that part.
pub fn provisional_order(a: &EventBody, b: &EventBody) -> Ordering {
    (a.hlc, a.origin_device, a.origin_seq).cmp(&(b.hlc, b.origin_device, b.origin_seq))
}
