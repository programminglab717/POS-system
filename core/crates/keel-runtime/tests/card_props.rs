//! Property tests of card payments (ADR-0035, decision 9) against a model terminal that keeps a
//! ledger by idempotency key: random sales, with and without tips, taken to the terminal, which
//! approves them in whole or in part, declines them, loses the request or loses its answer; time
//! passing; and the register stopping and opening again between any two steps. Then everything
//! is resolved, answering truthfully. The terminal is never asked to charge a key twice: a
//! request it may have seen is only ever asked about. And what the store recorded agrees with
//! the terminal's ledger: a payment is captured for what the terminal charged, and failed when
//! it charged nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::NonZeroU32;
use core::time::Duration;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use keel_domain::codec::{LastFour, Name, ProcessorRef, ReasonCode};
use keel_domain::order::{Mode, Order};
use keel_domain::payment::{Card, Entry, Payment, PaymentStatus};
use keel_domain::profile::demo;
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_runtime::{CardAsk, CardOutcome, CardRequest, CardTip, Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Clock, Currency, Id, Locale, ManualClock, Money, SeededEntropy, Timestamp};
use proptest::prelude::*;

type TestRuntime = Runtime<SoftwareSigner, SeededEntropy, ManualClock, SeededEntropy>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::env::temp_dir() };
        let dir = base.join(format!("keel-card-props-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn open(scratch: &Scratch, now: Timestamp, opened: u64) -> TestRuntime {
    let identity = Identity {
        device: id::<Device>(0xd1),
        signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap(),
    };
    let mut runtime = Runtime::open(
        scratch.0.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        identity,
        demo().unwrap(),
        ManualClock::new(now),
        SeededEntropy::new(opened * 2 + 1),
        SeededEntropy::new(opened * 2 + 2),
        Locale::EN_US,
    )
    .unwrap();
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime
}

/// What the terminal does with a request.
#[derive(Clone, Copy, Debug)]
enum Answer {
    /// Approves it, or for a status query says what it did; up to `limit` cents, when given.
    Truth { limit: Option<i64> },
    /// Declines it.
    Decline,
    /// Never gets it: the register hears nothing back.
    LostRequest,
    /// Does what it is asked, but its answer is lost.
    LostReply { limit: Option<i64> },
}

#[derive(Clone, Debug)]
enum Step {
    /// An espresso sold by card, with a tip of `tip` cents, or none.
    Sale {
        tip: Option<i64>,
    },
    /// The shell takes the next request to the terminal, which answers so.
    Take(Answer),
    Later(u64),
    /// The register stops, with whatever the shell had, and opens again.
    Restart,
}

fn any_limit() -> impl Strategy<Value = Option<i64>> {
    prop::option::weighted(0.3, 1_i64..400)
}

fn any_answer() -> impl Strategy<Value = Answer> {
    prop_oneof![
        4 => any_limit().prop_map(|limit| Answer::Truth { limit }),
        1 => Just(Answer::Decline),
        2 => Just(Answer::LostRequest),
        2 => any_limit().prop_map(|limit| Answer::LostReply { limit }),
    ]
}

fn any_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => prop::option::of(1_i64..100).prop_map(|tip| Step::Sale { tip }),
        5 => any_answer().prop_map(Step::Take),
        2 => (1_u64..90).prop_map(Step::Later),
        1 => Just(Step::Restart),
    ]
}

/// What the terminal did with a key: charged an amount, or declined it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Done {
    Charged(i64),
    Declined,
}

/// The model terminal: what it did with each key it saw.
#[derive(Default)]
struct Terminal {
    ledger: BTreeMap<String, Done>,
}

impl Terminal {
    /// Takes `request` and answers as `answer` says; the outcome the register hears.
    fn take(
        &mut self,
        request: &CardRequest,
        answer: Answer,
    ) -> Result<CardOutcome, TestCaseError> {
        let CardAsk::Sale { amount, tip, .. } = &request.ask else {
            return Err(TestCaseError::fail("only sales here"));
        };
        let asked = amount.minor() + tip.map_or(0, Money::minor);
        if !request.status {
            // The crux: a request is performed once at most, whatever happened since.
            prop_assert!(
                !self.ledger.contains_key(&request.key),
                "the terminal was asked to charge {} again",
                request.key
            );
        }
        let outcome = |done: Option<Done>| match done {
            Some(Done::Charged(cents)) => CardOutcome::Approved {
                approved: usd(cents),
                reference: Some(ProcessorRef::new("pi_model").unwrap()),
                card: Some(Card {
                    brand: Name::new("Visa").unwrap(),
                    last_four: LastFour::new("4242").unwrap(),
                    entry: Entry::Chip,
                    auth_code: None,
                }),
            },
            Some(Done::Declined) => CardOutcome::Declined {
                reason: ReasonCode::new("declined").unwrap(),
                note: None,
                reference: None,
            },
            None => CardOutcome::NotFound,
        };
        let perform = |ledger: &mut BTreeMap<String, Done>, limit: Option<i64>, declined: bool| {
            if request.status {
                // Asked about it: the ledger says, and nothing more happens.
                return ledger.get(&request.key).copied();
            }
            let done = if declined {
                Done::Declined
            } else {
                Done::Charged(limit.map_or(asked, |limit| limit.min(asked)))
            };
            ledger.insert(request.key.clone(), done);
            Some(done)
        };
        Ok(match answer {
            Answer::Truth { limit } => outcome(perform(&mut self.ledger, limit, false)),
            Answer::Decline => outcome(perform(&mut self.ledger, None, true)),
            Answer::LostRequest => CardOutcome::Unknown { retry_after: None },
            Answer::LostReply { limit } => {
                perform(&mut self.ledger, limit, false);
                CardOutcome::Unknown { retry_after: None }
            }
        })
    }
}

/// Runs `steps`, then resolves everything truthfully, and checks the store against the ledger.
fn run(steps: &[Step]) -> Result<(), TestCaseError> {
    let scratch = Scratch::new();
    let start: Timestamp = "2026-10-02T13:00:00Z".parse().unwrap();
    let mut opened = 0;
    let mut runtime = open(&scratch, start, opened);
    let mut terminal = Terminal::default();
    let mut orders: Vec<Id<Order>> = Vec::new();
    for step in steps {
        match step {
            Step::Sale { tip } => {
                let order = runtime.start_order(Mode::Takeout).unwrap().order;
                runtime.add_item(order, id(0x401), &[], NonZeroU32::MIN).unwrap();
                let tip = tip.map_or(CardTip::None, |cents| CardTip::Amount(usd(cents)));
                runtime.pay_card(order, tip, true).unwrap();
                orders.push(order);
            }
            Step::Take(answer) => {
                if let Some(request) = runtime.next_card_request().unwrap() {
                    let outcome = terminal.take(&request, *answer)?;
                    runtime.card_outcome(&request.key, outcome).unwrap();
                }
            }
            Step::Later(seconds) => {
                runtime.clock().advance(Duration::from_secs(*seconds)).unwrap();
            }
            Step::Restart => {
                let now = runtime.clock().now();
                drop(runtime);
                opened += 1;
                runtime = open(&scratch, now, opened);
            }
        }
    }
    // Resolved: every request answered truthfully, as time passes, until none is left.
    for _ in 0..200 {
        match runtime.next_card_request().unwrap() {
            Some(request) => {
                let outcome = terminal.take(&request, Answer::Truth { limit: None })?;
                runtime.card_outcome(&request.key, outcome).unwrap();
            }
            None => {
                runtime.clock().advance(Duration::from_secs(61)).unwrap();
            }
        }
    }
    prop_assert_eq!(runtime.next_card_request().unwrap(), None, "everything resolved");
    for order in orders {
        for view in runtime.card_payments(order).unwrap() {
            let payment = runtime.store().load(Payment::new(view.payment)).unwrap();
            let done = terminal.ledger.get(&view.payment.to_string()).copied();
            match (payment.status(), done) {
                (PaymentStatus::Captured(captured), Some(Done::Charged(cents))) => {
                    let total = captured.amount.minor() + captured.tip.map_or(0, Money::minor);
                    prop_assert_eq!(total, cents, "captured what the terminal charged");
                }
                (PaymentStatus::Failed(_), None | Some(Done::Declined)) => {}
                (status, done) => {
                    return Err(TestCaseError::fail(format!(
                        "the store has {status:?}, the terminal {done:?}"
                    )));
                }
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn a_card_is_charged_at_most_once_per_payment_and_the_store_agrees(
        steps in prop::collection::vec(any_step(), 1..30),
    ) {
        run(&steps)?;
    }
}
