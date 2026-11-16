//! The names of events' fields (ADR-0037, decision 3): what an export calls each of a payload's
//! top-level keys. They are the names each schema's decoder asks for, recorded as it decodes, so
//! a new schema version names its fields by being decodable at all.

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;

use crate::availability::AvailabilityEvent;
use crate::codec::named;
use crate::drawer::DrawerEvent;
use crate::hub::HubEvent;
use crate::kitchen::KitchenEvent;
use crate::order::OrderEvent;
use crate::payment::PaymentEvent;
use crate::publication::PublicationEvent;
use crate::refund::RefundEvent;
use crate::schema::{DomainEvent, SchemaId};
use crate::sequence::SequenceEvent;
use crate::staff::ApprovalEvent;
use crate::stock::StockEvent;

/// A payload's top-level key, and its name.
pub type Named = (u64, &'static str);

/// Every schema this kernel decodes, of every kind of stream: the schema registry, whole.
pub fn schemas() -> Vec<SchemaId> {
    [
        OrderEvent::SCHEMAS,
        PaymentEvent::SCHEMAS,
        DrawerEvent::SCHEMAS,
        RefundEvent::SCHEMAS,
        AvailabilityEvent::SCHEMAS,
        KitchenEvent::SCHEMAS,
        StockEvent::SCHEMAS,
        SequenceEvent::SCHEMAS,
        HubEvent::SCHEMAS,
        PublicationEvent::SCHEMAS,
        ApprovalEvent::SCHEMAS,
    ]
    .concat()
}

fn knows<E: DomainEvent>(schema: &SchemaRef) -> bool {
    E::SCHEMAS.iter().any(|known| known.matches(schema))
}

fn decoded<E: DomainEvent>(schema: &SchemaRef, payload: &Value) -> Option<Vec<Named>> {
    let (decoded, names) = named(|| E::from_value(schema, payload));
    decoded.ok().map(|_| names)
}

/// Each kind of event: whether it knows a schema, and its names for a payload of it.
type Kind = (fn(&SchemaRef) -> bool, fn(&SchemaRef, &Value) -> Option<Vec<Named>>);

const KINDS: [Kind; 11] = [
    (knows::<OrderEvent>, decoded::<OrderEvent>),
    (knows::<PaymentEvent>, decoded::<PaymentEvent>),
    (knows::<DrawerEvent>, decoded::<DrawerEvent>),
    (knows::<RefundEvent>, decoded::<RefundEvent>),
    (knows::<AvailabilityEvent>, decoded::<AvailabilityEvent>),
    (knows::<KitchenEvent>, decoded::<KitchenEvent>),
    (knows::<StockEvent>, decoded::<StockEvent>),
    (knows::<SequenceEvent>, decoded::<SequenceEvent>),
    (knows::<HubEvent>, decoded::<HubEvent>),
    (knows::<PublicationEvent>, decoded::<PublicationEvent>),
    (knows::<ApprovalEvent>, decoded::<ApprovalEvent>),
];

/// The names of `payload`'s top-level keys, and of its schema's other fields, as `schema`
/// decodes them, in the order its decoder asks for them; `None` if this kernel doesn't know the
/// schema, or the payload isn't valid for it.
pub fn names(schema: &SchemaRef, payload: &Value) -> Option<Vec<Named>> {
    let (_, decode) = KINDS.iter().find(|(knows, _)| knows(schema))?;
    decode(schema, payload)
}

/// How an export writes `name`, a decoder's name for a field: in camel case, so `unit price` is
/// `unitPrice`, and a one-word name as it is.
pub fn json_name(name: &str) -> String {
    let mut words = name.split(' ').filter(|word| !word.is_empty());
    let mut text: String = words.next().unwrap_or_default().to_owned();
    for word in words {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            text.extend(first.to_uppercase());
            text.push_str(chars.as_str());
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::json_name;

    #[test]
    fn a_name_of_several_words_is_written_in_camel_case() {
        assert_eq!(json_name("unit price"), "unitPrice");
        assert_eq!(json_name("as of"), "asOf");
        assert_eq!(json_name("amount"), "amount");
        assert_eq!(json_name("guest count"), "guestCount");
    }
}
