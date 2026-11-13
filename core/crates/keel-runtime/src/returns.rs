//! Returns and refunds at the register (ADR-0032): a closed order found from its receipt, the
//! lines left to return, and a return of some of them, refunded in cash out of the drawer, with
//! its approval beyond the member's role and its receipt, in one write.

use keel_doc::{Block, Doc};
use keel_domain::codec::Name;
use keel_domain::drawer::{CashRefunded, Drawer, DrawerCommand};
use keel_domain::order::{Line, Order, OrderCommand, OrderEvent, Reason, Return};
use keel_domain::payment::Tender;
use keel_domain::refund::{Refund, RefundCommand, RefundCompleted, RefundInitiated};
use keel_domain::staff::{Action, Request};
use keel_events::keys::Signer;
use keel_store::{OrderState, PaymentState};
use keel_types::{Clock, Entropy, Id, Money, MoneyError, Quantity, Unit};

use crate::error::RuntimeError;
use crate::print::{self, Job, JobKind};
use crate::runtime::{Runtime, append, known};
use crate::ticket::cash_due;
use crate::views::Amount;

/// A line of a closed order, and what is left of it to return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReturnableLine {
    /// The line.
    pub line: Id<Line>,
    /// What it sold.
    pub name: Name,
    /// How much was sold.
    pub quantity: Quantity,
    /// How much is left to return.
    pub left: Quantity,
    /// How much is left, as the locale shows it.
    pub left_text: String,
    /// How many are left, for a line sold by the each: whole ones; `None` for one sold by
    /// measure, which is returned whole.
    pub left_count: Option<u32>,
    /// What its order's checks charged it, its net and tax.
    pub charged: Amount,
}

/// A closed order's lines, and what was refunded of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Returnable {
    /// The order.
    pub order: Id<Order>,
    /// Its live lines, in the order they were added.
    pub lines: Vec<ReturnableLine>,
    /// What its returns refunded so far.
    pub refunded: Amount,
    /// Whether it was paid in cash alone, as a refund in cash needs.
    pub cash_only: bool,
}

/// A line returned, and what it refunded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundedLine {
    /// The line.
    pub line: Id<Line>,
    /// What it sold.
    pub name: Name,
    /// How much was returned, as the locale shows it.
    pub quantity_text: String,
    /// What it refunded: its net and tax.
    pub amount: Amount,
}

/// What a return did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refunded {
    /// The refund.
    pub refund: Id<Refund>,
    /// Each line returned.
    pub lines: Vec<RefundedLine>,
    /// What the return refunds: every line's net and tax.
    pub amount: Amount,
    /// What was handed back in cash, rounded as the location rounds cash.
    pub handed: Amount,
}

/// How many whole units `quantity` is, if it is a count of them.
fn whole(quantity: Quantity) -> Option<u32> {
    if quantity.unit() != Unit::Each {
        return None;
    }
    let micros = quantity.micros();
    let units = micros.checked_div(1_000_000)?;
    (micros.checked_rem(1_000_000)? == 0).then(|| u32::try_from(units).ok()).flatten()
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// The closed orders whose receipt code, the last six characters of their identifier, is
    /// `code`, whatever its case: the latest first.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn find_orders(&self, code: &str) -> Result<Vec<Id<Order>>, RuntimeError> {
        let code = code.trim().to_lowercase();
        if code.is_empty() {
            return Ok(Vec::new());
        }
        let mut found: Vec<Id<Order>> = self
            .store
            .orders(OrderState::Closed)?
            .into_iter()
            .map(|order| order.id)
            .filter(|order| order.to_string().ends_with(&code))
            .collect();
        found.reverse();
        Ok(found)
    }

    /// What is left to return of closed order `order` (ADR-0032).
    ///
    /// # Errors
    /// [`RuntimeError::UnknownOrder`] if the device holds no such order; [`RuntimeError`] if the
    /// store can't be read or an amount overflows.
    pub fn returnable(&self, order: Id<Order>) -> Result<Returnable, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let currency = self.profile.data().currency;
        let mut lines = Vec::new();
        for line in loaded.live_lines() {
            let Some((net, tax)) = loaded.charged(line.id()) else { continue };
            let returned =
                line.returned().map_or(Quantity::zero(line.quantity().unit()), |r| r.quantity);
            let left = line.quantity().checked_sub(returned)?;
            lines.push(ReturnableLine {
                line: line.id(),
                name: line.item().name.clone(),
                quantity: line.quantity(),
                left,
                left_text: self.locale.quantity(left),
                left_count: whole(left),
                charged: Amount::new(net.checked_add(tax)?, self.locale),
            });
        }
        let mut refunded = Money::zero(currency);
        for refund in self.store.refunds_of(order)? {
            if let Some(completed) = refund.completed {
                refunded = refunded.checked_add(completed.amount)?;
            }
        }
        Ok(Returnable {
            order,
            lines,
            refunded: Amount::new(refunded, self.locale),
            cash_only: self.paid_in_cash_alone(order)?,
        })
    }

    /// Whether every payment captured for `order` was in cash.
    fn paid_in_cash_alone(&self, order: Id<Order>) -> Result<bool, RuntimeError> {
        Ok(self.store.payments_of(order)?.iter().all(|payment| {
            payment.state != PaymentState::Captured
                || payment.info.as_ref().is_some_and(|info| info.tender == Tender::Cash)
        }))
    }

    /// Returns `lines` of closed order `order`, each by how much, for `reason` (ADR-0032): each
    /// refunded its share of what the order's checks charged it, in cash, rounded as the
    /// location rounds cash, out of the device's open drawer, if one is open. A refund is
    /// `refund.issue`, for what it refunds, which the member's role permits or a manager
    /// approves. The drawer kicks, and the refund's receipt prints.
    ///
    /// # Errors
    /// [`RuntimeError::CardRefund`] for an order a card paid; [`RuntimeError::Order`] if the order
    /// isn't closed, the device doesn't own it, or a line can't be returned by as much;
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an
    /// approval it doesn't have; [`RuntimeError`] if no one is signed in.
    pub fn return_lines(
        &mut self,
        order: Id<Order>,
        lines: &[(Id<Line>, Quantity)],
        reason: Reason,
    ) -> Result<Refunded, RuntimeError> {
        let meta = self.meta()?;
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        if !self.paid_in_cash_alone(order)? {
            return Err(RuntimeError::CardRefund);
        }
        let refund: Id<Refund> = self.ids.generate(meta.now)?;
        let asked = Return { refund, lines: lines.to_vec(), reason };
        let event =
            loaded.decide(self.location(), self.device, OrderCommand::ReturnLines(asked))?;
        let overflow = || RuntimeError::from(MoneyError::Overflow);
        let OrderEvent::LinesReturned(returned) = &event else { return Err(overflow()) };
        let total = returned.total().ok_or_else(overflow)?;
        // The order's own rules come before an approval is asked for what they would refuse.
        let granted = if total.is_positive() {
            let request =
                Request { action: Action::RefundIssue, amount: Some(total), percent: None };
            self.gate(request, Some(order.cast()))?
        } else {
            None
        };
        let cash = cash_due(&self.profile, total)?;
        let handed = cash.due;
        let locale = self.locale;
        let refunded_lines: Vec<RefundedLine> = returned
            .lines
            .iter()
            .map(|each| {
                let line = loaded.line(each.line).ok_or_else(overflow)?;
                Ok(RefundedLine {
                    line: each.line,
                    name: line.item().name.clone(),
                    quantity_text: locale.quantity(each.quantity),
                    amount: Amount::new(each.net.checked_add(each.tax)?, locale),
                })
            })
            .collect::<Result<_, RuntimeError>>()?;
        let view = Refunded {
            refund,
            lines: refunded_lines,
            amount: Amount::new(total, locale),
            handed: Amount::new(handed, locale),
        };
        let at = self.profile.business_day().local_time(meta.now)?;
        let receipt = crate::receipt::refund(&view, order, at, &self.profile, locale)?;
        let (location, device) = (self.location(), self.device);
        let drawer_session = self.open_drawer_id()?;
        let kick: Id<Job> = self.ids.generate(meta.now)?;
        let printed: Id<Job> = self.ids.generate(meta.now)?;
        let stream = self.approval_stream(granted.as_ref())?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            let meta = crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            let cause = append(w, order.cast(), &event, &meta)?;
            if total.is_positive() {
                let initiated = RefundInitiated { order, tender: Tender::Cash, amount: total };
                let started = Refund::new(refund).decide(
                    location,
                    device,
                    RefundCommand::Initiate(initiated),
                )?;
                append(w, refund.cast(), &started, &meta)?;
                let rounding = (!cash.rounding.is_zero()).then_some(cash.rounding);
                let completed = RefundCompleted { amount: total, rounding };
                let started = w.load(Refund::new(refund))?;
                let paid = started.decide(location, device, RefundCommand::Complete(completed))?;
                append(w, refund.cast(), &paid, &meta)?;
            }
            // The cash leaves the device's open drawer, in the same write; with none open, or one
            // that refuses it, the refund goes on, and the reports say so, as a sale's cash.
            if let Some(open) = drawer_session.filter(|_| handed.is_positive()) {
                let session = w.load(Drawer::new(open))?;
                let out = DrawerCommand::RefundCash(CashRefunded { refund, amount: handed });
                if let Ok(event) = session.decide(location, device, out) {
                    append(w, open.cast(), &event, &meta)?;
                }
            }
            if handed.is_positive() {
                let opened = Doc::new(vec![Block::DrawerKick])?;
                w.enqueue(&print::effect(kick, JobKind::Drawer, &opened, Some(cause))?, meta.now)?;
            }
            w.enqueue(&print::effect(printed, JobKind::Receipt, &receipt, Some(cause))?, meta.now)?;
            Ok(())
        })?;
        self.spend(granted.as_ref());
        Ok(view)
    }
}
