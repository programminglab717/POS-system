//! Known answers for kitchen tickets (ADR-0034): their payloads, pinned; the fold, a ticket's
//! first sending, showing and reroute standing and its latest bump or recall; and the commands,
//! which refuse what the fold would report, and what changes nothing.

use core::num::NonZeroU8;
use std::collections::BTreeSet;

use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Hlc, Id, Quantity, Unit};

use super::*;
use crate::aggregate::{Aggregate, EventMeta};
use crate::codec::{Name, Note, PayloadError};
use crate::order::{Mode, Prefix};
use crate::profile::Allergen;
use crate::schema::{DecodeError, DomainEvent};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn location() -> Id<Location> {
    id(0x100)
}

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

fn each(n: i64) -> Quantity {
    Quantity::from_whole(n, Unit::Each).unwrap()
}

/// A large latte with oat milk and two shots, one vanilla under the second; with milk in it.
fn latte() -> KitchenItem {
    let modifier = |text: &str, prefix, quantity: u8, depth: u8| KitchenModifier {
        name: name(text),
        prefix,
        quantity: NonZeroU8::new(quantity).unwrap(),
        depth: NonZeroU8::new(depth).unwrap(),
    };
    KitchenItem {
        line: id(0x900),
        name: name("Latte (Large)"),
        quantity: each(1),
        modifiers: vec![
            modifier("Oat milk", Prefix::Plain, 1, 1),
            modifier("Shot", Prefix::Extra, 2, 1),
            modifier("Vanilla", Prefix::Plain, 1, 2),
        ],
        notes: Some(Note::new("extra hot").unwrap()),
        allergens: [Allergen::Milk].into(),
        component_of: None,
    }
}

fn sent(items: Vec<KitchenItem>) -> TicketSent {
    TicketSent { order: id(0x800), station: id(0x701), mode: Mode::DineIn, items }
}

/// Applies events to a ticket, each with fresh metadata, a millisecond apart.
struct Script {
    ticket: KitchenTicket,
    events: u64,
}

impl Script {
    fn new() -> Script {
        Script { ticket: KitchenTicket::new(id(0xA00)), events: 0 }
    }

    fn apply_at(&mut self, device: u64, location: Id<Location>, event: &KitchenEvent) -> Id<Event> {
        self.events = self.events.checked_add(1).unwrap();
        let meta = EventMeta {
            event_id: id(0xE000_u64.checked_add(self.events).unwrap()),
            location,
            origin_device: id::<Device>(device),
            origin_seq: self.events.try_into().unwrap(),
            hlc: Hlc::new(1_794_560_400_000_u64.checked_add(self.events).unwrap(), 0).unwrap(),
            business_date: "2026-11-13".parse().unwrap(),
            actor: Actor::TeamMember(id(0x300)),
            approval: None,
        };
        self.ticket.apply(&meta, event);
        meta.event_id
    }

    fn apply(&mut self, event: &KitchenEvent) -> Id<Event> {
        self.apply_at(0xD1, location(), event)
    }

    fn kinds(&self) -> Vec<(Id<Event>, ConflictKind)> {
        self.ticket.conflicts().iter().map(|conflict| (conflict.event, conflict.kind)).collect()
    }
}

#[test]
fn a_ticket_is_sent_shown_bumped_and_recalled() {
    let mut script = Script::new();
    assert!(script.ticket.sent().is_none());
    let first = script.apply(&KitchenEvent::Sent(sent(vec![latte()])));
    let (what, when) = script.ticket.sent().unwrap();
    assert_eq!(what.items, [latte()]);
    assert_eq!(when.event, first);
    assert_eq!(when.at().unwrap().as_micros(), 1_794_560_400_001_000);
    // Two screens show it: the first stands.
    let shown = script.apply_at(0xD2, location(), &KitchenEvent::Shown);
    script.apply_at(0xD3, location(), &KitchenEvent::Shown);
    assert_eq!(script.ticket.shown().unwrap().event, shown);
    // Bumped, recalled, bumped again: the latest stands.
    script.apply_at(0xD2, location(), &KitchenEvent::Bumped);
    assert!(script.ticket.is_bumped());
    script.apply_at(0xD2, location(), &KitchenEvent::Recalled);
    assert!(!script.ticket.is_bumped());
    assert!(script.ticket.bumped().is_none());
    let bumped = script.apply_at(0xD2, location(), &KitchenEvent::Bumped);
    assert_eq!(script.ticket.bumped().unwrap().event, bumped);
    assert!(script.kinds().is_empty());
}

#[test]
fn what_doesnt_fit_is_reported() {
    let mut script = Script::new();
    // Before it is sent, nothing applies.
    let early = script.apply(&KitchenEvent::Bumped);
    assert!(!script.ticket.is_bumped());
    script.apply(&KitchenEvent::Sent(sent(vec![latte()])));
    let again = script.apply(&KitchenEvent::Sent(sent(vec![latte(), latte()])));
    assert_eq!(script.ticket.sent().unwrap().0.items.len(), 1, "the first sending stands");
    let elsewhere = script.apply_at(0xD1, id(0x101), &KitchenEvent::Bumped);
    assert!(!script.ticket.is_bumped());
    // Rerouted by the register, which hadn't heard of the screen's showing: both stand.
    let rerouted = script.apply(&KitchenEvent::Rerouted(RerouteTo::StationPrinter));
    let shown = script.apply_at(0xD2, location(), &KitchenEvent::Shown);
    script.apply(&KitchenEvent::Rerouted(RerouteTo::Nowhere));
    assert_eq!(script.ticket.rerouted().unwrap().0, RerouteTo::StationPrinter, "the first stands");
    assert_eq!(script.ticket.rerouted().unwrap().1.event, rerouted);
    assert_eq!(
        script.kinds(),
        [
            (early, ConflictKind::BeforeSent),
            (again, ConflictKind::SentTwice),
            (elsewhere, ConflictKind::WrongLocation),
            (shown, ConflictKind::ShownAndRerouted),
        ]
    );
}

#[test]
fn commands_refuse_what_the_fold_would_report_and_what_changes_nothing() {
    let mut script = Script::new();
    let decide = |script: &Script, command| script.ticket.decide(location(), command);
    assert_eq!(decide(&script, KitchenCommand::Show), Err(KitchenError::NotSent));
    assert_eq!(decide(&script, KitchenCommand::Bump), Err(KitchenError::NotSent));
    assert_eq!(
        decide(&script, KitchenCommand::Send(sent(Vec::new()))),
        Err(KitchenError::Invalid(PayloadError::Invalid("items")))
    );
    let event = decide(&script, KitchenCommand::Send(sent(vec![latte()]))).unwrap();
    script.apply(&event);
    assert_eq!(
        decide(&script, KitchenCommand::Send(sent(vec![latte()]))),
        Err(KitchenError::AlreadySent)
    );
    assert_eq!(
        script.ticket.decide(id(0x101), KitchenCommand::Show),
        Err(KitchenError::WrongLocation)
    );
    assert_eq!(decide(&script, KitchenCommand::Recall), Err(KitchenError::NotBumped));
    // Rerouted before a screen shows it: not again.
    let event = decide(&script, KitchenCommand::Reroute(RerouteTo::FallbackPrinter)).unwrap();
    script.apply(&event);
    assert_eq!(
        decide(&script, KitchenCommand::Reroute(RerouteTo::Nowhere)),
        Err(KitchenError::AlreadyRerouted)
    );
    // Rerouted, its showing is recorded no more.
    assert_eq!(decide(&script, KitchenCommand::Show), Err(KitchenError::AlreadyRerouted));
    let event = decide(&script, KitchenCommand::Bump).unwrap();
    script.apply(&event);
    assert_eq!(decide(&script, KitchenCommand::Bump), Err(KitchenError::AlreadyBumped));
    assert_eq!(decide(&script, KitchenCommand::Recall), Ok(KitchenEvent::Recalled));
    // A ticket a screen showed isn't rerouted.
    let mut shown = Script::new();
    shown.apply(&KitchenEvent::Sent(sent(vec![latte()])));
    shown.apply(&KitchenEvent::Shown);
    assert_eq!(
        decide(&shown, KitchenCommand::Reroute(RerouteTo::StationPrinter)),
        Err(KitchenError::AlreadyShown)
    );
    assert!(script.kinds().is_empty(), "a command never records what the fold reports");
    // Shown, a ticket isn't shown again.
    let event = decide(&shown, KitchenCommand::Bump).unwrap();
    shown.apply(&event);
    assert_eq!(decide(&shown, KitchenCommand::Show), Err(KitchenError::AlreadyShown));
}

#[test]
fn an_item_breaking_the_payloads_rules_is_refused() {
    let invalid = |change: &dyn Fn(&mut KitchenItem)| {
        let mut item = latte();
        change(&mut item);
        KitchenEvent::Sent(sent(vec![item])).validate()
    };
    assert_eq!(invalid(&|item| item.quantity = each(0)), Err(PayloadError::Invalid("quantity")));
    // The first modifier is at depth 1, and none more than one deeper than the one before.
    assert_eq!(
        invalid(&|item| item.modifiers[0].depth = NonZeroU8::new(2).unwrap()),
        Err(PayloadError::Invalid("depth"))
    );
    assert_eq!(
        invalid(&|item| item.modifiers[2].depth = NonZeroU8::new(3).unwrap()),
        Err(PayloadError::Invalid("depth"))
    );
    assert_eq!(invalid(&|_| {}), Ok(()));
    let many = KitchenEvent::Sent(sent(vec![latte(); MAX_ITEMS + 1]));
    assert_eq!(many.validate(), Err(PayloadError::Invalid("items")));
}

fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Pinned payloads, each as Python's `cbor2` encodes the documented key table, canonically:
/// ```python
/// import cbor2, uuid
/// i = lambda n: uuid.UUID(f"0192f0c1-0000-7000-8000-{n:012x}").bytes
/// latte = {1: i(0x900), 2: "Latte (Large)", 3: [1000000, "each"],
///          4: [{1: "Oat milk", 2: 0, 3: 1, 4: 1}, {1: "Shot", 2: 2, 3: 2, 4: 1},
///              {1: "Vanilla", 2: 0, 3: 1, 4: 2}],
///          5: "extra hot", 6: [6]}
/// cbor2.dumps({1: i(0x800), 2: i(0x701), 3: 0, 4: [latte]}, canonical=True).hex()
/// cbor2.dumps({}, canonical=True).hex()
/// cbor2.dumps({1: 1}, canonical=True).hex()
/// ```
#[test]
fn payloads_are_pinned() {
    let encoded = |event: &KitchenEvent| hex(&event.to_value().encode());
    assert_eq!(encoded(&KitchenEvent::Sent(sent(vec![latte()]))), PINNED_SENT);
    assert_eq!(encoded(&KitchenEvent::Shown), "a0");
    assert_eq!(encoded(&KitchenEvent::Bumped), "a0");
    assert_eq!(encoded(&KitchenEvent::Recalled), "a0");
    assert_eq!(encoded(&KitchenEvent::Rerouted(RerouteTo::FallbackPrinter)), "a10101");
    // Each decodes as it was written, under its own schema alone.
    for event in [
        KitchenEvent::Sent(sent(vec![latte()])),
        KitchenEvent::Shown,
        KitchenEvent::Bumped,
        KitchenEvent::Recalled,
        KitchenEvent::Rerouted(RerouteTo::Nowhere),
    ] {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(KitchenEvent::decode(&schema, &payload).unwrap(), event);
    }
    // A component names its line; an item with no allergens or notes leaves their keys out.
    let component = KitchenItem {
        modifiers: Vec::new(),
        notes: None,
        allergens: BTreeSet::new(),
        component_of: Some(name("Breakfast sandwich")),
        name: name("Hash brown"),
        ..latte()
    };
    let (schema, payload) = KitchenEvent::Sent(sent(vec![component.clone()])).encode().unwrap();
    assert_eq!(
        KitchenEvent::decode(&schema, &payload).unwrap(),
        KitchenEvent::Sent(sent(vec![component]))
    );
    // A showing with a key it doesn't define is refused.
    let (schema, _) = KitchenEvent::Shown.encode().unwrap();
    let extra = keel_events::cbor::decode(&[0xa1, 0x01, 0x01]).unwrap();
    assert!(matches!(KitchenEvent::from_value(&schema, &extra), Err(DecodeError::Malformed(_))));
}

/// The sending above, its latte with milk, extra hot.
const PINNED_SENT: &str = "a401500192f0c100007000800000000000080002500192f0c100007000800000000000070103000481a601500192f0c1000070008000000000000900026d4c6174746520284c617267652903821a000f424064656163680483a401684f6174206d696c6b020003010401a4016453686f74020203020401a4016756616e696c6c610200030104020569657874726120686f74068106";
