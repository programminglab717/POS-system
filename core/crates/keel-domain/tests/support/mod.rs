//! Shared support for property tests: identifiers, and strategies for order events.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use core::num::{NonZeroU8, NonZeroU16};

use keel_domain::codec::{CatalogVersion, Change, IdSet, Name, Note, ReasonCode};
use keel_domain::order::{
    AttributesChanged, Channel, ChosenModifier, ItemSnapshot, LineAdded, LineChanged, Mode,
    OrderCreated, OrderEvent, Placement, Prefix, Reason,
};
use keel_types::{Currency, Id, Money, Quantity, Unit};
use proptest::prelude::*;

/// A valid UUIDv7 ending in `n`.
pub(crate) fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

pub(crate) fn usd() -> Currency {
    Currency::from_code("USD").unwrap()
}

pub(crate) fn any_id<T>() -> impl Strategy<Value = Id<T>> {
    (0_u64..0xFFFF_FFFF_FFFF).prop_map(id)
}

pub(crate) fn any_currency() -> impl Strategy<Value = Currency> {
    prop::sample::select(vec!["USD", "EUR", "JPY", "KWD"])
        .prop_map(|code| Currency::from_code(code).unwrap())
}

/// A price in `currency`: zero or more, favouring small amounts and zero.
pub(crate) fn price(currency: Currency) -> impl Strategy<Value = Money> {
    prop_oneof![1 => Just(0_i64), 4 => 0_i64..10_000, 1 => 0_i64..=i64::MAX]
        .prop_map(move |minor| Money::from_minor(minor, currency))
}

/// A positive quantity.
pub(crate) fn any_quantity() -> impl Strategy<Value = Quantity> {
    let unit = prop::sample::select(vec![Unit::Each, Unit::Kilogram, Unit::Pound, Unit::Litre]);
    (prop_oneof![1_i64..10_000_000, 1_i64..=i64::MAX], unit)
        .prop_map(|(micros, unit)| Quantity::from_micros(micros, unit))
}

pub(crate) fn any_name() -> impl Strategy<Value = Name> {
    prop_oneof!["[A-Za-z][A-Za-z ]{0,20}", "\\PC{1,40}", "[a-zé]{200}"]
        .prop_filter_map("a valid name", |text| Name::new(&text).ok())
}

pub(crate) fn any_note() -> impl Strategy<Value = Note> {
    prop_oneof!["[a-z ]{1,30}", "[a-z]{1,10}\n[a-z]{1,10}", "\\PC{1,60}"]
        .prop_filter_map("a valid note", |text| Note::new(&text).ok())
}

pub(crate) fn any_reason() -> impl Strategy<Value = Reason> {
    let code = "[a-z][a-z0-9_]{0,31}".prop_map(|code| ReasonCode::new(&code).unwrap());
    (code, prop::option::of(any_note())).prop_map(|(code, note)| Reason { code, note })
}

pub(crate) fn any_code<T: Copy + core::fmt::Debug + 'static>(
    all: &'static [T],
) -> impl Strategy<Value = T> {
    prop::sample::select(all)
}

/// Modifiers priced in `currency`, nested up to `depth` levels.
pub(crate) fn modifiers(currency: Currency, depth: u32) -> BoxedStrategy<Vec<ChosenModifier>> {
    let nested = if depth == 0 { Just(Vec::new()).boxed() } else { modifiers(currency, depth - 1) };
    let modifier = (
        any_id(),
        any_name(),
        any_code(Prefix::ALL),
        any::<NonZeroU8>(),
        any_code(Placement::ALL),
        price(currency),
        nested,
    )
        .prop_map(|(modifier, name, prefix, quantity, placement, unit_price, modifiers)| {
            ChosenModifier { modifier, name, prefix, quantity, placement, unit_price, modifiers }
        });
    prop::collection::vec(modifier, 0..3).boxed()
}

pub(crate) fn any_created() -> impl Strategy<Value = OrderCreated> {
    (
        any_code(Channel::ALL),
        any_code(Mode::ALL),
        any_currency(),
        prop::option::of(any_id()),
        prop::option::of(any_id()),
        prop::option::of(any::<NonZeroU16>()),
        prop::option::of(any_id()),
        prop::option::of(any_id()),
    )
        .prop_map(
            |(channel, mode, currency, revenue_center, table, guest_count, customer, owner)| {
                OrderCreated {
                    channel,
                    mode,
                    currency,
                    revenue_center,
                    table,
                    guest_count,
                    customer,
                    owner,
                }
            },
        )
}

fn change<T: core::fmt::Debug + Clone + 'static>(
    value: impl Strategy<Value = T> + 'static,
) -> impl Strategy<Value = Option<Change<T>>> {
    prop_oneof![2 => Just(None), 1 => Just(Some(Change::Clear)), 2 => value.prop_map(|value| Some(Change::Set(value)))]
}

pub(crate) fn any_attributes_changed() -> impl Strategy<Value = AttributesChanged> {
    (
        prop::option::of(any_code(Mode::ALL)),
        change(any_id()),
        change(any_id()),
        change(any::<NonZeroU16>()),
        change(any_id()),
        change(any_id()),
    )
        .prop_map(|(mode, revenue_center, table, guest_count, customer, owner)| AttributesChanged {
            mode,
            revenue_center,
            table,
            guest_count,
            customer,
            owner,
        })
        .prop_filter("a change changes something", |changed| {
            *changed != AttributesChanged::default()
        })
}

pub(crate) fn any_line_added() -> impl Strategy<Value = LineAdded> {
    any_currency().prop_flat_map(|currency| {
        let item = (any_id(), any::<[u8; 32]>(), any_name(), any_id(), price(currency)).prop_map(
            |(variant, catalog_version, name, tax_category, unit_price)| ItemSnapshot {
                variant,
                catalog_version: CatalogVersion::from_bytes(catalog_version),
                name,
                tax_category,
                unit_price,
            },
        );
        (
            any_id(),
            item,
            any_quantity(),
            modifiers(currency, 2),
            prop::option::of(any::<NonZeroU16>()),
            prop::option::of(any::<NonZeroU8>()),
            prop::option::of(any_note()),
        )
            .prop_map(|(line, item, quantity, modifiers, seat, course, notes)| LineAdded {
                line,
                item,
                quantity,
                modifiers,
                seat,
                course,
                notes,
            })
    })
}

pub(crate) fn any_line_changed() -> impl Strategy<Value = LineChanged> {
    (
        any_id(),
        prop::option::of(any_quantity()),
        prop::option::of(any_currency().prop_flat_map(|currency| modifiers(currency, 1))),
        change(any::<NonZeroU16>()),
        change(any::<NonZeroU8>()),
        change(any_note()),
    )
        .prop_map(|(line, quantity, modifiers, seat, course, notes)| LineChanged {
            line,
            quantity,
            modifiers,
            seat,
            course,
            notes,
        })
        .prop_filter("a change changes something", |changed| {
            *changed != LineChanged::to(changed.line)
        })
}

pub(crate) fn any_id_set<T: 'static>() -> impl Strategy<Value = IdSet<T>> {
    prop::collection::btree_set(0_u64..0xFFFF_FFFF, 1..5)
        .prop_map(|ids| IdSet::new(ids.into_iter().map(id)).unwrap())
}

/// Any order event, of any kind.
pub(crate) fn any_event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        any_created().prop_map(OrderEvent::Created),
        any_attributes_changed().prop_map(OrderEvent::AttributesChanged),
        any_line_added().prop_map(OrderEvent::LineAdded),
        any_line_changed().prop_map(OrderEvent::LineChanged),
        any_id().prop_map(|line| OrderEvent::LineRemoved { line }),
        any_id_set().prop_map(|lines| OrderEvent::LinesFired { lines }),
        (any_id(), any_reason()).prop_map(|(line, reason)| OrderEvent::LineVoided { line, reason }),
        (any_id(), any_reason()).prop_map(|(line, reason)| OrderEvent::LineComped { line, reason }),
        any_reason().prop_map(|reason| OrderEvent::Voided { reason }),
        Just(OrderEvent::Abandoned),
    ]
}
