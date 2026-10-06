//! Property tests of the runtime (ADR-0023, decision 2) against a model of a register: random
//! intents on the demo café's profile, with and without cash rounding, some interrupted at a
//! point of their write, and the store now and then opened again. After every intent each
//! order's ticket must be the model's: its lines as the catalog rings them, and its amounts as
//! pricing prices the model's basket. Each intent is refused exactly when the model says, and an
//! interrupted one leaves nothing behind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU32};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use keel_domain::order::{ChosenModifier, Line, Mode, Order};
use keel_domain::profile::{Choice, Profile, Rung, demo};
use keel_domain::refs::{ModifierGroup, Variant};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_pricing::{Basket, Dining, round_cash};
use keel_runtime::{Identity, Runtime, RuntimeError, Ticket, TicketState};
use keel_store::{Faults, Point, StoreError, StoreKey};
use keel_types::{
    Currency, Id, Locale, ManualClock, Money, Quantity, RoundingMode, RoundingRule, SeededEntropy,
    Unit,
};
use proptest::prelude::*;

type TestRuntime = Runtime<SoftwareSigner, SeededEntropy, ManualClock, SeededEntropy>;

/// A directory of its own for a store, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = base.join(format!("keel-runtime-props-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Interrupts the write in progress at its `n`th point that can be refused, when set.
#[derive(Clone, Default)]
struct Plan(Arc<Mutex<Option<u32>>>);

impl Faults for Plan {
    fn proceed(&mut self, point: Point) -> bool {
        if !matches!(point, Point::Began | Point::Stored | Point::Committing) {
            return true;
        }
        let mut left = self.0.lock().unwrap();
        match *left {
            Some(1) => {
                *left = None;
                false
            }
            Some(n) => {
                *left = Some(n - 1);
                true
            }
            None => true,
        }
    }
}

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

#[derive(Clone, Debug)]
enum Intent {
    Start(Mode),
    /// Rings variant `variant`, with choices drawn from `seed`.
    Add {
        order: usize,
        variant: usize,
        seed: u64,
        quantity: u32,
    },
    Change {
        order: usize,
        line: usize,
        quantity: u32,
    },
    Remove {
        order: usize,
        line: usize,
    },
    Abandon {
        order: usize,
    },
    /// Tenders what is due, in cash or before cash rounding, and `extra` cents more (or less).
    Pay {
        order: usize,
        unrounded: bool,
        extra: i64,
    },
    Reopen,
}

fn any_intent() -> impl Strategy<Value = Intent> {
    let mode = prop::sample::select(vec![Mode::Takeout, Mode::DineIn, Mode::Pickup]);
    prop_oneof![
        2 => mode.prop_map(Intent::Start),
        8 => (0_usize..4, 0_usize..10, any::<u64>(), 1_u32..=3)
            .prop_map(|(order, variant, seed, quantity)| Intent::Add { order, variant, seed, quantity }),
        2 => (0_usize..4, 0_usize..4, 1_u32..=3)
            .prop_map(|(order, line, quantity)| Intent::Change { order, line, quantity }),
        2 => (0_usize..4, 0_usize..4).prop_map(|(order, line)| Intent::Remove { order, line }),
        1 => (0_usize..4).prop_map(|order| Intent::Abandon { order }),
        // Aimed at either side of what is due, before cash rounding and after.
        3 => (0_usize..4, any::<bool>(), prop_oneof![Just(0_i64), -2_i64..=2, -50_i64..=-1, 1_i64..=5000])
            .prop_map(|(order, unrounded, extra)| Intent::Pay { order, unrounded, extra }),
        1 => Just(Intent::Reopen),
    ]
}

/// A case: whether the location rounds cash to five cents, and the intents, each perhaps
/// interrupted at the `n`th point of its write.
fn any_case() -> impl Strategy<Value = (bool, Vec<(Intent, Option<u32>)>)> {
    (
        any::<bool>(),
        prop::collection::vec((any_intent(), prop::option::weighted(0.15, 1_u32..=6)), 1..40),
    )
}

/// A small, seeded generator, so that choices follow from one seed.
fn next(rng: &mut u64) -> u64 {
    *rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *rng >> 33
}

/// Choices for the groups `groups`, mostly within their bounds.
fn choices(
    profile: &Profile,
    groups: &[Id<ModifierGroup>],
    rng: &mut u64,
    depth: u8,
) -> Vec<Choice> {
    let mut chosen = Vec::new();
    for &id in groups {
        let group = profile.group(id).unwrap();
        let slack = u64::from(next(rng).is_multiple_of(6));
        let low = u64::from(group.min).saturating_sub(slack);
        let high = u64::from(group.max.get()) + slack;
        let mut wanted = low + next(rng) % (high - low + 1);
        let mut at = usize::try_from(next(rng)).unwrap() % group.modifiers.len();
        let mut tried = 0;
        while wanted > 0 && tried < group.modifiers.len() {
            let quantity = if group.repeat { (1 + next(rng) % 2).min(wanted) } else { 1 };
            let modifier = &group.modifiers[at];
            let under =
                if depth < 4 { choices(profile, &modifier.groups, rng, depth + 1) } else { vec![] };
            chosen.push(Choice {
                modifier: modifier.id,
                quantity: NonZeroU8::new(u8::try_from(quantity).unwrap()).unwrap(),
                choices: under,
            });
            wanted -= quantity;
            at = (at + 1) % group.modifiers.len();
            tried += 1;
        }
    }
    chosen
}

/// Every variant the demo sells, in the catalog's order.
fn variants(profile: &Profile) -> Vec<Id<Variant>> {
    profile
        .data()
        .catalog
        .items
        .iter()
        .flat_map(|item| item.variants.iter().map(|v| v.id))
        .collect()
}

#[derive(Clone, Debug)]
struct ModelLine {
    id: Id<Line>,
    rung: Rung,
    quantity: u32,
    live: bool,
}

#[derive(Clone, Debug)]
struct ModelOrder {
    id: Id<Order>,
    mode: Mode,
    state: TicketState,
    lines: Vec<ModelLine>,
    paid: Money,
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn pricing_modifier(chosen: &ChosenModifier) -> keel_pricing::Modifier {
    keel_pricing::Modifier {
        unit_price: chosen.unit_price,
        quantity: NonZeroU32::from(chosen.quantity),
        modifiers: chosen.modifiers.iter().map(pricing_modifier).collect(),
    }
}

/// The model's ticket of an order.
struct ModelTicket {
    /// Its live lines, and what each costs before tax.
    lines: Vec<(Id<Line>, Money)>,
    subtotal: Money,
    /// Each tax with something to tax.
    taxes: Vec<Money>,
    total: Money,
    paid: Money,
    due: Money,
    /// What is due in cash.
    cash: Money,
}

/// The model's ticket of `order`: its live lines as rung, priced as pricing prices them.
fn model_ticket(profile: &Profile, order: &ModelOrder) -> ModelTicket {
    let live: Vec<&ModelLine> = order.lines.iter().filter(|line| line.live).collect();
    let basket = Basket {
        currency: Currency::USD,
        lines: live
            .iter()
            .map(|line| keel_pricing::Line {
                unit_price: line.rung.item.unit_price,
                modifiers: line.rung.modifiers.iter().map(pricing_modifier).collect(),
                quantity: Quantity::from_whole(i64::from(line.quantity), Unit::Each).unwrap(),
                tax_category: line.rung.item.tax_category.cast(),
                comped: false,
                discounts: vec![],
                share: None,
            })
            .collect(),
        discounts: vec![],
        dining: if order.mode == Mode::DineIn { Dining::OnPremises } else { Dining::ToGo },
        exemptions: vec![],
    };
    let totals = keel_pricing::price(&basket, &profile.data().rules).unwrap();
    let lines = live.iter().zip(&totals.lines).map(|(line, t)| (line.id, t.gross)).collect();
    let taxes = totals.taxes.iter().filter(|t| t.taxable.is_positive()).map(|t| t.tax).collect();
    let (paid, due) = match order.state {
        TicketState::Open | TicketState::Closed => {
            (order.paid, totals.total.checked_sub(order.paid).unwrap())
        }
        TicketState::Voided | TicketState::Abandoned => (usd(0), usd(0)),
    };
    let cash = cash_due(profile, due);
    ModelTicket { lines, subtotal: totals.net, taxes, total: totals.total, paid, due, cash }
}

fn cash_due(profile: &Profile, due: Money) -> Money {
    match profile.data().cash_rounding {
        Some(rule) if due.is_positive() => round_cash(due, rule).unwrap().due,
        _ => due,
    }
}

/// Checks the runtime's ticket of every order against the model's.
fn check_tickets(
    runtime: &TestRuntime,
    profile: &Profile,
    orders: &[ModelOrder],
) -> Result<(), TestCaseError> {
    for order in orders {
        let ticket: Ticket = runtime.ticket(order.id).unwrap();
        let model = model_ticket(profile, order);
        prop_assert_eq!(ticket.state, order.state);
        let got: Vec<(Id<Line>, Money)> =
            ticket.lines.iter().map(|l| (l.line, l.amount.money)).collect();
        prop_assert_eq!(got, model.lines);
        for (line, model) in ticket.lines.iter().zip(order.lines.iter().filter(|l| l.live)) {
            prop_assert_eq!(&line.name, &model.rung.item.name);
            prop_assert_eq!(
                line.quantity,
                Quantity::from_whole(i64::from(model.quantity), Unit::Each).unwrap()
            );
            let mut flat = Vec::new();
            flatten(&model.rung.modifiers, 1, &mut flat);
            let shown: Vec<(u8, Id<keel_domain::refs::Modifier>, u8, Money)> = line
                .modifiers
                .iter()
                .map(|m| (m.depth, m.modifier, m.quantity, m.price.money))
                .collect();
            prop_assert_eq!(shown, flat);
        }
        prop_assert_eq!(ticket.subtotal.money, model.subtotal);
        let got_taxes: Vec<Money> = ticket.taxes.iter().map(|t| t.amount.money).collect();
        prop_assert_eq!(got_taxes, model.taxes);
        prop_assert_eq!(ticket.total.money, model.total);
        prop_assert_eq!(ticket.paid.money, model.paid);
        prop_assert_eq!(ticket.due.money, model.due);
        prop_assert_eq!(ticket.cash_due.money, model.cash);
        prop_assert_eq!(&ticket.total.text, &runtime.locale().money(model.total));
    }
    let open: Vec<Id<Order>> = runtime.open_orders().unwrap().iter().map(|t| t.order).collect();
    let expected: Vec<Id<Order>> =
        orders.iter().filter(|o| o.state == TicketState::Open).map(|o| o.id).collect();
    prop_assert_eq!(open, expected);
    Ok(())
}

fn flatten(
    modifiers: &[ChosenModifier],
    depth: u8,
    out: &mut Vec<(u8, Id<keel_domain::refs::Modifier>, u8, Money)>,
) {
    for chosen in modifiers {
        out.push((depth, chosen.modifier, chosen.quantity.get(), chosen.unit_price));
        flatten(&chosen.modifiers, depth + 1, out);
    }
}

/// Opens the runtime, signed in.
fn open(
    scratch: &Scratch,
    profile: &Profile,
    plan: &Plan,
    opened: u64,
    clock_at: &str,
) -> TestRuntime {
    let identity = Identity {
        device: id::<Device>(0xd1),
        signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap(),
    };
    let mut runtime = Runtime::open_with_faults(
        scratch.0.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        identity,
        profile.clone(),
        ManualClock::new(clock_at.parse().unwrap()),
        SeededEntropy::new(opened * 2 + 1),
        SeededEntropy::new(opened * 2 + 2),
        Locale::EN_US,
        plan.clone(),
    )
    .unwrap();
    runtime.sign_in(id(0x501)).unwrap();
    runtime
}

/// What the model expects of an intent: its outcome, and the points its write passes.
enum Expect {
    /// Refused, by the kind of refusal.
    Refused(&'static str),
    /// Done, its write passing this many points that can be refused.
    Done(u32),
}

fn kind(error: &RuntimeError) -> &'static str {
    match error {
        RuntimeError::Ring(_) => "ring",
        RuntimeError::Order(_) => "order",
        RuntimeError::Checkout(_) => "checkout",
        RuntimeError::TenderShort { .. } => "short",
        RuntimeError::Store(StoreError::Interrupted(_)) => "interrupted",
        _ => "other",
    }
}

/// An intent, ready to run.
type Act = Box<dyn FnOnce(&mut TestRuntime) -> Result<Option<Ticket>, RuntimeError>>;

/// What the model expects of `intent`, and the intent ready to run; `None` if it names an order
/// or a line the model doesn't have, or reopens the store.
fn prepare(
    profile: &Profile,
    variants: &[Id<Variant>],
    orders: &[ModelOrder],
    intent: &Intent,
) -> Option<(Expect, Act)> {
    let pick = |n: usize, len: usize| if len == 0 { None } else { Some(n % len) };
    let (expect, act): (Expect, Act) = match intent.clone() {
        Intent::Reopen => return None,
        Intent::Start(mode) => (Expect::Done(3), Box::new(move |r| r.start_order(mode).map(Some))),
        Intent::Add { order, variant, seed, quantity } => {
            let at = pick(order, orders.len())?;
            let variant = variants[variant % variants.len()];
            let (item, _) = profile.variant(variant).unwrap();
            let mut rng = seed;
            let chosen = choices(profile, &item.groups, &mut rng, 1);
            let expect = match profile.ring(variant, &chosen) {
                Err(_) => Expect::Refused("ring"),
                Ok(_) if orders[at].state != TicketState::Open => Expect::Refused("order"),
                Ok(_) => Expect::Done(3),
            };
            let id = orders[at].id;
            let quantity = NonZeroU32::new(quantity).unwrap();
            (expect, Box::new(move |r| r.add_item(id, variant, &chosen, quantity).map(Some)))
        }
        Intent::Change { order, line, quantity } => {
            let at = pick(order, orders.len())?;
            let l = pick(line, orders[at].lines.len())?;
            let target = &orders[at].lines[l];
            let expect = if orders[at].state != TicketState::Open
                || !target.live
                || target.quantity == quantity
            {
                Expect::Refused("order")
            } else {
                Expect::Done(3)
            };
            let (id, line) = (orders[at].id, target.id);
            let quantity = NonZeroU32::new(quantity).unwrap();
            (expect, Box::new(move |r| r.change_quantity(id, line, quantity).map(Some)))
        }
        Intent::Remove { order, line } => {
            let at = pick(order, orders.len())?;
            let l = pick(line, orders[at].lines.len())?;
            let target = &orders[at].lines[l];
            let expect = if orders[at].state == TicketState::Open && target.live {
                Expect::Done(3)
            } else {
                Expect::Refused("order")
            };
            let (id, line) = (orders[at].id, target.id);
            (expect, Box::new(move |r| r.remove_line(id, line).map(Some)))
        }
        Intent::Abandon { order } => {
            let at = pick(order, orders.len())?;
            let live = orders[at].lines.iter().filter(|l| l.live).count();
            let expect = if orders[at].state == TicketState::Open {
                Expect::Done(u32::try_from(live).unwrap() + 3)
            } else {
                Expect::Refused("order")
            };
            let id = orders[at].id;
            (expect, Box::new(move |r| r.abandon(id).map(Some)))
        }
        Intent::Pay { order, unrounded, extra } => {
            let at = pick(order, orders.len())?;
            let ModelTicket { due, cash, .. } = model_ticket(profile, &orders[at]);
            let live = orders[at].lines.iter().any(|l| l.live);
            let tendered = if unrounded { due } else { cash }.checked_add(usd(extra)).unwrap();
            let expect = match orders[at].state {
                TicketState::Open if !live => Expect::Refused("checkout"),
                TicketState::Open if tendered.minor() < cash.minor() => Expect::Refused("short"),
                // Started, captured, the check closed and the order closed.
                TicketState::Open if due.is_positive() => Expect::Done(6),
                TicketState::Open => Expect::Done(4),
                // Checkout refuses to close what isn't open.
                _ => Expect::Refused("checkout"),
            };
            let id = orders[at].id;
            (
                expect,
                Box::new(move |r| {
                    let paid = r.pay_cash(id, tendered)?;
                    assert_eq!(paid.change.money, tendered.checked_sub(cash).unwrap());
                    Ok(Some(paid.ticket))
                }),
            )
        }
    };
    Some((expect, act))
}

fn run(rounding: bool, intents: &[(Intent, Option<u32>)]) -> Result<(), TestCaseError> {
    let mut data = demo().unwrap().data().clone();
    if rounding {
        data.cash_rounding =
            Some(RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap()));
    }
    let profile = Profile::new(data).unwrap();
    let variants = variants(&profile);
    let scratch = Scratch::new();
    let plan = Plan::default();
    let mut opened = 0;
    let mut runtime = open(&scratch, &profile, &plan, opened, "2026-10-02T13:00:00Z");
    let mut orders: Vec<ModelOrder> = Vec::new();
    for (intent, fault) in intents {
        if matches!(intent, Intent::Reopen) {
            drop(runtime);
            opened += 1;
            runtime = open(&scratch, &profile, &plan, opened, "2026-10-02T13:00:00Z");
            check_tickets(&runtime, &profile, &orders)?;
            continue;
        }
        let Some((expect, outcome)) = prepare(&profile, &variants, &orders, intent) else {
            continue;
        };
        // Only an intent the model expects done is interrupted: one refused inside its write
        // would be interrupted before it is refused.
        let fault = if matches!(expect, Expect::Done(_)) { *fault } else { None };
        *plan.0.lock().unwrap() = fault;
        let result = outcome(&mut runtime);
        *plan.0.lock().unwrap() = None;
        match (expect, result) {
            (Expect::Refused(why), Err(error)) => {
                prop_assert_eq!(kind(&error), why, "{:?}: {:?}", intent, error);
            }
            (Expect::Done(points), Err(error)) => {
                prop_assert!(
                    fault.is_some_and(|k| k <= points) && kind(&error) == "interrupted",
                    "{:?} refused: {:?}",
                    intent,
                    error
                );
            }
            (Expect::Done(points), Ok(ticket)) => {
                prop_assert!(
                    fault.is_none_or(|k| k > points),
                    "{:?} wasn't interrupted at {:?}",
                    intent,
                    fault
                );
                apply(&profile, &mut orders, intent, ticket.as_ref(), &variants);
            }
            (Expect::Refused(why), Ok(_)) => {
                prop_assert!(false, "{:?} was done, not refused ({})", intent, why);
            }
        }
        check_tickets(&runtime, &profile, &orders)?;
    }
    Ok(())
}

/// Applies a done intent to the model, learning new identifiers from `ticket`.
fn apply(
    profile: &Profile,
    orders: &mut Vec<ModelOrder>,
    intent: &Intent,
    ticket: Option<&Ticket>,
    variants: &[Id<Variant>],
) {
    let ticket = ticket.unwrap();
    let at = |n: usize, orders: &[ModelOrder]| n % orders.len();
    match intent {
        Intent::Start(mode) => orders.push(ModelOrder {
            id: ticket.order,
            mode: *mode,
            state: TicketState::Open,
            lines: vec![],
            paid: usd(0),
        }),
        Intent::Add { order, variant, seed, quantity } => {
            let o = at(*order, orders);
            let variant = variants[variant % variants.len()];
            let (item, _) = profile.variant(variant).unwrap();
            let mut rng = *seed;
            let chosen = choices(profile, &item.groups, &mut rng, 1);
            let rung = profile.ring(variant, &chosen).unwrap();
            let known: Vec<Id<Line>> = orders[o].lines.iter().map(|l| l.id).collect();
            let new = ticket.lines.iter().find(|l| !known.contains(&l.line)).unwrap().line;
            orders[o].lines.push(ModelLine { id: new, rung, quantity: *quantity, live: true });
        }
        Intent::Change { order, line, quantity } => {
            let o = at(*order, orders);
            let l = line % orders[o].lines.len();
            orders[o].lines[l].quantity = *quantity;
        }
        Intent::Remove { order, line } => {
            let o = at(*order, orders);
            let l = line % orders[o].lines.len();
            orders[o].lines[l].live = false;
        }
        Intent::Abandon { order } => {
            let o = at(*order, orders);
            orders[o].lines.iter_mut().for_each(|line| line.live = false);
            orders[o].state = TicketState::Abandoned;
        }
        Intent::Pay { order, .. } => {
            let o = at(*order, orders);
            let due = model_ticket(profile, &orders[o]).due;
            orders[o].paid = orders[o].paid.checked_add(due).unwrap();
            orders[o].state = TicketState::Closed;
        }
        Intent::Reopen => {}
    }
}

proptest! {
    /// A register's intents do what the model says, and its tickets show what the model shows.
    #[test]
    fn a_register_rings_and_takes_cash_as_the_model_says((rounding, intents) in any_case()) {
        run(rounding, &intents)?;
    }
}

/// Found by the property: a closed order's ticket listed its lines by identifier, as the check's
/// snapshot does, rather than in the order they were rung. Opening the store again starts the
/// runtime's identifiers afresh, so the second line of order 2, rung after the store reopened,
/// sorted before the first once the order was paid.
#[test]
fn a_closed_tickets_lines_stay_in_the_order_they_were_rung() {
    let case = [
        (Intent::Start(Mode::Takeout), None),
        (Intent::Start(Mode::Takeout), None),
        (Intent::Start(Mode::Takeout), None),
        (Intent::Add { order: 2, variant: 0, seed: 424_697_622_282_041_585, quantity: 1 }, None),
        (Intent::Reopen, None),
        (Intent::Add { order: 2, variant: 0, seed: 969_140, quantity: 1 }, None),
        (Intent::Pay { order: 2, unrounded: false, extra: 0 }, None),
    ];
    run(false, &case).unwrap();
}
