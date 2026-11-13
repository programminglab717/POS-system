//! The runtime: a device's store, its location's profile, and the intents and views over them.

use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::BTreeSet;
use std::path::Path;

use keel_domain::checkout::Checkout;
use keel_domain::codec::{Change, IdSet, Note, ProfileVersion, ReasonCode};
use keel_domain::drawer::{CashTaken, Drawer, DrawerCommand};
use keel_domain::order::{
    Channel, Check, Discount, DiscountApplied, DiscountValue, LineAdded, LineChanged, LineRepriced,
    LineStatus, LinesAllocated, Mode, Order, OrderCommand, OrderCreated, Reason,
};
use keel_domain::payment::{
    CashTendered, Payment, PaymentCaptured, PaymentCommand, PaymentEvent, Tender,
};
use keel_domain::profile::{Choice, Member, Profile};
use keel_domain::refs::Variant;
use keel_domain::schema::DomainEvent;
use keel_domain::staff::{Action, Granted, Request};
use keel_events::envelope::{
    Actor, Aggregate as AggregateMarker, Correlation, Device, StreamKind, StreamRef, TeamMember,
};
use keel_events::keys::Signer;
use keel_events::log::EventDraft;
use keel_store::{Faults, NoFaults, OrderState, Store, StoreConfig, StoreKey, Writing};
use keel_types::{
    BusinessDate, Clock, Entropy, Id, IdGenerator, Locale, Money, Quantity, Timestamp, Unit,
};

use keel_doc::escpos::{process_id, render};
use keel_doc::{Block, Doc, PrinterProfile};
use keel_print::Outcome;

use crate::error::RuntimeError;
use crate::print::{self, Job, JobKind, JobView, PrintJob, Resolution};
use crate::receipt::{self, Closing};
use crate::staff::approved;
use crate::ticket::{self, Cash, cash_due};
use crate::views::{
    Amount, CashPaid, ItemView, MenuButton, MenuPage, MenuView, Ticket, group_views,
};

/// The most amounts [`Runtime::cash_offers`] offers.
pub const CASH_OFFERS: usize = 5;

/// The most checks an order is split evenly among at once (ADR-0030).
pub const MOST_CHECKS: u8 = 20;
/// How far ahead of the device's clock a received event's time may move it: a minute, as the
/// simulator allows.
const MAX_FORWARD_DRIFT: Duration = Duration::from_secs(60);

/// Who a device is: its identifier, and the key it signs its events with.
pub struct Identity<S> {
    /// The device.
    pub device: Id<Device>,
    /// Its signing key.
    pub signer: S,
}

/// The device runtime: what a shell drives (ADR-0023).
///
/// It holds the device's store and its location's profile. Each intent is one write of the
/// store, which makes the intent's answer too, its order's ticket: so an intent happens entirely
/// and answers, or doesn't happen at all. Each view is read whole from the store. It reads no
/// clock and draws no randomness but through its `Clock` and its `Entropy`, and does no I/O but
/// the store's.
pub struct Runtime<S, E, C, I> {
    pub(crate) store: Store<S, E>,
    pub(crate) profile: Profile,
    /// The profile's version: the SHA-256 of its encoding.
    pub(crate) profile_version: ProfileVersion,
    pub(crate) clock: C,
    pub(crate) ids: IdGenerator<I>,
    pub(crate) device: Id<Device>,
    /// The key the device signs with, which its roster must name.
    pub(crate) key: keel_events::keys::PublicKey,
    pub(crate) locale: Locale,
    pub(crate) member: Option<Id<TeamMember>>,
    /// Wrong PINs, and the lock they put on PIN entry.
    pub(crate) pins: crate::staff::PinGuard,
    /// An approval waiting for the intent it is for.
    pub(crate) pending: Option<crate::staff::Pending>,
    /// The request last refused for want of an approval, which an approval answers.
    pub(crate) asked: Option<crate::staff::Asked>,
    /// The printer jobs go to, once the shell sets one.
    printer: Option<PrinterProfile>,
    /// The job with the driver, if one is.
    sending: Option<Id<Job>>,
    /// Its replication, once started.
    pub(crate) sync: Option<crate::sync::Sync>,
    /// Whose publications the runtime takes, and how far into their log it has looked.
    pub(crate) published: crate::publication::Watch,
}

/// What every event of one intent records besides its payload.
#[derive(Clone)]
pub(crate) struct Meta {
    pub(crate) now: Timestamp,
    pub(crate) business_date: BusinessDate,
    pub(crate) actor: Actor,
    pub(crate) correlation: Id<Correlation>,
    /// The approval the intent's events name, if one approved it.
    pub(crate) approval: Option<Id<keel_events::envelope::Event>>,
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Opens the device's store at `path` with `key`, for the location `profile` describes,
    /// showing amounts as `locale` does. `entropy` is the store's, for its events' identifiers,
    /// and `ids` the runtime's, for orders, lines and payments.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be opened with `key`.
    #[expect(clippy::too_many_arguments, reason = "each is a separate thing the platform gives")]
    pub fn open(
        path: impl AsRef<Path>,
        key: StoreKey,
        identity: Identity<S>,
        profile: Profile,
        clock: C,
        entropy: E,
        ids: I,
        locale: Locale,
    ) -> Result<Self, RuntimeError> {
        Self::open_with_faults(path, key, identity, profile, clock, entropy, ids, locale, NoFaults)
    }

    /// Like [`Runtime::open`], with `faults` deciding whether each write goes on: for tests that
    /// interrupt writes.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be opened with `key`.
    #[expect(clippy::too_many_arguments, reason = "each is a separate thing the platform gives")]
    pub fn open_with_faults(
        path: impl AsRef<Path>,
        key: StoreKey,
        identity: Identity<S>,
        profile: Profile,
        clock: C,
        entropy: E,
        ids: I,
        locale: Locale,
        faults: impl Faults + Send + 'static,
    ) -> Result<Self, RuntimeError> {
        let config = StoreConfig {
            device: identity.device,
            location: profile.data().location,
            max_forward_drift: MAX_FORWARD_DRIFT,
        };
        let public_key = identity.signer.public_key().clone();
        let store =
            Store::open_with_faults(path, key, config, identity.signer, entropy, Box::new(faults))?;
        Ok(Runtime {
            store,
            profile_version: keel_domain::publication::version(&profile),
            profile,
            clock,
            ids: IdGenerator::new(ids),
            device: identity.device,
            locale,
            member: None,
            pins: crate::staff::PinGuard::default(),
            pending: None,
            asked: None,
            printer: None,
            sending: None,
            key: public_key,
            sync: None,
            published: crate::publication::Watch::default(),
        })
    }

    /// The location's profile: the one provisioned, until the runtime takes one the cloud
    /// publishes (ADR-0028).
    pub const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The profile's version: the SHA-256 of its encoding. A shell that shows the catalog reads
    /// it again when this changes.
    pub const fn profile_version(&self) -> ProfileVersion {
        self.profile_version
    }

    /// The clock the runtime reads.
    pub const fn clock(&self) -> &C {
        &self.clock
    }

    /// The locale amounts are shown in.
    pub const fn locale(&self) -> Locale {
        self.locale
    }

    /// The store, for reading what views don't show.
    pub const fn store(&self) -> &Store<S, E> {
        &self.store
    }

    /// The team member signed in.
    pub const fn signed_in(&self) -> Option<Id<TeamMember>> {
        self.member
    }

    /// What the events of an intent made now record: the business date, the team member signed
    /// in, and a correlation of their own.
    pub(crate) fn meta(&mut self) -> Result<Meta, RuntimeError> {
        let member = self.member.ok_or(RuntimeError::NotSignedIn)?;
        // A device writes only once its log is settled, or it could fork it, unless it has waited
        // for that and must sell as an island (ADR-0019).
        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now())) {
            return Err(RuntimeError::NotSettled);
        }
        let now = self.clock.now();
        Ok(Meta {
            now,
            business_date: self.profile.business_day().business_date_of(now)?,
            actor: Actor::TeamMember(member),
            correlation: self.ids.generate(now)?,
            approval: None,
        })
    }

    /// Starts an order at the register, owned by this device and the team member signed in.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order can't be written.
    pub fn start_order(&mut self, mode: Mode) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let order: Id<Order> = self.ids.generate(meta.now)?;
        let created = OrderCreated {
            channel: Channel::Pos,
            mode,
            currency: self.profile.data().currency,
            revenue_center: None,
            table: None,
            guest_count: None,
            customer: None,
            owner: self.member,
        };
        let event = Order::new(order).decide(
            self.location(),
            self.device,
            OrderCommand::Create(created),
        )?;
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            append(w, order.cast(), &event, &meta)?;
            view(w, order, &[], profile, locale)
        })
    }

    /// Adds `quantity` of `variant` to `order`, with the modifiers in `choices`, rung from the
    /// catalog (ADR-0023, decision 4).
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, the catalog refuses the choices, or the order
    /// refuses the line.
    pub fn add_item(
        &mut self,
        order: Id<Order>,
        variant: Id<Variant>,
        choices: &[Choice],
        quantity: NonZeroU32,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let rung = self.profile.ring(variant, choices)?;
        self.check_on_sale(variant, choices)?;
        let added = LineAdded {
            line: self.ids.generate(meta.now)?,
            item: rung.item,
            quantity: Quantity::from_whole(i64::from(quantity.get()), Unit::Each)?,
            modifiers: rung.modifiers,
            seat: None,
            course: None,
            notes: None,
        };
        self.decide(order, OrderCommand::AddLine(added), &meta)
    }

    /// Changes how many of `line` `order` has.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses the change.
    pub fn change_quantity(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
        quantity: NonZeroU32,
    ) -> Result<Ticket, RuntimeError> {
        let quantity = Quantity::from_whole(i64::from(quantity.get()), Unit::Each)?;
        // Fewer of a line takes some of it off: `line.remove`, as taking all of it off is. Found
        // in review: a line of ten could go to one with no approval.
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let now = loaded.line(line).ok_or(keel_domain::order::CommandError::UnknownLine(line))?;
        let fewer = now.quantity().micros() > quantity.micros();
        let more = now.quantity().micros() < quantity.micros();
        if fewer {
            self.check_unpaid(&loaded, now.allocation().iter().map(|share| share.check))?;
        }
        if more {
            self.check_line_on_sale(now)?;
        }
        // More of a line priced by hand moves its price further from the rung one, and more of a
        // discounted one takes more off: each is asked for as its own is. Found in review: a 10%
        // on one latte became 10% of a hundred with no approval.
        let request = if fewer {
            Some(Request::of(Action::LineRemove))
        } else if more && now.repriced().is_some() {
            let amount = self.override_amount(now.unit_price(), now.item().unit_price, quantity)?;
            Some(Request { action: Action::PriceOverride, amount: Some(amount), percent: None })
        } else if more && (!now.discounts().is_empty() || !loaded.discounts().is_empty()) {
            let rules = &self.profile.data().rules;
            let effect = loaded
                .quantity_effect(line, quantity, rules)?
                .ok_or(RuntimeError::UnknownOrder(order))?;
            Some(Request {
                action: Action::DiscountApply,
                amount: Some(effect.total),
                percent: Some(effect.percent),
            })
        } else {
            None
        };
        let granted = match request {
            Some(request) => self.gate(request, Some(order.cast()))?,
            None => None,
        };
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let change = LineChanged { quantity: Some(quantity), ..LineChanged::to(line) };
        let ticket = self.decide_approved(
            order,
            OrderCommand::ChangeLine(change),
            &meta,
            granted.as_ref(),
            stream,
        )?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Takes `line` off `order`: `line.remove`, which the member's role permits or a manager
    /// approves (ADR-0029).
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError`] if no one is signed in, or the order refuses.
    pub fn remove_line(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
    ) -> Result<Ticket, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        if let Some(now) = loaded.line(line) {
            self.check_unpaid(&loaded, now.allocation().iter().map(|share| share.check))?;
        }
        let granted = self.gate(Request::of(Action::LineRemove), Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(
            order,
            OrderCommand::RemoveLine(line),
            &meta,
            granted.as_ref(),
            stream,
        )?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Voids `line` of `order`, sent to the kitchen, for `reason`: it won't be served or
    /// charged; `line.remove`, as taking a line off is (ADR-0034, decision 3). The kitchen's
    /// screens strike it through; a ticket that printed it gets a void slip where it printed.
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError::PartlyPaid`] if a check it is on has a payment taken
    /// toward it; [`RuntimeError`] if no one is signed in, or the order refuses: the line isn't
    /// sent.
    pub fn void_line(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
        reason: Reason,
    ) -> Result<Ticket, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let found = loaded.line(line).ok_or(keel_domain::order::CommandError::UnknownLine(line))?;
        self.check_unpaid(&loaded, found.allocation().iter().map(|share| share.check))?;
        // The order's own rules first: a line not sent asks for no approval.
        if *found.status() != LineStatus::Fired {
            return Err(keel_domain::order::CommandError::LineNotFired(line).into());
        }
        let granted = self.gate(Request::of(Action::LineRemove), Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let slips = self.void_slips(order, &[line].into(), meta.now)?;
        let command = OrderCommand::VoidLine { line, reason };
        let ticket =
            self.decide_printing(order, command, &meta, granted.as_ref(), stream, &slips)?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Puts a discount on `line` of `order`, or on the whole order when `line` is `None`, for
    /// `reason`: `discount.apply`, which the member's role permits or a manager approves
    /// (ADR-0029, ADR-0030), asked for as what all the order's discounts would take together
    /// before tax, and the share its scope's discounts would take together: so a role's limit
    /// holds however many discounts there are (see [`keel_domain::order::DiscountEffect`]).
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError::DiscountTooLarge`] for an amount more than is left of its
    /// line; [`RuntimeError::PartlyPaid`] if a check it touches has a payment taken toward it;
    /// [`RuntimeError`] if no one is signed in, or the order refuses it: the line isn't live or
    /// is on a closed check, a check is closed, or the discount is invalid.
    pub fn apply_discount(
        &mut self,
        order: Id<Order>,
        line: Option<Id<keel_domain::order::Line>>,
        value: DiscountValue,
        reason: Reason,
    ) -> Result<Ticket, RuntimeError> {
        let discount = self.ids.generate(self.clock.now())?;
        let applied = DiscountApplied { discount, line, value, reason };
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        // The order's own rules come before an approval is asked for one it would refuse.
        let command = OrderCommand::ApplyDiscount(applied.clone());
        loaded.decide(self.location(), self.device, command.clone())?;
        let effect = loaded
            .discount_effect(&applied, &self.profile.data().rules)?
            .ok_or(RuntimeError::UnknownOrder(order))?;
        // An amount takes no more than is left: one that took less now would take more as the
        // line grew. Found in review: $500 off a $5.25 line took $210 once it was forty.
        if let DiscountValue::Amount(amount) = applied.value
            && effect.taken != amount
        {
            return Err(RuntimeError::DiscountTooLarge { left: effect.taken });
        }
        self.check_unpaid(&loaded, touched(&loaded, applied.line))?;
        // Asked for as all the order's discounts take together, and its scope's share: so many
        // discounts count as one. Found in review: five of 10% took 41% with no approval.
        let request = Request {
            action: Action::DiscountApply,
            amount: Some(effect.total),
            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Prices `line` of `order` again, at `unit_price` with its modifiers, for `reason`:
    /// `price.override`, for how far the new price is from the line's, for its whole quantity,
    /// which the member's role permits or a manager approves (ADR-0029, ADR-0030).
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError`] if no one is signed in, or the order refuses it: the
    /// line isn't live or is on a closed check, or the price is negative, foreign or the same.
    pub fn reprice_line(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
        unit_price: Money,
        reason: Reason,
    ) -> Result<Ticket, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let repriced = LineRepriced { line, unit_price, reason };
        let command = OrderCommand::RepriceLine(repriced);
        // The order's own rules come before an approval is asked for one it would refuse.
        loaded.decide(self.location(), self.device, command.clone())?;
        let now = loaded.line(line).ok_or(RuntimeError::UnknownOrder(order))?;
        self.check_unpaid(&loaded, now.allocation().iter().map(|share| share.check))?;
        // How far the price is from the one it was rung at, not from the last one set: a price
        // lowered step by step is asked for as all its steps. Found in review.
        let amount = self.override_amount(unit_price, now.item().unit_price, now.quantity())?;
        let request =
            Request { action: Action::PriceOverride, amount: Some(amount), percent: None };
        let granted = self.gate(request, Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Sets `line`'s notes, for the kitchen and the receipt, or clears them when `notes` is
    /// `None`. A line's notes change while it waits to be fired, and need no approval.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: the line was fired or
    /// taken off, or its notes already are `notes`.
    pub fn set_line_notes(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
        notes: Option<Note>,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let notes = Some(notes.map_or(Change::Clear, Change::Set));
        let change = LineChanged { notes, ..LineChanged::to(line) };
        self.decide(order, OrderCommand::ChangeLine(change), &meta)
    }

    /// Takes `discount` off `order`, which needs no approval.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: it has no such discount,
    /// or it is on a closed check.
    pub fn remove_discount(
        &mut self,
        order: Id<Order>,
        discount: Id<Discount>,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        self.decide(order, OrderCommand::RemoveDiscount(discount), &meta)
    }

    /// A stream for the approval `granted`, if there is one.
    pub(crate) fn approval_stream(
        &mut self,
        granted: Option<&Granted>,
    ) -> Result<Option<Id<AggregateMarker>>, RuntimeError> {
        granted.map(|_| self.ids.generate(self.clock.now())).transpose().map_err(RuntimeError::from)
    }

    /// Drops `order`: takes off every line it has, then abandons it, in one write. An order that
    /// has had lines on it is voided, `order.void`, which the member's role permits or a manager approves
    /// (ADR-0029); an empty one is dropped by whoever began it.
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError`] if no one is signed in, or the order refuses: it isn't
    /// open, or a line was sent to be prepared.
    pub fn abandon(&mut self, order: Id<Order>) -> Result<Ticket, RuntimeError> {
        // An order that ever had a line is voided, whether its lines were taken off first or not.
        // Found in review: taking each line off, then dropping the empty order, needed no approval.
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let granted = if loaded.lines().is_empty() {
            None
        } else {
            self.gate(Request::of(Action::OrderVoid), Some(order.cast()))?
        };
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let payments = self.payments(order)?;
        // An order with lines sent to the kitchen is voided, not abandoned: the kitchen is told,
        // and a ticket that printed gets a void slip (ADR-0034, decision 3).
        let sent: BTreeSet<Id<keel_domain::order::Line>> = loaded
            .live_lines()
            .filter(|line| line.was_fired())
            .map(keel_domain::order::Line::id)
            .collect();
        let slips =
            if sent.is_empty() { Vec::new() } else { self.void_slips(order, &sent, meta.now)? };
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        let ticket = self.store.write(|w| {
            let meta = approved(w, granted.as_ref(), stream, &meta)?;
            let mut loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            if loaded.lines().iter().any(keel_domain::order::Line::was_fired) {
                let dropped = Reason { code: ReasonCode::new("dropped")?, note: None };
                let event = loaded.decide(location, device, OrderCommand::Void(dropped))?;
                let voided = append(w, order.cast(), &event, &meta)?;
                for (job, printer, doc) in &slips {
                    w.enqueue(&print::kitchen_effect(*job, doc, printer, Some(voided))?, meta.now)?;
                }
                return view(w, order, &payments, profile, locale);
            }
            let live: Vec<_> = loaded.live_lines().map(keel_domain::order::Line::id).collect();
            for line in live {
                let event = loaded.decide(location, device, OrderCommand::RemoveLine(line))?;
                append(w, order.cast(), &event, &meta)?;
                loaded = w.load(Order::new(order))?;
            }
            let event = loaded.decide(location, device, OrderCommand::Abandon)?;
            append(w, order.cast(), &event, &meta)?;
            view(w, order, &payments, profile, locale)
        })?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// Takes `tendered` in cash for what `order` owes: [`Runtime::pay_check_cash`] for all the
    /// one check that holds something owes, its main one or the one its lines went to.
    ///
    /// # Errors
    /// [`RuntimeError::Split`] if more than one open check holds something: such an order is paid
    /// check by check; and as [`Runtime::pay_check_cash`].
    pub fn pay_cash(
        &mut self,
        order: Id<Order>,
        tendered: Money,
    ) -> Result<CashPaid, RuntimeError> {
        // The one open check that holds something, wherever its lines went: found in review, an
        // order whose lines had all moved off its main check couldn't be paid.
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let holding: Vec<Id<Check>> = loaded
            .checks()
            .iter()
            .filter(|check| check.is_open() && loaded.lines_on(check.id()).next().is_some())
            .map(Check::id)
            .collect();
        let check = match holding.as_slice() {
            [] => order.cast(),
            [only] => *only,
            _ => return Err(RuntimeError::Split),
        };
        self.pay_check_cash(order, check, None, tendered)
    }

    /// Takes `tendered` in cash toward `check` of `order`: `part` of what it owes, or all of it,
    /// with the location's cash rounding, when `part` is `None` or all it owes. The payment is
    /// initiated and captured; once the check's payments cover it, it closes and its receipt
    /// prints; once every check with a live line has closed, so does the order: all in one
    /// write. A check that holds something and owes nothing closes with no payment. Answers with
    /// the change and the order's ticket (ADR-0030).
    ///
    /// # Errors
    /// [`RuntimeError::TenderShort`] if `tendered` is less than what it pays,
    /// [`RuntimeError::WrongCurrency`] or [`RuntimeError::NegativeTender`] if it isn't cash at
    /// the location, and [`RuntimeError`] if no one is signed in, the part is nothing or more
    /// than the check owes, or the check can't be paid as it stands.
    pub fn pay_check_cash(
        &mut self,
        order: Id<Order>,
        check: Id<Check>,
        part: Option<Money>,
        tendered: Money,
    ) -> Result<CashPaid, RuntimeError> {
        let meta = self.meta()?;
        let currency = self.profile.data().currency;
        if tendered.currency() != currency || part.is_some_and(|part| part.currency() != currency) {
            return Err(RuntimeError::WrongCurrency);
        }
        if tendered.is_negative() {
            return Err(RuntimeError::NegativeTender);
        }
        let payment: Id<Payment> = self.ids.generate(meta.now)?;
        let drawer: Id<Job> = self.ids.generate(meta.now)?;
        let receipt_job: Id<Job> = self.ids.generate(meta.now)?;
        let cashier =
            self.member.and_then(|member| self.profile.member(member)).map(|m| m.name.clone());
        let closed_at = self.profile.business_day().local_time(meta.now)?;
        let payments = self.payments(order)?;
        let drawer_session = self.open_drawer_id()?;
        // What of the check is still pending goes to the kitchen as it is paid, in the payment's
        // own write: a counter order reaches the kitchen when it is paid (ADR-0034, decision 3).
        let firing = {
            let loaded = self.store.load(Order::new(order))?;
            known(&loaded)?;
            self.plan_firing(&loaded, Some(check), meta.now)?
        };
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            if let Some(firing) = &firing {
                crate::kitchen::write_firing(w, firing, location, device, &meta)?;
            }
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let rules = &profile.data().rules;
            let checkout = Checkout::new(&loaded, payments.iter(), rules, profile.rules_version());
            let due = checkout.balance(check)?.due;
            // A part that is all the check owes pays it as all of it does: rounded as cash is.
            let part = part.filter(|part| *part != due);
            let (applied, cash) = match part {
                None if due.is_positive() => (due, cash_due(profile, due)?),
                None => (due, Cash::none(due)),
                Some(part) => (part, Cash { due: part, rounding: Money::zero(currency) }),
            };
            if tendered.compare(cash.due)?.is_lt() {
                return Err(RuntimeError::TenderShort { due: cash.due });
            }
            let change = tendered.checked_sub(cash.due)?;
            let mut paid = payments.clone();
            if applied.is_positive() || part.is_some() {
                let started = checkout.start_payment(
                    location,
                    device,
                    payment,
                    check,
                    Tender::Cash,
                    applied,
                )?;
                append(w, payment.cast(), &started, &meta)?;
                let captured = PaymentCaptured {
                    amount: applied,
                    tip: None,
                    reference: None,
                    cash: Some(CashTendered {
                        tendered,
                        rounding: (!cash.rounding.is_zero()).then_some(cash.rounding),
                    }),
                };
                let paying = w.load(Payment::new(payment))?;
                let event = paying.decide(location, PaymentCommand::Capture(captured))?;
                let captured = append(w, payment.cast(), &event, &meta)?;
                paid.push(w.load(Payment::new(payment))?);
                // The cash goes into the device's open drawer, in the same write: the payment and
                // the drawer agree, or neither is written (ADR-0031). With none open, or one that
                // refuses it, as one in another currency once the location's changed, the sale
                // goes on, and the reports say so: a sale never waits for a drawer. Found in
                // review: a refusal refused the sale.
                if let (Some(open), PaymentEvent::Captured(captured)) = (drawer_session, &event)
                    && let Some(amount) = captured.received().filter(|amount| amount.is_positive())
                {
                    let session = w.load(Drawer::new(open))?;
                    let taken = DrawerCommand::TakeCash(CashTaken { payment, amount });
                    if let Ok(event) = session.decide(location, device, taken) {
                        append(w, open.cast(), &event, &meta)?;
                    }
                }
                // The drawer opens when cash changed hands.
                let kick = Doc::new(vec![Block::DrawerKick])?;
                w.enqueue(
                    &print::effect(drawer, JobKind::Drawer, &kick, Some(captured))?,
                    meta.now,
                )?;
            }
            let checkout = Checkout::new(&loaded, paid.iter(), rules, profile.rules_version());
            if checkout.balance(check)?.due.is_positive() {
                let ticket = ticket::ticket(&loaded, &paid, profile, locale)?;
                return Ok(CashPaid { change: Amount::new(change, locale), ticket });
            }
            let closed = checkout.close_check(location, device, check)?;
            let closing = append(w, order.cast(), &closed, &meta)?;
            let loaded = w.load(Order::new(order))?;
            // The order closes with the last of its checks that hold something.
            if finished(&loaded) {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, &meta)?;
            }
            let loaded = w.load(Order::new(order))?;
            let ticket = ticket::ticket(&loaded, &paid, profile, locale)?;
            let receipt =
                CheckReceipt { job: receipt_job, check, cashier: cashier.as_ref(), at: closed_at };
            enqueue_receipt(w, &ticket, &paid, &receipt, closing, profile, locale, meta.now)?;
            Ok(CashPaid { change: Amount::new(change, locale), ticket })
        })
    }

    /// Recalls `order`, parked on another device, to this one (ADR-0030, decision 6): asks the hub
    /// for it, from the lease this device saw. The hub answers through replication; once it
    /// grants the order, this device owns it. An order this device owns, or has asked for
    /// already, is answered with its ticket as it is.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: it isn't at this location,
    /// or the hub's lease can't move on.
    pub fn recall(&mut self, order: Id<Order>) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let ours = loaded.ownership().is_some_and(|ownership| ownership.device == self.device);
        let asked = loaded.requests().iter().any(|request| request.device == self.device);
        if ours || asked {
            return ticket::ticket(&loaded, &self.payments(order)?, &self.profile, self.locale);
        }
        self.decide(order, OrderCommand::RequestOwnership, &meta)
    }

    /// Takes `order` from the device that owns it without the hub, on a manager's word, for
    /// `reason` (ADR-0021, ADR-0030): for a device that can't reach the hub. Gated as
    /// `order.void` is, which a manager approves; the order is flagged for reconciliation.
    ///
    /// # Errors
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an approval
    /// it doesn't have; [`RuntimeError`] if no one is signed in, or the order refuses: this
    /// device owns it already, or it isn't at this location.
    pub fn take_over(&mut self, order: Id<Order>, reason: Reason) -> Result<Ticket, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let command = OrderCommand::OverrideOwnership(reason);
        // The order's own rules come before an approval is asked for one it would refuse.
        loaded.decide(self.location(), self.device, command.clone())?;
        let granted = self.gate(Request::of(Action::OrderVoid), Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;
        self.spend(granted.as_ref());
        Ok(ticket)
    }

    /// How far `price` is from `rung` over `quantity`, as the order charges each: each extended and
    /// rounded as the rules round, then the difference, by its size. Found in review: the
    /// difference rounded once could cross a limit by a cent the order never charged.
    fn override_amount(
        &self,
        price: Money,
        rung: Money,
        quantity: Quantity,
    ) -> Result<Money, RuntimeError> {
        let extension = self.profile.data().rules.extension;
        let at = |unit: Money| unit.mul_decimal(quantity.to_decimal(), extension);
        let difference = at(price)?.checked_sub(at(rung)?)?;
        Ok(if difference.is_negative() { difference.checked_neg()? } else { difference })
    }

    /// Refuses a change to what `checks` charge while one of them, open, has a payment captured
    /// toward it: the customer paid for what it was. Found in review: a discount after part of a
    /// check was paid left the customer owed change no one gave them.
    fn check_unpaid(
        &self,
        order: &Order,
        checks: impl IntoIterator<Item = Id<Check>>,
    ) -> Result<(), RuntimeError> {
        let payments = self.payments(order.id())?;
        let rules = &self.profile.data().rules;
        let checkout = Checkout::new(order, payments.iter(), rules, self.profile.rules_version());
        for check in checks {
            let open = order.check(check).is_some_and(Check::is_open);
            if open && !checkout.balance(check)?.captured.is_zero() {
                return Err(RuntimeError::PartlyPaid(check));
            }
        }
        Ok(())
    }

    /// The device this runtime is: the owner its orders name.
    pub fn device(&self) -> Id<Device> {
        self.device
    }

    /// Splits every live line of `order` evenly among `ways` checks: its open checks, in order,
    /// and as many new ones as it takes (ADR-0030). Each line goes in as many shares, so each
    /// check pays its part.
    ///
    /// # Errors
    /// [`RuntimeError::NothingToSplit`] if the order has no live line, or `ways` is less than 2
    /// or more than [`MOST_CHECKS`]; [`RuntimeError`] if no one is signed in, or the order
    /// refuses: a check is closed, or it is split that way already.
    pub fn split_evenly(&mut self, order: Id<Order>, ways: u8) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        if !(2..=MOST_CHECKS).contains(&ways) {
            return Err(RuntimeError::NothingToSplit);
        }
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let live: Vec<Id<keel_domain::order::Line>> =
            loaded.live_lines().map(keel_domain::order::Line::id).collect();
        let lines = IdSet::new(live).map_err(|_| RuntimeError::NothingToSplit)?;
        let open: Vec<Id<Check>> =
            loaded.checks().iter().filter(|check| check.is_open()).map(Check::id).collect();
        let more = usize::from(ways).saturating_sub(open.len());
        let mut minted = Vec::with_capacity(more);
        for _ in 0..more {
            minted.push(self.ids.generate(meta.now)?);
        }
        let targets: Vec<Id<Check>> =
            open.into_iter().chain(minted.iter().copied()).take(usize::from(ways)).collect();
        let checks = IdSet::new(targets).map_err(|_| RuntimeError::NothingToSplit)?;
        let allocated = LinesAllocated::splitting(&lines, &checks);
        self.allocate(order, &minted, allocated, &meta)
    }

    /// Moves `line` of `order`, whole, to `to`, an open check, or to a new check when `to` is
    /// `None` (ADR-0030).
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: the line isn't live or is
    /// on a closed check, the check is unknown or closed, or the line is there already.
    pub fn move_line(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
        to: Option<Id<Check>>,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let minted: Vec<Id<Check>> = match to {
            Some(_) => Vec::new(),
            None => vec![self.ids.generate(meta.now)?],
        };
        let check =
            to.or_else(|| minted.first().copied()).ok_or(RuntimeError::UnknownOrder(order))?;
        let lines = IdSet::new([line]).map_err(|_| RuntimeError::UnknownOrder(order))?;
        self.allocate(order, &minted, LinesAllocated::moving(&lines, check), &meta)
    }

    /// Opens the checks `opened`, then allocates `allocated`, in one write.
    fn allocate(
        &mut self,
        order: Id<Order>,
        opened: &[Id<Check>],
        allocated: LinesAllocated,
        meta: &Meta,
    ) -> Result<Ticket, RuntimeError> {
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            for &check in opened {
                let loaded = w.load(Order::new(order))?;
                known(&loaded)?;
                let event = loaded.decide(location, device, OrderCommand::OpenCheck(check))?;
                append(w, order.cast(), &event, meta)?;
            }
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let event = loaded.decide(location, device, OrderCommand::AllocateLines(allocated))?;
            append(w, order.cast(), &event, meta)?;
            view(w, order, &payments, profile, locale)
        })
    }

    /// Loads `order`, decides `command` on it, and appends the event, in one write that answers
    /// with the order's ticket.
    fn decide(
        &mut self,
        order: Id<Order>,
        command: OrderCommand,
        meta: &Meta,
    ) -> Result<Ticket, RuntimeError> {
        self.decide_approved(order, command, meta, None, None)
    }

    /// As [`Runtime::decide`], with the approval `granted`, if there is one, written in the same
    /// write in `stream`, and named by the event.
    fn decide_approved(
        &mut self,
        order: Id<Order>,
        command: OrderCommand,
        meta: &Meta,
        granted: Option<&Granted>,
        stream: Option<Id<AggregateMarker>>,
    ) -> Result<Ticket, RuntimeError> {
        self.decide_printing(order, command, meta, granted, stream, &[])
    }

    /// As [`Runtime::decide_approved`], enqueuing the kitchen's print jobs `slips` in the same
    /// write, each caused by the event.
    fn decide_printing(
        &mut self,
        order: Id<Order>,
        command: OrderCommand,
        meta: &Meta,
        granted: Option<&Granted>,
        stream: Option<Id<AggregateMarker>>,
        slips: &[(Id<Job>, keel_domain::profile::Printer, Doc)],
    ) -> Result<Ticket, RuntimeError> {
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            let meta = &approved(w, granted, stream, meta)?;
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let event = loaded.decide(location, device, command)?;
            let decided = append(w, order.cast(), &event, meta)?;
            for (job, printer, doc) in slips {
                w.enqueue(&print::kitchen_effect(*job, doc, printer, Some(decided))?, meta.now)?;
            }
            // A split order whose open checks were emptied, every check holding something closed,
            // closes. Found in review: it stayed open for good, nothing able to close it.
            let loaded = w.load(Order::new(order))?;
            if finished(&loaded) {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, meta)?;
            }
            view(w, order, &payments, profile, locale)
        })
    }

    pub(crate) fn location(&self) -> Id<keel_events::envelope::Location> {
        self.profile.data().location
    }

    /// The payments of `order` the store holds. An intent reads them just before its write, as a
    /// write can't list them: the runtime is its store's one writer, so nothing comes between.
    pub(crate) fn payments(&self, order: Id<Order>) -> Result<Vec<Payment>, RuntimeError> {
        let mut payments = Vec::new();
        for summary in self.store.payments_of(order)? {
            payments.push(self.store.load(Payment::new(summary.id))?);
        }
        Ok(payments)
    }

    /// The ticket of `order`.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownOrder`] if the store holds no such order, and [`RuntimeError`] if
    /// it can't be read or priced.
    pub fn ticket(&self, order: Id<Order>) -> Result<Ticket, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        ticket::ticket(&loaded, &self.payments(order)?, &self.profile, self.locale)
    }

    /// The tickets of the orders still open, oldest first.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read or an order priced.
    pub fn open_orders(&self) -> Result<Vec<Ticket>, RuntimeError> {
        self.store
            .orders(OrderState::Active)?
            .into_iter()
            .map(|order| self.ticket(order.id))
            .collect()
    }

    /// The tickets of the orders begun on today's business date, by the runtime's clock,
    /// whatever became of them, oldest first: the register's list of today's orders.
    ///
    /// It reads every order of each state, as reprinting does: a store query by business date
    /// comes when a day's orders are too many for it.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read, an order priced, or the clock is out of the
    /// range a business date has.
    pub fn orders_today(&self) -> Result<Vec<Ticket>, RuntimeError> {
        let today = self.profile.business_day().business_date_of(self.clock.now())?;
        let mut orders = Vec::new();
        for state in
            [OrderState::Active, OrderState::Closed, OrderState::Voided, OrderState::Abandoned]
        {
            orders.extend(
                self.store
                    .orders(state)?
                    .into_iter()
                    .filter(|order| order.business_date == Some(today)),
            );
        }
        // Identifiers are UUIDv7s: their order is the order they were made in.
        orders.sort_by_key(|order| order.id.to_bytes());
        orders.iter().map(|order| self.ticket(order.id)).collect()
    }

    /// What the register offers a cashier as cash tendered for `order`: what it owes in cash,
    /// then the smallest amounts above that in whole multiples of 1, 5, 10, 20, 50 and 100 of the
    /// currency's major unit, each once, at most [`CASH_OFFERS`] in all. An order that owes
    /// nothing is offered nothing more.
    ///
    /// # Errors
    /// As [`Runtime::ticket`].
    pub fn cash_offers(&self, order: Id<Order>) -> Result<Vec<Amount>, RuntimeError> {
        let due = self.ticket(order)?.cash_due.money;
        let currency = due.currency();
        let offers = ticket::cash_offers(due.minor(), currency.minor_units());
        Ok(offers
            .into_iter()
            .map(|minor| Amount::new(Money::from_minor(minor, currency), self.locale))
            .collect())
    }

    /// `minor` of the location's currency, as the register shows it: for an amount a cashier
    /// types.
    pub fn amount(&self, minor: i64) -> Amount {
        Amount::new(Money::from_minor(minor, self.profile.data().currency), self.locale)
    }

    /// The team members who may sign in, in the profile's order.
    pub fn team(&self) -> &[Member] {
        &self.profile.data().team
    }

    /// The menu, as the register shows it, with what is sold out now (ADR-0033).
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn menu(&self) -> Result<MenuView, RuntimeError> {
        let profile = &self.profile;
        let sold_out = self.sold_out_map()?;
        let pages = profile
            .data()
            .menu
            .pages
            .iter()
            .map(|page| MenuPage {
                name: page.name.clone(),
                buttons: page
                    .buttons
                    .iter()
                    .filter_map(|&variant| {
                        let (item, entry) = profile.variant(variant)?;
                        let groups =
                            || item.groups.iter().filter_map(|&group| profile.group(group));
                        Some(MenuButton {
                            variant,
                            name: profile.variant_name(variant)?.clone(),
                            price: Amount::new(entry.price, self.locale),
                            choices: !item.groups.is_empty(),
                            required: groups().any(|group| group.min > 0),
                            sold_out: sold_out
                                .get(&variant.cast())
                                .copied()
                                .map(crate::views::SoldOutUntil),
                        })
                    })
                    .collect(),
            })
            .collect();
        Ok(MenuView { pages })
    }

    /// `variant`, with the modifier groups offered for it.
    ///
    /// # Errors
    /// [`RuntimeError::Ring`] if the catalog has no such variant.
    pub fn item(&self, variant: Id<Variant>) -> Result<ItemView, RuntimeError> {
        let profile = &self.profile;
        let unknown =
            || RuntimeError::Ring(keel_domain::profile::RingError::UnknownVariant(variant));
        let (item, entry) = profile.variant(variant).ok_or_else(unknown)?;
        Ok(ItemView {
            variant,
            name: profile.variant_name(variant).ok_or_else(unknown)?.clone(),
            price: Amount::new(entry.price, self.locale),
            groups: group_views(profile, &item.groups, self.locale, &self.sold_out_map()?),
        })
    }
}

/// Print jobs (ADR-0024, decision 7).
impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Sets the printer jobs go to. Until one is set, jobs wait.
    pub const fn set_printer(&mut self, printer: PrinterProfile) {
        self.printer = Some(printer);
    }

    /// The jobs queued, with the driver, and unconfirmed, in the order they were enqueued.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn print_jobs(&self) -> Result<Vec<JobView>, RuntimeError> {
        let unfinished = self.store.unfinished_effects()?;
        Ok(unfinished.iter().filter_map(|queued| print::view(queued, self.sending)).collect())
    }

    /// The earliest job that is due, started and rendered for the printer, for the driver: one
    /// at a time, so `None` while one is with the driver, and while no printer is set. A job
    /// whose document can't be read fails, for a person to look at. A job due later than any
    /// wait it could have been given is due now: the clock was set back since it was enqueued
    /// or retried, and it mustn't wait for the clock to catch up.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read or written.
    pub fn next_print_job(&mut self) -> Result<Option<PrintJob>, RuntimeError> {
        // Tickets no screen showed in time print first, wherever this register's printer is.
        self.reroute_overdue()?;
        if self.sending.is_some() {
            return Ok(None);
        }
        // A kitchen job names its printer; another goes to the register's own, once it is set.
        // Kitchen printers are taken to be as wide as the register's, or 80 mm with none.
        let own = self.printer;
        let printer = own.unwrap_or(PrinterProfile::EPSON_80MM);
        let now = self.clock.now();
        let latest = print::after(now, print::MAX_WAIT)?;
        let mut pending: Vec<_> = self
            .store
            .unfinished_effects()?
            .into_iter()
            .enumerate()
            .filter(|(_, queued)| queued.state == keel_store::EffectState::Pending)
            .filter(|(_, queued)| queued.due <= now || queued.due > latest)
            .filter(|(_, queued)| {
                print::job_of(queued)
                    .is_some_and(|(_, kind)| kind == JobKind::Kitchen || own.is_some())
            })
            .collect();
        // The earliest due first, then the earliest enqueued; those the clock left behind, now.
        pending.sort_by_key(|(seq, queued)| (queued.due.min(now), *seq));
        for (_, queued) in pending {
            let Some((job, kind)) = print::job_of(&queued) else { continue };
            let key = queued.effect.key.clone();
            if queued.due > now {
                self.store.write(|w| w.reschedule(&key, now))?;
            }
            let Ok((doc, to)) = print::document(&queued) else {
                self.store.write(|w| -> Result<(), RuntimeError> {
                    w.start(&key, now)?;
                    w.fail(&key)?;
                    Ok(())
                })?;
                continue;
            };
            let started = self.store.write(|w| w.start(&key, now))?;
            self.sending = Some(job);
            return Ok(Some(PrintJob {
                job,
                kind,
                bytes: render(&doc, &printer),
                confirm: printer.confirm,
                process_id: process_id(print::process_number(job, started.attempts)),
                to,
            }));
        }
        Ok(None)
    }

    /// Records what the driver says happened to `job`: printed, it is done; not sent, it waits
    /// to be tried again, 2 s after its first attempt and twice as long after each, up to 60 s;
    /// unconfirmed, it stays so until staff [resolve](Runtime::resolve) it.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownJob`] unless `job` is the one with the driver, and
    /// [`RuntimeError::Store`] if the store can't be written, which leaves it unconfirmed.
    pub fn print_outcome(&mut self, job: Id<Job>, outcome: Outcome) -> Result<(), RuntimeError> {
        if self.sending != Some(job) {
            return Err(RuntimeError::UnknownJob(job));
        }
        // No longer with the driver, whatever the store makes of it: a job the store can't
        // record the outcome of stays running, and so unconfirmed.
        self.sending = None;
        let key = job.to_bytes();
        match outcome {
            Outcome::Printed { .. } => self.store.write(|w| w.finish(&key))?,
            Outcome::NotSent(_) => {
                let queued = self.store.effect(&key)?.ok_or(RuntimeError::UnknownJob(job))?;
                let due = print::after(self.clock.now(), print::backoff(queued.attempts))?;
                self.store.write(|w| w.retry(&key, due))?;
            }
            Outcome::Unconfirmed(_) => {}
        }
        Ok(())
    }

    /// What staff decide about the unconfirmed `job`: that it printed, which finishes it; to
    /// print it again, which ends it and enqueues its document again marked as a reprint; or to
    /// dismiss it, which ends it. Answers with the new job, when there is one.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownJob`] if there is no such job, [`RuntimeError::NotUnconfirmed`] if it
    /// isn't unconfirmed, and [`RuntimeError::Store`] if the store can't be written.
    pub fn resolve(
        &mut self,
        job: Id<Job>,
        resolution: Resolution,
    ) -> Result<Option<Id<Job>>, RuntimeError> {
        let key = job.to_bytes();
        let queued = self.store.effect(&key)?.ok_or(RuntimeError::UnknownJob(job))?;
        let (_, kind) = print::job_of(&queued).ok_or(RuntimeError::UnknownJob(job))?;
        if queued.state != keel_store::EffectState::Running || self.sending == Some(job) {
            return Err(RuntimeError::NotUnconfirmed(job));
        }
        match resolution {
            Resolution::Printed => {
                self.store.write(|w| w.finish(&key))?;
                Ok(None)
            }
            Resolution::Dismiss => {
                self.store.write(|w| w.fail(&key))?;
                Ok(None)
            }
            Resolution::Again => {
                let (doc, _) = print::document(&queued)?;
                // A receipt, and a kitchen ticket, printed again say so: a cook mustn't make it
                // twice (hardware §5).
                let doc = match kind {
                    JobKind::Receipt | JobKind::Kitchen => self.as_reprint(&doc)?,
                    JobKind::Drawer => doc,
                };
                let now = self.clock.now();
                let again: Id<Job> = self.ids.generate(now)?;
                let effect = print::again(again, &queued, &doc)?;
                self.store.write(|w| -> Result<(), RuntimeError> {
                    w.fail(&key)?;
                    w.enqueue(&effect, now)?;
                    Ok(())
                })?;
                Ok(Some(again))
            }
        }
    }

    /// The receipt the closed `order` printed when it closed, printed again marked "REPRINT"
    /// above and below: `receipt.reprint`, which the member's role permits or a manager approves
    /// (ADR-0029).
    ///
    /// # Errors
    /// [`RuntimeError::NotSignedIn`]; [`RuntimeError::ApprovalRequired`] or
    /// [`RuntimeError::NotApprover`] if it needs an approval it doesn't have;
    /// [`RuntimeError::NoReceipt`] if the order printed no receipt, and [`RuntimeError::Store`]
    /// if the store can't be read or written.
    pub fn reprint(&mut self, order: Id<Order>) -> Result<Id<Job>, RuntimeError> {
        // Someone is signed in first: whether a receipt exists is for them to learn.
        self.member.ok_or(RuntimeError::NotSignedIn)?;
        // Each receipt the order printed: one for each check of a split order, each by the close
        // that printed it, the last original of each. Found in review: only the last check's
        // could be printed again.
        let mut originals: Vec<(Doc, Id<keel_events::envelope::Event>)> = Vec::new();
        for queued in self.store.effects()? {
            if !matches!(print::job_of(&queued), Some((_, JobKind::Receipt))) {
                continue;
            }
            let Some(cause) = queued.effect.cause else { continue };
            let Some(event) = self.store.event(cause)? else { continue };
            if event.body().stream.id != order.cast() {
                continue;
            }
            let doc = Doc::decode(&queued.effect.payload)?;
            if is_reprint(&doc) {
                continue;
            }
            originals.retain(|(_, known)| *known != cause);
            originals.push((doc, cause));
        }
        if originals.is_empty() {
            return Err(RuntimeError::NoReceipt(order));
        }
        let granted = self.gate(Request::of(Action::ReceiptReprint), Some(order.cast()))?;
        let now = self.clock.now();
        let mut effects = Vec::with_capacity(originals.len());
        let mut last = None;
        for (doc, cause) in &originals {
            let doc = self.as_reprint(doc)?;
            let job: Id<Job> = self.ids.generate(now)?;
            effects.push(print::effect(job, JobKind::Receipt, &doc, Some(*cause))?);
            last = Some(job);
        }
        let job = last.ok_or(RuntimeError::NoReceipt(order))?;
        // An approval is an event, so it waits for a settled log as every write does.
        let meta = if granted.is_some() { Some(self.meta()?) } else { None };
        let stream = self.approval_stream(granted.as_ref())?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            if let Some(meta) = &meta {
                approved(w, granted.as_ref(), stream, meta)?;
            }
            for effect in &effects {
                w.enqueue(effect, now)?;
            }
            Ok(())
        })?;
        self.spend(granted.as_ref());
        Ok(job)
    }

    /// `doc` marked as a reprint above and below, unless it is already.
    fn as_reprint(&self, doc: &Doc) -> Result<Doc, RuntimeError> {
        if is_reprint(doc) {
            return Ok(doc.clone());
        }
        let banner = receipt::reprint_banner(self.locale);
        let mut after = doc.blocks().to_vec();
        // The banner goes above the feed and cut that end a receipt.
        let tail: Vec<Block> =
            match after.iter().rposition(|block| !matches!(block, Block::Feed(_) | Block::Cut)) {
                Some(last) => after.split_off(last.saturating_add(1)),
                None => Vec::new(),
            };
        let mut blocks = vec![banner.clone()];
        blocks.extend(after);
        blocks.push(banner);
        blocks.extend(tail);
        Ok(Doc::new(blocks)?)
    }
}

/// Whether `doc` is a reprint, in whatever language it was printed.
fn is_reprint(doc: &Doc) -> bool {
    doc.blocks().first().is_some_and(receipt::is_banner)
}

/// Fails unless `order` was created.
pub(crate) fn known(order: &Order) -> Result<(), RuntimeError> {
    order.info().map(|_| ()).ok_or(RuntimeError::UnknownOrder(order.id()))
}

/// The ticket of `order`, whose payments are `payments`, as the write `w` leaves it. An intent
/// makes its ticket inside its write, so that one whose ticket can't be made, its order no longer
/// priced without overflowing, is refused whole rather than done with no answer.
pub(crate) fn view<S: Signer, E: Entropy>(
    w: &Writing<'_, S, E>,
    order: Id<Order>,
    payments: &[Payment],
    profile: &Profile,
    locale: Locale,
) -> Result<Ticket, RuntimeError> {
    ticket::ticket(&w.load(Order::new(order))?, payments, profile, locale)
}

/// Appends `event` to the stream `stream` of its kind, with what every event of the intent
/// records, and answers with its identifier.
pub(crate) fn append<D: DomainEvent, S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    stream: Id<AggregateMarker>,
    event: &D,
    meta: &Meta,
) -> Result<Id<keel_events::envelope::Event>, RuntimeError> {
    let (schema, payload) = event.encode()?;
    let kind = StreamKind::new(D::STREAM)?;
    let draft = EventDraft {
        stream: StreamRef { kind, id: stream },
        schema,
        business_date: meta.business_date,
        actor: meta.actor.clone(),
        approval: meta.approval,
        causation: None,
        correlation: Some(meta.correlation),
        payload,
    };
    Ok(w.append(draft, meta.now)?.body().event_id)
}

/// The open checks of `order` a change to `line` touches, or to the whole order when it is
/// `None`.
fn touched(order: &Order, line: Option<Id<keel_domain::order::Line>>) -> Vec<Id<Check>> {
    match line.and_then(|line| order.line(line)) {
        Some(line) => line.allocation().iter().map(|share| share.check).collect(),
        None => order.checks().iter().map(Check::id).collect(),
    }
}

/// Whether `order`, active, is done: it has a closed check, and every check that holds a live
/// line is closed.
fn finished(order: &Order) -> bool {
    *order.status() == keel_domain::order::OrderStatus::Active
        && order.live_lines().next().is_some()
        && order.checks().iter().any(|check| !check.is_open())
        && order
            .checks()
            .iter()
            .all(|check| !check.is_open() || order.lines_on(check.id()).next().is_none())
}

/// The receipt of a check just closed, to print: its job, the check, and who closed it when.
struct CheckReceipt<'a> {
    job: Id<Job>,
    check: Id<Check>,
    cashier: Option<&'a keel_domain::codec::Name>,
    at: keel_types::LocalTime,
}

/// Enqueues the receipt of `receipt.check`, which `closing` closed: the check's own, for a split
/// order, with its number and its payments, or the order's. Nothing about it can refuse the
/// sale: one that can't be made whole is made short.
#[expect(clippy::too_many_arguments, reason = "a write's parts, gathered before it")]
fn enqueue_receipt<S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    ticket: &Ticket,
    paid: &[Payment],
    receipt: &CheckReceipt<'_>,
    closing: Id<keel_events::envelope::Event>,
    profile: &Profile,
    locale: Locale,
    now: Timestamp,
) -> Result<(), RuntimeError> {
    let check = receipt.check;
    let split = ticket.checks.iter().find(|known| known.check == check);
    let (shown, number) = split.map_or((ticket, None), |known| (&known.ticket, Some(known.number)));
    let on_check: Vec<Payment> = paid
        .iter()
        .filter(|payment| payment.info().is_some_and(|info| info.check == check))
        .cloned()
        .collect();
    let closing_info = Closing {
        payments: &on_check,
        cashier: receipt.cashier,
        at: receipt.at,
        reprint: false,
        check: number,
    };
    let doc = receipt::receipt(shown, &closing_info, profile, locale)
        .ok()
        .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)
        .map_or_else(|| receipt::short(shown, &closing_info, profile, locale), Ok)?;
    w.enqueue(&print::effect(receipt.job, JobKind::Receipt, &doc, Some(closing))?, now)?;
    Ok(())
}
