//! Shared support for property tests: identifiers, and strategies for order and payment events.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use core::num::{NonZeroU8, NonZeroU16};

use keel_domain::codec::{
    CatalogVersion, Change, IdSet, Name, Note, ProcessorRef, ReasonCode, RulesVersion,
};
use keel_domain::order::{
    Allocation, AttributesChanged, Channel, CheckClosed, ChosenModifier, Epoch, ItemSnapshot,
    Lease, LineAdded, LineChanged, LineCharge, LinesAllocated, Mode, OrderCreated, OrderEvent,
    OwnershipGranted, Placement, Prefix, Reason, Refusal, TaxCharge,
};
use keel_domain::payment::{
    CashTendered, PaymentAuthorized, PaymentCaptured, PaymentEnded, PaymentEvent, PaymentInitiated,
    Tender,
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

/// Allocations of lines to checks: sometimes of one line to several checks, among few lines
/// and checks, and sometimes of any lines to any checks; shares mostly small, sometimes large.
pub(crate) fn any_lines_allocated() -> impl Strategy<Value = LinesAllocated> {
    let shares = prop_oneof![4 => 1_u16..=4, 1 => 1_u16..=u16::MAX]
        .prop_map(|shares| NonZeroU16::new(shares).unwrap());
    let few = (0_u64..4, 0_u64..4);
    let any = (0_u64..0xFFFF_FFFF, 0_u64..0xFFFF_FFFF);
    prop::collection::btree_map(prop_oneof![3 => few, 1 => any], shares, 1..8).prop_map(
        |allocations| {
            let allocations = allocations.into_iter().map(|((line, check), shares)| Allocation {
                line: id(line),
                check: id(0x10 + check),
                shares,
            });
            LinesAllocated::new(allocations).unwrap()
        },
    )
}

/// An amount of zero or more: mostly small, sometimes large enough that the rules' sums must be
/// done carefully, but never so large that four of them overflow.
fn amount() -> impl Strategy<Value = i64> {
    prop_oneof![1 => Just(0_i64), 6 => 0_i64..10_000, 1 => 0_i64..=(i64::MAX / 16)]
}

/// A closed check's snapshot that satisfies the payload rules: one to four lines in ascending
/// order, each with a net between zero and its gross; taxes that add up to the lines' tax, in
/// ascending order, each taxing more than zero; a total that adds up; and payments whenever
/// there is something to pay.
pub(crate) fn any_check_closed() -> impl Strategy<Value = CheckClosed> {
    let line = (amount(), 0_i64..=4, amount());
    let lines = prop::collection::btree_map(0_u64..0xFFFF_FFFF, line, 1..=4);
    let taxes = prop::collection::btree_map(0_u64..0xFFFF_FFFF, (1_u64..=5, 1_i64..100_000), 0..=3);
    let payments = prop::option::of(any_id_set());
    (any_currency(), any_id(), any::<[u8; 32]>(), lines, taxes, payments).prop_map(
        |(currency, check, version, lines, taxes, payments)| {
            let money = |minor| Money::from_minor(minor, currency);
            // With no taxes, the lines have no tax either.
            let untaxed = taxes.is_empty();
            let lines: Vec<LineCharge> = lines
                .into_iter()
                .map(|(line, (gross, quarters, tax))| LineCharge {
                    line: id(line),
                    gross: money(gross),
                    net: money(gross / 4 * quarters),
                    tax: money(if untaxed { 0 } else { tax }),
                })
                .collect();
            let tax = Money::sum(currency, lines.iter().map(|line| line.tax)).unwrap();
            let net = Money::sum(currency, lines.iter().map(|line| line.net)).unwrap();
            // The lines' tax, shared among the taxes.
            let weights: Vec<u64> = taxes.values().map(|&(weight, _)| weight).collect();
            let parts = if untaxed { Vec::new() } else { tax.allocate(&weights).unwrap() };
            let taxes = taxes
                .into_iter()
                .zip(parts)
                .map(|((tax, (_, taxable)), amount)| TaxCharge {
                    tax: id(tax),
                    taxable: money(taxable),
                    amount,
                })
                .collect();
            let total = net.checked_add(tax).unwrap();
            CheckClosed {
                check,
                rules_version: RulesVersion::from_bytes(version),
                lines,
                taxes,
                total,
                payments: if total.is_positive() {
                    payments.or_else(|| Some(IdSet::new([id(0xB1)]).unwrap()))
                } else {
                    payments
                },
            }
        },
    )
}

pub(crate) fn any_reference() -> impl Strategy<Value = ProcessorRef> {
    prop_oneof!["[a-z]{2}_[A-Za-z0-9]{1,24}", "[!-~]{1,100}", "[!-~]{100}"]
        .prop_map(|text| ProcessorRef::new(&text).unwrap())
}

/// An amount more than zero in `currency`.
pub(crate) fn positive(currency: Currency) -> impl Strategy<Value = Money> {
    prop_oneof![4 => 1_i64..10_000, 1 => 1_i64..=(i64::MAX / 16)]
        .prop_map(move |minor| Money::from_minor(minor, currency))
}

fn any_ended() -> impl Strategy<Value = PaymentEnded> {
    (any_reason(), prop::option::of(any_reference()))
        .prop_map(|(reason, reference)| PaymentEnded { reason, reference })
}

/// A capture that satisfies the payload rules: for cash, what was tendered covers the amount,
/// the tip and the rounding.
pub(crate) fn any_captured() -> impl Strategy<Value = PaymentCaptured> {
    any_currency().prop_flat_map(|currency| {
        let cash = prop::option::of((0_i64..10_000, prop::option::of(1_i64..=100), any::<bool>()));
        (
            positive(currency),
            prop::option::of(positive(currency)),
            prop::option::of(any_reference()),
            cash,
        )
            .prop_map(move |(amount, tip, reference, cash)| {
                let due = tip.map_or(amount, |tip| amount.checked_add(tip).unwrap());
                let cash = cash.map(|(extra, rounding, down)| {
                    // Rounded down by no more than what is due.
                    let rounding = rounding
                        .map(|r| {
                            let r = if down { -r.min(due.minor()) } else { r };
                            Money::from_minor(r, currency)
                        })
                        .filter(|rounding| !rounding.is_zero());
                    let paid = rounding.map_or(due, |r| due.checked_add(r).unwrap());
                    CashTendered {
                        tendered: paid.checked_add(Money::from_minor(extra, currency)).unwrap(),
                        rounding,
                    }
                });
                PaymentCaptured { amount, tip, reference, cash }
            })
    })
}

/// Any payment event, of any kind.
pub(crate) fn any_payment_event() -> impl Strategy<Value = PaymentEvent> {
    prop_oneof![
        (any_id(), any_id(), any_code(Tender::ALL), any_currency().prop_flat_map(positive))
            .prop_map(|(order, check, tender, amount)| {
                PaymentEvent::Initiated(PaymentInitiated { order, check, tender, amount })
            }),
        (any_currency().prop_flat_map(positive), prop::option::of(any_reference())).prop_map(
            |(amount, reference)| {
                PaymentEvent::Authorized(PaymentAuthorized { amount, reference })
            }
        ),
        any_captured().prop_map(PaymentEvent::Captured),
        any_ended().prop_map(PaymentEvent::Failed),
        any_ended().prop_map(PaymentEvent::Voided),
    ]
}

/// A lease: mostly a few changes of owner in, sometimes at the end of the range.
pub(crate) fn any_lease() -> impl Strategy<Value = Lease> {
    prop_oneof![4 => 0_u64..=4, 1 => (Lease::MAX.get() - 2)..=Lease::MAX.get()]
        .prop_map(|n| Lease::new(n).unwrap())
}

/// A hub's epoch: mostly the first few, sometimes at the end of the range.
pub(crate) fn any_epoch() -> impl Strategy<Value = Epoch> {
    prop_oneof![4 => 1_u64..=4, 1 => (Lease::MAX.get() - 1)..=Lease::MAX.get()]
        .prop_map(|n| Epoch::new(n).unwrap())
}

/// Any of the hub's grants: a lease from 1, since a grant replaces one.
pub(crate) fn any_granted() -> impl Strategy<Value = OwnershipGranted> {
    (any_id(), any_id(), any_lease(), any_epoch()).prop_map(|(request, device, lease, epoch)| {
        let lease = if lease == Lease::FIRST { Lease::new(1).unwrap() } else { lease };
        OwnershipGranted { request, device, lease, epoch }
    })
}

/// Any of the events that move an order's ownership (ADR-0021).
pub(crate) fn any_ownership_event() -> impl Strategy<Value = OrderEvent> {
    prop_oneof![
        any_lease().prop_map(|lease| OrderEvent::OwnershipRequested { lease }),
        any_granted().prop_map(OrderEvent::OwnershipGranted),
        (any_id(), any_code(Refusal::ALL))
            .prop_map(|(request, refusal)| OrderEvent::OwnershipRefused { request, refusal }),
        (any_lease(), any_reason())
            .prop_map(|(lease, reason)| OrderEvent::OwnershipOverridden { lease, reason }),
    ]
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
        any_id().prop_map(|check| OrderEvent::CheckOpened { check }),
        any_lines_allocated().prop_map(OrderEvent::LinesAllocated),
        any_check_closed().prop_map(OrderEvent::CheckClosed),
        Just(OrderEvent::Closed),
        any_reason().prop_map(|reason| OrderEvent::Reopened { reason }),
        any_lease().prop_map(|lease| OrderEvent::OwnershipRequested { lease }),
        any_granted().prop_map(OrderEvent::OwnershipGranted),
        (any_id(), any_code(Refusal::ALL))
            .prop_map(|(request, refusal)| OrderEvent::OwnershipRefused { request, refusal }),
        (any_lease(), any_reason())
            .prop_map(|(lease, reason)| OrderEvent::OwnershipOverridden { lease, reason }),
    ]
}
