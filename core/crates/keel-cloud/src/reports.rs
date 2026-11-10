//! Reports from a location's durable copy (ADR-0028, decision 4): sales by business day and by
//! item, and payments by tender, over a range of business days. Each order's amounts are its
//! ticket's, priced by the kernel as every device prices it; the report only adds them up, with
//! checked arithmetic, in the location's currency.

use std::collections::BTreeMap;

use keel_domain::payment::Tender;
use keel_runtime::RuntimeError;
use keel_store::{OrderState, PaymentState};
use keel_types::{BusinessDate, Money, MoneyError, Quantity, QuantityError};
use serde::Serialize;

use crate::CloudRuntime;

/// Sales over a range of business days.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesReport {
    /// The first business day, `YYYY-MM-DD`.
    pub from: String,
    /// The last.
    pub to: String,
    /// The location's currency, ISO 4217; every amount is in its minor units.
    pub currency: String,
    /// How many digits of its minor unit there are.
    pub minor_units: u8,
    /// The orders closed over the range.
    pub orders: u64,
    /// What they came to, taxes included.
    pub total: i64,
    /// Each business day with an order closed, in order.
    pub days: Vec<DaySales>,
    /// What each item sold, by its name on the ticket, the most sold by amount first.
    pub items: Vec<ItemSales>,
    /// What each tender took.
    pub tenders: Vec<TenderTotal>,
    /// Orders closed over the range in another currency than the location's, left out of every
    /// total: none, unless a profile in another currency was published.
    pub other_currency: u64,
}

/// One business day's sales.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DaySales {
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// The orders closed.
    pub orders: u64,
    /// What they came to, taxes included.
    pub total: i64,
}

/// What one item sold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ItemSales {
    /// Its name, as its lines show it.
    pub name: String,
    /// How many, as a decimal: `3`, or `1.5` of an item sold by weight.
    pub quantity: String,
    /// The unit it is counted in: `each`, or `kg` for an item sold by weight.
    pub unit: String,
    /// Its lines' amounts, before tax.
    pub amount: i64,
}

/// What one tender took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TenderTotal {
    /// `cash` or `card`.
    pub tender: String,
    /// How many payments.
    pub payments: u64,
    /// What they captured.
    pub amount: i64,
}

/// Why a report can't be made.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReportError {
    /// The range ends before it begins.
    #[error("the range ends before it begins")]
    Range,
    /// The store couldn't be read, or an order priced.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// The totals overflow.
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// The quantities overflow, or are of different units.
    #[error(transparent)]
    Quantity(#[from] QuantityError),
}

/// `micros` millionths as a decimal, with no trailing zeros.
fn decimal(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let magnitude = micros.unsigned_abs();
    let (whole, fraction) = (magnitude / 1_000_000, magnitude % 1_000_000);
    if fraction == 0 {
        format!("{sign}{whole}")
    } else {
        let digits = format!("{fraction:06}");
        format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

/// The location's sales over the business days `from` to `to`, both included.
///
/// # Errors
/// [`ReportError`] if the range ends before it begins, the store can't be read, an order can't be
/// priced, or a total overflows.
pub fn sales(
    runtime: &CloudRuntime,
    from: BusinessDate,
    to: BusinessDate,
) -> Result<SalesReport, ReportError> {
    if to < from {
        return Err(ReportError::Range);
    }
    let currency = runtime.profile().data().currency;
    let zero = Money::zero(currency);
    let mut days: BTreeMap<BusinessDate, (u64, Money)> = BTreeMap::new();
    // An item by its name and its unit: two items of one name, one sold by weight, are two.
    let mut items: BTreeMap<(String, &'static str), (Quantity, Money)> = BTreeMap::new();
    let mut tenders: BTreeMap<u8, (u64, Money)> = BTreeMap::new();
    let (mut orders, mut total, mut other_currency) = (0_u64, zero, 0_u64);
    let closed = runtime.store().orders(OrderState::Closed).map_err(RuntimeError::from)?;
    for order in closed {
        let Some(date) = order.business_date.filter(|date| (from..=to).contains(date)) else {
            continue;
        };
        let ticket = runtime.ticket(order.id)?;
        // An order in another currency can't be added to the location's: it is counted apart.
        // Found in review: one failed the whole report.
        if ticket.total.money.currency() != currency {
            other_currency = other_currency.saturating_add(1);
            continue;
        }
        let day = days.entry(date).or_insert((0, zero));
        day.0 = day.0.saturating_add(1);
        day.1 = day.1.checked_add(ticket.total.money)?;
        orders = orders.saturating_add(1);
        total = total.checked_add(ticket.total.money)?;
        for line in &ticket.lines {
            let entry = items
                .entry((line.name.to_string(), line.quantity.unit().code()))
                .or_insert((Quantity::zero(line.quantity.unit()), zero));
            entry.0 = entry.0.checked_add(line.quantity)?;
            entry.1 = entry.1.checked_add(line.amount.money)?;
        }
        for payment in runtime.store().payments_of(order.id).map_err(RuntimeError::from)? {
            let (PaymentState::Captured, Some(info), Some(captured)) =
                (payment.state, payment.info, payment.captured)
            else {
                continue;
            };
            let key = match info.tender {
                Tender::Cash => 0,
                Tender::Card => 1,
            };
            let entry = tenders.entry(key).or_insert((0, zero));
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.checked_add(captured)?;
        }
    }
    let mut items: Vec<ItemSales> = items
        .into_iter()
        .map(|((name, unit), (quantity, amount))| ItemSales {
            name,
            quantity: decimal(quantity.micros()),
            unit: unit.to_owned(),
            amount: amount.minor(),
        })
        .collect();
    items.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.name.cmp(&b.name)));
    Ok(SalesReport {
        from: from.to_string(),
        to: to.to_string(),
        currency: currency.code().to_owned(),
        minor_units: currency.minor_units(),
        orders,
        total: total.minor(),
        days: days
            .into_iter()
            .map(|(date, (orders, total))| DaySales {
                date: date.to_string(),
                orders,
                total: total.minor(),
            })
            .collect(),
        items,
        tenders: tenders
            .into_iter()
            .map(|(key, (payments, amount))| TenderTotal {
                tender: if key == 0 { "cash" } else { "card" }.to_owned(),
                payments,
                amount: amount.minor(),
            })
            .collect(),
        other_currency,
    })
}

#[cfg(test)]
mod tests {
    use super::decimal;

    #[test]
    fn a_quantity_shows_as_a_decimal_with_no_trailing_zeros() {
        assert_eq!(decimal(3_000_000), "3");
        assert_eq!(decimal(1_500_000), "1.5");
        assert_eq!(decimal(250), "0.00025");
        assert_eq!(decimal(0), "0");
        assert_eq!(decimal(-2_000_001), "-2.000001");
        assert_eq!(decimal(i64::MIN), "-9223372036854.775808");
    }
}
