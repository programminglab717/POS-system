//! Card payments at the register (ADR-0035): a sale through the device's terminal, as an effect
//! in the store's outbox that the shell's connector executes, and a card refund likewise.
//!
//! - *Card* initiates a payment and enqueues its sale, in one write
//!   ([`Runtime::pay_check_card`]). A tip is the location's setting: offered on the register's
//!   screen ([`Runtime::tip_offers`]) or asked for by the terminal.
//! - The shell takes one request at a time ([`Runtime::next_card_request`]). A request's start
//!   commits before the terminal is asked. The first start of a sale or a refund performs it;
//!   every later one asks about it: an unknown outcome, or a request found started when the
//!   register opens, is never sent again, so a card is never charged twice for one payment.
//! - [`Runtime::card_outcome`] records what the terminal answered, in one write: a sale approved
//!   is captured, and settles its check as a cash payment does; one approved in part pays the
//!   check first and the tip after, and leaves the rest due; one declined fails; one no one knows
//!   is asked about after 5 s, doubling up to 60 s, and blocks its check meanwhile.
//! - A payment no terminal can answer for is settled by hand, on a manager's word
//!   ([`Runtime::settle_card`], `payment.resolve`).
//! - A card refund goes back, by reference, to the card payment a return names; one that fails
//!   stays owed, and is tried again on a manager's word ([`Runtime::retry_refund`]).

use core::time::Duration;

use keel_doc::Doc;
use keel_domain::checkout::Checkout;
use keel_domain::codec::{Note, ProcessorRef, ReasonCode};
use keel_domain::order::{Check, Order, Reason};
use keel_domain::payment::{
    Card, Payment, PaymentCaptured, PaymentCommand, PaymentEnded, PaymentEvent, Tender,
};
use keel_domain::profile::TipPrompt;
use keel_domain::refund::{Refund, RefundCommand, RefundCompleted, RefundFailed};
use keel_domain::staff::{Action, Granted, Request};
use keel_events::cbor::{Map, Value};
use keel_events::envelope::Event;
use keel_events::keys::Signer;
use keel_store::{Effect, EffectKind, EffectState, Queued};
use keel_types::{Clock, Currency, Entropy, Id, Money};

use crate::error::RuntimeError;
use crate::print::{self, Job, JobKind};
use crate::runtime::{CheckReceipt, Meta, Runtime, Settle, append, known, settle};
use crate::views::{Amount, Ticket};

/// The kind of a card sale's effect.
pub const SALE: &str = "card.sale";

/// The kind of a card refund's effect.
pub const REFUND: &str = "card.refund";

/// How long after an unknown outcome the terminal is first asked about it.
pub const FIRST_ASK: Duration = Duration::from_secs(5);

/// The longest the register waits between two questions about an unknown outcome.
pub const LONGEST_ASK: Duration = Duration::from_secs(60);

/// The system's component that records what a terminal answered.
const TERMINAL: &str = "terminal";

/// The reason a sale or a refund fails when the terminal never took it.
const NOT_RECEIVED: &str = "not_received";

/// The reason a payment fails when a manager says the card wasn't charged.
const NOT_CHARGED: &str = "not_charged";

/// The tip a card payment carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardTip {
    /// None.
    None,
    /// An amount chosen on the register's screen, before the card is presented.
    Amount(Money),
    /// Whatever the customer chooses on the terminal.
    Terminal,
}

/// What the register asks a terminal to do, or asks it about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRequest {
    /// The idempotency key the terminal or processor sees: the payment's identifier for a sale,
    /// the refund's and its attempt's for a refund.
    pub key: String,
    /// What it is about.
    pub ask: CardAsk,
    /// Whether the request asks what became of it, rather than doing it: it was started before,
    /// and its outcome isn't known.
    pub status: bool,
}

/// What a card request is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAsk {
    /// A sale: authorized and captured at once.
    Sale {
        /// The payment.
        payment: Id<Payment>,
        /// What it pays toward its check.
        amount: Money,
        /// The tip chosen on the register's screen, on top.
        tip: Option<Money>,
        /// The tip presets the terminal offers, in basis points, when it asks for the tip; empty
        /// when it doesn't.
        presets: Vec<u16>,
        /// Whether the terminal may approve less than asked.
        partial: bool,
    },
    /// A refund, by reference, to the card that paid.
    Refund {
        /// The refund.
        refund: Id<Refund>,
        /// Which attempt at it this is, from 1.
        attempt: u32,
        /// What it refunds.
        amount: Money,
        /// The processor's reference for the card payment it goes back to.
        reference: ProcessorRef,
    },
}

/// What a terminal answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardOutcome {
    /// Approved: `approved` charged, a tip included, or refunded; less than asked for a partial
    /// approval.
    Approved {
        /// What was charged or refunded: more than zero.
        approved: Money,
        /// The processor's reference.
        reference: Option<ProcessorRef>,
        /// The card.
        card: Option<Card>,
    },
    /// Declined: nothing moved.
    Declined {
        /// Why, such as `declined` or `cancelled`.
        reason: ReasonCode,
        /// What the terminal said.
        note: Option<Note>,
        /// The processor's reference.
        reference: Option<ProcessorRef>,
    },
    /// To a status query: the terminal never took the request.
    NotFound,
    /// No one knows: the terminal is to be asked again.
    Unknown {
        /// When to ask again, if the terminal said.
        retry_after: Option<Duration>,
    },
}

/// The tips the register offers for a card payment (ADR-0035, decision 5; H-10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipOffers {
    /// Where the tip is asked for; `None` when the location takes no tips.
    pub prompt: Option<TipPrompt>,
    /// What the presets are a percentage of: the payment's share of its check's pre-tax
    /// subtotal.
    pub base: Amount,
    /// Each preset, as a percentage and an amount.
    pub presets: Vec<TipOffer>,
}

/// One tip preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipOffer {
    /// The percentage, in basis points.
    pub percent: u16,
    /// What it comes to.
    pub amount: Amount,
}

/// Where a card payment of an order is, as the register shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardPaymentView {
    /// The payment.
    pub payment: Id<Payment>,
    /// The check it pays.
    pub check: Id<Check>,
    /// What it asked to pay toward the check.
    pub amount: Amount,
    /// Where it is.
    pub state: CardState,
    /// The card, as a receipt shows it, such as "Visa •••• 4242".
    pub card: Option<String>,
}

/// Where a card payment is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardState {
    /// Waiting for the terminal.
    Waiting,
    /// Its outcome isn't known: the terminal is being asked about it.
    Checking,
    /// Waiting for another device's terminal, which alone can answer for it.
    Elsewhere,
    /// Approved, for what it applied and the tip on top.
    Approved {
        /// What it applied to the check.
        applied: Amount,
        /// The tip on top.
        tip: Option<Amount>,
        /// Whether a manager settled it by hand.
        by_hand: bool,
    },
    /// Declined, or settled by hand as not charged.
    Declined {
        /// Why.
        reason: String,
    },
}

/// A card refund this register owes, as it shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRefundView {
    /// The refund.
    pub refund: Id<Refund>,
    /// The order whose return it pays.
    pub order: Id<Order>,
    /// What it refunds.
    pub amount: Amount,
    /// Whether the terminal has it now, or is being asked about it.
    pub pending: bool,
    /// How many attempts failed.
    pub failures: u32,
    /// Why the latest failed, if one did.
    pub failure: Option<String>,
}

/// The effect of `payment`'s sale: its key is the payment's identifier, and its payload
/// `{1: 0, 2: payment, 3: amount, 4: tip, 5: presets, 6: partial}`, in canonical CBOR, the tip
/// and presets absent when there are none.
pub(crate) fn sale_effect(
    payment: Id<Payment>,
    amount: Money,
    tip: Option<Money>,
    presets: &[u16],
    partial: bool,
    cause: Id<Event>,
) -> Result<Effect, RuntimeError> {
    let mut entries = vec![
        (Value::Unsigned(1), Value::Unsigned(0)),
        (Value::Unsigned(2), Value::Bytes(payment.to_bytes().to_vec())),
        (Value::Unsigned(3), money_value(amount)),
        (Value::Unsigned(6), Value::Bool(partial)),
    ];
    entries.extend(tip.map(|tip| (Value::Unsigned(4), money_value(tip))));
    if !presets.is_empty() {
        let presets = presets.iter().map(|preset| Value::Unsigned(u64::from(*preset))).collect();
        entries.push((Value::Unsigned(5), Value::Array(presets)));
    }
    effect(payment.to_bytes().to_vec(), SALE, entries, Some(cause))
}

/// The effect of `refund`'s `attempt`: its key is the refund's identifier and the attempt, and
/// its payload `{1: 1, 2: refund, 3: amount, 4: reference, 5: attempt, 6: receipt}`, the receipt
/// it prints once paid back absent when it has none.
pub(crate) fn refund_effect(
    refund: Id<Refund>,
    attempt: u32,
    amount: Money,
    reference: &ProcessorRef,
    receipt: Option<&Doc>,
    cause: Option<Id<Event>>,
) -> Result<Effect, RuntimeError> {
    let mut entries = vec![
        (Value::Unsigned(1), Value::Unsigned(1)),
        (Value::Unsigned(2), Value::Bytes(refund.to_bytes().to_vec())),
        (Value::Unsigned(3), money_value(amount)),
        (Value::Unsigned(4), Value::from(reference.as_str())),
        (Value::Unsigned(5), Value::Unsigned(u64::from(attempt))),
    ];
    entries.extend(receipt.map(|doc| (Value::Unsigned(6), Value::Bytes(doc.encode()))));
    effect(refund_key(refund, attempt), REFUND, entries, cause)
}

fn effect(
    key: Vec<u8>,
    kind: &str,
    entries: Vec<(Value, Value)>,
    cause: Option<Id<Event>>,
) -> Result<Effect, RuntimeError> {
    let payload = Value::Map(Map::from_entries(entries).map_err(|_| RuntimeError::Time)?);
    Ok(Effect { key, kind: EffectKind::new(kind)?, payload: payload.encode(), cause })
}

/// The key of `refund`'s `attempt`: the refund's identifier, then the attempt, big-endian.
fn refund_key(refund: Id<Refund>, attempt: u32) -> Vec<u8> {
    let mut key = refund.to_bytes().to_vec();
    key.extend_from_slice(&attempt.to_be_bytes());
    key
}

fn money_value(money: Money) -> Value {
    Value::Array(vec![Value::from(money.minor()), Value::from(money.currency().code())])
}

fn money_of(value: &Value) -> Option<Money> {
    let [minor, currency] = value.as_array()? else { return None };
    Some(Money::from_minor(minor.as_i64()?, Currency::from_code(currency.as_text()?).ok()?))
}

/// A card effect, read back: what it asks, and the receipt a refund prints once paid back.
struct Read {
    ask: CardAsk,
    receipt: Option<Doc>,
}

/// Whether `queued` is a card effect.
pub(crate) fn is_card(queued: &Queued) -> bool {
    matches!(queued.effect.kind.as_str(), SALE | REFUND)
}

fn read(queued: &Queued) -> Option<Read> {
    let value = keel_events::cbor::decode(&queued.effect.payload).ok()?;
    let map = value.as_map()?;
    let get = |key: u64| map.get(&Value::Unsigned(key));
    let id = |value: Option<&Value>| -> Option<[u8; 16]> { value?.as_bytes()?.try_into().ok() };
    let amount = money_of(get(3)?)?;
    match (queued.effect.kind.as_str(), get(1)?.as_u64()?) {
        (SALE, 0) => {
            let presets = match get(5) {
                None => Vec::new(),
                Some(presets) => presets
                    .as_array()?
                    .iter()
                    .map(|preset| u16::try_from(preset.as_u64()?).ok())
                    .collect::<Option<_>>()?,
            };
            let ask = CardAsk::Sale {
                payment: Id::from_bytes(id(get(2))?).ok()?,
                amount,
                tip: get(4).map(money_of).map_or(Some(None), |tip| tip.map(Some))?,
                presets,
                partial: get(6)?.as_bool()?,
            };
            Some(Read { ask, receipt: None })
        }
        (REFUND, 1) => {
            let ask = CardAsk::Refund {
                refund: Id::from_bytes(id(get(2))?).ok()?,
                attempt: u32::try_from(get(5)?.as_u64()?).ok()?,
                amount,
                reference: ProcessorRef::new(get(4)?.as_text()?).ok()?,
            };
            let receipt = match get(6) {
                None => None,
                Some(doc) => Some(Doc::decode(doc.as_bytes()?).ok()?),
            };
            Some(Read { ask, receipt })
        }
        _ => None,
    }
}

/// The idempotency key of what `ask` is about.
fn key_of(ask: &CardAsk) -> String {
    match ask {
        CardAsk::Sale { payment, .. } => payment.to_string(),
        CardAsk::Refund { refund, attempt, .. } => format!("{refund}-{attempt}"),
    }
}

/// How long to wait before asking a terminal again about a request started `attempts` times:
/// [`FIRST_ASK`], doubling with each attempt, up to [`LONGEST_ASK`].
fn ask_after(attempts: u32) -> Duration {
    let doublings = attempts.saturating_sub(1).min(8);
    FIRST_ASK.saturating_mul(1_u32 << doublings).min(LONGEST_ASK)
}

/// `amount × numerator ÷ denominator`, rounded half up, for amounts and a denominator more than
/// zero; `None` if it overflows, or the denominator isn't more than zero.
fn share(amount: Money, numerator: i64, denominator: i64) -> Option<Money> {
    if denominator <= 0 {
        return None;
    }
    let product = i128::from(amount.minor()).checked_mul(i128::from(numerator))?;
    let denominator = i128::from(denominator);
    let rounded = product.checked_add(denominator / 2)?.checked_div(denominator)?;
    Some(Money::from_minor(i64::try_from(rounded).ok()?, amount.currency()))
}

/// The payment a sale's effect key names.
fn payment_of_key(key: &[u8]) -> Result<Id<Payment>, RuntimeError> {
    let bytes: [u8; 16] = key.try_into().map_err(|_| RuntimeError::UnknownCardRequest)?;
    Ok(Id::from_bytes(bytes)?)
}

/// How a card reads on a receipt and a screen: "Visa •••• 4242".
pub(crate) fn card_text(card: &Card) -> String {
    format!("{} •••• {}", card.brand, card.last_four)
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// The tips the register offers for paying `part` of `check` of `order` by card, or all it
    /// owes when `part` is `None`: each of the location's presets as a percentage of the
    /// payment's share of the check's pre-tax subtotal, `amount paid × net ÷ total`, rounded half
    /// up, so a check paid at once tips on its net (H-10). With no tipping, there are none.
    ///
    /// # Errors
    /// [`RuntimeError`] if the order or the check is unknown, or can't be priced.
    pub fn tip_offers(
        &self,
        order: Id<Order>,
        check: Id<Check>,
        part: Option<Money>,
    ) -> Result<TipOffers, RuntimeError> {
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        let payments = self.payments(order)?;
        let ticket = crate::ticket::ticket(&loaded, &payments, &self.profile, self.locale)?;
        let shown = ticket
            .checks
            .iter()
            .find(|known| known.check == check)
            .map_or(&ticket, |known| &known.ticket);
        let rules = &self.profile.data().rules;
        let checkout = Checkout::new(&loaded, payments.iter(), rules, self.profile.rules_version());
        let due = checkout.balance(check)?.due;
        let paying = part.unwrap_or(due);
        let base = share(paying, shown.subtotal.money.minor(), shown.total.money.minor())
            .unwrap_or(Money::zero(paying.currency()));
        let tipping = self.profile.data().tipping.as_ref();
        let presets = tipping
            .map(|tipping| {
                tipping
                    .presets
                    .iter()
                    .map(|preset| {
                        let amount = share(base, i64::from(preset.get()), 10_000)
                            .ok_or(keel_types::MoneyError::Overflow)?;
                        Ok(TipOffer {
                            percent: preset.get(),
                            amount: Amount::new(amount, self.locale),
                        })
                    })
                    .collect::<Result<Vec<_>, RuntimeError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(TipOffers {
            prompt: tipping.map(|tipping| tipping.prompt),
            base: Amount::new(base, self.locale),
            presets,
        })
    }

    /// Pays what `order` owes by card: [`Runtime::pay_check_card`] for all the one check that
    /// holds something owes.
    ///
    /// # Errors
    /// [`RuntimeError::Split`] if more than one open check holds something; and as
    /// [`Runtime::pay_check_card`].
    pub fn pay_card(
        &mut self,
        order: Id<Order>,
        tip: CardTip,
        partial: bool,
    ) -> Result<Ticket, RuntimeError> {
        let check = self.holding_check(order)?;
        self.pay_check_card(order, check, None, tip, partial)
    }

    /// Starts paying `part` of `check` of `order` by card, or all it owes when `part` is `None`,
    /// with `tip` on top, approved in part if `partial`: the payment is initiated and its sale
    /// enqueued, in one write, for the shell to take to the terminal
    /// ([`Runtime::next_card_request`]). Until it has an outcome, the check takes no other
    /// payment. Answers with the order's ticket.
    ///
    /// # Errors
    /// [`RuntimeError::NoTips`] for a tip the location doesn't take, or one asked of the terminal
    /// where the register asks; [`RuntimeError::TipTooLarge`] for a tip of nothing, in another
    /// currency, or more than what it tips on; [`RuntimeError::WrongCurrency`] for a part in
    /// another currency; and [`RuntimeError`] if no one is signed in, or the check can't take
    /// the payment.
    pub fn pay_check_card(
        &mut self,
        order: Id<Order>,
        check: Id<Check>,
        part: Option<Money>,
        tip: CardTip,
        partial: bool,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let currency = self.profile.data().currency;
        if part.is_some_and(|part| part.currency() != currency) {
            return Err(RuntimeError::WrongCurrency);
        }
        let tipping = self.profile.data().tipping.as_ref();
        let (tip, presets) = match tip {
            CardTip::None => (None, Vec::new()),
            CardTip::Amount(_) if tipping.is_none() => return Err(RuntimeError::NoTips),
            CardTip::Amount(amount) => (Some(amount), Vec::new()),
            CardTip::Terminal => match tipping {
                Some(tipping) if tipping.prompt == TipPrompt::Terminal => {
                    (None, tipping.presets.iter().map(|preset| preset.get()).collect())
                }
                _ => return Err(RuntimeError::NoTips),
            },
        };
        let payment: Id<Payment> = self.ids.generate(meta.now)?;
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            let loaded = w.load(Order::new(order))?;
            known(&loaded)?;
            let rules = &profile.data().rules;
            let checkout = Checkout::new(&loaded, payments.iter(), rules, profile.rules_version());
            let amount = part.unwrap_or(checkout.balance(check)?.due);
            // A tip is more than nothing, and no more than what it tips on: a guard on a slip.
            if tip.is_some_and(|tip| {
                tip.currency() != currency
                    || !tip.is_positive()
                    || tip.compare(amount).is_ok_and(core::cmp::Ordering::is_gt)
            }) {
                return Err(RuntimeError::TipTooLarge);
            }
            let started =
                checkout.start_payment(location, device, payment, check, Tender::Card, amount)?;
            let cause = append(w, payment.cast(), &started, &meta)?;
            w.enqueue(&sale_effect(payment, amount, tip, &presets, partial, cause)?, meta.now)?;
            let mut paid = payments.clone();
            paid.push(w.load(Payment::new(payment))?);
            crate::runtime::view(w, order, &paid, profile, locale)
        })
    }

    /// The next card request for the shell to take to the terminal, if one is due and none is
    /// with the shell: a sale or a refund to perform, the first time it starts; one to ask about,
    /// every time after, so nothing is sent twice. A request found started when the register
    /// opens, with no outcome, is asked about. Its start commits before it is handed over.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read or written.
    pub fn next_card_request(&mut self) -> Result<Option<CardRequest>, RuntimeError> {
        if self.charging.is_some() {
            return Ok(None);
        }
        let now = self.clock.now();
        // A request due far beyond any wait was scheduled by a clock since set back: it is due.
        let latest = print::after(now, LONGEST_ASK.saturating_mul(2))?;
        for queued in self.store.unfinished_effects()? {
            if !is_card(&queued) {
                continue;
            }
            let key = queued.effect.key.clone();
            // A payload this kernel can't read is no request: it is left for a manager to settle,
            // never failed, so its payment can still be settled. Found in review.
            if read(&queued).is_none() {
                continue;
            }
            match queued.state {
                // A first attempt goes at once, whatever a clock set back says: a customer waits.
                EffectState::Pending
                    if queued.due <= now || queued.due > latest || queued.attempts == 0 =>
                {
                    if queued.due > now {
                        self.store.write(|w| w.reschedule(&key, now))?;
                    }
                }
                // Started, with no outcome, and not with the shell: in doubt, so asked about.
                EffectState::Running => self.store.write(|w| w.retry(&key, now))?,
                _ => continue,
            }
            let started = self.store.write(|w| w.start(&key, now))?;
            let Some(read) = read(&started) else { continue };
            let request =
                CardRequest { key: key_of(&read.ask), ask: read.ask, status: started.attempts > 1 };
            self.charging = Some(key);
            return Ok(Some(request));
        }
        Ok(None)
    }

    /// Records what the terminal answered to the request with `key`, the one with the shell, in
    /// one write. A sale approved is captured, for what it asked at most, and the rest of what
    /// was approved as its tip, and settles its check; one declined, or not found, fails; a
    /// refund approved completes, and prints its receipt; one declined fails, and stays owed.
    /// No one knowing, the request is asked about later, and nothing is recorded.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownCardRequest`] unless the request with `key` is with the shell;
    /// [`RuntimeError::BadOutcome`] for an approval of nothing or in another currency, which
    /// leaves the request with the shell; [`RuntimeError`] if the outcome can't be recorded,
    /// which leaves the request to be asked about.
    pub fn card_outcome(&mut self, key: &str, outcome: CardOutcome) -> Result<(), RuntimeError> {
        let bytes = self.charging.clone().ok_or(RuntimeError::UnknownCardRequest)?;
        let queued = self.store.effect(&bytes)?.ok_or(RuntimeError::UnknownCardRequest)?;
        let read = read(&queued).ok_or(RuntimeError::UnknownCardRequest)?;
        if key_of(&read.ask) != key {
            return Err(RuntimeError::UnknownCardRequest);
        }
        let asked = match &read.ask {
            CardAsk::Sale { amount, .. } | CardAsk::Refund { amount, .. } => *amount,
        };
        // Whatever happens next, the request is no longer with the shell: if it isn't recorded,
        // it is in doubt, and asked about later. Found in review: an answer refused left it with
        // the shell, and every card request after it waited for good.
        self.charging = None;
        let bad = matches!(&outcome, CardOutcome::Approved { approved, .. }
            if !approved.is_positive() || approved.currency() != asked.currency());
        let wait = match &outcome {
            CardOutcome::Unknown { retry_after } => {
                Some(retry_after.unwrap_or_else(|| ask_after(queued.attempts)).min(LONGEST_ASK))
            }
            _ if bad => Some(ask_after(queued.attempts)),
            _ => None,
        };
        if let Some(wait) = wait {
            let due = print::after(self.clock.now(), wait)?;
            self.store.write(|w| w.retry(&bytes, due))?;
            return if bad { Err(RuntimeError::BadOutcome) } else { Ok(()) };
        }
        let recorded = match read.ask {
            CardAsk::Sale { payment, .. } => self.sale_outcome(&bytes, payment, outcome),
            CardAsk::Refund { refund, .. } => {
                self.refund_outcome(&bytes, refund, outcome, read.receipt.as_ref())
            }
        };
        // Not recorded, it is asked about after a wait, not at once. Found in review.
        if recorded.is_err()
            && let Ok(due) = print::after(self.clock.now(), ask_after(queued.attempts))
        {
            let _retried = self.store.write(|w| w.retry(&bytes, due));
        }
        recorded
    }

    /// Takes back the card request the shell holds, as when its terminal stopped answering: the
    /// request is in doubt, asked about next, never sent again (ADR-0035). Found in review: a
    /// shell that never reported an outcome held every card request after it.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be written.
    pub fn release_card_request(&mut self) -> Result<(), RuntimeError> {
        let Some(key) = self.charging.take() else { return Ok(()) };
        let due = print::after(self.clock.now(), FIRST_ASK)?;
        self.store.write(|w| w.retry(&key, due))?;
        Ok(())
    }

    fn sale_outcome(
        &mut self,
        key: &[u8],
        payment: Id<Payment>,
        outcome: CardOutcome,
    ) -> Result<(), RuntimeError> {
        let meta = self.system_meta(TERMINAL)?;
        let loaded = self.store.load(Payment::new(payment))?;
        let info = loaded.info().cloned().ok_or(RuntimeError::UnknownCardRequest)?;
        let command = match outcome {
            CardOutcome::Approved { approved, reference, card } => {
                // What was approved pays the check first, up to what was asked, and the tip
                // after: so a partial approval leaves the rest due, and a tip, chosen anywhere,
                // is what the approval holds beyond it.
                let applied =
                    if approved.compare(info.amount)?.is_gt() { info.amount } else { approved };
                let tip = approved.checked_sub(applied)?;
                let tip = tip.is_positive().then_some(tip);
                PaymentCommand::Capture(PaymentCaptured {
                    amount: applied,
                    tip,
                    reference,
                    cash: None,
                    card,
                })
            }
            CardOutcome::Declined { reason, note, reference } => {
                PaymentCommand::Fail(PaymentEnded {
                    reason: Reason { code: reason, note },
                    reference,
                })
            }
            CardOutcome::NotFound => PaymentCommand::Fail(PaymentEnded {
                reason: Reason { code: ReasonCode::new(NOT_RECEIVED)?, note: None },
                reference: None,
            }),
            CardOutcome::Unknown { .. } => return Err(RuntimeError::UnknownCardRequest),
        };
        self.record_payment(key, &info, command, &meta, None)
    }

    /// Records `command` on the card payment `info` describes, and finishes its effect with
    /// `key`, in one write, with the approval `approval` grants, if one does; a capture fires
    /// what of its check is still pending and settles the check.
    fn record_payment(
        &mut self,
        key: &[u8],
        info: &keel_domain::payment::PaymentInfo,
        command: PaymentCommand,
        meta: &Meta,
        approval: Option<(Granted, Id<keel_events::envelope::Aggregate>)>,
    ) -> Result<(), RuntimeError> {
        let (order, check) = (info.order, info.check);
        let captured = matches!(command, PaymentCommand::Capture(_));
        let payments = self.payments(order)?;
        let firing = if captured {
            let loaded = self.store.load(Order::new(order))?;
            self.plan_firing(&loaded, Some(check), meta.now)?
        } else {
            None
        };
        let receipt_job: Id<Job> = self.ids.generate(meta.now)?;
        let cashier = self.taker_name(info)?;
        let at = self.profile.business_day().local_time(meta.now)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        let id = payment_of_key(key)?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            let (granted, stream) =
                approval.map_or((None, None), |(granted, stream)| (Some(granted), Some(stream)));
            let meta = &crate::staff::approved(w, granted.as_ref(), stream, meta)?;
            let event = w.load(Payment::new(id))?.decide(location, command)?;
            append(w, id.cast(), &event, meta)?;
            w.finish(key)?;
            if !matches!(event, PaymentEvent::Captured(_)) {
                return Ok(());
            }
            if let Some(firing) = &firing {
                crate::kitchen::write_firing(w, firing, location, device, meta)?;
            }
            let mut paid: Vec<Payment> =
                payments.iter().filter(|known| known.id() != id).cloned().collect();
            paid.push(w.load(Payment::new(id))?);
            let closing = Settle {
                order,
                check,
                location,
                device,
                receipt: CheckReceipt { job: receipt_job, check, cashier: cashier.as_ref(), at },
            };
            settle(w, &closing, &paid, meta, profile, locale)?;
            Ok(())
        })
    }

    /// The name of the member who took the payment `info` describes, if a member did.
    fn taker_name(
        &self,
        info: &keel_domain::payment::PaymentInfo,
    ) -> Result<Option<keel_domain::codec::Name>, RuntimeError> {
        let Some(event) = self.store.event(info.initiated_by)? else { return Ok(None) };
        Ok(match event.body().actor {
            keel_events::envelope::Actor::TeamMember(member) => {
                self.profile.member(member).map(|member| member.name.clone())
            }
            _ => None,
        })
    }

    fn refund_outcome(
        &mut self,
        key: &[u8],
        refund: Id<Refund>,
        outcome: CardOutcome,
        receipt: Option<&Doc>,
    ) -> Result<(), RuntimeError> {
        let meta = self.system_meta(TERMINAL)?;
        let loaded = self.store.load(Refund::new(refund))?;
        let info = *loaded.info().ok_or(RuntimeError::UnknownCardRequest)?;
        let command = match outcome {
            CardOutcome::Approved { approved, .. } => {
                let amount =
                    if approved.compare(info.amount)?.is_gt() { info.amount } else { approved };
                RefundCommand::Complete(RefundCompleted { amount, rounding: None })
            }
            CardOutcome::Declined { reason, note, reference } => {
                RefundCommand::Fail(RefundFailed {
                    reason: Reason { code: reason, note },
                    reference,
                })
            }
            CardOutcome::NotFound => RefundCommand::Fail(RefundFailed {
                reason: Reason { code: ReasonCode::new(NOT_RECEIVED)?, note: None },
                reference: None,
            }),
            CardOutcome::Unknown { .. } => return Err(RuntimeError::UnknownCardRequest),
        };
        let completes = matches!(command, RefundCommand::Complete(_));
        let printed: Id<Job> = self.ids.generate(meta.now)?;
        let (location, device) = (self.location(), self.device);
        self.store.write(|w| -> Result<(), RuntimeError> {
            let event = w.load(Refund::new(refund))?.decide(location, device, command)?;
            let cause = append(w, refund.cast(), &event, &meta)?;
            w.finish(key)?;
            if let Some(doc) = receipt.filter(|_| completes) {
                w.enqueue(&print::effect(printed, JobKind::Receipt, doc, Some(cause))?, meta.now)?;
            }
            Ok(())
        })
    }

    /// Settles by hand the card payment `payment`, which no terminal can answer for, on a
    /// manager's word (`payment.resolve`): `charged`, it is captured for what it asked, with the
    /// tip chosen on the register, no reference and no card, and settles its check; not, it fails
    /// as `not_charged`. Its request is done either way, so nothing asks about it again.
    ///
    /// # Errors
    /// [`RuntimeError::NotCardPayment`] unless it is an unresolved card payment this register
    /// took; [`RuntimeError::CardBusy`] while its request is with the shell;
    /// [`RuntimeError::ApprovalRequired`] or [`RuntimeError::NotApprover`] if it needs an
    /// approval it doesn't have; [`RuntimeError`] if no one is signed in.
    pub fn settle_card(
        &mut self,
        payment: Id<Payment>,
        charged: bool,
    ) -> Result<Ticket, RuntimeError> {
        let meta = self.meta()?;
        let key = payment.to_bytes().to_vec();
        if self.charging.as_deref() == Some(key.as_slice()) {
            return Err(RuntimeError::CardBusy);
        }
        let loaded = self.store.load(Payment::new(payment))?;
        let info = loaded.info().cloned().ok_or(RuntimeError::NotCardPayment)?;
        let queued = self.store.effect(&key)?.ok_or(RuntimeError::NotCardPayment)?;
        let unfinished = matches!(queued.state, EffectState::Pending | EffectState::Running);
        if info.tender != Tender::Card
            || !loaded.is_unresolved()
            || !is_card(&queued)
            || !unfinished
        {
            return Err(RuntimeError::NotCardPayment);
        }
        // The tip chosen on the register, if its request can be read: settled without, otherwise.
        let tip = match read(&queued).map(|read| read.ask) {
            Some(CardAsk::Sale { tip, .. }) => tip,
            _ => None,
        };
        let granted = self.gate(Request::of(Action::PaymentResolve), Some(payment.cast()))?;
        let command = if charged {
            PaymentCommand::Capture(PaymentCaptured {
                amount: info.amount,
                tip,
                reference: None,
                cash: None,
                card: None,
            })
        } else {
            PaymentCommand::Fail(PaymentEnded {
                reason: Reason { code: ReasonCode::new(NOT_CHARGED)?, note: None },
                reference: None,
            })
        };
        // Its request is started, if it waits, so that it can be finished with the outcome.
        if queued.state == EffectState::Pending {
            let now = self.clock.now();
            self.store.write(|w| -> Result<(), RuntimeError> {
                w.reschedule(&key, now)?;
                w.start(&key, now)?;
                Ok(())
            })?;
        }
        // The approval is written in its own stream, with the payment's outcome, which names it.
        let stream = self.approval_stream(granted.as_ref())?;
        let approval = granted.zip(stream);
        self.record_payment(&key, &info, command, &meta, approval)?;
        self.spend(granted.as_ref());
        self.ticket(info.order)
    }

    /// The card payments of `order`, each with where it is, oldest first.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn card_payments(&self, order: Id<Order>) -> Result<Vec<CardPaymentView>, RuntimeError> {
        let mut views = Vec::new();
        for payment in self.payments(order)? {
            let Some(info) = payment.info().filter(|info| info.tender == Tender::Card) else {
                continue;
            };
            let key = payment.id().to_bytes();
            let queued = self.store.effect(&key)?;
            let (state, card) = match payment.status() {
                keel_domain::payment::PaymentStatus::Captured(captured) => (
                    CardState::Approved {
                        applied: Amount::new(captured.amount, self.locale),
                        tip: captured.tip.map(|tip| Amount::new(tip, self.locale)),
                        by_hand: captured.card.is_none() && captured.reference.is_none(),
                    },
                    captured.card.as_ref().map(card_text),
                ),
                keel_domain::payment::PaymentStatus::Failed(ended)
                | keel_domain::payment::PaymentStatus::Voided(ended) => {
                    (CardState::Declined { reason: ended.reason.code.as_str().to_owned() }, None)
                }
                _ => match queued {
                    None => (CardState::Elsewhere, None),
                    // Started before, and not with the shell for the first time: its outcome
                    // isn't known, and the terminal is asked about it.
                    Some(queued)
                        if queued.attempts > 1
                            || (queued.attempts == 1
                                && self.charging.as_deref() != Some(&key[..])) =>
                    {
                        (CardState::Checking, None)
                    }
                    Some(_) => (CardState::Waiting, None),
                },
            };
            views.push(CardPaymentView {
                payment: payment.id(),
                check: info.check,
                amount: Amount::new(info.amount, self.locale),
                state,
                card,
            });
        }
        Ok(views)
    }

    /// The card refunds this register owes: started here, not yet paid back.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn owed_card_refunds(&self) -> Result<Vec<CardRefundView>, RuntimeError> {
        let effects = self.store.effects()?;
        let mut refunds: Vec<Id<Refund>> = Vec::new();
        for queued in &effects {
            if let Some(Read { ask: CardAsk::Refund { refund, .. }, .. }) = read(queued)
                && !refunds.contains(&refund)
            {
                refunds.push(refund);
            }
        }
        let mut owed = Vec::new();
        for refund in refunds {
            let loaded = self.store.load(Refund::new(refund))?;
            let Some(info) = loaded.info().filter(|_| loaded.is_owed()) else { continue };
            // With the terminal, or to be asked about: one of its attempts isn't finished.
            let pending = effects.iter().any(|queued| {
                matches!(read(queued), Some(Read { ask: CardAsk::Refund { refund: of, .. }, .. }) if of == refund)
                    && matches!(queued.state, EffectState::Pending | EffectState::Running)
            });
            owed.push(CardRefundView {
                refund,
                order: info.order,
                amount: Amount::new(info.amount, self.locale),
                pending,
                failures: loaded.failures(),
                failure: loaded.failed().map(|failed| failed.reason.code.as_str().to_owned()),
            });
        }
        Ok(owed)
    }

    /// Tries again the card refund `refund`, whose latest attempt failed: `refund.issue`, for
    /// what it refunds, which the member's role permits or a manager approves. A new attempt is
    /// enqueued, keyed by the refund and its number.
    ///
    /// # Errors
    /// [`RuntimeError::NotCardRefund`] unless it is a card refund this register started, owed,
    /// whose latest attempt failed; [`RuntimeError::ApprovalRequired`] or
    /// [`RuntimeError::NotApprover`] if it needs an approval it doesn't have.
    pub fn retry_refund(&mut self, refund: Id<Refund>) -> Result<(), RuntimeError> {
        let meta = self.meta()?;
        let loaded = self.store.load(Refund::new(refund))?;
        let info = *loaded.info().ok_or(RuntimeError::NotCardRefund)?;
        let failures = loaded.failures();
        if info.tender != Tender::Card
            || !loaded.is_owed()
            || failures == 0
            || info.device != self.device
        {
            return Err(RuntimeError::NotCardRefund);
        }
        let last =
            self.store.effect(&refund_key(refund, failures))?.ok_or(RuntimeError::NotCardRefund)?;
        let Some(Read { ask: CardAsk::Refund { amount, reference, .. }, receipt }) = read(&last)
        else {
            return Err(RuntimeError::NotCardRefund);
        };
        if !matches!(last.state, EffectState::Done | EffectState::Failed) {
            return Err(RuntimeError::NotCardRefund);
        }
        let request = Request { action: Action::RefundIssue, amount: Some(amount), percent: None };
        let granted = self.gate(request, Some(info.order.cast()))?;
        let attempt = failures.saturating_add(1);
        let effect = refund_effect(
            refund,
            attempt,
            amount,
            &reference,
            receipt.as_ref(),
            Some(info.initiated_by),
        )?;
        let stream = self.approval_stream(granted.as_ref())?;
        self.store.write(|w| -> Result<(), RuntimeError> {
            crate::staff::approved(w, granted.as_ref(), stream, &meta)?;
            w.enqueue(&effect, meta.now)?;
            Ok(())
        })?;
        self.spend(granted.as_ref());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_wait_longer_each_time_up_to_a_minute() {
        let waits: Vec<u64> = (1..=7).map(|attempts| ask_after(attempts).as_secs()).collect();
        assert_eq!(waits, [5, 10, 20, 40, 60, 60, 60]);
        assert_eq!(ask_after(u32::MAX), LONGEST_ASK);
    }

    #[test]
    fn a_share_rounds_half_up() {
        let usd = |minor| Money::from_minor(minor, Currency::USD);
        assert_eq!(share(usd(1000), 1, 3), Some(usd(333)));
        assert_eq!(share(usd(1000), 2, 3), Some(usd(667)));
        assert_eq!(share(usd(5), 1, 2), Some(usd(3)));
        assert_eq!(share(usd(1000), 1, 0), None);
        assert_eq!(share(usd(i64::MAX), i64::MAX, 1), None);
    }

    fn queued(effect: Effect) -> Queued {
        let at = keel_types::Timestamp::from_micros(1_794_560_400_000_000).unwrap();
        Queued {
            effect,
            state: EffectState::Pending,
            attempts: 0,
            enqueued: at,
            due: at,
            started: None,
        }
    }

    #[test]
    fn effects_read_back_as_they_were_written() {
        let usd = |minor| Money::from_minor(minor, Currency::USD);
        let payment: Id<Payment> = Id::parse("0192f0c1-0000-7000-8000-0000000000b1").unwrap();
        let cause: Id<Event> = Id::parse("0192f0c1-0000-7000-8000-0000000000e1").unwrap();
        let sale =
            sale_effect(payment, usd(1089), Some(usd(200)), &[1_500, 2_000], true, cause).unwrap();
        let ask = read(&queued(sale)).unwrap().ask;
        assert_eq!(
            ask,
            CardAsk::Sale {
                payment,
                amount: usd(1089),
                tip: Some(usd(200)),
                presets: vec![1_500, 2_000],
                partial: true
            }
        );
        assert_eq!(key_of(&ask), "0192f0c1-0000-7000-8000-0000000000b1");
        let refund: Id<Refund> = Id::parse("0192f0c1-0000-7000-8000-0000000000c1").unwrap();
        let reference = ProcessorRef::new("pi_1").unwrap();
        let again = refund_effect(refund, 2, usd(500), &reference, None, None).unwrap();
        assert_eq!(again.key.len(), 20);
        let ask = read(&queued(again)).unwrap().ask;
        assert_eq!(ask, CardAsk::Refund { refund, attempt: 2, amount: usd(500), reference });
        assert_eq!(key_of(&ask), "0192f0c1-0000-7000-8000-0000000000c1-2");
    }
}
