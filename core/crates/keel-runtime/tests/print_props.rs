//! Property tests of print jobs (ADR-0024, decisions 5 and 7) against a model of their states:
//! random sales, a printer set or not, jobs handed to the driver with every outcome, time passing,
//! the runtime stopping and opening again, reprints and staff's decisions. After every step the
//! jobs the runtime shows are the model's, each job the driver is handed is the one the model
//! says, and no job is ever handed to it again once any of it may have been sent.

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
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use keel_doc::PrinterProfile;
use keel_domain::order::{Mode, Order};
use keel_domain::profile::{Profile, ProfileData, demo};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_print::{NotSent, Outcome, Unconfirmed};
use keel_runtime::{Identity, Job, JobKind, JobState, Resolution, Runtime, RuntimeError};
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
        let dir = base.join(format!("keel-print-props-{}-{n}", std::process::id()));
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

/// The demo, its bottled water free, so that a sale of it owes nothing.
fn profile() -> Profile {
    let mut data: ProfileData = demo().unwrap().data().clone();
    for item in &mut data.catalog.items {
        for variant in &mut item.variants {
            if variant.id == id(0x409) {
                variant.price = Money::from_minor(0, Currency::USD);
            }
        }
    }
    Profile::new(data).unwrap()
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
        profile(),
        ManualClock::new(now),
        SeededEntropy::new(opened * 2 + 1),
        SeededEntropy::new(opened * 2 + 2),
        Locale::EN_US,
    )
    .unwrap();
    runtime.sign_in(id(0x501), "1234").unwrap();
    runtime
}

#[derive(Clone, Debug)]
enum Step {
    /// A sale paid in cash, or one that owes nothing.
    Sale {
        cash: bool,
    },
    SetPrinter,
    Next,
    Outcome(Outcome),
    /// An outcome for a job that isn't the driver's.
    StrayOutcome(prop::sample::Index),
    Resolve(prop::sample::Index, Resolution),
    Reprint(prop::sample::Index),
    Later(u64),
    /// To the time the next queued job is due, or a millisecond before.
    ToDue {
        early: bool,
    },
    /// The clock set back, as a correction does.
    Back(u64),
    /// The runtime stops and opens again, and the shell sets the printer again, or doesn't.
    Restart {
        printer: bool,
    },
}

fn any_outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        any::<bool>().prop_map(|paper_low| Outcome::Printed { paper_low }),
        Just(Outcome::NotSent(NotSent::Offline)),
        Just(Outcome::NotSent(NotSent::Unreachable)),
        Just(Outcome::Unconfirmed(Unconfirmed::NoWord)),
        Just(Outcome::Unconfirmed(Unconfirmed::Closed)),
    ]
}

fn any_step() -> impl Strategy<Value = Step> {
    let resolution =
        prop::sample::select(vec![Resolution::Printed, Resolution::Again, Resolution::Dismiss]);
    prop_oneof![
        3 => any::<bool>().prop_map(|cash| Step::Sale { cash }),
        1 => Just(Step::SetPrinter),
        6 => Just(Step::Next),
        6 => any_outcome().prop_map(Step::Outcome),
        2 => Just(Step::Outcome(Outcome::NotSent(NotSent::PaperOut))),
        1 => any::<prop::sample::Index>().prop_map(Step::StrayOutcome),
        3 => (any::<prop::sample::Index>(), resolution).prop_map(|(at, r)| Step::Resolve(at, r)),
        1 => any::<prop::sample::Index>().prop_map(Step::Reprint),
        // Around each wait a job is given: 2 s, 4 s, ..., 60 s.
        3 => prop::sample::select(vec![0_u64, 1_999, 2_000, 3_999, 4_000, 16_000, 60_000, 120_000]).prop_map(Step::Later),
        3 => any::<bool>().prop_map(|early| Step::ToDue { early }),
        1 => prop::bool::weighted(0.75).prop_map(|printer| Step::Restart { printer }),
        1 => prop::sample::select(vec![1_000_u64, 59_000, 61_000, 3_600_000]).prop_map(Step::Back),
    ]
}

/// A job as the model sees it.
#[derive(Clone, Debug)]
struct ModelJob {
    id: Id<Job>,
    kind: JobKind,
    /// `None` once it is over: printed, dismissed or ended to be printed again.
    state: Option<JobState>,
    attempts: u32,
    due: i64,
    /// Some of it may have been sent.
    tainted: bool,
}

/// How long a job waits after `attempts` attempts that sent nothing.
fn wait_after(attempts: u32) -> i64 {
    [2_000, 4_000, 8_000, 16_000, 32_000, 60_000][usize::try_from(attempts.min(6) - 1).unwrap()]
}

struct Model {
    jobs: Vec<ModelJob>,
    printer: bool,
    now: i64,
    orders: Vec<Id<Order>>,
}

impl Model {
    fn sending(&self) -> Option<usize> {
        self.jobs.iter().position(|job| job.state == Some(JobState::Sending))
    }

    /// Takes on the jobs the runtime enqueued: those it shows that the model doesn't know, which
    /// must be `kinds`, in order.
    fn enqueued(&mut self, runtime: &TestRuntime, kinds: &[JobKind]) {
        let shown = runtime.print_jobs().unwrap();
        let new: Vec<_> =
            shown.iter().filter(|view| self.jobs.iter().all(|job| job.id != view.job)).collect();
        assert_eq!(new.iter().map(|view| view.kind).collect::<Vec<_>>(), kinds);
        for view in new {
            self.jobs.push(ModelJob {
                id: view.job,
                kind: view.kind,
                state: Some(JobState::Queued),
                attempts: 0,
                due: self.now,
                tainted: false,
            });
        }
    }
}

proptest! {
    #[test]
    fn jobs_follow_the_model_and_none_is_sent_twice_by_itself(
        printer in prop::bool::weighted(0.8),
        steps in prop::collection::vec(any_step(), 1..60),
    ) {
        let scratch = Scratch::new();
        let start: Timestamp = "2026-10-02T13:00:00Z".parse().unwrap();
        let mut opened = 0;
        let mut runtime = open(&scratch, start, opened);
        let mut model = Model { jobs: Vec::new(), printer, now: start.as_millis(), orders: Vec::new() };
        if printer {
            runtime.set_printer(PrinterProfile::EPSON_80MM);
        }
        for step in steps {
            match step {
                Step::Sale { cash } => {
                    let order = runtime.start_order(Mode::Takeout).unwrap().order;
                    let (variant, tender) = if cash { (0x401, 1_000) } else { (0x409, 0) };
                    runtime.add_item(order, id(variant), &[], NonZeroU32::MIN).unwrap();
                    runtime.pay_cash(order, Money::from_minor(tender, Currency::USD)).unwrap();
                    model.orders.push(order);
                    let kinds: &[JobKind] = if cash { &[JobKind::Drawer, JobKind::Receipt] } else { &[JobKind::Receipt] };
                    model.enqueued(&runtime, kinds);
                }
                Step::SetPrinter => {
                    runtime.set_printer(PrinterProfile::EPSON_80MM);
                    model.printer = true;
                }
                Step::Next => {
                    let expected = if !model.printer || model.sending().is_some() {
                        None
                    } else {
                        // The earliest due, then the earliest enqueued; a job due later than any wait
                        // it could have been given was left behind by the clock, and is due now.
                        let due = |k: usize| {
                            let job = &model.jobs[k];
                            if job.due > model.now + 60_000 { model.now } else { job.due }
                        };
                        (0..model.jobs.len())
                            .filter(|&k| model.jobs[k].state == Some(JobState::Queued) && due(k) <= model.now)
                            .min_by_key(|&k| (due(k), k))
                    };
                    let got = runtime.next_print_job().unwrap();
                    match (expected, got) {
                        (None, None) => {}
                        (Some(at), Some(job)) => {
                            let modelled = &mut model.jobs[at];
                            prop_assert_eq!(job.job, modelled.id);
                            prop_assert!(!modelled.tainted, "a job some of which may have been sent was sent again");
                            modelled.state = Some(JobState::Sending);
                            modelled.attempts += 1;
                        }
                        (expected, got) => prop_assert!(false, "expected {expected:?}, got {got:?}"),
                    }
                }
                Step::Outcome(outcome) => {
                    let Some(at) = model.sending() else { continue };
                    runtime.print_outcome(model.jobs[at].id, outcome).unwrap();
                    let job = &mut model.jobs[at];
                    match outcome {
                        Outcome::Printed { .. } => job.state = None,
                        Outcome::NotSent(_) => {
                            job.state = Some(JobState::Queued);
                            job.due = model.now + wait_after(job.attempts);
                        }
                        Outcome::Unconfirmed(_) => {
                            job.state = Some(JobState::Unconfirmed);
                            job.tainted = true;
                        }
                    }
                }
                Step::StrayOutcome(at) => {
                    let sending = model.sending();
                    let others: Vec<usize> = (0..model.jobs.len()).filter(|&k| Some(k) != sending).collect();
                    if others.is_empty() { continue }
                    let job = model.jobs[*at.get(&others)].id;
                    let refused = runtime.print_outcome(job, Outcome::Printed { paper_low: false });
                    prop_assert!(matches!(refused, Err(RuntimeError::UnknownJob(_))), "{refused:?}");
                }
                Step::Resolve(at, resolution) => {
                    if model.jobs.is_empty() { continue }
                    let k = at.index(model.jobs.len());
                    let job = model.jobs[k].clone();
                    let result = runtime.resolve(job.id, resolution);
                    if job.state != Some(JobState::Unconfirmed) {
                        prop_assert!(matches!(result, Err(RuntimeError::NotUnconfirmed(_))), "{result:?}");
                        continue;
                    }
                    let again = result.unwrap();
                    model.jobs[k].state = None;
                    prop_assert_eq!(again.is_some(), resolution == Resolution::Again);
                    if again.is_some() {
                        model.enqueued(&runtime, &[job.kind]);
                    }
                }
                Step::Reprint(at) => {
                    if model.orders.is_empty() { continue }
                    let order = *at.get(&model.orders);
                    runtime.reprint(order).unwrap();
                    model.enqueued(&runtime, &[JobKind::Receipt]);
                }
                Step::Back(millis) => {
                    let now = runtime.clock().now();
                    let back = Timestamp::from_millis(now.as_millis() - i64::try_from(millis).unwrap()).unwrap();
                    runtime.clock().set(back);
                    model.now -= i64::try_from(millis).unwrap();
                }
                Step::Later(millis) => {
                    runtime.clock().advance(Duration::from_millis(millis)).unwrap();
                    model.now += i64::try_from(millis).unwrap();
                }
                Step::ToDue { early } => {
                    let next = model.jobs.iter().filter(|job| job.state == Some(JobState::Queued) && job.due > model.now).map(|job| job.due).min();
                    let Some(due) = next else { continue };
                    let millis = due - model.now - i64::from(early);
                    runtime.clock().advance(Duration::from_millis(u64::try_from(millis).unwrap())).unwrap();
                    model.now += millis;
                }
                Step::Restart { printer } => {
                    let now = runtime.clock().now();
                    drop(runtime);
                    opened += 1;
                    runtime = open(&scratch, now, opened);
                    // What was with the driver may have been sent; no printer is set.
                    for job in &mut model.jobs {
                        if job.state == Some(JobState::Sending) {
                            job.state = Some(JobState::Unconfirmed);
                            job.tainted = true;
                        }
                    }
                    model.printer = printer;
                    if printer {
                        runtime.set_printer(PrinterProfile::EPSON_80MM);
                    }
                }
            }
            let shown: Vec<_> = runtime
                .print_jobs()
                .unwrap()
                .into_iter()
                .map(|view| (view.job, view.kind, view.state, view.attempts))
                .collect();
            let modelled: Vec<_> = model
                .jobs
                .iter()
                .filter_map(|job| job.state.map(|state| (job.id, job.kind, state, job.attempts)))
                .collect();
            prop_assert_eq!(shown, modelled);
        }
    }
}
