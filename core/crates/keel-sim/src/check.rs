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
//! - **Sequencing** (ADR-0020): the hub's records number every event but records once, gapless
//!   from 1 in its epoch, and each device's log in order; every replica confirms the same, and
//!   holds every event confirmed but a forked device's other version; the feed gives them in
//!   number order. No event a device was told is store-durable is lost to a rollback.
//! - **Ownership** (ADR-0021): every request for an order the hub holds has exactly one answer,
//!   from the hub, after it, on the order's stream, and a grant gives the requesting device the
//!   lease after the one it saw; the hub's grants for each order name each lease once, in
//!   increasing order in its log; a stale grant only ever follows an override of the lease it
//!   replaced; and every replica agrees with the hub on each order's owner and lease, with no
//!   request left waiting.

use std::collections::{BTreeMap, BTreeSet};

use keel_domain::order::{ConflictKind, Lease, Order, OrderEvent, Refusal};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::SequenceEvent;
use keel_events::envelope::{Aggregate, Device, Event};
use keel_events::event::SignedEvent;
use keel_store::{OrderState, Reason, Store, StoreConfig, StoreKey, StoreSeq};
use keel_types::{Id, SeededEntropy};

use crate::node::{DRIFT, EPOCH, HUB, ORACLE, SimStore, device, here, signer, time};
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
    sequencing(run, &logs, &forked)?;
    ownership(run, &logs, &forked)?;
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
        if run.store_durable.contains(&body.event_id) {
            return Err(format!(
                "event {position} of node {}, which its device was told is store-durable, is lost",
                appended.node
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

/// Whether `event` is a sequencing record.
fn is_record(event: &SignedEvent) -> bool {
    event.body().stream.kind.as_str() == keel_domain::sequence::STREAM
}

/// Checks the hub's numbering, and every replica's confirmations and feed, against the logs.
fn sequencing(
    run: &mut Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
) -> Result<(), String> {
    // The numbers the hub's records give, each event once.
    let mut numbers: BTreeMap<(Id<Device>, u64), u64> = BTreeMap::new();
    let mut records: u64 = 0;
    for event in logs.get(&device(HUB)).into_iter().flatten().filter(|event| is_record(event)) {
        let body = event.body();
        let Ok(SequenceEvent::Assigned(record)) =
            SequenceEvent::decode(&body.schema, &body.payload)
        else {
            return Err("the hub wrote a record that doesn't decode".to_owned());
        };
        if record.epoch != EPOCH {
            return Err(format!("the hub numbered in epoch {}", record.epoch));
        }
        records = records.saturating_add(1);
        for (number, run_of) in record.numbered() {
            for position in run_of.from..=run_of.to {
                let offset = position.saturating_sub(run_of.from);
                let key = (run_of.device, position);
                if numbers.insert(key, number.saturating_add(offset)).is_some() {
                    return Err(format!("{:?} {position} is numbered twice", run_of.device));
                }
            }
        }
    }
    // Every event but a record is numbered, each device's log in order, gapless.
    for (origin, log) in logs {
        let mut last = 0;
        for event in log.iter().filter(|event| !is_record(event)) {
            let position = event.body().origin_seq.get();
            let Some(&number) = numbers.get(&(*origin, position)) else {
                return Err(format!("{origin:?} {position} is never numbered"));
            };
            if number <= last {
                return Err(format!("{origin:?} {position} is numbered {number}, after {last}"));
            }
            last = number;
        }
    }
    let count = u64::try_from(numbers.len()).unwrap_or(u64::MAX);
    if numbers.values().copied().collect::<BTreeSet<u64>>() != (1..=count).collect() {
        return Err("the hub's numbers have gaps".to_owned());
    }
    run.report.sequenced = [records, count];
    // Every replica confirms each event as the hub numbered it, a forked device's aside, and holds
    // every log confirmed to its end.
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for (origin, log) in logs.iter().filter(|(origin, _)| !forked.contains(*origin)) {
            for event in log {
                let position = event.body().origin_seq.get();
                let expected = numbers
                    .get(&(*origin, position))
                    .map(|&number| StoreSeq { epoch: EPOCH, number });
                if store.store_seq(*origin, position).map_err(text)? != expected {
                    return Err(format!("node {n} confirms {origin:?} {position} otherwise"));
                }
            }
        }
        let confirmed = store.confirmed().map_err(text)?;
        for (origin, log) in logs.iter().filter(|(origin, _)| !forked.contains(*origin)) {
            let length = u64::try_from(log.len()).unwrap_or(u64::MAX);
            if confirmed.get(origin).copied().unwrap_or(0) != length {
                return Err(format!(
                    "node {n} holds {origin:?} confirmed up to {:?}, of {length}",
                    confirmed.get(origin)
                ));
            }
        }
        // The feed: the confirmed events in number order.
        let feed: Vec<(u64, Id<Device>, u64)> = store
            .sequenced(EPOCH, 0, u32::MAX)
            .map_err(text)?
            .into_iter()
            .map(|entry| {
                (
                    entry.number,
                    entry.event.body().origin_device,
                    entry.event.body().origin_seq.get(),
                )
            })
            .filter(|(_, origin, _)| !forked.contains(origin))
            .collect();
        let mut expected: Vec<(u64, Id<Device>, u64)> = numbers
            .iter()
            .filter(|((origin, _), _)| !forked.contains(origin))
            .map(|(&(origin, position), &number)| (number, origin, position))
            .collect();
        expected.sort_unstable();
        if feed != expected {
            return Err(format!("node {n}'s feed differs from the hub's numbering"));
        }
    }
    Ok(())
}

/// The ownership events the hub holds (ADR-0021), decoded, by identifier.
type Owning<'a> = BTreeMap<Id<Event>, (OrderEvent, &'a SignedEvent)>;

/// The ownership events in `logs`, decoded.
fn ownership_events(logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>) -> Result<Owning<'_>, String> {
    let mut events = BTreeMap::new();
    for event in logs.values().flatten() {
        let body = event.body();
        if body.stream.kind.as_str() != OrderEvent::STREAM {
            continue;
        }
        let decoded = OrderEvent::decode(&body.schema, &body.payload).map_err(text)?;
        if matches!(
            decoded,
            OrderEvent::OwnershipRequested { .. }
                | OrderEvent::OwnershipGranted(_)
                | OrderEvent::OwnershipRefused { .. }
                | OrderEvent::OwnershipOverridden { .. }
        ) {
            events.insert(body.event_id, (decoded, event));
        }
    }
    Ok(events)
}

/// Checks the hub's answers to requests for orders against the logs, the orders' folds on the
/// hub, and every replica's view of each order's ownership; counts what the run did.
fn ownership(
    run: &mut Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
) -> Result<(), String> {
    let events = ownership_events(logs)?;
    let answers = answers(&events)?;
    grants_in_order(logs, &events)?;
    let orders: BTreeSet<Id<Aggregate>> = logs
        .values()
        .flatten()
        .map(|event| &event.body().stream)
        .filter(|stream| stream.kind.as_str() == OrderEvent::STREAM)
        .map(|stream| stream.id)
        .collect();
    let hub = store_of(run, HUB)?;
    let folds = folds_on_hub(hub, &orders, &events)?;
    // Every replica agrees with the hub on each order's owner and lease, and no request waits,
    // but for the orders a forked device wrote to, which replicas hold different versions of.
    let touched = touched_by_forks(run, forked)?;
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for order in orders.difference(&touched) {
            let (theirs, hubs) =
                (store.load(Order::new(order.cast())), hub.load(Order::new(order.cast())));
            let (theirs, hubs) = (theirs.map_err(text)?, hubs.map_err(text)?);
            if theirs.ownership() != hubs.ownership() || !theirs.requests().is_empty() {
                return Err(format!(
                    "node {n} sees order {order:?} owned as {:?} with {} requests waiting; the \
                     hub, as {:?}",
                    theirs.ownership(),
                    theirs.requests().len(),
                    hubs.ownership()
                ));
            }
        }
    }
    run.report.answers = answers;
    run.report.ownership = folds;
    Ok(())
}

/// Checks that every request the hub holds has exactly one answer, and each answer is the
/// hub's, after the request it names, on the same order, and a grant gives the requesting
/// device the lease after the request's, in the hub's epoch. Counts the requests, the grants,
/// and the refusals for each reason.
fn answers(events: &Owning<'_>) -> Result<[u64; 5], String> {
    let mut answered: BTreeMap<Id<Event>, u64> = BTreeMap::new();
    let mut answers = [0_u64; 5];
    for (decoded, answer) in events.values() {
        let (request, refusal) = match decoded {
            OrderEvent::OwnershipGranted(granted) => (granted.request, None),
            OrderEvent::OwnershipRefused { request, refusal } => (*request, Some(*refusal)),
            _ => continue,
        };
        let body = answer.body();
        if body.origin_device != device(HUB) {
            return Err(format!("{:?} answered request {request:?}", body.origin_device));
        }
        let Some((OrderEvent::OwnershipRequested { lease }, asked)) = events.get(&request) else {
            return Err(format!("the hub answered {request:?}, which it holds no request as"));
        };
        let asked = asked.body();
        if asked.stream != body.stream || asked.hlc >= body.hlc {
            return Err(format!("the hub's answer to {request:?} isn't after it on its order"));
        }
        if let OrderEvent::OwnershipGranted(granted) = decoded
            && (granted.device != asked.origin_device
                || Some(granted.lease) != lease.next()
                || granted.epoch.get() != EPOCH)
        {
            return Err(format!("the hub's grant for {request:?} is wrong: {granted:?}"));
        }
        let count = answered.entry(request).or_insert(0);
        *count = count.saturating_add(1);
        let kind = match refusal {
            None => 1,
            Some(Refusal::LeaseMoved) => 2,
            Some(Refusal::AlreadyOwner) => 3,
            Some(Refusal::PaymentInProgress) => 4,
        };
        if let Some(sum) = answers.get_mut(kind) {
            *sum = sum.saturating_add(1);
        }
    }
    for (id, (decoded, _)) in events {
        if matches!(decoded, OrderEvent::OwnershipRequested { .. }) {
            let count = answered.get(id).copied().unwrap_or(0);
            if count != 1 {
                return Err(format!("request {id:?} has {count} answers"));
            }
            answers[0] = answers[0].saturating_add(1);
        }
    }
    Ok(answers)
}

/// Checks that the hub's grants for each order name each lease once, in increasing order in its
/// log.
fn grants_in_order(
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    events: &Owning<'_>,
) -> Result<(), String> {
    let mut granted: BTreeMap<Id<Aggregate>, Lease> = BTreeMap::new();
    for event in logs.get(&device(HUB)).into_iter().flatten() {
        let body = event.body();
        let Some((OrderEvent::OwnershipGranted(grant), _)) = events.get(&body.event_id) else {
            continue;
        };
        if let Some(before) = granted.insert(body.stream.id, grant.lease)
            && before >= grant.lease
        {
            return Err(format!(
                "the hub granted lease {} of order {:?} after lease {}",
                grant.lease.get(),
                body.stream.id,
                before.get()
            ));
        }
    }
    Ok(())
}

/// Checks that, in each order's fold on the hub, a stale grant only ever follows an override of
/// the lease it replaced: the hub grants each lease once, so only an override, which the hub
/// hadn't heard of when it granted, can have taken that lease first. Counts the overrides that
/// applied, the stale overrides, the stale grants, and the events recorded without ownership.
fn folds_on_hub(
    hub: &SimStore,
    orders: &BTreeSet<Id<Aggregate>>,
    events: &Owning<'_>,
) -> Result<[u64; 4], String> {
    let mut counts = [0_u64; 4];
    for &order in orders {
        let folded = hub.load(Order::new(order.cast())).map_err(text)?;
        let mut overridden: BTreeSet<Lease> = BTreeSet::new();
        for conflict in folded.conflicts() {
            let kind = match conflict.kind {
                ConflictKind::Overridden(_) => {
                    let Some((OrderEvent::OwnershipOverridden { lease, .. }, _)) =
                        events.get(&conflict.event)
                    else {
                        return Err(format!("order {order:?}: an override that isn't one"));
                    };
                    overridden.insert(*lease);
                    0
                }
                ConflictKind::StaleOverride => 1,
                ConflictKind::StaleGrant => {
                    let Some((OrderEvent::OwnershipGranted(grant), _)) =
                        events.get(&conflict.event)
                    else {
                        return Err(format!("order {order:?}: a stale grant that isn't one"));
                    };
                    let replaced = grant.lease.get().checked_sub(1).and_then(Lease::new);
                    if !replaced.is_some_and(|lease| overridden.contains(&lease)) {
                        return Err(format!(
                            "order {order:?}: the hub's grant of lease {} is stale, and no \
                             override of the lease before it came first",
                            grant.lease.get()
                        ));
                    }
                    2
                }
                ConflictKind::NotOwner { .. } => 3,
                _ => continue,
            };
            if let Some(sum) = counts.get_mut(kind) {
                *sum = sum.saturating_add(1);
            }
        }
    }
    Ok(counts)
}

/// The streams a forked device wrote to, in any replica's version of its log.
fn touched_by_forks(
    run: &Run,
    forked: &BTreeSet<Id<Device>>,
) -> Result<BTreeSet<Id<Aggregate>>, String> {
    let mut touched = BTreeSet::new();
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for origin in forked {
            for event in store.log(*origin, 0, u32::MAX).map_err(text)? {
                touched.insert(event.body().stream.id);
            }
        }
    }
    Ok(touched)
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
