//! Returns and refunds at the register (ADR-0032): a closed order found from its receipt, the
//! lines left to return, and a return of some of them, refunded in cash out of the drawer, with
//! its approval beyond the member's role and its receipt, in one write; or, by reference, to a
//! card payment of the order, through the terminal (ADR-0035).

use keel_doc::{Block, Doc};
use keel_domain::codec::Name;
use keel_domain::drawer::{CashRefunded, Drawer, DrawerCommand};
use keel_domain::order::{Line, Order, OrderCommand, OrderEvent, Reason, Return};
use keel_domain::payment::{Payment, Tender};
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
    /// Whether it was paid in cash alone.
    pub cash_only: bool,
    /// What is left to refund in cash: the cash it took, less what its cash refunds refunded;
    /// `None` if a payment that settled it isn't known here, captured.
    pub cash_left: Option<Amount>,
    /// Its card payments that can be refunded to, by reference, each with what is left.
    pub cards: Vec<RefundableCard>,
}

/// A card payment a return can be refunded to (ADR-0035).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefundableCard {
    /// The payment.
    pub payment: Id<Payment>,
    /// The card, as a receipt shows it.
    pub card: Option<String>,
    /// What is left to refund to it: what it applied, its tip left out, less what refunds to it
    /// were started for.
    pub left: Amount,
}

/// Where a return's refund goes (ADR-0035).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundTo {
    /// In cash, out of the drawer.
    Cash,
    /// To a card payment of the order, by reference.
    Card(Id<Payment>),
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
    /// What was handed back in cash, rounded as the location rounds cash; nothing for a card
    /// refund.
    pub handed: Amount,
    /// The card it goes back to, for a card refund, as a receipt shows it.
    pub card: Option<String>,
}

/// What a cash refund of `total` hands back, after the order's refunds came to `sums`: rounded
/// over all its cash refunds, and no more than the cash `taken`.
fn handed_back(
    profile: &keel_domain::profile::Profile,
    sums: &Sums,
    taken: Money,
    total: Money,
) -> Result<Money, RuntimeError> {
    let cash_owed = sums.cash.checked_add(total)?;
    let rounded = cash_due(profile, cash_owed)?.due;
    let most = if rounded.compare(taken)?.is_gt() { taken } else { rounded };
    let handed = most.checked_sub(sums.handed)?;
    Ok(if handed.is_positive() && total.is_positive() {
        handed
    } else {
        Money::zero(total.currency())
    })
}

/// Each line `returned` returns of `loaded`, and what it refunds.
fn refunded_lines(
    loaded: &Order,
    returned: &keel_domain::order::LinesReturned,
    locale: keel_types::Locale,
) -> Result<Vec<RefundedLine>, RuntimeError> {
    returned
        .lines
        .iter()
        .map(|each| {
            let line = loaded.line(each.line).ok_or(MoneyError::Overflow)?;
            Ok(RefundedLine {
                line: each.line,
                name: line.item().name.clone(),
                quantity_text: locale.quantity(each.quantity),
                amount: Amount::new(each.net.checked_add(each.tax)?, locale),
            })
        })
        .collect()
}

/// Starts card refund `refund` in the write `w`, as `initiated` says, and enqueues its first
/// attempt, by `reference`, with the `receipt` it prints once paid back, if it fits (ADR-0035).
fn start_card_refund<S: Signer, E: Entropy>(
    w: &mut keel_store::Writing<'_, S, E>,
    refund: Id<Refund>,
    initiated: RefundInitiated,
    reference: &keel_domain::codec::ProcessorRef,
    receipt: &Doc,
    meta: &crate::runtime::Meta,
    device: Id<keel_events::envelope::Device>,
) -> Result<(), RuntimeError> {
    let location = w.load(Order::new(initiated.order))?.info().map(|info| info.location);
    let location = location.ok_or(RuntimeError::UnknownOrder(initiated.order))?;
    let started =
        Refund::new(refund).decide(location, device, RefundCommand::Initiate(initiated))?;
    let cause = append(w, refund.cast(), &started, meta)?;
    let fits = receipt.encode().len() <= keel_store::MAX_PAYLOAD / 2;
    let effect = crate::card::refund_effect(
        refund,
        1,
        initiated.amount,
        reference,
        Some(receipt).filter(|_| fits),
        Some(cause),
    )?;
    w.enqueue(&effect, meta.now)?;
    Ok(())
}

/// What a closed order took: the cash, and its card payments that can be refunded to.
struct Taken {
    cash: Money,
    cards: Vec<RefundableCard>,
}

/// What an order's refunds came to.
struct Sums {
    /// What they refunded, paid back.
    refunded: Money,
    /// What its cash refunds refunded.
    cash: Money,
    /// The cash they handed back.
    handed: Money,
    /// What they were started for, paid back or not.
    started: Money,
}

/// How many characters a receipt's code has: the last six of its order's identifier.
pub(crate) const RECEIPT_CODE: usize = 6;

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
    /// `code`, whatever its case: the latest first; none for a code of another length. Found in
    /// review: a shorter one matched whatever order it ended.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn find_orders(&self, code: &str) -> Result<Vec<Id<Order>>, RuntimeError> {
        let code = code.trim().to_lowercase();
        if code.chars().count() != RECEIPT_CODE {
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
        let sums = self.refunded_of(order)?;
        let taken = self.cash_taken(&loaded)?;
        let cash_left =
            taken.as_ref().map(|taken| taken.cash.checked_sub(sums.cash)).transpose()?.map(
                |left| {
                    Amount::new(
                        if left.is_negative() { Money::zero(left.currency()) } else { left },
                        self.locale,
                    )
                },
            );
        Ok(Returnable {
            order,
            lines,
            refunded: Amount::new(sums.refunded, self.locale),
            cash_only: taken.as_ref().is_some_and(|taken| taken.cards.is_empty()),
            cash_left,
            cards: taken.map(|taken| taken.cards).unwrap_or_default(),
        })
    }

    /// What closed order `loaded` took, if every payment that settled its checks is known here,
    /// captured: the cash its customers paid, and its card payments with what is left to refund
    /// to each; `None` otherwise, as for an order whose payments haven't reached this register.
    /// Found in review: a payment not yet here, or not yet captured here, passed for cash.
    fn cash_taken(&self, loaded: &Order) -> Result<Option<Taken>, RuntimeError> {
        let mut taken =
            Taken { cash: Money::zero(self.profile.data().currency), cards: Vec::new() };
        let settled = loaded.checks().iter().filter_map(|check| check.closed()?.payments.as_ref());
        let refunds = self.store.refunds_of(loaded.id())?;
        for payment in settled.flat_map(keel_domain::codec::IdSet::iter) {
            let Some(summary) = self.store.payment(payment)? else { return Ok(None) };
            let Some(tender) = summary.info.as_ref().map(|info| info.tender) else {
                return Ok(None);
            };
            if summary.state != PaymentState::Captured {
                return Ok(None);
            }
            let paid = self.store.load(Payment::new(payment))?;
            let Some(captured) = paid.captured() else { return Ok(None) };
            match tender {
                Tender::Cash => {
                    let received = captured.received().ok_or(MoneyError::Overflow)?;
                    taken.cash = taken.cash.checked_add(received)?;
                }
                // A card payment is refunded to by its reference: one settled by hand has none.
                Tender::Card if captured.reference.is_some() => {
                    let mut left = captured.amount;
                    for refund in refunds.iter().filter_map(|refund| refund.info.as_ref()) {
                        if refund.payment == Some(payment) {
                            left = left.checked_sub(refund.amount)?;
                        }
                    }
                    taken.cards.push(RefundableCard {
                        payment,
                        card: captured.card.as_ref().map(crate::card::card_text),
                        left: Amount::new(left, self.locale),
                    });
                }
                // One with no reference, as one settled by hand, can't go back to its card; nor is
                // its order paid in cash: it isn't refunded at the register. Found in review: it
                // passed for cash, and a return refunded nothing.
                Tender::Card => return Ok(None),
            }
        }
        Ok(Some(taken))
    }

    /// What of `returned` doesn't go back to stock: each line of an item whose stock is tracked,
    /// in its stock's unit (ADR-0036).
    fn unstocked(
        &self,
        loaded: &Order,
        returned: &keel_domain::order::LinesReturned,
    ) -> Result<Vec<(Id<keel_domain::refs::Variant>, Quantity)>, RuntimeError> {
        let mut unstocked = Vec::new();
        for each in &returned.lines {
            let Some(line) = loaded.line(each.line) else { continue };
            let variant = line.item().variant;
            let tracked = self.store.stock_levels(variant)?;
            if tracked.is_some_and(|(unit, _)| unit == each.quantity.unit()) {
                unstocked.push((variant, each.quantity));
            }
        }
        Ok(unstocked)
    }

    /// Where a refund of `total` goes `to`, out of what the order `taken` took, its refunds
    /// having come to `sums`: the card payment and its reference, for a card refund. To a card,
    /// no more than is left of it; in cash, for an order a card paid too, no more than the cash
    /// it took less what its cash refunds refunded. For one paid in cash alone, what is handed
    /// back is capped instead, as before: cash rounded down took less than its lines refund.
    fn refund_route(
        &self,
        taken: &Taken,
        to: RefundTo,
        sums: &Sums,
        total: Money,
    ) -> Result<Option<(RefundableCard, keel_domain::codec::ProcessorRef)>, RuntimeError> {
        let card = match to {
            RefundTo::Cash => None,
            RefundTo::Card(payment) => {
                let card = taken.cards.iter().find(|card| card.payment == payment);
                let card = card.cloned().ok_or(RuntimeError::NotCardPayment)?;
                // A refundable card has a reference: one settled by hand has none.
                let reference = self
                    .store
                    .load(Payment::new(payment))?
                    .captured()
                    .and_then(|captured| captured.reference.clone())
                    .ok_or(RuntimeError::NotCardPayment)?;
                Some((card, reference))
            }
        };
        let left = match &card {
            Some((card, _)) => Some(card.left.money),
            None if taken.cards.is_empty() => None,
            None => Some(taken.cash.checked_sub(sums.cash)?),
        };
        if let Some(left) = left
            && total.compare(left)?.is_gt()
        {
            let left = if left.is_negative() { Money::zero(left.currency()) } else { left };
            return Err(RuntimeError::RefundTooLarge { left });
        }
        Ok(card)
    }

    /// What the refunds of `order` came to: what they refunded, what its cash refunds refunded,
    /// the cash they handed back, and what every refund of it was started for, paid back or not.
    fn refunded_of(&self, order: Id<Order>) -> Result<Sums, RuntimeError> {
        let zero = Money::zero(self.profile.data().currency);
        let mut sums = Sums { refunded: zero, cash: zero, handed: zero, started: zero };
        for refund in self.store.refunds_of(order)? {
            let Some(info) = refund.info else { continue };
            sums.started = sums.started.checked_add(info.amount)?;
            let Some(completed) = refund.completed else { continue };
            sums.refunded = sums.refunded.checked_add(completed.amount)?;
            if info.tender == Tender::Cash {
                sums.cash = sums.cash.checked_add(completed.amount)?;
                let handed = completed.handed().ok_or(MoneyError::Overflow)?;
                sums.handed = sums.handed.checked_add(handed)?;
            }
        }
        Ok(sums)
    }

    /// Returns `lines` of closed order `order`, each by how much, for `reason` (ADR-0032): each
    /// refunded its share of what the order's checks charged it, in cash, rounded as the
    /// location rounds cash, out of the device's open drawer, if one is open. What is handed back
    /// is rounded over everything the order's refunds refunded, and is never more than the cash
    /// the order took: found in review, refunds rounded one by one handed back more. A refund is
    /// `refund.issue`, for everything the order's refunds refunded with it, which the member's
    /// role permits or a manager approves: found in review, a return split in parts passed a
    /// role's limit part by part. The drawer kicks, and the refund's receipt prints.
    ///
    /// # Errors
    /// [`RuntimeError::CardRefund`] for an order a card paid, or one this register doesn't know
    /// was paid in cash; and as [`Runtime::return_lines_to`].
    pub fn return_lines(
        &mut self,
        order: Id<Order>,
        lines: &[(Id<Line>, Quantity)],
        reason: Reason,
    ) -> Result<Refunded, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        if !self.cash_taken(&loaded)?.is_some_and(|taken| taken.cards.is_empty()) {
            return Err(RuntimeError::CardRefund);
        }
        self.return_lines_to(order, lines, reason, RefundTo::Cash)
    }

    /// Returns `lines` of closed order `order`, each by how much, for `reason`, refunded `to`
    /// cash, as [`Runtime::return_lines`] does, up to the cash the order took less what its cash
    /// refunds refunded; or to one of its card payments, by reference (ADR-0035): the refund is
    /// started, and its request enqueued for the shell to take to the terminal, with the
    /// receipt it prints once paid back.
    ///
    /// # Errors
    /// [`RuntimeError::RefundTooLarge`] for more than is left to refund in cash, or to the card;
    /// [`RuntimeError::NotCardPayment`] for a payment that isn't one of the order's card
    /// payments, refundable by reference; [`RuntimeError::CardRefund`] if a payment of the order
    /// isn't known here; [`RuntimeError::Order`] if the order isn't closed, the device doesn't
    /// own it, or a line can't be returned by as much; [`RuntimeError::ApprovalRequired`] or
    /// [`RuntimeError::NotApprover`] if it needs an approval it doesn't have;
    /// [`RuntimeError`] if no one is signed in.
    pub fn return_lines_to(
        &mut self,
        order: Id<Order>,
        lines: &[(Id<Line>, Quantity)],
        reason: Reason,
        to: RefundTo,
    ) -> Result<Refunded, RuntimeError> {
        self.return_lines_with(order, lines, reason, to, true)
    }

    /// Returns `lines` as [`Runtime::return_lines_to`] does, putting them back in stock if
    /// `restock`; if not, the lines of items whose stock is tracked are recorded wasted, for the
    /// return's reason, in its write (ADR-0036, decision 4). An item not tracked has no stock to
    /// put back, or not.
    ///
    /// # Errors
    /// As [`Runtime::return_lines_to`].
    pub fn return_lines_with(
        &mut self,
        order: Id<Order>,
        lines: &[(Id<Line>, Quantity)],
        reason: Reason,
        to: RefundTo,
        restock: bool,
    ) -> Result<Refunded, RuntimeError> {
        let meta = self.meta()?;
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let taken = self.cash_taken(&loaded)?.ok_or(RuntimeError::CardRefund)?;
        let refund: Id<Refund> = self.ids.generate(meta.now)?;
        let asked = Return { refund, lines: lines.to_vec(), reason };
        let command = OrderCommand::ReturnLines(asked);
        let event = loaded.decide(self.location(), self.device, command.clone())?;
        let overflow = || RuntimeError::from(MoneyError::Overflow);
        let OrderEvent::LinesReturned(returned) = &event else { return Err(overflow()) };
        let total = returned.total().ok_or_else(overflow)?;
        let sums = self.refunded_of(order)?;
        // Approved as everything the order's refunds were started for, with this one.
        let owed = sums.started.checked_add(total)?;
        let card = self.refund_route(&taken, to, &sums, total)?;
        // The order's own rules come before an approval is asked for what they would refuse.
        let granted = if total.is_positive() {
            let request =
                Request { action: Action::RefundIssue, amount: Some(owed), percent: None };
            self.gate(request, Some(order.cast()))?
        } else {
            None
        };
        let handed = if card.is_some() {
            Money::zero(total.currency())
        } else {
            handed_back(&self.profile, &sums, taken.cash, total)?
        };
        let rounding = handed.checked_sub(total)?;
        let locale = self.locale;
        let view = Refunded {
            refund,
            lines: refunded_lines(&loaded, returned, locale)?,
            amount: Amount::new(total, locale),
            handed: Amount::new(handed, locale),
            card: card.as_ref().and_then(|(card, _)| card.card.clone()),
        };
        let at = self.profile.business_day().local_time(meta.now)?;
        let receipt = crate::receipt::refund(&view, order, at, &self.profile, locale)?;
        let unstocked = if restock { Vec::new() } else { self.unstocked(&loaded, returned)? };
        let (location, device) = (self.location(), self.device);
        let drawer_session = self.open_drawer_id()?;
        let kick: Id<Job> = self.ids.generate(meta.now)?;
        let printed: Id<Job> = self.ids.generate(meta.now)?;
        let stream = self.approval_stream(granted.as_ref())?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            let meta = crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            // Decided again in the write, on the order as it stands: found in review, another
            // write between the read and this one went unchecked.
            let now = w.load(Order::new(order))?.decide(location, device, command)?;
            if now != event {
                return Err(RuntimeError::OrderChanged);
            }
            let cause = append(w, order.cast(), &event, &meta)?;
            waste(w, &unstocked, &returned.reason, location, &meta)?;
            if let Some((card, reference)) = card.as_ref().filter(|_| total.is_positive()) {
                let initiated = RefundInitiated {
                    order,
                    tender: Tender::Card,
                    amount: total,
                    payment: Some(card.payment),
                };
                return start_card_refund(w, refund, initiated, reference, &receipt, &meta, device);
            }
            if total.is_positive() {
                let initiated =
                    RefundInitiated { order, tender: Tender::Cash, amount: total, payment: None };
                let started = Refund::new(refund).decide(
                    location,
                    device,
                    RefundCommand::Initiate(initiated),
                )?;
                append(w, refund.cast(), &started, &meta)?;
                let rounding = (!rounding.is_zero()).then_some(rounding);
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

/// Records `unstocked` wasted, for `reason`, in the write `w` (ADR-0036).
fn waste<S: Signer, E: Entropy>(
    w: &mut keel_store::Writing<'_, S, E>,
    unstocked: &[(Id<keel_domain::refs::Variant>, Quantity)],
    reason: &Reason,
    location: Id<keel_events::envelope::Location>,
    meta: &crate::runtime::Meta,
) -> Result<(), RuntimeError> {
    use keel_domain::stock::{Movement, Stock, StockCommand, StockMoved};
    for (variant, quantity) in unstocked {
        let stock: Id<Stock> = variant.cast();
        let moved = StockMoved {
            movement: Movement::Wasted,
            quantity: *quantity,
            reason: Some(reason.clone()),
        };
        let wasted =
            w.load(Stock::new(stock))?.decide(location, meta.now, StockCommand::Move(moved))?;
        append(w, stock.cast(), &wasted, meta)?;
    }
    Ok(())
}
