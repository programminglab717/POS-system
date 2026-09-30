//! The invariants every run must keep (ADR-0019), checked once the replicas agree after healing.
//!
//! - **Convergence:** every replica holds the same log of every device, and projections equal
//!   to those of a fresh store given every event.
//! - **No loss:** every event a device appended is in its device's log everywhere, unless it was
//!   lost to a rollback of its device: made after the copy the device was restored from, and
//!   held by no other replica.
//! - **Causality:** each event's HLC is later than every event its device held when it made it.
//! - **No forks and no quarantine:** no replica refused anything, except the other version of
//!   the log of a device that wrote before its log settled after a rollback, as an island; such
//!   a device's log is left out of the checks above.
//! - **Every store checks clean,** and every replicator's version vector is its store's.

use std::collections::{BTreeMap, BTreeSet};

use keel_events::envelope::{Aggregate, Device};
use keel_events::event::SignedEvent;
use keel_store::{OrderState, Reason, Store, StoreConfig, StoreKey};
use keel_types::{Id, SeededEntropy};

use crate::node::{DRIFT, HUB, ORACLE, SimStore, device, here, signer, time};
use crate::run::Run;

fn store_of(run: &Run, n: u8) -> Result<&SimStore, String> {
    run.nodes.get(&n).and_then(|node| node.store.as_ref()).ok_or(format!("node {n} is down"))
}

fn text(error: impl core::fmt::Debug) -> String {
    format!("{error:?}")
}

/// Checks every invariant of `run`, once its replicas agree.
pub(crate) fn after(run: &mut Run) -> Result<(), String> {
    let forked: BTreeSet<Id<Device>> = run.forked.iter().map(|&n| device(n)).collect();
    let logs = logs(run, &forked)?;
    let lost = no_loss(run, &logs, &forked)?;
    run.report.lost = lost;
    run.report.forked = u64::try_from(run.forked.len()).unwrap_or(u64::MAX);
    causality(run, &logs, &forked)?;
    quarantine(run, &forked)?;
    for (&n, node) in &mut run.nodes {
        let (Some(store), Some(replicator)) = (node.store.as_mut(), node.replicator.as_ref())
        else {
            return Err(format!("node {n} is down"));
        };
        let problems = store.check().map_err(text)?;
        if !problems.is_empty() {
            return Err(format!("node {n}'s store checks with problems: {problems:?}"));
        }
        let stored = store.version_vector().map_err(text)?;
        if &stored != replicator.version_vector() {
            return Err(format!(
                "node {n}'s replicator holds {:?}, its store {stored:?}",
                replicator.version_vector()
            ));
        }
        let stats = replicator.stats();
        let sums = [stats.gaps, stats.duplicates, stats.timeouts, stats.stalls];
        for (sum, add) in run.report.replication.iter_mut().zip(sums) {
            *sum = sum.saturating_add(add);
        }
    }
    projections(run, &logs, &forked)
}

/// Every device's log, as the hub holds it, after checking that every replica holds the same
/// (a forked device's aside).
fn logs(
    run: &Run,
    forked: &BTreeSet<Id<Device>>,
) -> Result<BTreeMap<Id<Device>, Vec<SignedEvent>>, String> {
    let hub = store_of(run, HUB)?;
    let mut logs = BTreeMap::new();
    for origin in hub.version_vector().map_err(text)?.into_keys() {
        logs.insert(origin, hub.log(origin, 0, u32::MAX).map_err(text)?);
    }
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        let held = store.version_vector().map_err(text)?;
        let origins: BTreeSet<Id<Device>> = held.keys().chain(logs.keys()).copied().collect();
        for origin in origins.difference(forked) {
            let log = store.log(*origin, 0, u32::MAX).map_err(text)?;
            let expected = logs.get(origin).map_or(&[][..], Vec::as_slice);
            if log != expected {
                return Err(format!(
                    "node {n} holds {} events of {origin:?}, the hub {}: they differ",
                    log.len(),
                    expected.len()
                ));
            }
        }
    }
    Ok(logs)
}

/// Checks that every event appended is in its device's log, unless a rollback lost it; returns
/// how many were lost.
fn no_loss(
    run: &Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
) -> Result<u64, String> {
    let mut lost: u64 = 0;
    for appended in &run.appended {
        let body = appended.event.body();
        if forked.contains(&body.origin_device) {
            continue;
        }
        let position = body.origin_seq.get();
        let at = usize::try_from(position).ok().and_then(|p| p.checked_sub(1));
        let held = at
            .and_then(|at| logs.get(&body.origin_device).and_then(|log| log.get(at)))
            .is_some_and(|event| event == &appended.event);
        if held {
            continue;
        }
        let rolled_back = run.rollbacks.iter().any(|rollback| {
            rollback.node == appended.node && rollback.at > appended.at && position > rollback.kept
        });
        if !rolled_back {
            return Err(format!(
                "event {position} of node {}, appended at {} ms, is lost",
                appended.node, appended.at
            ));
        }
        lost = lost.saturating_add(1);
    }
    Ok(lost)
}

/// Checks that each event kept is later than everything its device held when it made it.
fn causality(
    run: &Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
) -> Result<(), String> {
    for appended in &run.appended {
        let body = appended.event.body();
        let kept = logs
            .get(&body.origin_device)
            .is_some_and(|log| log.iter().any(|event| event == &appended.event));
        if forked.contains(&body.origin_device) || !kept {
            continue;
        }
        if body.hlc <= appended.after {
            return Err(format!(
                "event {} of node {} has HLC {:?}, no later than {:?}, which its device held",
                body.origin_seq, appended.node, body.hlc, appended.after
            ));
        }
    }
    Ok(())
}

/// Checks that no replica refused anything but a forked device's fork.
fn quarantine(run: &Run, forked: &BTreeSet<Id<Device>>) -> Result<(), String> {
    for &n in run.nodes.keys() {
        for refused in store_of(run, n)?.quarantine().map_err(text)? {
            let of_forked = SignedEvent::from_stored(&refused.message)
                .is_ok_and(|event| forked.contains(&event.body().origin_device));
            let broken = matches!(refused.reason, Reason::Fork | Reason::BrokenLink);
            if !broken || !of_forked {
                return Err(format!("node {n} quarantined a message: {:?}", refused.reason));
            }
        }
    }
    Ok(())
}

/// Checks every replica's projections against a fresh store given every event: row by row for
/// every order and payment, and the orders in each state. Streams a forked device wrote to are
/// left out, since replicas hold different versions of its log.
fn projections(
    run: &Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
) -> Result<(), String> {
    let mut oracle = oracle(run)?;
    let every: Vec<Vec<u8>> = logs.values().flatten().map(SignedEvent::to_bytes).collect();
    let registry = &run.registry;
    oracle
        .write(|w| {
            every
                .iter()
                .map(|bytes| w.receive(bytes, registry, time(0, 0)))
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(text)?;
    let mut streams: BTreeMap<&str, BTreeSet<Id<Aggregate>>> = BTreeMap::new();
    let mut touched_by_forks: BTreeSet<Id<Aggregate>> = BTreeSet::new();
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for origin in store.version_vector().map_err(text)?.into_keys() {
            for event in store.log(origin, 0, u32::MAX).map_err(text)? {
                let stream = &event.body().stream;
                if forked.contains(&origin) {
                    touched_by_forks.insert(stream.id);
                }
                let kind = if stream.kind.as_str() == "payment" { "payment" } else { "order" };
                streams.entry(kind).or_default().insert(stream.id);
            }
        }
    }
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for &order in streams.get("order").into_iter().flatten() {
            if touched_by_forks.contains(&order) {
                continue;
            }
            let (theirs, expected) = (store.order(order.cast()), oracle.order(order.cast()));
            if theirs.map_err(text)? != expected.map_err(text)? {
                return Err(format!("node {n}'s projection of order {order:?} differs"));
            }
        }
        for &payment in streams.get("payment").into_iter().flatten() {
            if touched_by_forks.contains(&payment) {
                continue;
            }
            let (theirs, expected) =
                (store.payment(payment.cast()), oracle.payment(payment.cast()));
            if theirs.map_err(text)? != expected.map_err(text)? {
                return Err(format!("node {n}'s projection of payment {payment:?} differs"));
            }
        }
        if touched_by_forks.is_empty() {
            for state in OrderState::ALL {
                if store.orders(state).map_err(text)? != oracle.orders(state).map_err(text)? {
                    return Err(format!("node {n}'s orders in state {state:?} differ"));
                }
            }
        }
    }
    Ok(())
}

/// A fresh store, for the oracle's device.
fn oracle(run: &Run) -> Result<SimStore, String> {
    let dir = run.base.join("oracle");
    std::fs::create_dir_all(&dir).map_err(text)?;
    let config = StoreConfig { device: device(ORACLE), location: here(), max_forward_drift: DRIFT };
    Store::open(
        dir.join("store.db"),
        StoreKey::new(&mut [ORACLE; 32]),
        config,
        signer(ORACLE).map_err(text)?,
        SeededEntropy::new(run.config.seed),
    )
    .map_err(text)
}
