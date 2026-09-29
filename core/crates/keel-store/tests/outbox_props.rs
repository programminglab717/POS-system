//! Property tests for the outbox, against a model of an effect's states.
//!
//! A case is a sequence of writes and reopenings. A write enqueues, starts, finishes, retries and
//! fails effects under a few keys, appends events that effects can name as their cause, and lets
//! time pass; then it commits or fails. The model says what each change does, and why a change is
//! refused: an unknown key, another effect under the key, an effect in the wrong state or not yet
//! due, a cause the store doesn't hold. After every step, the outbox the store reads back is
//! checked against the model: every effect, the due ones in order, and the running ones.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use keel_events::envelope::Event;
use keel_store::{
    Effect, EffectError, EffectKind, EffectState, Enqueued, Queued, Store, StoreError,
};
use keel_types::{Id, SeededEntropy, Timestamp};
use proptest::prelude::*;
use support::{OWN, Scratch, at, config, draft, id, key};

const KEYS: [&[u8]; 4] = [b"k0", b"k1", b"k2", b"key-3"];
const KINDS: [&str; 2] = ["print.receipt", "payment.card_sale"];

#[derive(Clone, Copy, Debug)]
enum Cause {
    None,
    /// The event the store's device appended most recently.
    Latest,
    /// An event the store doesn't hold.
    Missing,
}

#[derive(Clone, Debug)]
enum Step {
    Enqueue { key: usize, kind: usize, payload: u8, cause: Cause },
    Start(usize),
    Finish(usize),
    Retry { key: usize, delay: u16 },
    Fail(usize),
    Append,
    Wait(u16),
}

#[derive(Clone, Debug)]
enum Op {
    Write(Vec<Step>, bool),
    Reopen,
}

fn any_step() -> impl Strategy<Value = Step> {
    let k = 0_usize..KEYS.len();
    let cause = prop_oneof![
        3 => Just(Cause::None),
        2 => Just(Cause::Latest),
        1 => Just(Cause::Missing),
    ];
    prop_oneof![
        4 => (k.clone(), 0_usize..2, 0_u8..2, cause)
            .prop_map(|(key, kind, payload, cause)| Step::Enqueue { key, kind, payload, cause }),
        4 => k.clone().prop_map(Step::Start),
        2 => k.clone().prop_map(Step::Finish),
        3 => (k.clone(), 0_u16..300).prop_map(|(key, delay)| Step::Retry { key, delay }),
        1 => k.prop_map(Step::Fail),
        1 => Just(Step::Append),
        2 => (0_u16..200).prop_map(Step::Wait),
    ]
}

fn any_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => (prop::collection::vec(any_step(), 1..8), prop::bool::weighted(0.8))
            .prop_map(|(steps, commits)| Op::Write(steps, commits)),
        1 => Just(Op::Reopen),
    ]
}

/// The model: every effect, in the order enqueued, and the events effects may name.
#[derive(Clone, Debug, Default)]
struct Model {
    effects: Vec<Queued>,
    events: Vec<Id<Event>>,
}

impl Model {
    fn get(&mut self, key: &[u8]) -> Option<&mut Queued> {
        self.effects.iter_mut().find(|queued| queued.effect.key == key)
    }

    /// What the model says `step` does at `now`: the outcome, having applied it.
    fn apply(
        &mut self,
        step: &Step,
        effect: Option<&Effect>,
        now: Timestamp,
    ) -> Result<Outcome, EffectError> {
        match step {
            Step::Enqueue { .. } => {
                let effect = effect.unwrap();
                if let Some(existing) = self.get(&effect.key) {
                    return if existing.effect == *effect {
                        Ok(Outcome::Enqueued(Enqueued::Already))
                    } else {
                        Err(EffectError::KeyInUse)
                    };
                }
                if effect.cause.is_some_and(|cause| !self.events.contains(&cause)) {
                    return Err(EffectError::UnknownCause);
                }
                self.effects.push(Queued {
                    effect: effect.clone(),
                    state: EffectState::Pending,
                    attempts: 0,
                    enqueued: now,
                    due: now,
                    started: None,
                });
                Ok(Outcome::Enqueued(Enqueued::New))
            }
            Step::Start(k) => {
                let queued = self.get(KEYS[*k]).ok_or(EffectError::Unknown)?;
                if queued.state != EffectState::Pending {
                    return Err(EffectError::State(queued.state));
                }
                if queued.due > now {
                    return Err(EffectError::NotDue(queued.due));
                }
                queued.state = EffectState::Running;
                queued.attempts += 1;
                queued.started = Some(now);
                Ok(Outcome::Started(queued.clone()))
            }
            Step::Finish(k) | Step::Fail(k) | Step::Retry { key: k, .. } => {
                let queued = self.get(KEYS[*k]).ok_or(EffectError::Unknown)?;
                if queued.state != EffectState::Running {
                    return Err(EffectError::State(queued.state));
                }
                match step {
                    Step::Finish(_) => queued.state = EffectState::Done,
                    Step::Fail(_) => queued.state = EffectState::Failed,
                    Step::Retry { delay, .. } => {
                        queued.state = EffectState::Pending;
                        queued.due = at(millis(now) + i64::from(*delay));
                    }
                    _ => unreachable!(),
                }
                Ok(Outcome::Changed)
            }
            Step::Append | Step::Wait(_) => Ok(Outcome::Changed),
        }
    }

    fn due(&self, now: Timestamp, limit: usize) -> Vec<Queued> {
        let mut due: Vec<(usize, &Queued)> = self
            .effects
            .iter()
            .enumerate()
            .filter(|(_, queued)| queued.state == EffectState::Pending && queued.due <= now)
            .collect();
        due.sort_by_key(|(seq, queued)| (queued.due, *seq));
        due.into_iter().take(limit).map(|(_, queued)| queued.clone()).collect()
    }

    fn running(&self) -> Vec<Queued> {
        self.effects.iter().filter(|queued| queued.state == EffectState::Running).cloned().collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Enqueued(Enqueued),
    Started(Queued),
    Changed,
}

fn millis(at: Timestamp) -> i64 {
    at.as_millis() - support::at(0).as_millis()
}

type TestStore = Store<keel_events::keys::SoftwareSigner, SeededEntropy>;

fn agrees(store: &TestStore, model: &Model, now: Timestamp) -> Result<(), TestCaseError> {
    prop_assert_eq!(store.effects().unwrap(), model.effects.clone());
    // Due now, and later, when effects retried with a delay are due too.
    for later in [0, 150, 100_000] {
        let when = at(millis(now) + later);
        for limit in [1_u32, 2, 10] {
            prop_assert_eq!(
                store.due_effects(when, limit).unwrap(),
                model.due(when, usize::try_from(limit).unwrap())
            );
        }
    }
    prop_assert_eq!(store.running_effects().unwrap(), model.running());
    for key in KEYS {
        let expected = model.effects.iter().find(|queued| queued.effect.key == key).cloned();
        prop_assert_eq!(store.effect(key).unwrap(), expected);
    }
    Ok(())
}

#[derive(Debug)]
enum WriteError {
    Store(StoreError),
    Test(String),
    Failed,
}

impl From<StoreError> for WriteError {
    fn from(error: StoreError) -> WriteError {
        WriteError::Store(error)
    }
}

proptest! {
    /// Every change the outbox makes, or refuses, is the model's; a failed write changes nothing;
    /// and a reopened store holds what committed, running effects included.
    #[test]
    fn the_outbox_does_what_the_model_says(ops in prop::collection::vec(any_op(), 1..12)) {
        let scratch = Scratch::new("outbox");
        let open = |seed: u64| -> TestStore {
            Store::open(scratch.db(), config(), key(OWN), SeededEntropy::new(seed)).unwrap()
        };
        let mut store = open(1);
        let mut model = Model::default();
        let mut now_ms = 0_i64;
        let mut reopened = 0;
        for op in &ops {
            match op {
                Op::Write(steps, commits) => {
                    let mut tentative = model.clone();
                    let written = store.write(|w| {
                        for step in steps {
                            now_ms += 1;
                            if let Step::Wait(wait) = step {
                                now_ms += i64::from(*wait);
                            }
                            let now = at(now_ms);
                            let effect = match step {
                                Step::Enqueue { key, kind, payload, cause } => Some(Effect {
                                    key: KEYS[*key].to_vec(),
                                    kind: EffectKind::new(KINDS[*kind]).unwrap(),
                                    payload: vec![*payload; usize::from(*payload) + 1],
                                    cause: match cause {
                                        Cause::None => None,
                                        Cause::Latest => tentative.events.last().copied(),
                                        Cause::Missing => Some(id(0xDEAD)),
                                    },
                                }),
                                _ => None,
                            };
                            let want = tentative.apply(step, effect.as_ref(), now);
                            let got: Result<Outcome, StoreError> = match step {
                                Step::Enqueue { .. } => {
                                    w.enqueue(effect.as_ref().unwrap(), now).map(Outcome::Enqueued)
                                }
                                Step::Start(k) => w.start(KEYS[*k], now).map(Outcome::Started),
                                Step::Finish(k) => w.finish(KEYS[*k]).map(|()| Outcome::Changed),
                                Step::Fail(k) => w.fail(KEYS[*k]).map(|()| Outcome::Changed),
                                Step::Retry { key, delay } => w
                                    .retry(KEYS[*key], at(now_ms + i64::from(*delay)))
                                    .map(|()| Outcome::Changed),
                                Step::Append => {
                                    let event = w.append(draft(1, 0), now)?;
                                    tentative.events.push(event.body().event_id);
                                    Ok(Outcome::Changed)
                                }
                                Step::Wait(_) => Ok(Outcome::Changed),
                            };
                            match (got, want) {
                                (Ok(got), Ok(want)) if got == want => {}
                                (Err(StoreError::Effect(got)), Err(want)) if got == want => {}
                                (got, want) => {
                                    return Err(WriteError::Test(format!(
                                        "{step:?}: got {got:?}, want {want:?}"
                                    )));
                                }
                            }
                        }
                        if *commits { Ok(()) } else { Err(WriteError::Failed) }
                    });
                    match written {
                        Ok(()) => model = tentative,
                        Err(WriteError::Failed) => prop_assert!(!*commits),
                        Err(WriteError::Test(message)) => prop_assert!(false, "{}", message),
                        Err(WriteError::Store(error)) => prop_assert!(false, "store error: {error:?}"),
                    }
                }
                Op::Reopen => {
                    reopened += 1;
                    drop(store);
                    store = open(100 + reopened);
                }
            }
            agrees(&store, &model, at(now_ms))?;
        }
    }
}
