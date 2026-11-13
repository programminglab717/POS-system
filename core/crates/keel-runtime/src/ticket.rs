//! An order's ticket: its lines, what it costs and what it still owes.

use keel_domain::checkout::Checkout;
use keel_domain::order::{Check, DiscountApplied, DiscountValue, Line, Order, OrderStatus};
use keel_domain::payment::Payment;
use keel_domain::profile::Profile;
use keel_pricing::{Rules, Tax, round_cash};
use keel_types::{Currency, Id, Locale, Money};

use crate::error::RuntimeError;
use crate::views::{
    Amount, Ticket, TicketCheck, TicketDiscount, TicketLine, TicketRepriced, TicketState,
    TicketTax, ticket_modifiers,
};

/// What is due in cash, and the cash rounding in it.
pub(crate) struct Cash {
    /// What the customer pays.
    pub(crate) due: Money,
    /// What the rounding added: negative when it rounded down, and zero without one.
    pub(crate) rounding: Money,
}

impl Cash {
    /// Nothing due, in `due`'s currency.
    pub(crate) fn none(due: Money) -> Cash {
        let zero = Money::from_minor(0, due.currency());
        Cash { due: zero, rounding: zero }
    }
}

/// What is due in cash for `due`, rounded as the location rounds cash.
pub(crate) fn cash_due(profile: &Profile, due: Money) -> Result<Cash, RuntimeError> {
    Ok(match profile.data().cash_rounding {
        Some(rule) => {
            let rounded = round_cash(due, rule)?;
            Cash { due: rounded.due, rounding: rounded.rounding }
        }
        None => Cash { due, rounding: Money::from_minor(0, due.currency()) },
    })
}

/// What one check's lines were, or are, charged: each line's part, and what its discounts and
/// the whole order's took of it.
struct Charges {
    /// Each line on the check, in the order's order: the line, its part's gross before tax, and
    /// what each of its discounts took of the part.
    lines: Vec<(Id<Line>, Money, Vec<Money>)>,
    /// What each of the whole order's discounts took of the check.
    discounts: Vec<Money>,
    subtotal: Money,
    /// Each tax with something to tax, and what it charged.
    taxes: Vec<(Id<Tax>, Money)>,
    total: Money,
}

impl Charges {
    fn none(currency: Currency) -> Charges {
        let zero = Money::zero(currency);
        Charges {
            lines: Vec::new(),
            discounts: Vec::new(),
            subtotal: zero,
            taxes: Vec::new(),
            total: zero,
        }
    }

    /// These charges and `other`'s, added up: a line's parts, each discount's, each tax's.
    fn add(mut self, other: Charges) -> Result<Charges, RuntimeError> {
        for (line, gross, taken) in other.lines {
            match self.lines.iter_mut().find(|(known, _, _)| *known == line) {
                Some((_, sum, sums)) => {
                    *sum = sum.checked_add(gross)?;
                    for (sum, part) in sums.iter_mut().zip(&taken) {
                        *sum = sum.checked_add(*part)?;
                    }
                }
                None => self.lines.push((line, gross, taken)),
            }
        }
        if self.discounts.is_empty() {
            self.discounts = other.discounts;
        } else {
            for (sum, part) in self.discounts.iter_mut().zip(&other.discounts) {
                *sum = sum.checked_add(*part)?;
            }
        }
        for (tax, amount) in other.taxes {
            match self.taxes.iter_mut().find(|(known, _)| *known == tax) {
                Some((_, sum)) => *sum = sum.checked_add(amount)?,
                None => self.taxes.push((tax, amount)),
            }
        }
        self.subtotal = self.subtotal.checked_add(other.subtotal)?;
        self.total = self.total.checked_add(other.total)?;
        Ok(self)
    }
}

/// The ticket of `order`, created, whose payments are `payments`: each check charged as its
/// snapshot says if it is closed, or as pricing charges it now, and the order's amounts the
/// checks' added up, so they are what the checks' customers pay. With more than one check, the
/// ticket lists each, with its own lines and amounts (ADR-0030).
pub(crate) fn ticket(
    order: &Order,
    payments: &[Payment],
    profile: &Profile,
    locale: Locale,
) -> Result<Ticket, RuntimeError> {
    let currency = profile.data().currency;
    let mut whole = Charges::none(currency);
    let (mut paid, mut due) = (Money::zero(currency), Money::zero(currency));
    let mut checks = Vec::new();
    for check in order.checks() {
        let charges = check_charges(order, check, &profile.data().rules)?;
        let (check_paid, check_due) = balance(order, payments, profile, check.id())?;
        paid = paid.checked_add(check_paid)?;
        due = due.checked_add(check_due)?;
        let shown = !charges.lines.is_empty() || !check.is_open();
        if shown {
            let state = match TicketState::of(order.status()) {
                TicketState::Open if !check.is_open() => TicketState::Closed,
                state => state,
            };
            let view = view(order, &charges, state, check_paid, check_due, profile, locale)?;
            checks.push(TicketCheck {
                check: check.id(),
                number: check.number().get(),
                ticket: view,
            });
        }
        whole = whole.add(charges)?;
    }
    let state = TicketState::of(order.status());
    let mut ticket = view(order, &whole, state, paid, due, profile, locale)?;
    // Listed whenever more than the main check is in play: found in review, an order whose lines
    // had all moved off it listed no check to pay.
    let main: Id<Check> = order.id().cast();
    if checks.len() > 1 || checks.iter().any(|known| known.check != main) {
        ticket.checks = checks;
    }
    Ok(ticket)
}

/// What a check's captured payments covered, and what it still owes; nothing for an order
/// voided or abandoned.
fn balance(
    order: &Order,
    payments: &[Payment],
    profile: &Profile,
    check: Id<Check>,
) -> Result<(Money, Money), RuntimeError> {
    let zero = Money::zero(profile.data().currency);
    Ok(match *order.status() {
        OrderStatus::Active | OrderStatus::Closed => {
            let rules = &profile.data().rules;
            let checkout = Checkout::new(order, payments, rules, profile.rules_version());
            let balance = checkout.balance(check)?;
            (balance.captured, balance.due)
        }
        OrderStatus::Voided(_) | OrderStatus::Abandoned => (zero, zero),
    })
}

/// A ticket's view of `charges`, for `order`'s lines.
fn view(
    order: &Order,
    charges: &Charges,
    state: TicketState,
    paid: Money,
    due: Money,
    profile: &Profile,
    locale: Locale,
) -> Result<Ticket, RuntimeError> {
    let rules = &profile.data().rules;
    let amount = |money: Money| Amount::new(money, locale);
    // In the order the lines were rung: a snapshot lists its lines by identifier.
    let lines = order
        .lines()
        .iter()
        .filter_map(|line| {
            let (_, gross, taken) =
                charges.lines.iter().find(|(known, _, _)| *known == line.id())?;
            Some(ticket_line(line, *gross, taken, locale))
        })
        .collect();
    // In the location's order of taxes: a snapshot lists them by identifier. A tax the rules no
    // longer have goes last.
    let mut taxes: Vec<&(Id<Tax>, Money)> = charges.taxes.iter().collect();
    taxes.sort_by_key(|(tax, _)| {
        rules.taxes.iter().position(|known| known.id == *tax).unwrap_or(usize::MAX)
    });
    let taxes = taxes
        .into_iter()
        .map(|&(tax, charged)| TicketTax {
            tax,
            name: tax_name(rules, tax),
            amount: amount(charged),
        })
        .collect();
    let cash = if due.is_positive() { cash_due(profile, due)?.due } else { due };
    Ok(Ticket {
        order: order.id(),
        state,
        lines,
        discounts: ticket_discounts(order.discounts(), &charges.discounts, locale),
        subtotal: amount(charges.subtotal),
        taxes,
        total: amount(charges.total),
        paid: amount(paid),
        due: amount(due),
        cash_due: amount(cash),
        checks: Vec::new(),
        owner: order.ownership().map(|ownership| ownership.device),
        asking: order.requests().iter().map(|request| request.device).collect(),
    })
}

/// What `check` was charged, as its snapshot says, if it is closed, or else what pricing charges
/// its basket now. A snapshot says what each line came to, not what each discount took, so the
/// discounts are priced again: a closed check's lines and the whole order's discounts can't
/// change without a conflict, so they price as they did.
fn check_charges(order: &Order, check: &Check, rules: &Rules) -> Result<Charges, RuntimeError> {
    let basket = order.check_basket(check.id()).ok_or(RuntimeError::UnknownOrder(order.id()))?;
    let totals = keel_pricing::price(&basket, rules)?;
    let on: Vec<Id<Line>> = order.lines_on(check.id()).map(Line::id).collect();
    let taken = |line: Id<Line>| {
        on.iter()
            .position(|known| *known == line)
            .and_then(|at| totals.lines.get(at))
            .map_or_else(Vec::new, |line| line.discounts.clone())
    };
    let Some(closed) = check.closed() else {
        let lines = on
            .iter()
            .zip(&totals.lines)
            .map(|(&line, line_totals)| (line, line_totals.gross, line_totals.discounts.clone()))
            .collect();
        let taxes = totals
            .taxes
            .iter()
            .filter(|tax| tax.taxable.is_positive())
            .map(|tax| (tax.id, tax.tax))
            .collect();
        return Ok(Charges {
            lines,
            discounts: totals.discounts.clone(),
            subtotal: totals.net,
            taxes,
            total: totals.total,
        });
    };
    let lines =
        closed.lines.iter().map(|charge| (charge.line, charge.gross, taken(charge.line))).collect();
    let subtotal =
        Money::sum(closed.total.currency(), closed.lines.iter().map(|charge| charge.net))?;
    let taxes = closed.taxes.iter().map(|charge| (charge.tax, charge.amount)).collect();
    Ok(Charges { lines, discounts: totals.discounts, subtotal, taxes, total: closed.total })
}

/// The discounts `applied`, each with what it took, as `taken` gives them in the same order.
fn ticket_discounts(
    applied: &[DiscountApplied],
    taken: &[Money],
    locale: Locale,
) -> Vec<TicketDiscount> {
    applied
        .iter()
        .zip(taken)
        .map(|(applied, &taken)| TicketDiscount {
            discount: applied.discount,
            reason: applied.reason.code.clone(),
            percent: match applied.value {
                DiscountValue::Percent(bp) => Some(bp),
                DiscountValue::Amount(_) => None,
            },
            amount: Amount::new(taken, locale),
        })
        .collect()
}

/// A ticket's line for `line`, charged `gross` before tax, its discounts having taken `taken`.
fn ticket_line(line: &Line, gross: Money, taken: &[Money], locale: Locale) -> TicketLine {
    let mut modifiers = Vec::new();
    ticket_modifiers(line.modifiers(), 1, locale, &mut modifiers);
    TicketLine {
        line: line.id(),
        name: line.item().name.clone(),
        quantity: line.quantity(),
        quantity_text: locale.quantity(line.quantity()),
        modifiers,
        amount: Amount::new(gross, locale),
        discounts: ticket_discounts(line.discounts(), taken, locale),
        repriced: line.repriced().map(|repriced| TicketRepriced {
            unit_price: Amount::new(repriced.unit_price, locale),
            rung: Amount::new(line.item().unit_price, locale),
            reason: repriced.reason.code.clone(),
        }),
        notes: line.notes().cloned(),
        sent: line.was_fired(),
    }
}

/// The name of the tax `tax`, as the rules give it.
fn tax_name(rules: &Rules, tax: Id<Tax>) -> String {
    rules
        .taxes
        .iter()
        .find(|candidate| candidate.id == tax)
        .map(|tax| tax.name.clone())
        .unwrap_or_default()
}

/// The amounts of cash a register offers for `due`, in minor units of a currency with
/// `minor_units` decimals: `due`, then the smallest amounts above it in whole multiples of 1, 5,
/// 10, 20, 50 and 100 of a unit, each once, at most [`crate::CASH_OFFERS`] in all; `due` alone if
/// it is nothing or less, or an amount above it would overflow. The unit is the currency's major
/// unit, or 100 of it for a currency without decimals, whose smallest notes are hundreds or
/// thousands: ¥1,234 is offered ¥1,300, ¥1,500, ¥2,000 and ¥5,000. Found in review: a currency
/// without decimals was offered ¥1,235. The location's own denominations come with its profile.
pub(crate) fn cash_offers(due: i64, minor_units: u8) -> Vec<i64> {
    let mut offers = vec![due];
    if due <= 0 {
        return offers;
    }
    let major = if minor_units == 0 {
        100
    } else {
        10_i64.checked_pow(u32::from(minor_units)).unwrap_or(1)
    };
    for step in [1_i64, 5, 10, 20, 50, 100] {
        // The smallest multiple of the step above what is due.
        let above = major
            .checked_mul(step)
            .and_then(|unit| due.checked_div(unit)?.checked_add(1)?.checked_mul(unit));
        if let Some(above) = above
            && !offers.contains(&above)
            && offers.len() < crate::CASH_OFFERS
        {
            offers.push(above);
        }
    }
    offers
}
