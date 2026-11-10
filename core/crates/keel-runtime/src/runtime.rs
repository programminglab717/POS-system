//! The runtime: a device's store, its location's profile, and the intents and views over them.

use core::num::NonZeroU32;
use core::time::Duration;
use std::path::Path;

use keel_domain::checkout::Checkout;
use keel_domain::codec::ProfileVersion;
use keel_domain::order::{
    Channel, Check, LineAdded, LineChanged, Mode, Order, OrderCommand, OrderCreated,
};
use keel_domain::payment::{CashTendered, Payment, PaymentCaptured, PaymentCommand, Tender};
use keel_domain::profile::{Choice, Member, Profile};
use keel_domain::refs::Variant;
use keel_domain::schema::DomainEvent;
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
use crate::ticket::{self, Cash, cash_due};
use crate::views::{
    Amount, CashPaid, ItemView, MenuButton, MenuPage, MenuView, Ticket, group_views,
};

/// The most amounts [`Runtime::cash_offers`] offers.
pub const CASH_OFFERS: usize = 5;
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
    locale: Locale,
    pub(crate) member: Option<Id<TeamMember>>,
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
pub(crate) struct Meta {
    pub(crate) now: Timestamp,
    pub(crate) business_date: BusinessDate,
    pub(crate) actor: Actor,
    pub(crate) correlation: Id<Correlation>,
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

    /// Signs `member` in: the events of every intent from now on record them.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownMember`] if the profile's team doesn't have them.
    pub fn sign_in(&mut self, member: Id<TeamMember>) -> Result<(), RuntimeError> {
        self.profile.member(member).ok_or(RuntimeError::UnknownMember(member))?;
        self.member = Some(member);
        Ok(())
    }

    /// Signs the team member out: intents are refused until another signs in.
    pub const fn sign_out(&mut self) {
        self.member = None;
    }

    /// The team member signed in.
    pub const fn signed_in(&self) -> Option<Id<TeamMember>> {
        self.member
    }

    /// What the events of an intent made now record: the business date, the team member signed
    /// in, and a correlation of their own.
    fn meta(&mut self) -> Result<Meta, RuntimeError> {
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
        let meta = self.meta()?;
        let change = LineChanged {
            quantity: Some(Quantity::from_whole(i64::from(quantity.get()), Unit::Each)?),
            ..LineChanged::to(line)
        };
        self.decide(order, OrderCommand::ChangeLine(change), &meta)
    }

    /// Takes `line` off `order`.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses.
    pub fn remove_line(
        &mut self,
        order: Id<Order>,
        line: Id<keel_domain::order::Line>,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        self.decide(order, OrderCommand::RemoveLine(line), &meta)
    }

    /// Drops `order`: takes off every line it has, then abandons it, in one write.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: it isn't open, or a line
    /// was sent to be prepared.
    pub fn abandon(&mut self, order: Id<Order>) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            let mut loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let live: Vec<_> = loaded.live_lines().map(keel_domain::order::Line::id).collect();
            for line in live {
                let event = loaded.decide(location, device, OrderCommand::RemoveLine(line))?;
                append(w, order.cast(), &event, &meta)?;
                loaded = w.load(Order::new(order))?;
            }
            let event = loaded.decide(location, device, OrderCommand::Abandon)?;
            append(w, order.cast(), &event, &meta)?;
            view(w, order, &payments, profile, locale)
        })
    }

    /// Takes `tendered` in cash for what `order` owes: the payment is initiated and captured,
    /// with the location's cash rounding, and the order's check and the order closed, all in
    /// one write. Nothing owed, the order closes with no payment. Answers with the change and the
    /// order's ticket.
    ///
    /// # Errors
    /// [`RuntimeError::TenderShort`] if `tendered` is less than what is due in cash,
    /// [`RuntimeError::WrongCurrency`] or [`RuntimeError::NegativeTender`] if it isn't cash at
    /// the location, and [`RuntimeError`] if no one is signed in or the order can't be paid as it
    /// stands.
    pub fn pay_cash(
        &mut self,
        order: Id<Order>,
        tendered: Money,
    ) -> Result<CashPaid, RuntimeError> {
        let meta = self.meta()?;
        if tendered.currency() != self.profile.data().currency {
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
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let check: Id<Check> = order.cast();
            let rules = &profile.data().rules;
            let checkout = Checkout::new(&loaded, payments.iter(), rules, profile.rules_version());
            let due = checkout.balance(check)?.due;
            let cash = if due.is_positive() { cash_due(profile, due)? } else { Cash::none(due) };
            if tendered.compare(cash.due)?.is_lt() {
                return Err(RuntimeError::TenderShort { due: cash.due });
            }
            let change = tendered.checked_sub(cash.due)?;
            let mut paid = payments.clone();
            if due.is_positive() {
                let started =
                    checkout.start_payment(location, device, payment, check, Tender::Cash, due)?;
                append(w, payment.cast(), &started, &meta)?;
                let captured = PaymentCaptured {
                    amount: due,
                    tip: None,
                    reference: None,
                    cash: Some(CashTendered {
                        tendered,
                        rounding: (!cash.rounding.is_zero()).then_some(cash.rounding),
                    }),
                };
                let paying = w.load(Payment::new(payment))?;
                let event = paying.decide(location, PaymentCommand::Capture(captured))?;
                append(w, payment.cast(), &event, &meta)?;
                paid.push(w.load(Payment::new(payment))?);
            }
            let checkout = Checkout::new(&loaded, paid.iter(), rules, profile.rules_version());
            let closed = checkout.close_check(location, device, check)?;
            append(w, order.cast(), &closed, &meta)?;
            let loaded = w.load(Order::new(order))?;
            let event = loaded.decide(location, device, OrderCommand::Close)?;
            let closing = append(w, order.cast(), &event, &meta)?;
            let ticket = ticket::ticket(&w.load(Order::new(order))?, &paid, profile, locale)?;
            // The drawer opens when cash changed hands, then the receipt prints: one job after
            // the other, in the order they are enqueued.
            if due.is_positive() {
                let kick = Doc::new(vec![Block::DrawerKick])?;
                w.enqueue(
                    &print::effect(drawer, JobKind::Drawer, &kick, Some(closing))?,
                    meta.now,
                )?;
            }
            let closing_info = Closing {
                payments: &paid,
                cashier: cashier.as_ref(),
                at: closed_at,
                reprint: false,
            };
            // Nothing about the receipt can refuse the sale: one that can't be made whole is made
            // short.
            let doc = receipt::receipt(&ticket, &closing_info, profile, locale)
                .ok()
                .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)
                .map_or_else(|| receipt::short(&ticket, &closing_info, profile, locale), Ok)?;
            w.enqueue(
                &print::effect(receipt_job, JobKind::Receipt, &doc, Some(closing))?,
                meta.now,
            )?;
            Ok(CashPaid { change: Amount::new(change, locale), ticket })
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
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let event = loaded.decide(location, device, command)?;
            append(w, order.cast(), &event, meta)?;
            view(w, order, &payments, profile, locale)
        })
    }

    fn location(&self) -> Id<keel_events::envelope::Location> {
        self.profile.data().location
    }

    /// The payments of `order` the store holds. An intent reads them just before its write, as a
    /// write can't list them: the runtime is its store's one writer, so nothing comes between.
    fn payments(&self, order: Id<Order>) -> Result<Vec<Payment>, RuntimeError> {
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

    /// The menu, as the register shows it.
    pub fn menu(&self) -> MenuView {
        let profile = &self.profile;
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
                        })
                    })
                    .collect(),
            })
            .collect();
        MenuView { pages }
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
            groups: group_views(profile, &item.groups, self.locale),
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
        let Some(printer) = self.printer else { return Ok(None) };
        if self.sending.is_some() {
            return Ok(None);
        }
        let now = self.clock.now();
        let latest = print::after(now, print::MAX_WAIT)?;
        let mut pending: Vec<_> = self
            .store
            .unfinished_effects()?
            .into_iter()
            .enumerate()
            .filter(|(_, queued)| queued.state == keel_store::EffectState::Pending)
            .filter(|(_, queued)| queued.due <= now || queued.due > latest)
            .filter(|(_, queued)| print::job_of(queued).is_some())
            .collect();
        // The earliest due first, then the earliest enqueued; those the clock left behind, now.
        pending.sort_by_key(|(seq, queued)| (queued.due.min(now), *seq));
        for (_, queued) in pending {
            let Some((job, kind)) = print::job_of(&queued) else { continue };
            let key = queued.effect.key.clone();
            if queued.due > now {
                self.store.write(|w| w.reschedule(&key, now))?;
            }
            let Ok(doc) = Doc::decode(&queued.effect.payload) else {
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
                let doc = Doc::decode(&queued.effect.payload)?;
                let doc = match kind {
                    JobKind::Receipt => self.as_reprint(&doc)?,
                    JobKind::Drawer => doc,
                };
                let now = self.clock.now();
                let again: Id<Job> = self.ids.generate(now)?;
                let effect = print::effect(again, kind, &doc, queued.effect.cause)?;
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
    /// above and below.
    ///
    /// # Errors
    /// [`RuntimeError::NoReceipt`] if the order printed no receipt, and [`RuntimeError::Store`]
    /// if the store can't be read or written.
    pub fn reprint(&mut self, order: Id<Order>) -> Result<Id<Job>, RuntimeError> {
        let mut original = None;
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
            if !is_reprint(&doc) {
                original = Some((doc, cause));
            }
        }
        let (doc, cause) = original.ok_or(RuntimeError::NoReceipt(order))?;
        let doc = self.as_reprint(&doc)?;
        let now = self.clock.now();
        let job: Id<Job> = self.ids.generate(now)?;
        let effect = print::effect(job, JobKind::Receipt, &doc, Some(cause))?;
        self.store.write(|w| w.enqueue(&effect, now))?;
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
fn known(order: &Order) -> Result<(), RuntimeError> {
    order.info().map(|_| ()).ok_or(RuntimeError::UnknownOrder(order.id()))
}

/// The ticket of `order`, whose payments are `payments`, as the write `w` leaves it. An intent
/// makes its ticket inside its write, so that one whose ticket can't be made, its order no longer
/// priced without overflowing, is refused whole rather than done with no answer.
fn view<S: Signer, E: Entropy>(
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
        approval: None,
        causation: None,
        correlation: Some(meta.correlation),
        payload,
    };
    Ok(w.append(draft, meta.now)?.body().event_id)
}
