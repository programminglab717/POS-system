//! The device's cash drawer (ADR-0031): its session opened with a float, cash paid in, paid out
//! and dropped, a no-sale, and its blind close, each gated as the member's role says.

use keel_doc::{Align, Block, Doc, Style};
use keel_domain::drawer::{CashMoved, Drawer, DrawerCommand, DrawerEvent, Movement};
use keel_domain::order::Reason;
use keel_domain::staff::{Action, Request};
use keel_events::keys::Signer;
use keel_store::DrawerState;
use keel_types::{Clock, Entropy, Id, Money};

use crate::error::RuntimeError;
use crate::print::{self, Job, JobKind};
use crate::runtime::{Runtime, append};
use crate::views::{Amount, DrawerView};

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// The device's open drawer session, if it has one it can work here: one opened at another
    /// location, or in another currency, as before the location's profile changed, would refuse
    /// every cash sale and its own close. It is left open, for a person to look at, and another
    /// can be opened. Found in review: the register was stuck.
    pub(crate) fn open_drawer_id(&self) -> Result<Option<Id<Drawer>>, RuntimeError> {
        let (location, currency) = (self.location(), self.profile.data().currency);
        let open = self.store.drawers_of(self.device, DrawerState::Open)?;
        Ok(open
            .iter()
            .rev()
            .find(|summary| {
                summary.info.as_ref().is_some_and(|info| {
                    info.location == location && info.float.currency() == currency
                })
            })
            .map(|summary| summary.id))
    }

    /// The device's open drawer: its float and how many no-sales, but neither what has moved
    /// through it nor what it should hold, which the close is blind to; `None` with no drawer
    /// open.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn drawer(&self) -> Result<Option<DrawerView>, RuntimeError> {
        let Some(open) = self.open_drawer_id()? else { return Ok(None) };
        let session = self.store.load(Drawer::new(open))?;
        Ok(DrawerView::of(&session, self.locale))
    }

    /// Opens the device's drawer with `float` (ADR-0031): one at a time.
    ///
    /// # Errors
    /// [`RuntimeError::DrawerOpen`] if one is open; [`RuntimeError`] if no one is signed in, or
    /// the float is negative or in another currency.
    pub fn open_drawer(&mut self, float: Money) -> Result<DrawerView, RuntimeError> {
        if self.open_drawer_id()?.is_some() {
            return Err(RuntimeError::DrawerOpen);
        }
        let meta = self.meta()?;
        let id: Id<Drawer> = self.ids.generate(meta.now)?;
        let currency = self.profile.data().currency;
        let event = Drawer::new(id).decide(
            self.location(),
            self.device,
            DrawerCommand::Open { float, currency },
        )?;
        let locale = self.locale;
        self.store.write(|w| {
            append(w, id.cast(), &event, &meta)?;
            let session = w.load(Drawer::new(id))?;
            DrawerView::of(&session, locale).ok_or(RuntimeError::NoDrawer)
        })
    }

    /// Puts `amount` of cash into the drawer, for `reason`: not gated.
    ///
    /// # Errors
    /// As [`Runtime::pay_out`], but for an approval.
    pub fn pay_in(&mut self, amount: Money, reason: Reason) -> Result<DrawerView, RuntimeError> {
        self.move_cash(Movement::PaidIn, amount, reason)
    }

    /// Takes `amount` of cash out of the drawer for an expense, for `reason`: `drawer.paid_out`,
    /// for its amount, which the member's role permits or a manager approves.
    ///
    /// # Errors
    /// [`RuntimeError::NoDrawer`] with no drawer open; [`RuntimeError::ApprovalRequired`] or
    /// [`RuntimeError::NotApprover`] if it needs an approval it doesn't have; [`RuntimeError`] if
    /// no one is signed in, or the amount isn't more than zero in the location's currency.
    pub fn pay_out(&mut self, amount: Money, reason: Reason) -> Result<DrawerView, RuntimeError> {
        self.move_cash(Movement::PaidOut, amount, reason)
    }

    /// Takes `amount` of cash to the safe, for `reason`: not gated.
    ///
    /// # Errors
    /// As [`Runtime::pay_out`], but for an approval.
    pub fn drop_cash(&mut self, amount: Money, reason: Reason) -> Result<DrawerView, RuntimeError> {
        self.move_cash(Movement::Drop, amount, reason)
    }

    fn move_cash(
        &mut self,
        kind: Movement,
        amount: Money,
        reason: Reason,
    ) -> Result<DrawerView, RuntimeError> {
        let open = self.open_drawer_id()?.ok_or(RuntimeError::NoDrawer)?;
        let session = self.store.load(Drawer::new(open))?;
        let command = DrawerCommand::MoveCash(CashMoved { kind, amount, reason });
        // The drawer's own rules come before an approval is asked for one it would refuse.
        let event = session.decide(self.location(), self.device, command)?;
        let granted = if kind == Movement::PaidOut {
            let request =
                Request { action: Action::DrawerPaidOut, amount: Some(amount), percent: None };
            self.gate(request, Some(open.cast()))?
        } else {
            None
        };
        self.drawer_write(open, &event, granted, None)
    }

    /// Opens the drawer with no sale, for `reason`: `drawer.no_sale`, which the member's role
    /// permits or a manager approves. The drawer's kick prints at once.
    ///
    /// # Errors
    /// As [`Runtime::pay_out`].
    pub fn no_sale(&mut self, reason: Reason) -> Result<DrawerView, RuntimeError> {
        let open = self.open_drawer_id()?.ok_or(RuntimeError::NoDrawer)?;
        let session = self.store.load(Drawer::new(open))?;
        let event = session.decide(self.location(), self.device, DrawerCommand::NoSale(reason))?;
        let granted = self.gate(Request::of(Action::DrawerNoSale), Some(open.cast()))?;
        let kick = Doc::new(vec![Block::DrawerKick])?;
        let job: Id<Job> = self.ids.generate(self.clock.now())?;
        self.drawer_write(open, &event, granted, Some(&(job, JobKind::Drawer, kick)))
    }

    /// Closes the device's drawer with what was `counted`, blind: what it should hold is said only
    /// once the count is recorded. A close over or short is `drawer.variance`, for the size of
    /// the variance, which the member's role permits or a manager approves; an exact one needs
    /// nothing. The drawer's report prints. Answers with the report.
    ///
    /// # Errors
    /// As [`Runtime::pay_out`]; [`RuntimeError::ApprovalRequired`] says no more than that one is
    /// needed.
    pub fn close_drawer(&mut self, counted: Money) -> Result<DrawerView, RuntimeError> {
        let open = self.open_drawer_id()?.ok_or(RuntimeError::NoDrawer)?;
        let session = self.store.load(Drawer::new(open))?;
        let event =
            session.decide(self.location(), self.device, DrawerCommand::Close { counted })?;
        let DrawerEvent::Closed(closed) = &event else { return Err(RuntimeError::NoDrawer) };
        let variance = closed.variance().ok_or(RuntimeError::NoDrawer)?;
        let granted = if variance.is_zero() {
            None
        } else {
            let size = if variance.is_negative() { variance.checked_neg()? } else { variance };
            let request =
                Request { action: Action::DrawerVariance, amount: Some(size), percent: None };
            self.gate(request, Some(open.cast()))?
        };
        // The report as the session closes, to print: the session, and its close.
        let report =
            DrawerView::closing(&session, closed, self.locale).ok_or(RuntimeError::NoDrawer)?;
        let doc = drawer_report(&report, self.profile.data().name.as_str())?;
        let job: Id<Job> = self.ids.generate(self.clock.now())?;
        self.drawer_write(open, &event, granted, Some(&(job, JobKind::Receipt, doc)))
    }

    /// Appends `event` to drawer `open`, with the approval `granted` if there is one, and the print
    /// job `print` if given, in one write; spends the approval.
    fn drawer_write(
        &mut self,
        open: Id<Drawer>,
        event: &DrawerEvent,
        granted: Option<keel_domain::staff::Granted>,
        print: Option<&(Id<Job>, JobKind, Doc)>,
    ) -> Result<DrawerView, RuntimeError> {
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let locale = self.locale;
        let view = self.store.write(|w| {
            let meta = crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            let cause = append(w, open.cast(), event, &meta)?;
            if let Some((job, kind, doc)) = print {
                w.enqueue(&print::effect(*job, *kind, doc, Some(cause))?, meta.now)?;
            }
            let session = w.load(Drawer::new(open))?;
            DrawerView::of(&session, locale).ok_or(RuntimeError::NoDrawer)
        })?;
        self.spend(granted.as_ref());
        Ok(view)
    }
}

/// The drawer's report, as it prints at the close: its float, what moved through it, what it
/// should have held, what was counted, and the variance.
fn drawer_report(report: &DrawerView, location: &str) -> Result<Doc, RuntimeError> {
    let row = |left: &str, amount: &Option<Amount>| {
        Block::row(left, amount.as_ref().map(|amount| amount.text.clone()).unwrap_or_default())
    };
    let mut blocks = vec![
        Block::text(location, Align::Center, Style::LARGE),
        Block::text("DRAWER CLOSE", Align::Center, Style::default()),
        Block::Rule,
        Block::row("Float", report.float.text.clone()),
        row("Cash sales", &report.taken),
        row("Paid in", &report.paid_in),
        row("Paid out", &report.paid_out),
        row("Dropped", &report.dropped),
        row("Refunds", &report.refunded),
        Block::row("No sales", report.no_sales.to_string()),
        Block::Rule,
    ];
    if let (Some(expected), Some(counted), Some(variance)) =
        (&report.expected, &report.counted, &report.variance)
    {
        blocks.push(Block::row("Expected", expected.text.clone()));
        blocks.push(Block::row("Counted", counted.text.clone()));
        let words = if variance.money.is_negative() {
            "SHORT"
        } else if variance.money.is_zero() {
            "EXACT"
        } else {
            "OVER"
        };
        blocks.push(Block::Row {
            left: words.to_owned(),
            right: variance.text.clone(),
            indent: 0,
            style: Style { bold: true, wide: false, tall: true },
        });
    }
    blocks.push(Block::Feed(2));
    blocks.push(Block::Cut);
    Ok(Doc::new(blocks)?)
}
