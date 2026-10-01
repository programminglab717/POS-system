//! Property tests for order and payment event payloads, and sequencing records, against an
//! independent model of the schemas: payloads round-trip; a payload with a field changed, removed
//! or added is accepted exactly when the model says it is valid, and then has exactly one
//! encoding; and the rules that span fields or sit at a boundary (one currency per payload, text
//! lengths, identifier sets in ascending order, allocations in order and in lowest terms,
//! snapshots that add up, cash that covers what is paid, runs that follow on, numbers up to the
//! largest) are aimed at directly. A sequencing record's numbers are checked against a model that
//! counts run by run.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_domain::order::{CheckClosed, LineCharge, OrderEvent, TaxCharge};
use keel_domain::payment::PaymentEvent;
use keel_domain::schema::{DecodeError, DomainEvent, SchemaId};
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_events::cbor::{Map, Value};
use keel_events::envelope::{Device, SchemaName, SchemaRef};
use keel_events::hash::EventHash;
use keel_types::{Currency, Id, Money, Unit};
use proptest::prelude::*;
use support::{
    any_captured, any_check_closed, any_event, any_line_added, any_line_changed,
    any_lines_allocated, any_ownership_event, any_payment_event, modifiers,
};

/// An event of any aggregate.
#[derive(Clone, Debug, PartialEq)]
enum Event {
    Order(OrderEvent),
    Payment(PaymentEvent),
    Sequence(SequenceEvent),
}

impl Event {
    fn schema(&self) -> SchemaId {
        match self {
            Event::Order(event) => event.schema(),
            Event::Payment(event) => event.schema(),
            Event::Sequence(event) => event.schema(),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Event::Order(event) => event.to_value(),
            Event::Payment(event) => event.to_value(),
            Event::Sequence(event) => event.to_value(),
        }
    }
}

/// Decodes `payload` as `schema`, by the kind of stream its name belongs to, and re-encodes it.
fn decode(schema: &SchemaRef, payload: &Value) -> Result<Value, DecodeError> {
    let name = schema.name.as_str();
    if name.starts_with("payment.") {
        PaymentEvent::from_value(schema, payload).map(|event| event.to_value())
    } else if name.starts_with("sequence.") {
        SequenceEvent::from_value(schema, payload).map(|event| event.to_value())
    } else {
        OrderEvent::from_value(schema, payload).map(|event| event.to_value())
    }
}

/// Any event of any kind of stream.
fn any_any_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        3 => any_event().prop_map(Event::Order),
        1 => any_payment_event().prop_map(Event::Payment),
        1 => any_assigned().prop_map(|record| Event::Sequence(SequenceEvent::Assigned(record))),
    ]
}

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
    /// A 32-byte content hash: a catalog or rules version.
    Hash32,
    IdSet,
    Modifiers,
    /// Allocations of lines to checks.
    Allocations,
    /// A processor's reference.
    Reference,
    /// What a closed check charged for its lines.
    LineCharges,
    /// What a closed check charged for each tax.
    TaxCharges,
    /// A sequencing record's epoch or number, or a position in a log: from 1 to [`LARGEST`]. A
    /// grant's lease and epoch too.
    Number,
    /// A lease an order's change of owner replaces: from 0 to [`LARGEST`].
    Lease,
    /// A sequencing record's runs.
    Runs,
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
            (3, Hash32, Required),
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
        "order.voided" | "order.reopened" => vec![(1, Reason, Required), (2, Note, Optional)],
        "order.abandoned" | "order.closed" => vec![],
        "order.lines_allocated" => vec![(1, Allocations, Required)],
        "order.check_closed" => vec![
            (1, Id, Required),
            (2, Hash32, Required),
            (3, LineCharges, Required),
            (4, TaxCharges, Required),
            (5, Money, Required),
            (6, IdSet, Optional),
        ],
        "payment.initiated" => {
            vec![(1, Id, Required), (2, Id, Required), (3, Code(1), Required), (4, Money, Required)]
        }
        "payment.authorized" => vec![(1, Money, Required), (2, Reference, Optional)],
        "payment.captured" => vec![
            (1, Money, Required),
            (2, Money, Optional),
            (3, Reference, Optional),
            (4, Money, Optional),
            (5, Money, Optional),
        ],
        "payment.failed" | "payment.voided" => {
            vec![(1, Reason, Required), (2, Note, Optional), (3, Reference, Optional)]
        }
        "sequence.assigned" => {
            vec![(1, Number, Required), (2, Number, Required), (3, Runs, Required)]
        }
        "order.ownership_requested" => vec![(1, Lease, Required)],
        "order.ownership_granted" => {
            vec![(1, Id, Required), (2, Id, Required), (3, Number, Required), (4, Number, Required)]
        }
        "order.ownership_refused" => vec![(1, Id, Required), (2, Code(2), Required)],
        "order.ownership_overridden" => {
            vec![(1, Lease, Required), (2, Reason, Required), (3, Note, Optional)]
        }
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

/// The entries of an array of maps with exactly the keys 1 to `keys`, each a `kinds` value.
fn records(value: &Value, kinds: &[Kind]) -> Option<Vec<Vec<Value>>> {
    value
        .as_array()?
        .iter()
        .map(|item| {
            let map = item.as_map()?;
            let keys: Vec<u64> = map.iter().filter_map(|(key, _)| key.as_u64()).collect();
            let expected: Vec<u64> = (1..=u64::try_from(kinds.len()).unwrap()).collect();
            if keys != expected || map.len() != kinds.len() {
                return None;
            }
            let values: Vec<Value> = map.iter().map(|(_, value)| value.clone()).collect();
            values
                .iter()
                .zip(kinds)
                .all(|(value, &kind)| valid_value(kind, value))
                .then_some(values)
        })
        .collect()
}

/// Whether identifiers come in strictly ascending byte order.
fn strictly_ascending(ids: &[&Value]) -> bool {
    ids.windows(2).all(|pair| pair[0].as_bytes() < pair[1].as_bytes())
}

/// Whether a sum of amounts fits an amount.
fn fits(sum: i128) -> bool {
    i64::try_from(sum).is_ok()
}

/// The rules of a closed check's snapshot, which span its fields.
fn snapshot_valid(map: &Map) -> bool {
    let get = |key: u64| map.get(&Value::Unsigned(key)).unwrap();
    let (total, currency) = money(get(5)).unwrap();
    let lines = records(get(3), &[Id, Money, Money, Money]).unwrap();
    let taxes = records(get(4), &[Id, Money, Money]).unwrap();
    let amounts = |values: &[Value]| {
        values[1..].iter().map(|value| money(value).unwrap()).collect::<Vec<_>>()
    };
    let lines_valid = !lines.is_empty()
        && strictly_ascending(&lines.iter().map(|line| &line[0]).collect::<Vec<_>>())
        && lines.iter().all(|line| {
            let [(gross, _), (net, _), (tax, _)] = amounts(line)[..] else { return false };
            amounts(line).iter().all(|&(_, c)| c == currency)
                && 0 <= net
                && net <= gross
                && tax >= 0
        });
    let taxes_valid = strictly_ascending(&taxes.iter().map(|tax| &tax[0]).collect::<Vec<_>>())
        && taxes.iter().all(|tax| {
            let [(taxable, _), (amount, _)] = amounts(tax)[..] else { return false };
            amounts(tax).iter().all(|&(_, c)| c == currency) && taxable > 0 && amount >= 0
        });
    let sum = |rows: &[Vec<Value>], at: usize| -> i128 {
        rows.iter().map(|row| i128::from(money(&row[at]).unwrap().0)).sum()
    };
    let (net, tax, taxed) = (sum(&lines, 2), sum(&lines, 3), sum(&taxes, 2));
    lines_valid
        && taxes_valid
        && fits(net)
        && fits(tax)
        && fits(taxed)
        && tax == taxed
        && net + tax == i128::from(total)
        && (total <= 0 || map.get(&Value::Unsigned(6)).is_some())
}

/// The rules of a capture, which span its fields.
fn capture_valid(map: &Map) -> bool {
    let get = |key: u64| map.get(&Value::Unsigned(key)).map(|value| money(value).unwrap());
    let (amount, currency) = get(1).unwrap();
    let in_currency = |key: u64| get(key).is_none_or(|(_, c)| c == currency);
    let tip = get(2).map_or(0, |(tip, _)| tip);
    let rounding = get(5).map_or(0, |(rounding, _)| rounding);
    // Paid step by step, as amounts: each running total must be one.
    let with_tip = i128::from(amount) + i128::from(tip);
    let paid = with_tip + i128::from(rounding);
    let cash_valid = match get(4) {
        None => get(5).is_none(),
        Some((tendered, _)) => {
            fits(with_tip) && fits(paid) && paid >= 0 && i128::from(tendered) >= paid
        }
    };
    amount > 0
        && [2, 4, 5].into_iter().all(in_currency)
        && get(2).is_none_or(|(tip, _)| tip > 0)
        && get(5).is_none_or(|(rounding, _)| rounding != 0)
        && cash_valid
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
        Hash32 => value.as_bytes().is_some_and(|bytes| bytes.len() == 32),
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
        Reference => value.as_text().is_some_and(|text| {
            (1..=100).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_graphic())
        }),
        LineCharges => records(value, &[Id, Money, Money, Money]).is_some(),
        TaxCharges => records(value, &[Id, Money, Money]).is_some(),
        Number => value.as_u64().is_some_and(|n| (1..=LARGEST).contains(&n)),
        Lease => value.as_u64().is_some_and(|n| n <= LARGEST),
        Runs => runs_valid(value),
    }
}

/// The largest epoch, number or position a sequencing record may hold: 2^63 − 1 (ADR-0020).
const LARGEST: u64 = (1 << 63) - 1;

/// The most runs a sequencing record may hold (ADR-0020).
const MOST_RUNS: usize = 1024;

/// A sequencing record's runs, if each is `[device, from, to, hash]`: a UUIDv7, two unsigned
/// integers and 32 bytes. Gives each run's device, first and last positions.
fn runs_of(value: &Value) -> Option<Vec<(Vec<u8>, u64, u64)>> {
    value
        .as_array()?
        .iter()
        .map(|run| {
            let [device, from, to, hash] = run.as_array()? else { return None };
            let shaped = is_uuid_v7(device) && hash.as_bytes().is_some_and(|hash| hash.len() == 32);
            shaped.then_some((device.as_bytes()?.to_vec(), from.as_u64()?, to.as_u64()?))
        })
        .collect()
}

/// Whether runs keep a record's rules: from 1 to [`MOST_RUNS`] of them, each from a first to a
/// last position no earlier, both from 1 to [`LARGEST`]; no two neighbours of one device; and
/// each run of a device starting right after the device's run before it.
fn runs_valid(value: &Value) -> bool {
    let Some(runs) = runs_of(value) else { return false };
    let in_range = |n: u64| (1..=LARGEST).contains(&n);
    // Where each device's next run must start.
    let mut next: std::collections::BTreeMap<Vec<u8>, u128> = std::collections::BTreeMap::new();
    let follow_on = runs.iter().all(|(device, from, to)| {
        let follows = next.get(device).is_none_or(|&start| start == u128::from(*from));
        next.insert(device.clone(), u128::from(*to) + 1);
        follows
    });
    (1..=MOST_RUNS).contains(&runs.len())
        && runs.iter().all(|&(_, from, to)| in_range(from) && in_range(to) && from <= to)
        && runs.windows(2).all(|pair| pair[0].0 != pair[1].0)
        && follow_on
}

/// How many events valid runs number.
fn events_in(runs: &Value) -> u128 {
    runs_of(runs).unwrap().iter().map(|&(_, from, to)| u128::from(to - from) + 1).sum()
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
        "order.check_closed" => snapshot_valid(map),
        "payment.initiated" => money(get(4).unwrap()).unwrap().0 > 0,
        "payment.authorized" => money(get(1).unwrap()).unwrap().0 > 0,
        "payment.captured" => capture_valid(map),
        // The last number the record assigns is the largest at most.
        "sequence.assigned" => {
            u128::from(get(2).unwrap().as_u64().unwrap()) + events_in(get(3).unwrap()) - 1
                <= u128::from(LARGEST)
        }
        _ => true,
    }
}

fn schema_ref(event: &Event) -> SchemaRef {
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
        Hash32 => prop::collection::vec(any::<u8>(), 31..=33).prop_map(Value::Bytes).boxed(),
        Reference => prop_oneof![
            text("[!-~]{1,3}"),
            text("[!-~]{99,101}"),
            text("[a-z]{1,4} [a-z]{1,4}"),
            Just(Value::from("")),
            Just(Value::from("pi_é")),
            Just(Value::from("tab\there")),
        ]
        .boxed(),
        LineCharges => charges_near_miss(3),
        TaxCharges => charges_near_miss(4),
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
        Number | Lease => number_near_miss(),
        Runs => runs_near_miss(),
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

/// Epochs and numbers close to a sequencing record's: valid, at the ends, or past them.
fn number_near_miss() -> BoxedStrategy<Value> {
    let int = |range: core::ops::RangeInclusive<i64>| range.prop_map(Value::integer);
    let text = |pattern: &'static str| pattern.prop_map(Value::Text);
    prop_oneof![
        prop::sample::select(vec![0, 1, 2, LARGEST - 1, LARGEST, LARGEST + 1, u64::MAX])
            .prop_map(Value::Unsigned),
        int(-2..=-1),
        text("[0-9]{1,3}"),
    ]
    .boxed()
}

/// Runs close to a sequencing record's: valid, spoiled in one way, as many as a record may hold
/// and one more, or none.
fn runs_near_miss() -> BoxedStrategy<Value> {
    prop_oneof![
        2 => any_assigned().prop_map(|record| runs_value(&record)),
        8 => (any_assigned(), any_spoil())
            .prop_map(|(record, (how, at, other, choice))| {
                spoil_runs(&runs_value(&record), how, at, other, choice)
            }),
        1 => prop::sample::select(vec![MOST_RUNS - 1, MOST_RUNS, MOST_RUNS + 1])
            .prop_map(taking_turns),
        1 => Just(Value::Array(Vec::new())),
    ]
    .boxed()
}

/// Charges close to a snapshot's: its lines' (key 3) or taxes' (key 4), valid or spoiled.
fn charges_near_miss(key: u64) -> BoxedStrategy<Value> {
    let field = move |closed: &CheckClosed| {
        let payload = OrderEvent::CheckClosed(closed.clone()).to_value();
        payload.as_map().unwrap().get(&Value::Unsigned(key)).unwrap().clone()
    };
    prop_oneof![
        2 => any_check_closed().prop_map(move |closed| field(&closed)),
        // A valid list spoiled: out of order, repeated, an amount changed, a field removed, or
        // another currency.
        4 => (any_check_closed(), 0_u8..5, any::<prop::sample::Index>(), -2_i64..=2)
            .prop_map(move |(closed, how, at, by)| spoil_charges(&field(&closed), how, at, by)),
        1 => Just(Value::Array(Vec::new())),
    ]
    .boxed()
}

/// Charges with one thing wrong, or right after all: the entry at `at` swapped with the next,
/// repeated, with an amount moved by `by`, with a field removed, or in another currency.
fn spoil_charges(value: &Value, how: u8, at: prop::sample::Index, by: i64) -> Value {
    let mut items = value.as_array().unwrap().to_vec();
    if items.is_empty() {
        return value.clone();
    }
    let (i, count) = (at.index(items.len()), items.len());
    let mut entries = items[i].as_map().unwrap().clone().into_entries();
    let amounts = entries.len() - 1;
    match how {
        0 if count > 1 => items.swap(i, (i + 1) % count),
        1 => items.insert(i, items[i].clone()),
        2 => {
            let which = usize::try_from(by.unsigned_abs()).unwrap() % amounts;
            let (_, amount) = &mut entries[1 + which];
            let [minor, code] = amount.as_array().unwrap() else { panic!("an amount") };
            let moved = as_i64(minor).unwrap().saturating_add(by);
            *amount = Value::Array(vec![Value::integer(moved), code.clone()]);
            items[i] = Value::Map(Map::from_entries(entries).unwrap());
        }
        3 => {
            entries.pop();
            items[i] = Value::Map(Map::from_entries(entries).unwrap());
        }
        _ => {
            for (key, amount) in &mut entries {
                if key.as_u64() != Some(1) {
                    recurrency(amount);
                }
            }
            items[i] = Value::Map(Map::from_entries(entries).unwrap());
        }
    }
    Value::Array(items)
}

/// Changes one thing in a valid snapshot, aimed at one of its rules, and keeps the other rules
/// where it can. `how` picks the change; `at` and `other` pick the lines or taxes it touches,
/// `other` never the same as `at` when there are two; `by` is the amount it sets or moves:
/// - 0: a line's gross set to its net and `by`: no sum includes the gross;
/// - 1: a line's net set to `by`, the total moved to match;
/// - 2: a line's tax set to `by`, the difference moved to another line, so the sums hold;
/// - 3: a tax's amount set to `by`, the difference moved to another tax;
/// - 4: a tax's taxable amount set to `by`: no sum includes it;
/// - 5, 6: two lines, or two taxes, swapped;
/// - 7, 8: a line repeated with nothing charged, or a tax repeated with no tax;
/// - 9: no lines, no taxes and a total of zero;
/// - 10, 11: an amount of a line, or of a tax, in another currency;
/// - 12 to 15: the total, a line's net or tax, or a tax's amount, moved by `by`;
/// - 16: the payments dropped;
/// - otherwise, nothing: the snapshot stays valid.
///
/// With only one line or one tax, a change that moves a difference to another moves it to the
/// taxes and the total instead, which may break a second rule; the model judges either way.
fn spoil_snapshot(
    closed: &mut CheckClosed,
    how: u8,
    at: prop::sample::Index,
    other: prop::sample::Index,
    by: i64,
) {
    let set = |money: Money, minor: i64| Money::from_minor(minor, money.currency());
    let moved = |money: Money, by: i64| set(money, money.minor() + by);
    let foreign = |money: Money| {
        let usd = Currency::from_code("USD").unwrap();
        let other = if money.currency() == usd { Currency::from_code("EUR").unwrap() } else { usd };
        Money::from_minor(money.minor(), other)
    };
    // An entry, and another one whenever there are two.
    let pick = |count: usize| {
        let first = at.index(count);
        let second = if count > 1 { (first + 1 + other.index(count - 1)) % count } else { first };
        (first, second)
    };
    let (i, k) = pick(closed.lines.len());
    let (j, l) = if closed.taxes.is_empty() { (0, 0) } else { pick(closed.taxes.len()) };
    let taxed = !closed.taxes.is_empty();
    match how {
        0 => closed.lines[i].gross = moved(closed.lines[i].net, by),
        1 => {
            let old = closed.lines[i].net;
            closed.lines[i].net = set(old, by);
            closed.total = moved(closed.total, by - old.minor());
        }
        2 => {
            let old = closed.lines[i].tax;
            closed.lines[i].tax = set(old, by);
            let rest = old.minor() - by;
            if k == i {
                closed.total = moved(closed.total, -rest);
                if let Some(tax) = closed.taxes.first_mut() {
                    tax.amount = moved(tax.amount, -rest);
                }
            } else {
                closed.lines[k].tax = moved(closed.lines[k].tax, rest);
            }
        }
        3 if taxed => {
            let old = closed.taxes[j].amount;
            closed.taxes[j].amount = set(old, by);
            let rest = old.minor() - by;
            if l == j {
                closed.lines[i].tax = moved(closed.lines[i].tax, -rest);
                closed.total = moved(closed.total, -rest);
            } else {
                closed.taxes[l].amount = moved(closed.taxes[l].amount, rest);
            }
        }
        4 if taxed => closed.taxes[j].taxable = set(closed.taxes[j].taxable, by),
        5 => closed.lines.swap(i, k),
        6 if taxed => closed.taxes.swap(j, l),
        7 => {
            let nothing = set(closed.total, 0);
            let repeat = LineCharge {
                line: closed.lines[i].line,
                gross: nothing,
                net: nothing,
                tax: nothing,
            };
            closed.lines.insert(i + 1, repeat);
        }
        8 if taxed => {
            let repeat = TaxCharge { amount: set(closed.total, 0), ..closed.taxes[j] };
            closed.taxes.insert(j + 1, repeat);
        }
        9 => {
            closed.lines.clear();
            closed.taxes.clear();
            closed.total = set(closed.total, 0);
        }
        10 => {
            let line = &mut closed.lines[i];
            match other.index(3) {
                0 => line.gross = foreign(line.gross),
                1 => line.net = foreign(line.net),
                _ => line.tax = foreign(line.tax),
            }
        }
        11 if taxed => {
            let tax = &mut closed.taxes[j];
            if other.index(2) == 0 {
                tax.taxable = foreign(tax.taxable);
            } else {
                tax.amount = foreign(tax.amount);
            }
        }
        12 => closed.total = moved(closed.total, by),
        13 => closed.lines[i].net = moved(closed.lines[i].net, by),
        14 => closed.lines[i].tax = moved(closed.lines[i].tax, by),
        15 if taxed => closed.taxes[j].amount = moved(closed.taxes[j].amount, by),
        16 => closed.payments = None,
        _ => {}
    }
}

/// Device `n`.
fn device(n: u64) -> Id<Device> {
    support::id(n)
}

/// A hash for the run at `index` of a record: bytes that step up, all different, so that a byte
/// lost or moved shows.
fn run_hash(index: usize) -> EventHash {
    let first = u8::try_from(index % 256).unwrap();
    EventHash::from_bytes(core::array::from_fn(|k| first.wrapping_add(u8::try_from(k).unwrap())))
}

/// A valid sequencing record: up to three devices taking turns, so that most have several runs,
/// each starting in its log at the beginning or near the largest position, with runs of one to
/// three events or up to the end of its log, a device's turns side by side making one run. Its
/// first number is 1, the largest that fits, or any between.
fn any_assigned() -> impl Strategy<Value = Assigned> {
    let epoch = prop_oneof![3 => 1_u64..=3, 1 => (LARGEST - 2)..=LARGEST];
    let start = || prop_oneof![3 => 1_u64..=5, 1 => (LARGEST - 4)..=LARGEST];
    let length = prop_oneof![6 => 1_u64..=3, 1 => Just(u64::MAX)];
    let turns = prop::collection::vec((0_usize..3, length), 1..=12);
    let first = (0_u8..3, any::<u64>());
    (epoch, [start(), start(), start()], turns, first).prop_map(
        |(epoch, mut next, turns, (which, any))| {
            let mut runs: Vec<Run> = Vec::new();
            let mut count = 0_u64;
            for (who, length) in turns {
                let from = next[who];
                // The events the record can still number.
                let room = LARGEST - count;
                if from > LARGEST || room == 0 {
                    continue;
                }
                let to = from.saturating_add(length.min(room) - 1).min(LARGEST);
                count += to - from + 1;
                next[who] = to + 1;
                let id = device(u64::try_from(who).unwrap() + 1);
                let last = run_hash(runs.len());
                match runs.last_mut() {
                    Some(run) if run.device == id => {
                        run.to = to;
                        run.last = last;
                    }
                    _ => runs.push(Run { device: id, from, to, last }),
                }
            }
            let largest_first = LARGEST - count + 1;
            let first = match which {
                0 => 1,
                1 => largest_first,
                _ => 1 + any % largest_first,
            };
            Assigned { epoch, first, runs }
        },
    )
}

/// What [`spoil_runs`] takes: which change, which runs, and which value.
fn any_spoil() -> impl Strategy<Value = (u8, prop::sample::Index, prop::sample::Index, u8)> {
    (0_u8..14, any::<prop::sample::Index>(), any::<prop::sample::Index>(), any::<u8>())
}

/// The payload encoding of a record's runs.
fn runs_value(record: &Assigned) -> Value {
    let payload = SequenceEvent::Assigned(record.clone()).to_value();
    payload.as_map().unwrap().get(&Value::Unsigned(3)).unwrap().clone()
}

/// `count` runs of two devices taking turns, an event each: valid up to [`MOST_RUNS`].
fn taking_turns(count: usize) -> Value {
    let runs = (1_u64..)
        .flat_map(|at| [(1, at), (2, at)])
        .take(count)
        .enumerate()
        .map(|(index, (n, at))| Run { device: device(n), from: at, to: at, last: run_hash(index) })
        .collect();
    runs_value(&Assigned { epoch: 1, first: 1, runs })
}

/// Runs with one thing wrong, or right after all. `how` picks the change, `at` the run it
/// touches, `other` another run, and `choice` a value within the change:
/// - 0: two runs swapped;
/// - 1: a run repeated;
/// - 2: a run dropped;
/// - 3, 4: a run's first or last position set to 0, 1, one less, one more, the largest, one
///   past it, or the largest integer;
/// - 5: a run moved, both ends, by −2, −1, 1 or 2 positions, into a gap or an overlap with the
///   runs of its device around it;
/// - 6: a run given its neighbour's device;
/// - 7: a run split in two, side by side;
/// - 8: a hash a byte short or long;
/// - 9: a device that isn't a UUIDv7;
/// - 10: a run with an entry dropped or added;
/// - 11: a run given a device of its own;
/// - 12: every run stretched over its device's whole log, from 1 to the largest position;
/// - otherwise, nothing.
fn spoil_runs(
    value: &Value,
    how: u8,
    at: prop::sample::Index,
    other: prop::sample::Index,
    choice: u8,
) -> Value {
    let mut runs: Vec<Vec<Value>> =
        value.as_array().unwrap().iter().map(|run| run.as_array().unwrap().to_vec()).collect();
    let count = runs.len();
    let (i, j) = (at.index(count), other.index(count));
    let position = |value: &Value, choice: u8| {
        let position = value.as_u64().unwrap();
        let choices = [
            0,
            1,
            position.wrapping_sub(1),
            position.wrapping_add(1),
            LARGEST,
            LARGEST + 1,
            u64::MAX,
        ];
        Value::Unsigned(choices[usize::from(choice) % choices.len()])
    };
    match how {
        0 => runs.swap(i, j),
        1 => runs.insert(i, runs[i].clone()),
        2 if count > 1 => {
            runs.remove(i);
        }
        3 => runs[i][1] = position(&runs[i][1], choice),
        4 => runs[i][2] = position(&runs[i][2], choice),
        5 => {
            let by = [-2_i64, -1, 1, 2][usize::from(choice) % 4];
            for end in &mut runs[i][1..=2] {
                *end = Value::Unsigned(end.as_u64().unwrap().wrapping_add_signed(by));
            }
        }
        6 if count > 1 => {
            let neighbour = if i + 1 < count { i + 1 } else { i - 1 };
            runs[i][0] = runs[neighbour][0].clone();
        }
        7 => {
            let (from, to) = (runs[i][1].as_u64().unwrap(), runs[i][2].as_u64().unwrap());
            if from < to {
                let mut second = runs[i].clone();
                runs[i][2] = Value::Unsigned(from);
                second[1] = Value::Unsigned(from + 1);
                runs.insert(i + 1, second);
            }
        }
        8 => runs[i][3] = Value::Bytes(vec![0xAB; if choice.is_multiple_of(2) { 31 } else { 33 }]),
        9 => runs[i][0] = Value::Bytes(vec![0; 16]),
        10 => {
            if choice.is_multiple_of(2) {
                runs[i].pop();
            } else {
                runs[i].push(Value::Null);
            }
        }
        11 => runs[i][0] = support_id_value(0xD0 + u64::try_from(i).unwrap()),
        12 => {
            for run in &mut runs {
                run[1] = Value::Unsigned(1);
                run[2] = Value::Unsigned(LARGEST);
            }
        }
        _ => {}
    }
    Value::Array(runs.into_iter().map(Value::Array).collect())
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
/// price in a list of modifiers or charges, at any depth.
fn recurrency(value: &mut Value) {
    if money(value).is_some() {
        let Value::Array(parts) = value else { return };
        parts[1] = other_currency(&parts[1]);
        return;
    }
    match value {
        Value::Array(items) => items.iter_mut().for_each(recurrency),
        Value::Map(map) => {
            let mut entries = map.clone().into_entries();
            for (_, value) in &mut entries {
                recurrency(value);
            }
            *map = Map::from_entries(entries).unwrap();
        }
        _ => {}
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

/// Checks that `event`'s payload, with `change`, is accepted exactly when the model says it is
/// valid, and then has exactly one encoding.
fn accepted_exactly_when_valid(event: &Event, change: &FieldChange) -> Result<(), TestCaseError> {
    let schema = schema_ref(event);
    let payload = changed(schema.name.as_str(), &event.to_value(), change);
    let valid = valid_payload(schema.name.as_str(), &payload);
    match decode(&schema, &payload) {
        Ok(decoded) => {
            prop_assert!(valid, "accepted an invalid payload");
            prop_assert_eq!(decoded.encode(), payload.encode());
        }
        Err(error) => prop_assert!(!valid, "rejected a valid payload: {}", error),
    }
    Ok(())
}

/// Version 1 of the schema named `name`.
fn schema_named(name: &str) -> SchemaRef {
    SchemaRef { name: SchemaName::new(name).unwrap(), version: 1.try_into().unwrap() }
}

/// The keys of the fields of `schema` that hold prices.
fn priced_keys(schema: &str) -> Vec<u64> {
    rules(schema)
        .iter()
        .filter(|rule| matches!(rule.1, Money | Modifiers | LineCharges | TaxCharges))
        .map(|rule| rule.0)
        .collect()
}

/// An event of `events`, and a change to its payload.
fn any_case(events: impl Strategy<Value = Event>) -> impl Strategy<Value = (Event, FieldChange)> {
    events.prop_flat_map(|event| {
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
    fn events_round_trip(
        event in any_event(),
        payment in any_payment_event(),
        record in any_assigned(),
    ) {
        let (schema, payload) = event.encode().unwrap();
        prop_assert!(valid_payload(schema.name.as_str(), &payload.value().unwrap()));
        prop_assert_eq!(OrderEvent::decode(&schema, &payload), Ok(event));
        let (schema, payload) = payment.encode().unwrap();
        prop_assert!(valid_payload(schema.name.as_str(), &payload.value().unwrap()));
        prop_assert_eq!(PaymentEvent::decode(&schema, &payload), Ok(payment));
        let sequenced = SequenceEvent::Assigned(record.clone());
        let (schema, payload) = sequenced.encode().unwrap();
        prop_assert!(valid_payload(schema.name.as_str(), &payload.value().unwrap()));
        prop_assert_eq!(SequenceEvent::decode(&schema, &payload), Ok(sequenced));
        prop_assert_eq!(Assigned::new(record.epoch, record.first, record.runs.clone()), Ok(record));
    }

    /// A payload with one field changed, removed or added is accepted exactly when the model
    /// says it is valid, and then re-encodes to exactly the same bytes.
    #[test]
    fn changed_payloads_are_accepted_exactly_when_valid(
        (event, change) in any_case(any_event().prop_map(Event::Order)),
    ) {
        accepted_exactly_when_valid(&event, &change)?;
    }

    /// The same for payment payloads.
    #[test]
    fn changed_payment_payloads_are_accepted_exactly_when_valid(
        (event, change) in any_case(any_payment_event().prop_map(Event::Payment)),
    ) {
        accepted_exactly_when_valid(&event, &change)?;
    }

    /// The same for the events that move an order's ownership (ADR-0021), on their own, so that
    /// each of their fields is changed often.
    #[test]
    fn changed_ownership_payloads_are_accepted_exactly_when_valid(
        (event, change) in any_case(any_ownership_event().prop_map(Event::Order)),
    ) {
        accepted_exactly_when_valid(&event, &change)?;
    }

    /// Every number in an ownership payload, a lease or an epoch, set to each end of its range,
    /// next to them, and past them, decodes exactly when the model says it is valid: a grant's
    /// lease and epoch start at 1, a request's or an override's lease at 0.
    #[test]
    fn ownership_numbers_decode_exactly_within_their_ranges(event in any_ownership_event()) {
        let event = Event::Order(event);
        let numbers = rules(event.schema().name)
            .into_iter()
            .filter(|(_, kind, _)| matches!(kind, Number | Lease));
        for (key, _, _) in numbers {
            for n in [0, 1, 2, LARGEST - 1, LARGEST, LARGEST + 1, u64::MAX] {
                let change = FieldChange::Set { key, value: Value::Unsigned(n) };
                accepted_exactly_when_valid(&event, &change)?;
            }
        }
    }

    /// The same for sequencing records.
    #[test]
    fn changed_sequencing_records_are_accepted_exactly_when_valid(
        (event, change) in any_case(
            any_assigned().prop_map(|record| Event::Sequence(SequenceEvent::Assigned(record))),
        ),
    ) {
        accepted_exactly_when_valid(&event, &change)?;
    }

    /// Runs decode only when they keep every rule: a record's runs, spoiled in one way aimed at
    /// one rule, decode exactly when the model says they are valid.
    #[test]
    fn runs_decode_only_when_they_keep_every_rule(
        record in any_assigned(),
        (how, at, other, choice) in any_spoil(),
    ) {
        let payload = SequenceEvent::Assigned(record).to_value();
        let runs = payload.as_map().unwrap().get(&Value::Unsigned(3)).unwrap();
        let change = FieldChange::Set { key: 3, value: spoil_runs(runs, how, at, other, choice) };
        let spoiled = changed("sequence.assigned", &payload, &change);
        let decoded = SequenceEvent::from_value(&schema_named("sequence.assigned"), &spoiled);
        prop_assert_eq!(decoded.is_ok(), valid_payload("sequence.assigned", &spoiled));
    }

    /// A record holds at most [`MOST_RUNS`] runs: around the limit, two devices taking turns
    /// decode exactly while they are within it.
    #[test]
    fn records_hold_at_most_the_most_runs(count in (MOST_RUNS - 2)..=(MOST_RUNS + 2)) {
        let mut entries = SequenceEvent::Assigned(Assigned { epoch: 1, first: 1, runs: Vec::new() })
            .to_value()
            .as_map()
            .unwrap()
            .clone()
            .into_entries();
        entries.retain(|(key, _)| key.as_u64() != Some(3));
        entries.push((Value::Unsigned(3), taking_turns(count)));
        let payload = Value::Map(Map::from_entries(entries).unwrap());
        let decoded = SequenceEvent::from_value(&schema_named("sequence.assigned"), &payload);
        prop_assert_eq!(decoded.is_ok(), count <= MOST_RUNS);
        prop_assert_eq!(valid_payload("sequence.assigned", &payload), count <= MOST_RUNS);
    }

    /// A record numbers its events in order, from its first number on, run by run, and knows
    /// the number of each event it covers and of no other: a model counting run by run agrees.
    #[test]
    fn records_number_their_events_in_order(record in any_assigned()) {
        let mut before: u128 = 0;
        let mut firsts = Vec::new();
        for run in &record.runs {
            let first = u128::from(record.first) + before;
            firsts.push(u64::try_from(first).unwrap());
            for position in [run.from, run.from + (run.to - run.from) / 2, run.to] {
                let number = record.number_of(run.device, position).map(u128::from);
                prop_assert_eq!(number, Some(first + u128::from(position - run.from)));
            }
            before += u128::from(run.to - run.from) + 1;
        }
        prop_assert_eq!(u128::from(record.count()), before);
        prop_assert_eq!(u128::from(record.last()), u128::from(record.first) + before - 1);
        let numbered: Vec<u64> = record.numbered().map(|(number, _)| number).collect();
        prop_assert_eq!(numbered, firsts);
        // Just outside each device's stretch of its log, and in another device's log, nothing.
        for run in &record.runs {
            let runs = record.runs.iter().filter(|other| other.device == run.device);
            let lowest = runs.clone().map(|other| other.from).min().unwrap();
            let highest = runs.map(|other| other.to).max().unwrap();
            prop_assert_eq!(record.number_of(run.device, lowest - 1), None);
            prop_assert_eq!(record.number_of(run.device, highest + 1), None);
        }
        prop_assert_eq!(record.number_of(device(9), 1), None);
    }

    /// The numbers a record assigns reach exactly the largest: with the largest first number
    /// that fits, a record decodes, and with one larger, it doesn't.
    #[test]
    fn records_number_up_to_exactly_the_largest(record in any_assigned(), past in 0_u64..=2) {
        let first = LARGEST - record.count() + 1 + past;
        let payload = SequenceEvent::Assigned(Assigned { first, ..record }).to_value();
        let decoded = SequenceEvent::from_value(&schema_named("sequence.assigned"), &payload);
        prop_assert_eq!(decoded.is_ok(), past == 0);
    }

    /// A change changes something: with every optional field removed, an attribute or line
    /// change is refused, and with some left, accepted exactly when the model says so.
    #[test]
    fn a_change_changes_something(
        event in prop_oneof![
            support::any_attributes_changed().prop_map(|changed| Event::Order(OrderEvent::AttributesChanged(changed))),
            any_line_changed().prop_map(|changed| Event::Order(OrderEvent::LineChanged(changed))),
        ],
        emptied in any::<bool>(),
    ) {
        let change = if emptied { FieldChange::KeepOnlyRequired } else { FieldChange::AddText };
        let schema = schema_ref(&event);
        let payload = changed(schema.name.as_str(), &event.to_value(), &change);
        if emptied {
            prop_assert!(!valid_payload(schema.name.as_str(), &payload));
        }
        accepted_exactly_when_valid(&event, &change)?;
    }

    /// Every price in a payload is in one currency. Moving one price, at any depth, or every
    /// price in one field into another currency leaves a valid payload exactly when no price
    /// stays behind in the old one.
    #[test]
    fn a_payload_has_one_currency(
        event in prop_oneof![
            any_line_added().prop_map(|added| Event::Order(OrderEvent::LineAdded(added))),
            any_line_changed().prop_map(|changed| Event::Order(OrderEvent::LineChanged(changed))),
            any_check_closed().prop_map(|closed| Event::Order(OrderEvent::CheckClosed(closed))),
            any_captured().prop_map(|captured| Event::Payment(PaymentEvent::Captured(captured))),
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
        prop_assert_eq!(decode(&schema, &payload).is_ok(), valid);
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

    /// Processor references are 1 to 100 ASCII letters, digits and punctuation, with no spaces.
    /// A reference decoded from a payload follows the same rules.
    #[test]
    fn processor_references_hold_exactly_their_limits(
        length in prop_oneof![Just(0_usize), 1_usize..3, 98_usize..=102],
        letter in prop::sample::select(vec!['a', 'Z', '7', '_', '~']),
        odd in prop::option::weighted(0.3, (prop::sample::select(vec![' ', 'é', '\t', '\u{7f}']), any::<prop::sample::Index>())),
    ) {
        let mut chars = vec![letter; length];
        if let Some((odd, at)) = odd
            && !chars.is_empty()
        {
            let at = at.index(chars.len());
            chars[at] = odd;
        }
        let text: String = chars.into_iter().collect();
        let value = Value::from(text.as_str());
        prop_assert_eq!(keel_domain::codec::ProcessorRef::new(&text).is_ok(), valid_value(Reference, &value));
        let authorized = Map::from_entries(vec![
            (Value::Unsigned(1), Value::Array(vec![Value::Unsigned(100), Value::from("USD")])),
            (Value::Unsigned(2), value.clone()),
        ]).unwrap();
        prop_assert_eq!(
            decode(&schema_named("payment.authorized"), &Value::Map(authorized)).is_ok(),
            valid_value(Reference, &value)
        );
    }

    /// A snapshot decodes only if it keeps every rule. Each case changes one thing, aimed at
    /// one rule, and keeps the other rules where it can (see `spoil_snapshot`): a gross below
    /// its net, a net, tax or taxable amount at or below zero with the sums still adding up,
    /// lines or taxes out of order or repeated, no lines at all, an amount in another currency,
    /// a sum a unit or two off, or the payments dropped. The payload decodes exactly when the
    /// model says it is valid.
    #[test]
    fn snapshots_decode_only_when_they_keep_every_rule(
        closed in any_check_closed(),
        how in 0_u8..18,
        at in any::<prop::sample::Index>(),
        other in any::<prop::sample::Index>(),
        by in prop_oneof![Just(-1_i64), Just(1), -2_i64..=2],
    ) {
        let mut closed = closed;
        spoil_snapshot(&mut closed, how, at, other, by);
        let payload = OrderEvent::CheckClosed(closed).to_value();
        let schema = schema_named("order.check_closed");
        prop_assert_eq!(decode(&schema, &payload).is_ok(), valid_payload("order.check_closed", &payload));
    }

    /// Cash covers what is paid: the amount, the tip and the rounding, which add up to zero or
    /// more. What was tendered, a unit or two either side of that, and roundings around zero,
    /// with or without what was tendered, decode exactly when the model says so.
    #[test]
    fn cash_decodes_only_when_it_covers_what_is_paid(
        amount in prop_oneof![1_i64..5, 1_i64..10_000],
        tip in prop::option::of(prop_oneof![0_i64..3, 1_i64..1_000]),
        rounding in prop::option::of(-3_i64..=3),
        over in -2_i64..=2,
        tendered in prop::bool::weighted(0.9),
    ) {
        let usd = |minor: i64| Value::Array(vec![Value::integer(minor), Value::from("USD")]);
        let paid = amount + tip.unwrap_or(0) + rounding.unwrap_or(0);
        let mut entries = vec![(Value::Unsigned(1), usd(amount))];
        entries.extend(tip.map(|tip| (Value::Unsigned(2), usd(tip))));
        entries.extend(tendered.then(|| (Value::Unsigned(4), usd(paid + over))));
        entries.extend(rounding.map(|rounding| (Value::Unsigned(5), usd(rounding))));
        let payload = Value::Map(Map::from_entries(entries).unwrap());
        prop_assert_eq!(
            decode(&schema_named("payment.captured"), &payload).is_ok(),
            valid_payload("payment.captured", &payload)
        );
    }

    /// Identifier sets are built from any identifiers that are distinct and not empty, and hold
    /// them in ascending byte order. A payload's set decodes only if its identifiers come in
    /// strictly ascending byte order.
    #[test]
    fn identifier_sets_are_sets(ids in prop::collection::vec(0_u64..8, 0..6)) {
        let ids: Vec<Id<()>> = ids.into_iter().map(support::id).collect();
        let distinct: std::collections::BTreeSet<[u8; 16]> = ids.iter().map(|id| id.to_bytes()).collect();
        match keel_domain::codec::IdSet::new(ids.clone()) {
            Ok(set) => {
                prop_assert!(!ids.is_empty() && distinct.len() == ids.len());
                let bytes: Vec<[u8; 16]> = set.iter().map(Id::to_bytes).collect();
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
    fn unknown_schemas_are_reported(event in any_any_event(), version in 2_u32..5) {
        let schema = schema_ref(&event);
        let newer = SchemaRef { name: schema.name.clone(), version: version.try_into().unwrap() };
        prop_assert_eq!(decode(&newer, &event.to_value()), Err(DecodeError::UnknownSchema));
        for name in ["order.unknown", "payment.unknown", "sequence.unknown"] {
            let renamed = SchemaRef { name: SchemaName::new(name).unwrap(), version: schema.version };
            prop_assert_eq!(decode(&renamed, &event.to_value()), Err(DecodeError::UnknownSchema));
        }
        // No kind of stream decodes another's schemas.
        let payload = event.to_value();
        let as_order = OrderEvent::from_value(&schema, &payload).map(|_| ());
        let as_payment = PaymentEvent::from_value(&schema, &payload).map(|_| ());
        let as_sequence = SequenceEvent::from_value(&schema, &payload).map(|_| ());
        let crossed = match &event {
            Event::Order(_) => [as_payment, as_sequence],
            Event::Payment(_) => [as_order, as_sequence],
            Event::Sequence(_) => [as_order, as_payment],
        };
        for decoded in crossed {
            prop_assert_eq!(decoded, Err(DecodeError::UnknownSchema));
        }
    }
}
