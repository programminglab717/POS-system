//! A location's dashboard (ADR-0037, decision 2; O-01): one business day from the durable copy,
//! as the registers' own X report and views compute it, and the same weekday a week before. The
//! back office asks again every 10 s while it is shown; the devices and their replication are
//! the replication query's.

use std::collections::BTreeMap;

use keel_domain::aggregate::fold;
use keel_domain::order::{Order, OrderEvent, OrderStatus};
use keel_domain::schema::DomainEvent;
use keel_domain::stock::time_of;
use keel_events::envelope::{StreamKind, StreamRef};
use keel_runtime::{DayReport, ReportKind, RuntimeError};
use keel_store::{OrderState, StoreError};
use keel_types::{BusinessDate, Id, Money, Timestamp};
use serde::Serialize;

use crate::CloudRuntime;
use crate::reports::{self, ItemSales, ReportError, TenderTotal};

/// How many items the dashboard ranks.
pub const TOP_ITEMS: usize = 10;

/// A business day's dashboard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    /// The business day, `YYYY-MM-DD`.
    pub date: String,
    /// The location's currency, ISO 4217; every amount is in its minor units.
    pub currency: String,
    /// How many digits of its minor unit there are.
    pub minor_units: u8,
    /// The day's figures.
    pub day: DayFigures,
    /// The same weekday a week before, to compare.
    pub week_before: DayFigures,
    /// The orders closed in each hour of the location's local time, the hours with none left out,
    /// from the hour its business day begins: with a cutoff at 4:00, 3:00 comes last.
    pub hours: Vec<HourSales>,
    /// The items that sold the most by amount, at most [`TOP_ITEMS`].
    pub top_items: Vec<ItemSales>,
    /// What each tender took.
    pub tenders: Vec<TenderTotal>,
    /// Orders open now.
    pub open_orders: u64,
    /// How long the oldest of them has been open, in seconds.
    pub oldest_open_seconds: Option<u64>,
    /// The kitchen's tickets.
    pub kitchen: KitchenFigures,
    /// Items and modifiers 86'd now, by name.
    pub sold_out: Vec<String>,
    /// Items whose stock on hand is below zero.
    pub below_zero: Vec<StockFigure>,
}

/// A day's figures, from its X report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayFigures {
    /// The orders closed.
    pub orders: u64,
    /// What their lines were sold for, before tax.
    pub net: i64,
    /// What they came to, taxes included.
    pub total: i64,
    /// What discounts and comps took.
    pub discounts: i64,
    /// The orders voided, or dropped once they had lines.
    pub voided: u64,
    /// What was refunded.
    pub refunded: i64,
    /// The tips taken.
    pub tips: i64,
}

/// The orders closed in an hour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HourSales {
    /// The hour, 0 to 23, local time.
    pub hour: u8,
    /// The orders closed.
    pub orders: u64,
    /// What they came to, taxes included.
    pub total: i64,
}

/// The kitchen's tickets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitchenFigures {
    /// Tickets sent and not yet bumped.
    pub waiting: u64,
    /// The day's tickets bumped.
    pub bumped: u64,
    /// The median time from sent to bumped, in seconds, of those.
    pub median_seconds: Option<u64>,
    /// The longest.
    pub longest_seconds: Option<u64>,
}

/// An item's stock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StockFigure {
    /// Its name.
    pub name: String,
    /// On hand, as the locale shows it.
    pub on_hand: String,
}

fn figures(report: &DayReport) -> Result<DayFigures, ReportError> {
    let sum = |amounts: &mut dyn Iterator<Item = Money>| -> Result<i64, ReportError> {
        let mut total = Money::zero(report.total.money.currency());
        for amount in amounts {
            total = total.checked_add(amount)?;
        }
        Ok(total.minor())
    };
    Ok(DayFigures {
        orders: report.orders,
        net: report.net.money.minor(),
        total: report.total.money.minor(),
        discounts: report.discounts.money.checked_add(report.comps.money)?.minor(),
        voided: report.voided,
        refunded: sum(&mut report.refunds.iter().map(|line| line.amount.money))?,
        tips: sum(&mut report.tips.iter().map(|line| line.amount.money))?,
    })
}

/// When order `id` last closed, by its events' clock: the last of its events that took it from
/// open to closed, as the fold applied them; `None` if none did.
fn closed_at(runtime: &CloudRuntime, id: Id<Order>) -> Result<Option<Timestamp>, RuntimeError> {
    let kind = StreamKind::new(OrderEvent::STREAM)
        .map_err(|_| RuntimeError::Store(StoreError::Corrupt("the order stream's kind")))?;
    let stream = StreamRef { kind, id: id.cast() };
    let mut order = Order::new(id);
    let mut closed = None;
    for event in runtime.store().stream(&stream)? {
        let was_closed = *order.status() == OrderStatus::Closed;
        fold(&mut order, &event);
        if !was_closed && *order.status() == OrderStatus::Closed {
            closed = time_of(event.body().hlc).or(closed);
        }
    }
    Ok(closed)
}

/// Puts `hours` in the order of a business day that begins at `cutoff`: its hour first, the hours
/// before it, after midnight, last.
fn in_day_order(hours: &mut [HourSales], cutoff: u8) {
    hours.sort_by_key(|sales| (sales.hour < cutoff, sales.hour));
}

/// The median of `sorted`, whose middle index is `middle`: the mean of the two middle ones of an
/// even number, rounded down.
fn median(sorted: &[u64], middle: usize) -> Option<u64> {
    let upper = *sorted.get(middle)?;
    if sorted.len() % 2 == 1 {
        return Some(upper);
    }
    let lower = *sorted.get(middle.checked_sub(1)?)?;
    lower.checked_add(upper.saturating_sub(lower).checked_div(2)?)
}

fn seconds_between(later: Timestamp, earlier: Timestamp) -> Option<u64> {
    later.duration_since(earlier).map(|duration| duration.as_secs())
}

/// The dashboard of business day `date`, as of `now`.
///
/// # Errors
/// [`ReportError`] if the store can't be read, an order can't be priced, or a total overflows.
pub fn dashboard(
    runtime: &CloudRuntime,
    date: BusinessDate,
    now: Timestamp,
) -> Result<Dashboard, ReportError> {
    let profile = runtime.profile();
    let currency = profile.data().currency;
    let report = runtime.day_report(date, ReportKind::X)?;
    let mut week_before = date;
    for _ in 0..7 {
        week_before = week_before.previous().map_err(|_| ReportError::Range)?;
    }
    let before = runtime.day_report(week_before, ReportKind::X)?;
    let sales = reports::sales(runtime, date, date)?;
    // The hours: each order closed on the day, by the local hour it closed.
    let day = profile.business_day();
    let mut hours: BTreeMap<u8, (u64, Money)> = BTreeMap::new();
    let closed = runtime.store().orders(OrderState::Closed).map_err(RuntimeError::from)?;
    for order in closed.iter().filter(|order| order.business_date == Some(date)) {
        let Some(at) = closed_at(runtime, order.id)? else { continue };
        let Ok(local) = day.local_time(at) else { continue };
        let Ok(hour) = u8::try_from(local.hour) else { continue };
        let ticket = runtime.ticket(order.id)?;
        if ticket.total.money.currency() != currency {
            continue;
        }
        let entry = hours.entry(hour).or_insert((0, Money::zero(currency)));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.checked_add(ticket.total.money)?;
    }
    // Open orders, and the oldest's age by its first event.
    let open = runtime.store().orders(OrderState::Active).map_err(RuntimeError::from)?;
    let oldest = open.iter().filter_map(|order| time_of(order.first)).min();
    // The kitchen: what waits, and how long the day's bumped tickets took.
    let waiting = runtime.store().open_kitchen_tickets().map_err(RuntimeError::from)?;
    let mut took: Vec<u64> = runtime
        .store()
        .day_kitchen_tickets(date)
        .map_err(RuntimeError::from)?
        .iter()
        .filter_map(|ticket| {
            let (sent, _) = ticket.sent?;
            seconds_between(ticket.bumped_at?, sent)
        })
        .collect();
    took.sort_unstable();
    let middle = took.len().checked_div(2).unwrap_or(0);
    let stock = runtime.stock()?;
    let mut top_items = sales.items;
    top_items.truncate(TOP_ITEMS);
    Ok(Dashboard {
        date: date.to_string(),
        currency: currency.code().to_owned(),
        minor_units: currency.minor_units(),
        day: figures(&report)?,
        week_before: figures(&before)?,
        hours: {
            // In the day's order: found in review, the hours after midnight came first.
            let cutoff = profile.data().business_day.cutoff_hour;
            let mut hours: Vec<HourSales> = hours
                .into_iter()
                .map(|(hour, (orders, total))| HourSales { hour, orders, total: total.minor() })
                .collect();
            in_day_order(&mut hours, cutoff);
            hours
        },
        top_items,
        tenders: sales.tenders,
        open_orders: u64::try_from(open.len()).unwrap_or(u64::MAX),
        oldest_open_seconds: oldest.and_then(|oldest| seconds_between(now, oldest)),
        kitchen: KitchenFigures {
            waiting: u64::try_from(waiting.len()).unwrap_or(u64::MAX),
            bumped: u64::try_from(took.len()).unwrap_or(u64::MAX),
            // Of an even number, the two middle ones' mean: found in review, the upper one.
            median_seconds: median(&took, middle),
            longest_seconds: took.last().copied(),
        },
        sold_out: runtime
            .sold_out()?
            .into_iter()
            .map(|item| item.name.as_str().to_owned())
            .collect(),
        below_zero: stock
            .into_iter()
            .filter(|view| view.on_hand.is_negative())
            .map(|view| StockFigure { name: view.name, on_hand: view.on_hand_text })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{HourSales, in_day_order, median};

    #[test]
    fn a_day_from_a_4_am_cutoff_ends_after_midnight() {
        let mut hours: Vec<HourSales> = [0, 3, 4, 12, 23]
            .into_iter()
            .map(|hour| HourSales { hour, orders: 1, total: 1 })
            .collect();
        in_day_order(&mut hours, 4);
        assert_eq!(hours.iter().map(|sales| sales.hour).collect::<Vec<_>>(), [4, 12, 23, 0, 3]);
        in_day_order(&mut hours, 0);
        assert_eq!(hours.iter().map(|sales| sales.hour).collect::<Vec<_>>(), [0, 3, 4, 12, 23]);
    }

    #[test]
    fn the_median_of_an_even_number_is_the_middle_twos_mean() {
        assert_eq!(median(&[], 0), None);
        assert_eq!(median(&[7], 0), Some(7));
        assert_eq!(median(&[10, 20], 1), Some(15));
        assert_eq!(median(&[10, 20, 31], 1), Some(20));
        assert_eq!(median(&[1, 2, 3, 6], 2), Some(2));
        assert_eq!(median(&[u64::MAX - 1, u64::MAX], 1), Some(u64::MAX - 1));
    }
}
