//! The field names an export gives each payload's keys (ADR-0037, decision 3), from the
//! decoders: every key of every event of every kind is named, every schema the kernel decodes
//! has names, and `docs/architecture/export.md` lists them as they are. Run with
//! `KEEL_EXPORT_DOC=1` to write the list after changing a schema.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU8;

use keel_domain::fields::{Named, json_name, names, schemas};
use keel_domain::hub::{Claimed, Cut, Epoch, HubEvent, Succession};
use keel_domain::profile::demo;
use keel_domain::publication::publish;
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_domain::staff::{Action, ApprovalEvent, Granted, Request};
use keel_events::cbor::{Map, Value};
use keel_events::hash::EventHash;
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, TestRunner};
use support::{
    any_availability_event, any_drawer_event, any_event, any_kitchen_event, any_ownership_event,
    any_payment_event, any_refund_event, any_stock_event, id,
};

/// What every decoder named, by schema (`name/version`), and the keys its payloads held.
type Seen = BTreeMap<(String, u32), (Vec<Named>, Vec<u64>)>;

fn see<E: DomainEvent>(seen: &mut Seen, event: &E) {
    let schema = event.schema();
    let payload = event.to_value();
    let found = names(&schema.to_ref().unwrap(), &payload)
        .unwrap_or_else(|| panic!("{} v{} isn't decoded", schema.name, schema.version));
    let keys: Vec<u64> =
        payload.as_map().unwrap().iter().map(|(key, _)| key.as_u64().unwrap()).collect();
    for key in &keys {
        assert!(
            found.iter().any(|(named, _)| named == key),
            "{} v{}: key {key} has no name: {found:?}",
            schema.name,
            schema.version
        );
    }
    let entry = seen.entry((schema.name.to_owned(), schema.version)).or_default();
    for named in found {
        if !entry.0.contains(&named) {
            entry.0.push(named);
        }
    }
    entry.1.extend(keys);
}

fn sample<T: core::fmt::Debug>(strategy: impl Strategy<Value = T>, n: usize) -> Vec<T> {
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        proptest::test_runner::TestRng::deterministic_rng(
            proptest::test_runner::RngAlgorithm::ChaCha,
        ),
    );
    (0..n).map(|_| strategy.new_tree(&mut runner).unwrap().current()).collect()
}

/// Events of every kind: many of each generated, and those with no generator made.
fn everything() -> Seen {
    let mut seen = Seen::new();
    for event in sample(any_event(), 4_000).iter().chain(&sample(any_ownership_event(), 300)) {
        see(&mut seen, event);
    }
    for event in sample(any_payment_event(), 600) {
        see(&mut seen, &event);
    }
    for event in sample(any_drawer_event(), 600) {
        see(&mut seen, &event);
    }
    for event in sample(any_refund_event(), 600) {
        see(&mut seen, &event);
    }
    for event in sample(any_availability_event(), 200) {
        see(&mut seen, &event);
    }
    for event in sample(any_kitchen_event(), 400) {
        see(&mut seen, &event);
    }
    for event in sample(any_stock_event(), 300) {
        see(&mut seen, &event);
    }
    let run = Run { device: id(0xD1), from: 1, to: 3, last: EventHash::from_bytes([7; 32]) };
    see(&mut seen, &SequenceEvent::Assigned(Assigned { epoch: 1, first: 1, runs: vec![run] }));
    let priority = NonZeroU8::new(1).unwrap();
    see(&mut seen, &HubEvent::Claimed(Claimed { epoch: Epoch::FIRST, priority, succeeds: None }));
    let succeeds =
        Some(Succession { previous: id(0x100), cuts: vec![Cut { device: id(0xD1), position: 4 }] });
    see(
        &mut seen,
        &HubEvent::Claimed(Claimed { epoch: Epoch::new(2).unwrap(), priority, succeeds }),
    );
    for event in publish(&demo().unwrap()).unwrap() {
        see(&mut seen, &event);
    }
    for action in Action::ALL {
        let granted = Granted::new(id(0x501), Request::of(*action), Some(id(0x700))).unwrap();
        see(&mut seen, &ApprovalEvent::Granted(granted));
    }
    seen
}

/// The reference, as `docs/architecture/export.md` lists it after its last heading.
fn reference(seen: &Seen) -> String {
    let mut text = String::new();
    for ((name, version), (named, _)) in seen {
        let fields: Vec<String> =
            named.iter().map(|(key, field)| format!("{key} `{}`", json_name(field))).collect();
        let fields = if fields.is_empty() { "none".to_owned() } else { fields.join(", ") };
        writeln!(text, "| `{name}` | {version} | {fields} |").unwrap();
    }
    format!("| Schema | Version | Fields: key and name |\n|---|---|---|\n{text}")
}

const DOC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/architecture/export.md");
const HEADING: &str = "## Fields by schema";

#[test]
fn every_key_of_every_schema_is_named_and_the_reference_is_current() {
    let seen = everything();
    let missing: Vec<String> = schemas()
        .iter()
        .filter(|schema| !seen.contains_key(&(schema.name.to_owned(), schema.version)))
        .map(|schema| format!("{} v{}", schema.name, schema.version))
        .collect();
    assert!(missing.is_empty(), "schemas no event here holds, so with no names: {missing:?}");
    // A name is never given to two keys of one schema.
    for ((name, version), (named, _)) in &seen {
        let mut fields: Vec<String> = named.iter().map(|(_, field)| json_name(field)).collect();
        fields.sort_unstable();
        fields.dedup();
        assert_eq!(fields.len(), named.len(), "{name} v{version} names two keys alike: {named:?}");
    }
    let doc = std::fs::read_to_string(DOC).unwrap();
    let at = doc.find(HEADING).expect("the export doc's fields heading") + HEADING.len();
    let intro_end = doc[at..].find("\n| Schema").map_or(doc.len(), |n| at + n);
    let wanted = format!("{}\n{}", &doc[..intro_end], reference(&seen));
    if std::env::var_os("KEEL_EXPORT_DOC").is_some() {
        std::fs::write(DOC, &wanted).unwrap();
    } else {
        assert!(
            doc == wanted,
            "docs/architecture/export.md's fields aren't current: run with KEEL_EXPORT_DOC=1"
        );
    }
}

/// A payload that isn't valid, or of a schema no kernel knows, has no names.
#[test]
fn what_doesnt_decode_has_no_names() {
    let schema = keel_domain::stock::StockEvent::SCHEMAS[0];
    assert_eq!(names(&schema.to_ref().unwrap(), &Value::Map(Map::default())), None);
    let unknown = keel_events::envelope::SchemaRef {
        name: keel_events::envelope::SchemaName::new("stock.teleported").unwrap(),
        version: 1.try_into().unwrap(),
    };
    assert_eq!(names(&unknown, &Value::Map(Map::default())), None);
}
