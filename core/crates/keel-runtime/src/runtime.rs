//! The runtime: a device's store, its location's profile, and the intents and views over them.

use core::num::NonZeroU32;
use core::time::Duration;
use std::path::Path;

use keel_domain::checkout::Checkout;
use keel_domain::order::{
    Channel, Check, LineAdded, LineChanged, Mode, Order, OrderCommand, OrderCreated,
};
use keel_domain::payment::{CashTendered, Payment, PaymentCaptured, PaymentCommand, Tender};
use keel_domain::profile::{Choice, Profile};
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

use crate::error::RuntimeError;
use crate::ticket::{self, Cash, cash_due};
use crate::views::{
    Amount, CashPaid, ItemView, MenuButton, MenuPage, MenuView, Ticket, group_views,
};

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
    store: Store<S, E>,
    profile: Profile,
    clock: C,
    ids: IdGenerator<I>,
    device: Id<Device>,
    locale: Locale,
    member: Option<Id<TeamMember>>,
}

/// What every event of one intent records besides its payload.
struct Meta {
    now: Timestamp,
    business_date: BusinessDate,
    actor: Actor,
    correlation: Id<Correlation>,
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
        let store =
            Store::open_with_faults(path, key, config, identity.signer, entropy, Box::new(faults))?;
        Ok(Runtime {
            store,
            profile,
            clock,
            ids: IdGenerator::new(ids),
            device: identity.device,
            locale,
            member: None,
        })
    }

    /// The location's profile.
    pub const fn profile(&self) -> &Profile {
        &self.profile
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
            append(w, order.cast(), &event, &meta)?;
            let ticket = ticket::ticket(&w.load(Order::new(order))?, &paid, profile, locale)?;
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
/// records.
fn append<D: DomainEvent, S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    stream: Id<AggregateMarker>,
    event: &D,
    meta: &Meta,
) -> Result<(), RuntimeError> {
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
    w.append(draft, meta.now).map(|_| ()).map_err(RuntimeError::from)
}
