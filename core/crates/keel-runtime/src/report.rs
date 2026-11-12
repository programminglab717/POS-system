//! X and Z reports, from local data, offline (ADR-0031, decision 6): a business day's sales, of
//! every order the device holds, its tenders, voids, drawer sessions, and cash taken with no
//! drawer open.

use std::collections::BTreeSet;

use keel_doc::{Align, Block, Doc, Style};
use keel_domain::drawer::Drawer;
use keel_domain::payment::{Payment, Tender};
use keel_events::keys::Signer;
use keel_store::{DrawerState, OrderState, PaymentState};
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
    /// The day's end, once every drawer session of the day on the device is closed.
    Z,
}

/// A business day's report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayReport {
    /// X or Z.
    pub kind: ReportKind,
    /// The business day.
    pub date: BusinessDate,
    /// The orders closed.
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
    /// What each tender captured, with how many payments: cash first.
    pub tenders: Vec<TenderLine>,
    /// The orders voided, or dropped once they had lines.
    pub voided: u64,
    /// Each drawer session opened on the day, of every device, the earliest first.
    pub drawers: Vec<DrawerView>,
    /// Cash captured with no drawer open, and how many payments: for a manager to look at.
    pub cash_without_drawer: Amount,
    /// How many cash payments were captured with no drawer open.
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
    tenders: Vec<(Tender, u64, Money)>,
    cash_payments: Vec<(Id<Payment>, Money)>,
}

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
    /// [`RuntimeError::DrawerOpen`] for a Z report while one of the device's drawer sessions of
    /// the day is open; [`RuntimeError`] if the store can't be read, an order priced, or a total
    /// overflows.
    pub fn day_report(
        &self,
        date: BusinessDate,
        kind: ReportKind,
    ) -> Result<DayReport, RuntimeError> {
        let drawers = self.store.drawers_on(date)?;
        if kind == ReportKind::Z
            && drawers.iter().any(|drawer| {
                drawer.state == DrawerState::Open
                    && drawer.info.as_ref().is_some_and(|info| info.device == self.device)
            })
        {
            return Err(RuntimeError::DrawerOpen);
        }
        let currency = self.profile.data().currency;
        let zero = Money::zero(currency);
        let Sales { orders, voided, gross, discounts, net, tax, total, mut tenders, cash_payments } =
            self.sales(date)?;
        tenders.sort_by_key(|(tender, _, _)| tender.code());
        // Cash no drawer counted: its payments are in no session's.
        let mut counted: BTreeSet<Id<Payment>> = BTreeSet::new();
        let mut views = Vec::with_capacity(drawers.len());
        for summary in &drawers {
            let session = self.store.load(Drawer::new(summary.id))?;
            counted.extend(session.payments().iter().copied());
            views.extend(DrawerView::of(&session, self.locale));
        }
        let (mut cash_without_drawer, mut payments_without_drawer) = (zero, 0_u64);
        for (payment, amount) in cash_payments {
            if !counted.contains(&payment) {
                cash_without_drawer = cash_without_drawer.checked_add(amount)?;
                payments_without_drawer = payments_without_drawer.saturating_add(1);
            }
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

    /// The sales of business day `date`, of the orders the device holds, in its currency.
    fn sales(&self, date: BusinessDate) -> Result<Sales, RuntimeError> {
        let currency = self.profile.data().currency;
        let zero = Money::zero(currency);
        let (mut orders, mut voided) = (0_u64, 0_u64);
        let (mut gross, mut discounts, mut net, mut tax, mut total) =
            (zero, zero, zero, zero, zero);
        let mut tenders: Vec<(Tender, u64, Money)> = Vec::new();
        let mut cash_payments: Vec<(Id<Payment>, Money)> = Vec::new();
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
        for state in [OrderState::Closed] {
            for order in self.store.orders(state)? {
                if order.business_date != Some(date) {
                    continue;
                }
                let ticket = self.ticket(order.id)?;
                if ticket.total.money.currency() != currency {
                    continue;
                }
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
                for payment in self.store.payments_of(order.id)? {
                    let (PaymentState::Captured, Some(info), Some(captured)) =
                        (payment.state, payment.info, payment.captured)
                    else {
                        continue;
                    };
                    let amount = captured.checked_add(payment.tip.unwrap_or(zero))?;
                    match tenders.iter_mut().find(|(tender, _, _)| *tender == info.tender) {
                        Some((_, count, sum)) => {
                            *count = count.saturating_add(1);
                            *sum = sum.checked_add(amount)?;
                        }
                        None => tenders.push((info.tender, 1, amount)),
                    }
                    if info.tender == Tender::Cash {
                        cash_payments.push((payment.id, amount));
                    }
                }
            }
        }
        Ok(Sales { orders, voided, gross, discounts, net, tax, total, tenders, cash_payments })
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
        blocks.push(row("Cash sales", &drawer.taken));
        blocks.push(row("Paid in", &drawer.paid_in));
        blocks.push(row("Paid out", &drawer.paid_out));
        blocks.push(row("Dropped", &drawer.dropped));
        match (&drawer.expected, &drawer.counted, &drawer.variance) {
            (Some(expected), Some(counted), Some(variance)) => {
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
