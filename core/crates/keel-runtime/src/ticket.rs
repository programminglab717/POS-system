//! An order's ticket: its lines, what it costs and what it still owes.

use keel_domain::checkout::Checkout;
use keel_domain::order::{Check, CheckClosed, Line, Order, OrderStatus};
use keel_domain::payment::Payment;
use keel_domain::profile::Profile;
use keel_pricing::{Rules, Tax, round_cash};
use keel_types::{Id, Locale, Money};

use crate::error::RuntimeError;
use crate::views::{Amount, Ticket, TicketLine, TicketState, TicketTax, ticket_modifiers};

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

/// What an order's lines were, or are, charged.
struct Charges {
    lines: Vec<TicketLine>,
    subtotal: Money,
    taxes: Vec<TicketTax>,
    total: Money,
}

/// The ticket of `order`, created, whose payments are `payments`.
pub(crate) fn ticket(
    order: &Order,
    payments: &[Payment],
    profile: &Profile,
    locale: Locale,
) -> Result<Ticket, RuntimeError> {
    let rules = &profile.data().rules;
    let currency = profile.data().currency;
    let check: Id<Check> = order.id().cast();
    // A closed check's snapshot says what it was charged; an open one is priced as it stands.
    let charges = match order.check(check).and_then(|check| check.closed()) {
        Some(closed) => charged(order, closed, rules, locale)?,
        None => priced(order, rules, locale)?,
    };
    let zero = Money::from_minor(0, currency);
    let (paid, due) = match *order.status() {
        OrderStatus::Active | OrderStatus::Closed => {
            let checkout = Checkout::new(order, payments, rules, profile.rules_version());
            let balance = checkout.balance(check)?;
            (balance.captured, balance.due)
        }
        OrderStatus::Voided(_) | OrderStatus::Abandoned => (zero, zero),
    };
    let cash = if due.is_positive() { cash_due(profile, due)?.due } else { due };
    let amount = |money: Money| Amount::new(money, locale);
    Ok(Ticket {
        order: order.id(),
        state: TicketState::of(order.status()),
        lines: charges.lines,
        subtotal: amount(charges.subtotal),
        taxes: charges.taxes,
        total: amount(charges.total),
        paid: amount(paid),
        due: amount(due),
        cash_due: amount(cash),
    })
}

/// What a closed check's snapshot says its lines and taxes were charged.
fn charged(
    order: &Order,
    closed: &CheckClosed,
    rules: &Rules,
    locale: Locale,
) -> Result<Charges, RuntimeError> {
    // In the order the lines were rung, as an open order's ticket lists them: the snapshot
    // lists its lines by identifier.
    let lines = order
        .lines()
        .iter()
        .filter_map(|line| {
            let charge = closed.lines.iter().find(|charge| charge.line == line.id())?;
            Some(ticket_line(line, charge.gross, locale))
        })
        .collect();
    let subtotal =
        Money::sum(closed.total.currency(), closed.lines.iter().map(|charge| charge.net))?;
    // In the location's order of taxes, too, as an open order's ticket lists them: the snapshot
    // lists them by identifier. A tax the rules no longer have goes last.
    let mut charges: Vec<_> = closed.taxes.iter().collect();
    charges.sort_by_key(|charge| {
        rules.taxes.iter().position(|tax| tax.id == charge.tax).unwrap_or(usize::MAX)
    });
    let taxes = charges
        .into_iter()
        .map(|charge| TicketTax {
            tax: charge.tax,
            name: tax_name(rules, charge.tax),
            amount: Amount::new(charge.amount, locale),
        })
        .collect();
    Ok(Charges { lines, subtotal, taxes, total: closed.total })
}

/// What pricing charges an open order's live lines and taxes.
fn priced(order: &Order, rules: &Rules, locale: Locale) -> Result<Charges, RuntimeError> {
    let basket = order.basket().ok_or(RuntimeError::UnknownOrder(order.id()))?;
    let totals = keel_pricing::price(&basket, rules)?;
    let lines = order
        .live_lines()
        .zip(&totals.lines)
        .map(|(line, line_totals)| ticket_line(line, line_totals.gross, locale))
        .collect();
    let taxes = totals
        .taxes
        .iter()
        .filter(|tax| tax.taxable.is_positive())
        .map(|tax| TicketTax {
            tax: tax.id,
            name: tax_name(rules, tax.id),
            amount: Amount::new(tax.tax, locale),
        })
        .collect();
    Ok(Charges { lines, subtotal: totals.net, taxes, total: totals.total })
}

/// A ticket's line for `line`, charged `gross` before tax.
fn ticket_line(line: &Line, gross: Money, locale: Locale) -> TicketLine {
    let mut modifiers = Vec::new();
    ticket_modifiers(line.modifiers(), 1, locale, &mut modifiers);
    TicketLine {
        line: line.id(),
        name: line.item().name.clone(),
        quantity: line.quantity(),
        quantity_text: locale.quantity(line.quantity()),
        modifiers,
        amount: Amount::new(gross, locale),
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
