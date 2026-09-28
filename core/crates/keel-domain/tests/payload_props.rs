//! Property tests for order event payloads, against an independent model of the schemas:
//! payloads round-trip; a payload with a field changed, removed or added is accepted exactly
//! when the model says it is valid, and then has exactly one encoding; and the rules that span
//! fields or sit at a boundary (one currency per payload, text lengths, identifier sets in
//! ascending order, allocations in order and in lowest terms) are aimed at directly.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_domain::order::OrderEvent;
use keel_domain::schema::DomainEvent;
use keel_events::cbor::{Map, Value};
use keel_events::envelope::{SchemaName, SchemaRef};
use keel_types::{Currency, Unit};
use proptest::prelude::*;
use support::{any_event, any_line_added, any_line_changed, any_lines_allocated, modifiers};

/// How a field may appear in a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    Required,
    Optional,
    /// Optional, and `null` clears it.
    Clearable,
}

/// What a field holds.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Id,
    Money,
    Quantity,
    Currency,
    Name,
    Note,
    Reason,
    /// An enumeration code from 0 to the given maximum.
    Code(u64),
    Count8,
    Count16,
    CatalogVersion,
    IdSet,
    Modifiers,
    /// Allocations of lines to checks.
    Allocations,
}

use Kind::*;
use Presence::{Clearable, Optional, Required};

/// The documented keys of each schema: the model.
fn rules(schema: &str) -> Vec<(u64, Kind, Presence)> {
    match schema {
        "order.created" => vec![
            (1, Code(7), Required),
            (2, Code(7), Required),
            (3, Currency, Required),
            (4, Id, Optional),
            (5, Id, Optional),
            (6, Count16, Optional),
            (7, Id, Optional),
            (8, Id, Optional),
        ],
        "order.attributes_changed" => vec![
            (1, Code(7), Optional),
            (2, Id, Clearable),
            (3, Id, Clearable),
            (4, Count16, Clearable),
            (5, Id, Clearable),
            (6, Id, Clearable),
        ],
        "order.line_added" => vec![
            (1, Id, Required),
            (2, Id, Required),
            (3, CatalogVersion, Required),
            (4, Name, Required),
            (5, Id, Required),
            (6, Money, Required),
            (7, Quantity, Required),
            (8, Modifiers, Required),
            (9, Count16, Optional),
            (10, Count8, Optional),
            (11, Note, Optional),
        ],
        "order.line_changed" => vec![
            (1, Id, Required),
            (2, Quantity, Optional),
            (3, Modifiers, Optional),
            (4, Count16, Clearable),
            (5, Count8, Clearable),
            (6, Note, Clearable),
        ],
        "order.line_removed" | "order.check_opened" => vec![(1, Id, Required)],
        "order.lines_fired" => vec![(1, IdSet, Required)],
        "order.line_voided" | "order.line_comped" => {
            vec![(1, Id, Required), (2, Reason, Required), (3, Note, Optional)]
        }
        "order.voided" => vec![(1, Reason, Required), (2, Note, Optional)],
        "order.abandoned" => vec![],
        "order.lines_allocated" => vec![(1, Allocations, Required)],
        other => panic!("no rules for {other}"),
    }
}

fn is_uuid_v7(value: &Value) -> bool {
    value.as_bytes().is_some_and(|b| b.len() == 16 && b[6] >> 4 == 7 && b[8] >> 6 == 0b10)
}

/// An integer that fits an `i64`.
fn as_i64(value: &Value) -> Option<i64> {
    match *value {
        Value::Unsigned(n) => i64::try_from(n).ok(),
        Value::Negative(n) => i64::try_from(n).ok().map(|n| -1 - n),
        _ => None,
    }
}

fn is_text(value: &Value, max: usize, line_feeds: bool) -> bool {
    value.as_text().is_some_and(|text| {
        let count = text.chars().count();
        (1..=max).contains(&count)
            && text.chars().all(|c| !(c.is_control()) || (line_feeds && c == '\n'))
    })
}

fn currency_of(value: &Value) -> Option<Currency> {
    Currency::from_code(value.as_text()?).ok()
}

/// A money value's amount and currency.
fn money(value: &Value) -> Option<(i64, Currency)> {
    let [amount, code] = value.as_array()? else { return None };
    Some((as_i64(amount)?, currency_of(code)?))
}

/// Every price in a list of modifiers, if it is a valid one.
fn modifier_prices(value: &Value, prices: &mut Vec<(i64, Currency)>) -> bool {
    let Some(items) = value.as_array() else { return false };
    items.iter().all(|item| {
        let Some(map) = item.as_map() else { return false };
        let keys: Vec<u64> = map.iter().filter_map(|(key, _)| key.as_u64()).collect();
        if keys != [1, 2, 3, 4, 5, 6, 7] || map.len() != 7 {
            return false;
        }
        let get = |key: u64| map.get(&Value::Unsigned(key)).unwrap();
        let Some(price) = money(get(6)) else { return false };
        prices.push(price);
        valid_value(Id, get(1))
            && valid_value(Name, get(2))
            && valid_value(Code(5), get(3))
            && valid_value(Count8, get(4))
            && valid_value(Code(6), get(5))
            && modifier_prices(get(7), prices)
    })
}

/// An allocation's line, check and shares, if it is a well-formed one.
fn allocation(value: &Value) -> Option<(Vec<u8>, Vec<u8>, u64)> {
    let map = value.as_map()?;
    let keys: Vec<u64> = map.iter().filter_map(|(key, _)| key.as_u64()).collect();
    if keys != [1, 2, 3] || map.len() != 3 {
        return None;
    }
    let get = |key: u64| map.get(&Value::Unsigned(key)).unwrap();
    let valid = valid_value(Id, get(1)) && valid_value(Id, get(2)) && valid_value(Count16, get(3));
    valid.then(|| {
        (
            get(1).as_bytes().unwrap().to_vec(),
            get(2).as_bytes().unwrap().to_vec(),
            get(3).as_u64().unwrap(),
        )
    })
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Whether allocations are well formed, not empty, in strictly ascending order of line then
/// check, with each line's shares in lowest terms.
fn allocations_valid(value: &Value) -> bool {
    let Some(items) = value.as_array() else { return false };
    let Some(parsed) = items.iter().map(allocation).collect::<Option<Vec<_>>>() else {
        return false;
    };
    let ascending =
        parsed.windows(2).all(|pair| (&pair[0].0, &pair[0].1) < (&pair[1].0, &pair[1].1));
    let mut divisors: std::collections::BTreeMap<Vec<u8>, u64> = std::collections::BTreeMap::new();
    for (line, _, shares) in &parsed {
        let divisor = divisors.entry(line.clone()).or_insert(0);
        *divisor = gcd(*divisor, *shares);
    }
    !parsed.is_empty() && ascending && divisors.values().all(|&divisor| divisor == 1)
}

/// Whether the prices are all zero or more, and all in one currency (or `currency`, if given).
fn prices_consistent(prices: &[(i64, Currency)], currency: Option<Currency>) -> bool {
    let currency = currency.or_else(|| prices.first().map(|&(_, currency)| currency));
    prices.iter().all(|&(amount, price_currency)| amount >= 0 && Some(price_currency) == currency)
}

fn valid_value(kind: Kind, value: &Value) -> bool {
    match kind {
        Id => is_uuid_v7(value),
        Money => money(value).is_some(),
        Quantity => value.as_array().is_some_and(|parts| match parts {
            [micros, unit] => {
                as_i64(micros).is_some()
                    && unit.as_text().is_some_and(|code| Unit::from_code(code).is_ok())
            }
            _ => false,
        }),
        Currency => currency_of(value).is_some(),
        Name => is_text(value, 200, false),
        Note => is_text(value, 500, true),
        Reason => value.as_text().is_some_and(|text| {
            let mut chars = text.chars();
            text.len() <= 32
                && chars.next().is_some_and(|c| c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }),
        Code(max) => value.as_u64().is_some_and(|code| code <= max),
        Count8 => value.as_u64().is_some_and(|n| (1..=255).contains(&n)),
        Count16 => value.as_u64().is_some_and(|n| (1..=65_535).contains(&n)),
        CatalogVersion => value.as_bytes().is_some_and(|bytes| bytes.len() == 32),
        IdSet => value.as_array().is_some_and(|ids| {
            !ids.is_empty()
                && ids.iter().all(is_uuid_v7)
                && ids.windows(2).all(|pair| pair[0].as_bytes() < pair[1].as_bytes())
        }),
        Modifiers => {
            let mut prices = Vec::new();
            modifier_prices(value, &mut prices)
                && value.as_array().unwrap().iter().all(|item| {
                    let mut own = Vec::new();
                    modifier_prices(&Value::Array(vec![item.clone()]), &mut own)
                        && prices_consistent(&own, None)
                })
        }
        Allocations => allocations_valid(value),
    }
}

/// Whether `payload` is a valid payload of `schema`, by the model.
fn valid_payload(schema: &str, payload: &Value) -> bool {
    let Some(map) = payload.as_map() else { return false };
    let rules = rules(schema);
    let fields_valid = map.iter().all(|(key, value)| {
        key.as_u64().and_then(|key| rules.iter().find(|rule| rule.0 == key)).is_some_and(
            |&(_, kind, presence)| {
                (presence == Clearable && *value == Value::Null) || valid_value(kind, value)
            },
        )
    });
    let required_present = rules
        .iter()
        .filter(|rule| rule.2 == Required)
        .all(|rule| map.get(&Value::Unsigned(rule.0)).is_some());
    if !fields_valid || !required_present {
        return false;
    }
    let get = |key: u64| map.get(&Value::Unsigned(key));
    let all_prices = |value: &Value| {
        let mut prices = Vec::new();
        modifier_prices(value, &mut prices);
        prices
    };
    match schema {
        "order.attributes_changed" => !map.is_empty(),
        "order.line_added" => {
            let (amount, currency) = money(get(6).unwrap()).unwrap();
            let micros = as_i64(&get(7).unwrap().as_array().unwrap()[0]).unwrap();
            amount >= 0
                && micros > 0
                && prices_consistent(&all_prices(get(8).unwrap()), Some(currency))
        }
        "order.line_changed" => {
            let quantity_positive =
                get(2).is_none_or(|quantity| as_i64(&quantity.as_array().unwrap()[0]).unwrap() > 0);
            let modifiers_consistent =
                get(3).is_none_or(|modifiers| prices_consistent(&all_prices(modifiers), None));
            map.len() > 1 && quantity_positive && modifiers_consistent
        }
        _ => true,
    }
}

fn schema_ref(event: &OrderEvent) -> SchemaRef {
    event.schema().to_ref().unwrap()
}

/// Values close to what a field of `kind` holds: valid, nearly valid, or anything.
fn near_miss(kind: Kind) -> BoxedStrategy<Value> {
    let int = |range: core::ops::RangeInclusive<i64>| range.prop_map(Value::integer);
    let text = |pattern: &'static str| pattern.prop_map(Value::Text);
    let aimed: BoxedStrategy<Value> = match kind {
        Id | IdSet => prop_oneof![
            any::<[u8; 16]>().prop_map(|b| Value::Bytes(b.to_vec())),
            (0_u64..10).prop_map(support_id_value),
            prop::collection::vec((0_u64..6).prop_map(support_id_value), 0..4)
                .prop_map(Value::Array),
            prop::collection::vec(any::<u8>(), 15..=17).prop_map(Value::Bytes),
        ]
        .boxed(),
        Money | Quantity => {
            let amount = prop_oneof![
                int(-5..=5),
                Just(Value::Unsigned(u64::MAX)),
                Just(Value::integer(i64::MIN))
            ];
            let code =
                prop::sample::select(vec!["USD", "EUR", "usd", "XXXX", "each", "kg", "KG", ""])
                    .prop_map(Value::from);
            prop_oneof![
                4 => (amount.clone(), code.clone()).prop_map(|(a, c)| Value::Array(vec![a, c])),
                1 => amount.clone().prop_map(|a| Value::Array(vec![a])),
                1 => (amount, code.clone(), code).prop_map(|(a, c, d)| Value::Array(vec![a, c, d])),
            ]
            .boxed()
        }
        Currency => prop::sample::select(vec!["USD", "JPY", "usd", "US", "USDX", "ABC"])
            .prop_map(Value::from)
            .boxed(),
        Name | Note | Reason => prop_oneof![
            text("[a-z][a-z0-9_]{0,33}"),
            text("[a-z][a-z0-9_]{31,32}"),
            text("[A-Za-z _\n\t]{0,10}"),
            text("[a-z]{199,201}"),
            text("[a-z]{499,501}"),
            Just(Value::from("")),
            Just(Value::from("tab\there")),
        ]
        .boxed(),
        Code(_) | Count8 | Count16 => prop_oneof![
            (0_u64..10).prop_map(Value::Unsigned),
            prop::sample::select(vec![255_u64, 256, 65_535, 65_536]).prop_map(Value::Unsigned),
            int(-2..=-1),
        ]
        .boxed(),
        CatalogVersion => {
            prop::collection::vec(any::<u8>(), 31..=33).prop_map(Value::Bytes).boxed()
        }
        Modifiers => prop_oneof![
            2 => support::any_currency()
                .prop_flat_map(|currency| modifiers(currency, 1))
                .prop_map(|list| keel_domain_value(&list)),
            1 => Just(Value::Array(vec![Value::Null])),
            // A valid, non-empty list with a price made negative, or a currency changed.
            3 => (
                support::any_currency()
                    .prop_flat_map(|currency| modifiers(currency, 1))
                    .prop_filter("some modifiers", |list| !list.is_empty()),
                any::<bool>(),
                any::<bool>()
            )
                .prop_map(|(list, negative, nested)| {
                    let mut value = keel_domain_value(&list);
                    corrupt_first_price(&mut value, negative, nested);
                    value
                }),
        ]
        .boxed(),
        Allocations => prop_oneof![
            3 => any_lines_allocated().prop_map(|allocated| allocations_value(&allocated)),
            // Valid allocations spoiled: out of order, repeated, not in lowest terms, a share of
            // zero, or a field missing.
            4 => (any_lines_allocated(), 0_u8..5, any::<prop::sample::Index>())
                .prop_map(|(allocated, how, at)| spoil(&allocations_value(&allocated), how, at)),
            1 => Just(Value::Array(Vec::new())),
        ]
        .boxed(),
    };
    let anything = prop_oneof![
        Just(Value::Null),
        any::<u64>().prop_map(Value::Unsigned),
        ".{0,5}".prop_map(Value::Text),
        Just(Value::Map(Map::new())),
    ];
    prop_oneof![4 => aimed, 1 => anything].boxed()
}

/// The payload encoding of allocations.
fn allocations_value(allocated: &keel_domain::order::LinesAllocated) -> Value {
    let payload = OrderEvent::LinesAllocated(allocated.clone()).to_value();
    payload.as_map().unwrap().get(&Value::Unsigned(1)).unwrap().clone()
}

/// Allocations with one thing wrong, or right after all: the entry at `at` swapped with the
/// next, repeated, with its shares doubled, with no shares, or with its check removed.
fn spoil(value: &Value, how: u8, at: prop::sample::Index) -> Value {
    let mut items = value.as_array().unwrap().to_vec();
    let (i, count) = (at.index(items.len()), items.len());
    let set = |item: &Value, key: u64, new: Option<Value>| {
        let mut entries = item.as_map().unwrap().clone().into_entries();
        entries.retain(|(existing, _)| existing.as_u64() != Some(key));
        if let Some(new) = new {
            entries.push((Value::Unsigned(key), new));
        }
        Value::Map(Map::from_entries(entries).unwrap())
    };
    match how {
        0 if count > 1 => items.swap(i, (i + 1) % count),
        1 => items.insert(i, items[i].clone()),
        2 => {
            let shares =
                items[i].as_map().unwrap().get(&Value::Unsigned(3)).unwrap().as_u64().unwrap();
            items[i] = set(&items[i], 3, Some(Value::Unsigned(shares * 2)));
        }
        3 => items[i] = set(&items[i], 3, Some(Value::Unsigned(0))),
        _ => items[i] = set(&items[i], 2, None),
    }
    Value::Array(items)
}

fn support_id_value(n: u64) -> Value {
    Value::Bytes(support::id::<()>(n).to_bytes().to_vec())
}

/// The payload encoding of a list of modifiers, via an event that carries them.
fn keel_domain_value(list: &[keel_domain::order::ChosenModifier]) -> Value {
    let mut changed = keel_domain::order::LineChanged::to(support::id(1));
    changed.modifiers = Some(list.to_vec());
    let payload = OrderEvent::LineChanged(changed).to_value();
    payload.as_map().unwrap().get(&Value::Unsigned(3)).unwrap().clone()
}

/// A modifier map with its price made negative (same currency) or put in another currency.
fn corrupt_price(modifier: &Value, negative: bool) -> Value {
    let mut entries = modifier.as_map().unwrap().clone().into_entries();
    for (key, value) in &mut entries {
        if key.as_u64() == Some(6) {
            let [amount, code] = value.as_array().unwrap() else { panic!("a price") };
            let other = if code.as_text() == Some("USD") { "EUR" } else { "USD" };
            *value = if negative {
                Value::Array(vec![Value::integer(-1), code.clone()])
            } else {
                Value::Array(vec![amount.clone(), Value::from(other)])
            };
        }
    }
    Value::Map(Map::from_entries(entries).unwrap())
}

/// Corrupts the first modifier's price, or that of the first modifier under it.
fn corrupt_first_price(value: &mut Value, negative: bool, nested: bool) {
    let Value::Array(items) = value else { return };
    let Some(first) = items.first().cloned() else { return };
    items[0] = if nested {
        let mut entries = first.as_map().unwrap().clone().into_entries();
        for (key, value) in &mut entries {
            if key.as_u64() == Some(7)
                && let Value::Array(children) = value
                && let Some(child) = children.first().cloned()
            {
                children[0] = corrupt_price(&child, negative);
            }
        }
        Value::Map(Map::from_entries(entries).unwrap())
    } else {
        corrupt_price(&first, negative)
    };
}

/// A change to one entry of a payload's map.
#[derive(Clone, Debug)]
enum FieldChange {
    Set {
        key: u64,
        value: Value,
    },
    Remove {
        key: u64,
    },
    AddText,
    /// Remove every field that isn't required.
    KeepOnlyRequired,
    /// Replace the amount of a money or quantity field, keeping its currency or unit.
    Amount {
        key: u64,
        amount: Value,
    },
    /// Put every price in a money or modifiers field into another currency, consistently.
    Recurrency {
        key: u64,
    },
}

/// The currency a price is moved to: US dollars become euros, and anything else US dollars.
fn other_currency(code: &Value) -> Value {
    Value::from(if code.as_text() == Some("USD") { "EUR" } else { "USD" })
}

/// Puts every price in `value` into another currency, consistently: a money value's, or every
/// modifier's in a list, at any depth.
fn recurrency(value: &mut Value) {
    let Value::Array(parts) = value else { return };
    if let [_, code] = parts.as_mut_slice()
        && code.as_text().is_some()
    {
        *code = other_currency(code);
        return;
    }
    for item in parts {
        let Some(map) = item.as_map() else { continue };
        let mut entries = map.clone().into_entries();
        for (key, value) in &mut entries {
            if matches!(key.as_u64(), Some(6 | 7)) {
                recurrency(value);
            }
        }
        *item = Value::Map(Map::from_entries(entries).unwrap());
    }
}

/// `value` with the price at position `target`, counting prices depth first from zero, put into
/// another currency. `seen` counts the prices met, so after a call it has grown by the number of
/// prices in `value`.
fn reprice(value: &Value, target: usize, seen: &mut usize) -> Value {
    match value {
        Value::Array(parts) if money(value).is_some() => {
            let hit = *seen == target;
            *seen += 1;
            if hit {
                Value::Array(vec![parts[0].clone(), other_currency(&parts[1])])
            } else {
                value.clone()
            }
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| reprice(item, target, seen)).collect())
        }
        Value::Map(map) => {
            let entries: Vec<(Value, Value)> =
                map.iter().map(|(key, item)| (key.clone(), reprice(item, target, seen))).collect();
            Value::Map(Map::from_entries(entries).unwrap())
        }
        other => other.clone(),
    }
}

/// Version 1 of the schema named `name`.
fn schema_named(name: &str) -> SchemaRef {
    SchemaRef { name: SchemaName::new(name).unwrap(), version: 1.try_into().unwrap() }
}

/// The keys of the fields of `schema` that hold prices.
fn priced_keys(schema: &str) -> Vec<u64> {
    rules(schema)
        .iter()
        .filter(|rule| matches!(rule.1, Money | Modifiers))
        .map(|rule| rule.0)
        .collect()
}

fn any_case() -> impl Strategy<Value = (OrderEvent, FieldChange)> {
    any_event().prop_flat_map(|event| {
        let rules = rules(event.schema().name);
        let keys: Vec<u64> = rules.iter().map(|rule| rule.0).collect();
        let aimed = if rules.is_empty() {
            Just(FieldChange::AddText).boxed()
        } else {
            prop::sample::select(rules)
                .prop_flat_map(|(key, kind, presence)| {
                    let value = if presence == Clearable {
                        prop_oneof![4 => near_miss(kind), 1 => Just(Value::Null)].boxed()
                    } else {
                        near_miss(kind)
                    };
                    value.prop_map(move |value| FieldChange::Set { key, value })
                })
                .boxed()
        };
        let amounts: Vec<u64> = crate::rules(event.schema().name)
            .iter()
            .filter(|rule| matches!(rule.1, Money | Quantity))
            .map(|rule| rule.0)
            .collect();
        let amount = if amounts.is_empty() {
            Just(FieldChange::AddText).boxed()
        } else {
            let value = prop_oneof![
                (-2_i64..=2).prop_map(Value::integer),
                Just(Value::integer(i64::MIN)),
                Just(Value::integer(i64::MAX)),
                Just(Value::Unsigned(u64::MAX)),
            ];
            (prop::sample::select(amounts), value)
                .prop_map(|(key, amount)| FieldChange::Amount { key, amount })
                .boxed()
        };
        let removed = if keys.is_empty() {
            Just(FieldChange::AddText).boxed()
        } else {
            prop::sample::select(keys).prop_map(|key| FieldChange::Remove { key }).boxed()
        };
        let priced = priced_keys(event.schema().name);
        let recurrency = if priced.is_empty() {
            Just(FieldChange::AddText).boxed()
        } else {
            prop::sample::select(priced).prop_map(|key| FieldChange::Recurrency { key }).boxed()
        };
        let change = prop_oneof![
            6 => aimed,
            2 => removed,
            1 => (0_u64..14).prop_map(|key| FieldChange::Set { key, value: Value::Unsigned(1) }),
            1 => Just(FieldChange::AddText),
            1 => Just(FieldChange::KeepOnlyRequired),
            2 => amount,
            2 => recurrency,
        ];
        (Just(event), change)
    })
}

fn changed(schema: &str, payload: &Value, change: &FieldChange) -> Value {
    let mut entries = payload.as_map().unwrap().clone().into_entries();
    match change {
        FieldChange::Set { key, value } => {
            entries.retain(|(existing, _)| existing.as_u64() != Some(*key));
            entries.push((Value::Unsigned(*key), value.clone()));
        }
        FieldChange::Remove { key } => {
            entries.retain(|(existing, _)| existing.as_u64() != Some(*key));
        }
        FieldChange::AddText => entries.push((Value::from("extra"), Value::Null)),
        FieldChange::Amount { key, amount } => {
            for (existing, value) in &mut entries {
                if existing.as_u64() == Some(*key)
                    && let Value::Array(parts) = value
                {
                    parts[0] = amount.clone();
                }
            }
        }
        FieldChange::Recurrency { key } => {
            for (_, value) in
                entries.iter_mut().filter(|(existing, _)| existing.as_u64() == Some(*key))
            {
                recurrency(value);
            }
        }
        FieldChange::KeepOnlyRequired => {
            let required: Vec<u64> =
                rules(schema).iter().filter(|rule| rule.2 == Required).map(|rule| rule.0).collect();
            entries.retain(|(key, _)| key.as_u64().is_some_and(|key| required.contains(&key)));
        }
    }
    Value::Map(Map::from_entries(entries).unwrap())
}

proptest! {
    /// Every event survives encoding and decoding, and its payload is valid by the model.
    #[test]
    fn events_round_trip(event in any_event()) {
        let (schema, payload) = event.encode().unwrap();
        prop_assert!(valid_payload(schema.name.as_str(), &payload.value().unwrap()));
        prop_assert_eq!(OrderEvent::decode(&schema, &payload), Ok(event));
    }

    /// A payload with one field changed, removed or added is accepted exactly when the model
    /// says it is valid, and then re-encodes to exactly the same bytes.
    #[test]
    fn changed_payloads_are_accepted_exactly_when_valid((event, change) in any_case()) {
        let schema = schema_ref(&event);
        let payload = changed(schema.name.as_str(), &event.to_value(), &change);
        let valid = valid_payload(schema.name.as_str(), &payload);
        match OrderEvent::from_value(&schema, &payload) {
            Ok(decoded) => {
                prop_assert!(valid, "accepted an invalid payload");
                prop_assert_eq!(decoded.to_value().encode(), payload.encode());
            }
            Err(error) => prop_assert!(!valid, "rejected a valid payload: {}", error),
        }
    }

    /// Every price in a payload is in one currency. Moving one price, at any depth, or every
    /// price in one field into another currency leaves a valid payload exactly when no price
    /// stays behind in the old one.
    #[test]
    fn a_payload_has_one_currency(
        event in prop_oneof![
            any_line_added().prop_map(OrderEvent::LineAdded),
            any_line_changed().prop_map(OrderEvent::LineChanged),
        ],
        pick in any::<prop::sample::Index>(),
        whole_field in any::<bool>(),
    ) {
        let schema = schema_ref(&event);
        let name = schema.name.as_str();
        let original = event.to_value();
        let payload = if whole_field {
            let priced = priced_keys(name);
            changed(name, &original, &FieldChange::Recurrency { key: priced[pick.index(priced.len())] })
        } else {
            let mut prices = 0;
            reprice(&original, usize::MAX, &mut prices);
            if prices == 0 { original.clone() } else { reprice(&original, pick.index(prices), &mut 0) }
        };
        let valid = valid_payload(name, &payload);
        prop_assert_eq!(OrderEvent::from_value(&schema, &payload).is_ok(), valid);
    }

    /// Names hold 1 to 200 characters and notes 1 to 500, counted in characters rather than
    /// bytes, with no control characters, except that notes may break lines. A note decoded
    /// from a payload follows the same rules.
    #[test]
    fn text_fields_hold_exactly_their_limits(
        length in prop_oneof![Just(0_usize), 1_usize..3, 198_usize..=202, 498_usize..=502],
        letter in prop::sample::select(vec!['a', 'é', '中']),
        special in prop::option::of((
            prop::sample::select(vec!['\n', '\t', '\u{7f}', '\u{85}', ' ']),
            any::<prop::sample::Index>(),
        )),
    ) {
        let mut chars = vec![letter; length];
        if let Some((special, at)) = special
            && !chars.is_empty()
        {
            let at = at.index(chars.len());
            chars[at] = special;
        }
        let text: String = chars.into_iter().collect();
        let value = Value::from(text.as_str());
        prop_assert_eq!(keel_domain::codec::Name::new(&text).is_ok(), valid_value(Name, &value));
        prop_assert_eq!(keel_domain::codec::Note::new(&text).is_ok(), valid_value(Note, &value));
        let voided = Map::from_entries(vec![
            (Value::Unsigned(1), Value::from("walkout")),
            (Value::Unsigned(2), value.clone()),
        ]).unwrap();
        prop_assert_eq!(
            OrderEvent::from_value(&schema_named("order.voided"), &Value::Map(voided)).is_ok(),
            valid_value(Note, &value)
        );
    }

    /// Reason codes are 1 to 32 bytes: a lowercase letter, then lowercase letters, digits and
    /// underscores. A code decoded from a payload follows the same rules.
    #[test]
    fn reason_codes_hold_exactly_their_limits(
        length in prop_oneof![Just(0_usize), 1_usize..3, 30_usize..=34],
        first in prop::sample::select(vec!['a', 'z', 'A', '0', '_']),
        rest in prop::sample::select(vec!['a', '9', '_']),
        odd in prop::option::weighted(0.3, (prop::sample::select(vec!['-', 'B', 'é', ' ']), any::<prop::sample::Index>())),
    ) {
        let mut chars: Vec<char> = (0..length).map(|at| if at == 0 { first } else { rest }).collect();
        if let Some((odd, at)) = odd
            && !chars.is_empty()
        {
            let at = at.index(chars.len());
            chars[at] = odd;
        }
        let text: String = chars.into_iter().collect();
        let value = Value::from(text.as_str());
        prop_assert_eq!(keel_domain::codec::ReasonCode::new(&text).is_ok(), valid_value(Reason, &value));
        let voided = Map::from_entries(vec![(Value::Unsigned(1), value.clone())]).unwrap();
        prop_assert_eq!(
            OrderEvent::from_value(&schema_named("order.voided"), &Value::Map(voided)).is_ok(),
            valid_value(Reason, &value)
        );
    }

    /// Identifier sets are built from any identifiers that are distinct and not empty, and hold
    /// them in ascending byte order. A payload's set decodes only if its identifiers come in
    /// strictly ascending byte order.
    #[test]
    fn identifier_sets_are_sets(ids in prop::collection::vec(0_u64..8, 0..6)) {
        let ids: Vec<keel_types::Id<()>> = ids.into_iter().map(support::id).collect();
        let distinct: std::collections::BTreeSet<[u8; 16]> = ids.iter().map(|id| id.to_bytes()).collect();
        match keel_domain::codec::IdSet::new(ids.clone()) {
            Ok(set) => {
                prop_assert!(!ids.is_empty() && distinct.len() == ids.len());
                let bytes: Vec<[u8; 16]> = set.iter().map(keel_types::Id::to_bytes).collect();
                prop_assert_eq!(bytes, distinct.into_iter().collect::<Vec<_>>());
            }
            Err(_) => prop_assert!(ids.is_empty() || distinct.len() < ids.len()),
        }
        let encoded = ids.iter().map(|id| Value::Bytes(id.to_bytes().to_vec())).collect();
        let fired = Map::from_entries(vec![(Value::Unsigned(1), Value::Array(encoded))]).unwrap();
        let ascending = !ids.is_empty()
            && ids.windows(2).all(|pair| pair[0].to_bytes() < pair[1].to_bytes());
        prop_assert_eq!(
            OrderEvent::from_value(&schema_named("order.lines_fired"), &Value::Map(fired)).is_ok(),
            ascending
        );
    }

    /// Allocations are built from any lines, checks and shares with no line allocated to the
    /// same check twice, and are kept in order of line then check, each line's shares reduced to
    /// lowest terms without changing their proportions. A payload's allocations decode only in
    /// that form.
    #[test]
    fn allocations_have_one_form(
        triples in prop::collection::vec((0_u64..3, 0_u64..3, 1_u16..=6), 0..6),
    ) {
        let allocations: Vec<keel_domain::order::Allocation> = triples
            .iter()
            .map(|&(line, check, shares)| keel_domain::order::Allocation {
                line: support::id(line),
                check: support::id(0x10 + check),
                shares: core::num::NonZeroU16::new(shares).unwrap(),
            })
            .collect();
        let pairs: std::collections::BTreeSet<(u64, u64)> =
            triples.iter().map(|&(line, check, _)| (line, check)).collect();
        match keel_domain::order::LinesAllocated::new(allocations.clone()) {
            Ok(allocated) => {
                prop_assert!(!triples.is_empty() && pairs.len() == triples.len());
                prop_assert!(allocations_valid(&allocations_value(&allocated)));
                // Each line keeps its proportions: shares scaled by one factor per line.
                for given in &allocations {
                    let kept = allocated.iter().find(|a| a.line == given.line && a.check == given.check).unwrap();
                    for other in allocations.iter().filter(|a| a.line == given.line) {
                        let other_kept = allocated.iter().find(|a| a.line == other.line && a.check == other.check).unwrap();
                        prop_assert_eq!(
                            u64::from(given.shares.get()) * u64::from(other_kept.shares.get()),
                            u64::from(other.shares.get()) * u64::from(kept.shares.get())
                        );
                    }
                }
            }
            Err(_) => prop_assert!(triples.is_empty() || pairs.len() < triples.len()),
        }
        let raw = Value::Array(allocations.iter().map(|a| {
            Value::Map(Map::from_entries(vec![
                (Value::Unsigned(1), Value::Bytes(a.line.to_bytes().to_vec())),
                (Value::Unsigned(2), Value::Bytes(a.check.to_bytes().to_vec())),
                (Value::Unsigned(3), Value::Unsigned(u64::from(a.shares.get()))),
            ]).unwrap())
        }).collect());
        let payload = Map::from_entries(vec![(Value::Unsigned(1), raw.clone())]).unwrap();
        prop_assert_eq!(
            OrderEvent::from_value(&schema_named("order.lines_allocated"), &Value::Map(payload)).is_ok(),
            allocations_valid(&raw)
        );
    }

    /// Payloads never decode under another schema's name or version by accident: every
    /// unknown schema is reported as unknown, whatever the payload.
    #[test]
    fn unknown_schemas_are_reported(event in any_event(), version in 2_u32..5) {
        let schema = schema_ref(&event);
        let newer = SchemaRef { name: schema.name.clone(), version: version.try_into().unwrap() };
        prop_assert_eq!(
            OrderEvent::from_value(&newer, &event.to_value()),
            Err(keel_domain::schema::DecodeError::UnknownSchema)
        );
        let renamed = SchemaRef { name: SchemaName::new("order.unknown").unwrap(), version: schema.version };
        prop_assert_eq!(
            OrderEvent::from_value(&renamed, &event.to_value()),
            Err(keel_domain::schema::DecodeError::UnknownSchema)
        );
    }
}
