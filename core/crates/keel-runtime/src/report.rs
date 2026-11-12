//! X and Z reports, from local data, offline (ADR-0031, decision 6): a business day's sales, of
//! every order the device holds, by the day each was rung; its tenders, by the day each payment
//! was taken; voids; drawer sessions; and cash no drawer took.

use keel_doc::{Align, Block, Doc, Style};
use keel_domain::drawer::Drawer;
use keel_domain::payment::{Payment, PaymentCaptured, Tender};
use keel_events::keys::Signer;
use keel_store::{OrderState, PaymentState};
use keel_types::{BusinessDate, Clock, Entropy, Id, Money};

use crate::error::RuntimeError;
use crate::print::{self, Job, JobKind};
use crate::runtime::Runtime;
use crate::views::{Amount, DrawerView};

/// Which report: the day so far, or its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    /// The day so far, at any time, changing nothing.
    X,
    /// The day's end, once every drawer session on the device is closed.
    Z,
}

/// A business day's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayReport {
    /// X or Z.
    pub kind: ReportKind,
    /// The business day.
    pub date: BusinessDate,
    /// The orders closed, of those rung on the day.
    pub orders: u64,
    /// What their lines came to before discounts and comps.
    pub gross: Amount,
    /// What discounts took.
    pub discounts: Amount,
    /// What comps took.
    pub comps: Amount,
    /// What the lines were sold for, before tax: gross less discounts and comps.
    pub net: Amount,
    /// The tax charged.
    pub tax: Amount,
    /// What the orders came to: net and tax.
    pub total: Amount,
    /// What each tender took on the day, of every order, with how many payments: cash first.
    pub tenders: Vec<TenderLine>,
    /// The orders voided, or dropped once they had lines.
    pub voided: u64,
    /// Each drawer session opened on the day, of every device, the earliest first.
    pub drawers: Vec<DrawerView>,
    /// Cash taken on the day that no drawer session took, as with none open: for a manager to
    /// look at. A payment of another register shows here until its drawer's events arrive.
    pub cash_without_drawer: Amount,
    /// How many cash payments no drawer session took.
    pub payments_without_drawer: u64,
}

/// A day's sales, added up.
struct Sales {
    orders: u64,
    voided: u64,
    gross: Money,
    discounts: Money,
    net: Money,
    tax: Money,
    total: Money,
}

/// What each tender took, with how many payments; and the cash no drawer took, with how many.
type Tenders = (Vec<(Tender, u64, Money)>, Money, u64);

/// What one tender captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TenderLine {
    /// The tender.
    pub tender: Tender,
    /// How many payments.
    pub payments: u64,
    /// What they captured, tips included.
    pub amount: Amount,
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// The report of business day `date`, of every order the device holds: an X report, or a
    /// Z report (ADR-0031, decision 6).
    ///
    /// # Errors
    /// [`RuntimeError::DrawerOpen`] for a Z report while one of the device's drawer sessions is
    /// open, whatever day it opened; [`RuntimeError`] if the store can't be read, an order priced, or a total
    /// overflows.
    pub fn day_report(
        &self,
        date: BusinessDate,
        kind: ReportKind,
    ) -> Result<DayReport, RuntimeError> {
        // Every session of the device, whatever day it opened: one left open past the day's
        // end holds the next day's cash. Found in review.
        if kind == ReportKind::Z && self.open_drawer_id()?.is_some() {
            return Err(RuntimeError::DrawerOpen);
        }
        let Sales { orders, voided, gross, discounts, net, tax, total } = self.sales(date)?;
        let (mut tenders, cash_without_drawer, payments_without_drawer) = self.tenders(date)?;
        tenders.sort_by_key(|(tender, _, _)| tender.code());
        let mut views = Vec::new();
        for summary in self.store.drawers_on(date)? {
            let session = self.store.load(Drawer::new(summary.id))?;
            views.extend(DrawerView::of(&session, self.locale));
        }
        let comps = gross.checked_sub(discounts)?.checked_sub(net)?;
        let amount = |money: Money| Amount::new(money, self.locale);
        Ok(DayReport {
            kind,
            date,
            orders,
            gross: amount(gross),
            discounts: amount(discounts),
            comps: amount(comps),
            net: amount(net),
            tax: amount(tax),
            total: amount(total),
            tenders: tenders
                .into_iter()
                .map(|(tender, payments, sum)| TenderLine { tender, payments, amount: amount(sum) })
                .collect(),
            voided,
            drawers: views,
            cash_without_drawer: amount(cash_without_drawer),
            payments_without_drawer,
        })
    }

    /// What each tender took on business day `date`, by the day each payment was taken, of every
    /// order, closed or not, in the location's currency: what the customer paid, tips and cash
    /// rounding included. And the cash no drawer session took, with how many payments. Found in
    /// review: payments were by their order's day, of closed orders only, and cash rounding was
    /// left out.
    fn tenders(&self, date: BusinessDate) -> Result<Tenders, RuntimeError> {
        let currency = self.profile.data().currency;
        let mut tenders: Vec<(Tender, u64, Money)> = Vec::new();
        let (mut without, mut payments_without) = (Money::zero(currency), 0_u64);
        for payment in self.store.payments_on(date)? {
            let (PaymentState::Captured, Some(info)) = (payment.state, payment.info) else {
                continue;
            };
            let loaded = self.store.load(Payment::new(payment.id))?;
            let Some(amount) = loaded.captured().and_then(PaymentCaptured::received) else {
                continue;
            };
            if amount.currency() != currency {
                continue;
            }
            match tenders.iter_mut().find(|(tender, _, _)| *tender == info.tender) {
                Some((_, count, sum)) => {
                    *count = count.saturating_add(1);
                    *sum = sum.checked_add(amount)?;
                }
                None => tenders.push((info.tender, 1, amount)),
            }
            if info.tender == Tender::Cash && !self.store.in_drawer(payment.id)? {
                without = without.checked_add(amount)?;
                payments_without = payments_without.saturating_add(1);
            }
        }
        Ok((tenders, without, payments_without))
    }

    /// The sales of business day `date`, by the day each order was rung, of the orders the device
    /// holds in the location's currency.
    fn sales(&self, date: BusinessDate) -> Result<Sales, RuntimeError> {
        let currency = self.profile.data().currency;
        let zero = Money::zero(currency);
        let (mut orders, mut voided) = (0_u64, 0_u64);
        let (mut gross, mut discounts, mut net, mut tax, mut total) =
            (zero, zero, zero, zero, zero);
        // An order dropped once it had lines is voided, as the register gates it (ADR-0029).
        for order in self
            .store
            .orders(OrderState::Voided)?
            .into_iter()
            .chain(self.store.orders(OrderState::Abandoned)?)
        {
            if order.business_date != Some(date) {
                continue;
            }
            let loaded = self.store.load(keel_domain::order::Order::new(order.id))?;
            if order.state == OrderState::Voided || !loaded.lines().is_empty() {
                voided = voided.saturating_add(1);
            }
        }
        for order in self.store.orders(OrderState::Closed)? {
            // One in another currency is left out before it is priced, which would fail on it.
            // Found in review: it failed the report.
            if order.business_date != Some(date)
                || order.info.as_ref().is_none_or(|info| info.currency != currency)
            {
                continue;
            }
            let ticket = self.ticket(order.id)?;
            orders = orders.saturating_add(1);
            for line in &ticket.lines {
                gross = gross.checked_add(line.amount.money)?;
                for discount in &line.discounts {
                    discounts = discounts.checked_add(discount.amount.money)?;
                }
            }
            for discount in &ticket.discounts {
                discounts = discounts.checked_add(discount.amount.money)?;
            }
            net = net.checked_add(ticket.subtotal.money)?;
            for charged in &ticket.taxes {
                tax = tax.checked_add(charged.amount.money)?;
            }
            total = total.checked_add(ticket.total.money)?;
        }
        Ok(Sales { orders, voided, gross, discounts, net, tax, total })
    }

    /// Prints the report of business day `date`: its job.
    ///
    /// # Errors
    /// As [`Runtime::day_report`]; [`RuntimeError`] if the job can't be queued.
    pub fn print_report(
        &mut self,
        date: BusinessDate,
        kind: ReportKind,
    ) -> Result<Id<Job>, RuntimeError> {
        let report = self.day_report(date, kind)?;
        let doc = report_doc(&report, self.profile.data().name.as_str())?;
        let now = self.clock.now();
        let job: Id<Job> = self.ids.generate(now)?;
        let effect = print::effect(job, JobKind::Receipt, &doc, None)?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            w.enqueue(&effect, now)?;
            Ok(())
        })?;
        Ok(job)
    }

    /// Today's business date, by the runtime's clock.
    ///
    /// # Errors
    /// [`RuntimeError`] if the clock is out of the range a business date has.
    pub fn today(&self) -> Result<BusinessDate, RuntimeError> {
        Ok(self.profile.business_day().business_date_of(self.clock.now())?)
    }
}

/// The report as it prints.
fn report_doc(report: &DayReport, location: &str) -> Result<Doc, RuntimeError> {
    let row = |left: &str, amount: &Amount| Block::row(left, amount.text.clone());
    let sum = |left: &str, amount: &Option<Amount>| {
        Block::row(left, amount.as_ref().map(|amount| amount.text.clone()).unwrap_or_default())
    };
    let title = match report.kind {
        ReportKind::X => "X REPORT",
        ReportKind::Z => "Z REPORT",
    };
    let mut blocks = vec![
        Block::text(location, Align::Center, Style::LARGE),
        Block::text(title, Align::Center, Style::default()),
        Block::text(report.date.to_string(), Align::Center, Style::default()),
        Block::Rule,
        Block::row("Orders", report.orders.to_string()),
        row("Gross", &report.gross),
        row("Discounts", &report.discounts),
        row("Comps", &report.comps),
        row("Net", &report.net),
        row("Tax", &report.tax),
        Block::Row {
            left: "TOTAL".to_owned(),
            right: report.total.text.clone(),
            indent: 0,
            style: Style { bold: true, wide: false, tall: true },
        },
        Block::Rule,
    ];
    for tender in &report.tenders {
        let name = match tender.tender {
            Tender::Cash => "Cash",
            Tender::Card => "Card",
        };
        blocks
            .push(Block::row(format!("{name} ({})", tender.payments), tender.amount.text.clone()));
    }
    blocks.push(Block::row("Voided orders", report.voided.to_string()));
    for (n, drawer) in (1_u32..).zip(&report.drawers) {
        blocks.push(Block::Rule);
        blocks.push(Block::text(format!("Drawer {n}"), Align::Left, Style::default()));
        blocks.push(row("Float", &drawer.float));
        match (&drawer.expected, &drawer.counted, &drawer.variance) {
            (Some(expected), Some(counted), Some(variance)) => {
                // What moved through it is shown once it is counted: before, the count is blind.
                blocks.push(sum("Cash sales", &drawer.taken));
                blocks.push(sum("Paid in", &drawer.paid_in));
                blocks.push(sum("Paid out", &drawer.paid_out));
                blocks.push(sum("Dropped", &drawer.dropped));
                blocks.push(row("Expected", expected));
                blocks.push(row("Counted", counted));
                blocks.push(row("Over/short", variance));
            }
            _ => blocks.push(Block::row("Still open", "")),
        }
    }
    if report.payments_without_drawer > 0 {
        blocks.push(Block::Rule);
        blocks.push(Block::row(
            format!("Cash with no drawer ({})", report.payments_without_drawer),
            report.cash_without_drawer.text.clone(),
        ));
    }
    blocks.push(Block::Feed(2));
    blocks.push(Block::Cut);
    Ok(Doc::new(blocks)?)
}
