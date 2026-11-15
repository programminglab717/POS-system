//! Card payments across the boundary (ADR-0035): the requests the register hands the shell's
//! connector, the outcomes the connector reports back, the tips offered, and where each card
//! payment and refund is. Amounts are in the location's currency's minor units.

use core::time::Duration;

use keel_domain::codec::{AuthCode, LastFour, Name, Note, ProcessorRef, ReasonCode};
use keel_domain::payment::{Card, Entry};
use keel_domain::profile::TipPrompt as RuntimeTipPrompt;
use keel_runtime as runtime;
use keel_types::{Currency, Money};

use crate::records::Amount;

/// The tip a card payment carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardTip {
    /// None.
    None,
    /// An amount chosen on the register's screen, in minor units.
    Amount {
        /// The tip.
        minor: i64,
    },
    /// Whatever the customer chooses on the terminal.
    Terminal,
}

impl CardTip {
    pub(crate) fn of(self, currency: Currency) -> runtime::CardTip {
        match self {
            CardTip::None => runtime::CardTip::None,
            CardTip::Amount { minor } => {
                runtime::CardTip::Amount(Money::from_minor(minor, currency))
            }
            CardTip::Terminal => runtime::CardTip::Terminal,
        }
    }
}

/// Where a customer is asked for a tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TipPrompt {
    /// On the register's screen, before the card.
    Screen,
    /// On the terminal.
    Terminal,
}

/// One tip preset, as a percentage and an amount.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TipOffer {
    /// The percentage, in basis points: 1500 is 15%.
    pub percent: u16,
    /// What it comes to.
    pub amount: Amount,
}

/// The tips the register offers for a card payment, on the pre-tax subtotal (H-10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TipOffers {
    /// Where the tip is asked for; absent when the location takes no tips.
    pub prompt: Option<TipPrompt>,
    /// What the presets are a percentage of.
    pub base: Amount,
    /// Each preset.
    pub presets: Vec<TipOffer>,
}

impl From<&runtime::TipOffers> for TipOffers {
    fn from(offers: &runtime::TipOffers) -> TipOffers {
        TipOffers {
            prompt: offers.prompt.map(|prompt| match prompt {
                RuntimeTipPrompt::Screen => TipPrompt::Screen,
                RuntimeTipPrompt::Terminal => TipPrompt::Terminal,
            }),
            base: (&offers.base).into(),
            presets: offers
                .presets
                .iter()
                .map(|offer| TipOffer { percent: offer.percent, amount: (&offer.amount).into() })
                .collect(),
        }
    }
}

/// What a card request is about.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardAsk {
    /// A sale: authorized and captured at once.
    Sale {
        /// The payment's identifier.
        payment: String,
        /// What it pays toward its check.
        amount: i64,
        /// The tip chosen on the register's screen, on top.
        tip: Option<i64>,
        /// The tip presets, in basis points, when the terminal asks for the tip; empty otherwise.
        presets: Vec<u16>,
        /// Whether the terminal may approve less than asked.
        partial: bool,
    },
    /// A refund, by reference, to the card that paid.
    Refund {
        /// The refund's identifier.
        refund: String,
        /// Which attempt this is, from 1.
        attempt: u32,
        /// What it refunds.
        amount: i64,
        /// The processor's reference for the card payment it goes back to.
        reference: String,
    },
}

/// What the register asks the terminal to do, or about: the connector takes it, and reports what
/// happened with `card_outcome`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardRequest {
    /// The idempotency key the terminal or processor must see.
    pub key: String,
    /// What it is about.
    pub ask: CardAsk,
    /// Whether to ask what became of it, rather than do it: it was started before. Never perform
    /// a request that asks about one.
    pub status: bool,
}

impl From<&runtime::CardRequest> for CardRequest {
    fn from(request: &runtime::CardRequest) -> CardRequest {
        let ask = match &request.ask {
            runtime::CardAsk::Sale { payment, amount, tip, presets, partial } => CardAsk::Sale {
                payment: payment.to_string(),
                amount: amount.minor(),
                tip: tip.map(Money::minor),
                presets: presets.clone(),
                partial: *partial,
            },
            runtime::CardAsk::Refund { refund, attempt, amount, reference } => CardAsk::Refund {
                refund: refund.to_string(),
                attempt: *attempt,
                amount: amount.minor(),
                reference: reference.as_str().to_owned(),
            },
        };
        CardRequest { key: request.key.clone(), ask, status: request.status }
    }
}

/// How a card was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardEntry {
    /// Tapped: contactless, a card or a wallet.
    Tap,
    /// Its chip.
    Chip,
    /// Its stripe.
    Swipe,
    /// Its number keyed in on the terminal.
    Keyed,
}

/// What a terminal tells of the card: never more than a receipt shows.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardRead {
    /// Its brand, such as `Visa`.
    pub brand: String,
    /// The last four digits of its number.
    pub last_four: String,
    /// How it was read.
    pub entry: CardEntry,
    /// The issuer's authorization code, 1 to 12 ASCII letters and digits.
    pub auth_code: Option<String>,
}

/// What a terminal answered to a card request.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardOutcome {
    /// Approved: what was charged, a tip included, or refunded; less than asked for a partial
    /// approval.
    Approved {
        /// The amount, more than zero.
        approved: i64,
        /// The processor's reference.
        reference: Option<String>,
        /// The card.
        card: Option<CardRead>,
    },
    /// Declined: nothing moved.
    Declined {
        /// Why, as a reason code such as `declined` or `cancelled`.
        reason: String,
        /// What the terminal said.
        note: Option<String>,
        /// The processor's reference.
        reference: Option<String>,
    },
    /// To a status query: the terminal never took the request.
    NotFound,
    /// No one knows: ask again later.
    Unknown {
        /// When to ask again, in milliseconds, if the terminal said.
        retry_after_ms: Option<u64>,
    },
}

impl CardOutcome {
    /// The outcome, in the location's `currency`. What the terminal said that a payment can't
    /// hold is dropped, never the outcome: an approval moved money, so it is recorded whatever
    /// its card's details, and a decline whose reason isn't a reason code is declined all the
    /// same. Found in review: a refusal left the request with the connector, and froze every
    /// card request after it.
    pub(crate) fn of(self, currency: Currency) -> runtime::CardOutcome {
        let reference = |text: Option<String>| text.and_then(|text| ProcessorRef::new(&text).ok());
        match self {
            CardOutcome::Approved { approved, reference: given, card } => {
                let card = card.and_then(|card| {
                    Some(Card {
                        brand: Name::new(&card.brand).ok()?,
                        last_four: LastFour::new(&card.last_four).ok()?,
                        entry: match card.entry {
                            CardEntry::Tap => Entry::Tap,
                            CardEntry::Chip => Entry::Chip,
                            CardEntry::Swipe => Entry::Swipe,
                            CardEntry::Keyed => Entry::Keyed,
                        },
                        auth_code: card.auth_code.and_then(|code| AuthCode::new(&code).ok()),
                    })
                });
                runtime::CardOutcome::Approved {
                    approved: Money::from_minor(approved, currency),
                    reference: reference(given),
                    card,
                }
            }
            CardOutcome::Declined { reason, note, reference: given } => {
                // A reason that isn't a code is declined all the same, kept as the note.
                let (code, note) = match ReasonCode::new(&reason) {
                    Ok(code) => (code, note),
                    Err(_) => match ReasonCode::new("declined") {
                        Ok(code) => (code, note.or(Some(reason))),
                        Err(_) => return runtime::CardOutcome::Unknown { retry_after: None },
                    },
                };
                let note = note.and_then(|note| Note::new(&note).ok());
                runtime::CardOutcome::Declined { reason: code, note, reference: reference(given) }
            }
            CardOutcome::NotFound => runtime::CardOutcome::NotFound,
            CardOutcome::Unknown { retry_after_ms } => runtime::CardOutcome::Unknown {
                retry_after: retry_after_ms.map(Duration::from_millis),
            },
        }
    }
}

/// Where a card payment is.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardState {
    /// Waiting for the terminal.
    Waiting,
    /// Its outcome isn't known: the terminal is being asked about it.
    Checking,
    /// Waiting for another register's terminal, which alone can answer for it.
    Elsewhere,
    /// Approved.
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
        /// Why, as a reason code.
        reason: String,
    },
}

/// A card payment of an order, and where it is.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardPayment {
    /// The payment's identifier.
    pub payment: String,
    /// The check it pays.
    pub check: String,
    /// What it asked to pay toward the check.
    pub amount: Amount,
    /// Where it is.
    pub state: CardState,
    /// The card, as a receipt shows it, such as "Visa •••• 4242".
    pub card: Option<String>,
}

impl From<&runtime::CardPaymentView> for CardPayment {
    fn from(view: &runtime::CardPaymentView) -> CardPayment {
        CardPayment {
            payment: view.payment.to_string(),
            check: view.check.to_string(),
            amount: (&view.amount).into(),
            state: match &view.state {
                runtime::CardState::Waiting => CardState::Waiting,
                runtime::CardState::Checking => CardState::Checking,
                runtime::CardState::Elsewhere => CardState::Elsewhere,
                runtime::CardState::Approved { applied, tip, by_hand } => CardState::Approved {
                    applied: applied.into(),
                    tip: tip.as_ref().map(Amount::from),
                    by_hand: *by_hand,
                },
                runtime::CardState::Declined { reason } => {
                    CardState::Declined { reason: reason.clone() }
                }
            },
            card: view.card.clone(),
        }
    }
}

/// A card refund this register owes.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardRefund {
    /// The refund's identifier.
    pub refund: String,
    /// The order whose return it pays.
    pub order: String,
    /// What it refunds.
    pub amount: Amount,
    /// Whether the terminal has it now, or is being asked about it.
    pub pending: bool,
    /// How many attempts failed.
    pub failures: u32,
    /// Why the latest failed, as a reason code, if one did.
    pub failure: Option<String>,
}

impl From<&runtime::CardRefundView> for CardRefund {
    fn from(view: &runtime::CardRefundView) -> CardRefund {
        CardRefund {
            refund: view.refund.to_string(),
            order: view.order.to_string(),
            amount: (&view.amount).into(),
            pending: view.pending,
            failures: view.failures,
            failure: view.failure.clone(),
        }
    }
}

/// A card payment a return can be refunded to.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RefundableCard {
    /// The payment's identifier.
    pub payment: String,
    /// The card, as a receipt shows it.
    pub card: Option<String>,
    /// What is left to refund to it.
    pub left: Amount,
}

impl From<&runtime::RefundableCard> for RefundableCard {
    fn from(card: &runtime::RefundableCard) -> RefundableCard {
        RefundableCard {
            payment: card.payment.to_string(),
            card: card.card.clone(),
            left: (&card.left).into(),
        }
    }
}

/// The tips one member took on a day.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TipLine {
    /// The member's identifier: absent for tips no member took.
    pub member: Option<String>,
    /// Their name.
    pub name: Option<String>,
    /// How many payments tipped.
    pub payments: u64,
    /// What the tips came to.
    pub amount: Amount,
}

impl From<&runtime::TipLine> for TipLine {
    fn from(line: &runtime::TipLine) -> TipLine {
        TipLine {
            member: line.member.map(|member| member.to_string()),
            name: line.name.as_ref().map(ToString::to_string),
            payments: line.payments,
            amount: (&line.amount).into(),
        }
    }
}
