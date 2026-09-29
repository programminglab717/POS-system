//! Known-answer tests for the outbox: an effect's states, and what each change needs.

use keel_types::Timestamp;

use super::projections::{created, order_draft};
use super::*;

fn kind(name: &str) -> EffectKind {
    EffectKind::new(name).unwrap()
}

fn effect(key: &[u8], payload: &[u8]) -> Effect {
    Effect {
        key: key.to_vec(),
        kind: kind("print.receipt"),
        payload: payload.to_vec(),
        cause: None,
    }
}

fn written<T>(
    store: &mut TestStore,
    f: impl FnOnce(&mut Writing<'_, SoftwareSigner, SeededEntropy>) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    store.write(f)
}

/// Checks that a change was refused for `expected`.
fn refused<T: core::fmt::Debug>(result: &Result<T, StoreError>, expected: &EffectError) {
    assert!(matches!(result, Err(StoreError::Effect(error)) if error == expected), "{result:?}");
}

fn state(store: &TestStore, key: &[u8]) -> (EffectState, u32, Timestamp) {
    let queued = store.effect(key).unwrap().unwrap();
    (queued.state, queued.attempts, queued.due)
}

#[test]
fn an_effect_moves_through_its_states() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    assert_eq!(
        written(&mut store, |w| w.enqueue(&effect(b"a", b"receipt 1"), at(0))).unwrap(),
        Enqueued::New
    );
    let queued = store.effect(b"a").unwrap().unwrap();
    assert_eq!(queued.effect, effect(b"a", b"receipt 1"));
    assert_eq!(
        (queued.state, queued.attempts, queued.enqueued, queued.due, queued.started),
        (EffectState::Pending, 0, at(0), at(0), None)
    );
    assert_eq!(store.due_effects(at(0), 10).unwrap(), core::slice::from_ref(&queued));
    // Started: running, one attempt.
    let started = written(&mut store, |w| w.start(b"a", at(5))).unwrap();
    assert_eq!(
        (started.state, started.attempts, started.started),
        (EffectState::Running, 1, Some(at(5)))
    );
    assert_eq!(store.effect(b"a").unwrap().unwrap(), started);
    assert!(store.due_effects(at(10), 10).unwrap().is_empty());
    assert_eq!(store.running_effects().unwrap(), [started]);
    // Retried: pending again, due later, and not due before.
    written(&mut store, |w| w.retry(b"a", at(1_000))).unwrap();
    assert_eq!(state(&store, b"a"), (EffectState::Pending, 1, at(1_000)));
    assert!(store.due_effects(at(999), 10).unwrap().is_empty());
    assert!(
        matches!(written(&mut store, |w| w.start(b"a", at(999))), Err(StoreError::Effect(EffectError::NotDue(due))) if due == at(1_000))
    );
    written(&mut store, |w| w.start(b"a", at(1_000))).unwrap();
    assert_eq!(state(&store, b"a"), (EffectState::Running, 2, at(1_000)));
    // Finished.
    written(&mut store, |w| w.finish(b"a")).unwrap();
    assert_eq!(state(&store, b"a").0, EffectState::Done);
    // Another fails for good.
    written(&mut store, |w| {
        w.enqueue(&effect(b"b", b""), at(2_000))?;
        w.start(b"b", at(2_000))?;
        w.fail(b"b")
    })
    .unwrap();
    assert_eq!(state(&store, b"b"), (EffectState::Failed, 1, at(2_000)));
    let states: Vec<EffectState> =
        store.effects().unwrap().iter().map(|queued| queued.state).collect();
    assert_eq!(states, [EffectState::Done, EffectState::Failed]);
}

#[test]
fn due_effects_come_earliest_due_first_then_in_the_order_enqueued() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    written(&mut store, |w| {
        for (key, due) in [(b"a", 30), (b"b", 10), (b"c", 20), (b"d", 10)] {
            w.enqueue(&effect(key, b""), at(due))?;
        }
        Ok(())
    })
    .unwrap();
    let keys =
        |due: Vec<Queued>| due.into_iter().map(|queued| queued.effect.key).collect::<Vec<_>>();
    assert_eq!(
        keys(store.due_effects(at(30), 10).unwrap()),
        [b"b".to_vec(), b"d".to_vec(), b"c".to_vec(), b"a".to_vec()]
    );
    assert_eq!(keys(store.due_effects(at(25), 2).unwrap()), [b"b".to_vec(), b"d".to_vec()]);
    assert_eq!(keys(store.due_effects(at(9), 10).unwrap()), Vec::<Vec<u8>>::new());
}

#[test]
fn enqueuing_again_changes_nothing_and_keys_are_unique() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    written(&mut store, |w| w.enqueue(&effect(b"a", b"x"), at(0))).unwrap();
    written(&mut store, |w| w.start(b"a", at(0))).unwrap();
    // The same effect again: nothing changes, even though it is running.
    assert_eq!(
        written(&mut store, |w| w.enqueue(&effect(b"a", b"x"), at(5))).unwrap(),
        Enqueued::Already
    );
    assert_eq!(state(&store, b"a"), (EffectState::Running, 1, at(0)));
    // Another effect under the key is refused.
    let other = written(&mut store, |w| w.enqueue(&effect(b"a", b"y"), at(5)));
    assert!(matches!(other, Err(StoreError::Effect(EffectError::KeyInUse))));
    let mut kinded = effect(b"a", b"x");
    kinded.kind = kind("print.kitchen");
    let other = written(&mut store, |w| w.enqueue(&kinded, at(5)));
    assert!(matches!(other, Err(StoreError::Effect(EffectError::KeyInUse))));
}

#[test]
fn an_effect_needs_its_cause_stored() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let mut caused = effect(b"a", b"");
    caused.cause = Some(id(0x9999));
    let refused = written(&mut store, |w| w.enqueue(&caused, at(0)));
    assert!(matches!(refused, Err(StoreError::Effect(EffectError::UnknownCause))));
    // Its cause stored in the same write.
    let cause = written(&mut store, |w| {
        let event = w.append(order_draft(id(0x7000), &created()), at(0))?;
        let effect = Effect { cause: Some(event.body().event_id), ..effect(b"a", b"") };
        w.enqueue(&effect, at(0))?;
        Ok(event.body().event_id)
    })
    .unwrap();
    assert_eq!(store.effect(b"a").unwrap().unwrap().effect.cause, Some(cause));
}

#[test]
fn changes_an_effect_is_in_no_state_for_are_refused() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    refused(&written(&mut store, |w| w.start(b"a", at(0))), &EffectError::Unknown);
    refused(&written(&mut store, |w| w.finish(b"a")), &EffectError::Unknown);
    written(&mut store, |w| w.enqueue(&effect(b"a", b""), at(0))).unwrap();
    refused(&written(&mut store, |w| w.finish(b"a")), &EffectError::State(EffectState::Pending));
    refused(
        &written(&mut store, |w| w.retry(b"a", at(9))),
        &EffectError::State(EffectState::Pending),
    );
    refused(&written(&mut store, |w| w.fail(b"a")), &EffectError::State(EffectState::Pending));
    written(&mut store, |w| w.start(b"a", at(0))).unwrap();
    refused(
        &written(&mut store, |w| w.start(b"a", at(0))),
        &EffectError::State(EffectState::Running),
    );
    written(&mut store, |w| w.finish(b"a")).unwrap();
    for change in 0..4 {
        let result = written(&mut store, |w| match change {
            0 => w.start(b"a", at(0)).map(|_| ()),
            1 => w.finish(b"a"),
            2 => w.retry(b"a", at(0)),
            _ => w.fail(b"a"),
        });
        refused(&result, &EffectError::State(EffectState::Done));
    }
    // Out of bounds.
    refused(
        &written(&mut store, |w| w.enqueue(&effect(b"", b""), at(0))),
        &EffectError::Invalid("an effect's key"),
    );
    refused(
        &written(&mut store, |w| w.enqueue(&effect(&[1; MAX_KEY + 1], b""), at(0))),
        &EffectError::Invalid("an effect's key"),
    );
    refused(
        &written(&mut store, |w| w.enqueue(&effect(b"z", &vec![0; MAX_PAYLOAD + 1]), at(0))),
        &EffectError::Invalid("an effect's payload"),
    );
    written(&mut store, |w| w.enqueue(&effect(&[1; MAX_KEY], &vec![0; MAX_PAYLOAD]), at(0)))
        .unwrap();
}

#[test]
fn kinds_are_dotted_lowercase_words() {
    for good in ["print", "print.receipt", "payment.card_sale", "fiscal.de.tse2"] {
        assert_eq!(EffectKind::new(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        ".",
        "print.",
        ".print",
        "Print",
        "print..receipt",
        "print-receipt",
        "2print",
        "print.2x",
        "print receipt",
    ] {
        assert!(EffectKind::new(bad).is_err(), "{bad}");
    }
    assert!(EffectKind::new(&"a".repeat(64)).is_ok());
    assert!(EffectKind::new(&"a".repeat(65)).is_err());
}

#[test]
fn effects_commit_with_their_write() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let failed: Result<(), StoreError> = store.write(|w| {
        w.enqueue(&effect(b"a", b""), at(0))?;
        Err(StoreError::Corrupt("the caller changed its mind"))
    });
    assert!(failed.is_err());
    assert_eq!(store.effect(b"a").unwrap(), None);
    written(&mut store, |w| w.enqueue(&effect(b"a", b""), at(0))).unwrap();
    let failed: Result<(), StoreError> = store.write(|w| {
        w.start(b"a", at(0))?;
        Err(StoreError::Corrupt("the caller changed its mind"))
    });
    assert!(failed.is_err());
    assert_eq!(state(&store, b"a"), (EffectState::Pending, 0, at(0)));
}

#[test]
fn a_running_effect_is_in_doubt_after_a_restart() {
    let dir = TempDir::new();
    {
        let mut store = open(&dir.db());
        written(&mut store, |w| {
            w.enqueue(&effect(b"a", b""), at(0))?;
            w.enqueue(&effect(b"b", b""), at(0))?;
            w.start(b"a", at(1)).map(|_| ())
        })
        .unwrap();
    }
    let store = open(&dir.db());
    let running: Vec<Vec<u8>> =
        store.running_effects().unwrap().into_iter().map(|queued| queued.effect.key).collect();
    assert_eq!(running, [b"a".to_vec()]);
    // It isn't offered again: only the pending one is due.
    let due: Vec<Vec<u8>> = store
        .due_effects(at(100), 10)
        .unwrap()
        .into_iter()
        .map(|queued| queued.effect.key)
        .collect();
    assert_eq!(due, [b"b".to_vec()]);
}
