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
//! - **Hub terms** (ADR-0022): every replica holds the same winning claim, and its device alone
//!   serves as the hub; the chain of terms is whole, a term for each epoch from the winning
//!   claim's down to 1.
//! - **Sequencing** (ADR-0020, ADR-0022): the records that count on the chain number every event
//!   but records exactly once, each epoch's numbers gapless from 1, and each device's log in
//!   order; every replica confirms the same, and holds every event confirmed but a forked
//!   device's other version; each epoch's feed gives them in number order. No event a device was
//!   told is store-durable is lost to a rollback. As the run goes, a candidate for the hub writes
//!   records and answers only while it serves, in the epoch of the term it holds.
//! - **Ownership** (ADR-0021, ADR-0022): every request for an order has an answer, and no term
//!   answers one twice; each answer is a candidate's, after the request, on the order's stream,
//!   and a grant gives the requesting device the lease after the one it saw; each term's grants
//!   for each order name each lease once, in increasing order in its log; a stale grant only ever
//!   follows an override, or another term's grant, of the lease it replaced; and every replica
//!   agrees with the hub on each order's owner and lease, with no request left waiting.

use std::collections::{BTreeMap, BTreeSet};

use keel_domain::hub::{self, Claim, HubEvent, Term};
use keel_domain::order::{ConflictKind, Lease, Order, OrderEvent, Refusal};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, SequenceEvent};
use keel_events::envelope::{Aggregate, Device, Event};
use keel_events::event::SignedEvent;
use keel_store::{OrderState, Reason, Store, StoreConfig, StoreKey, StoreSeq};
use keel_types::{Id, SeededEntropy};

use crate::node::{DRIFT, HUB, ORACLE, STANDBY, SimStore, device, here, signer, time};
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
    let chain = terms(run, &logs)?;
    sequencing(run, &logs, &forked, &chain)?;
    ownership(run, &logs, &forked, &chain)?;
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

/// The record `event` holds.
fn record_of(event: &SignedEvent) -> Result<Assigned, String> {
    let body = event.body();
    match SequenceEvent::decode(&body.schema, &body.payload) {
        Ok(SequenceEvent::Assigned(record)) => Ok(record),
        _ => Err(format!("{:?} wrote a record that doesn't decode", body.origin_device)),
    }
}

/// Every claim in `logs`.
fn claims(logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>) -> Result<Vec<Claim>, String> {
    let mut claims = Vec::new();
    for event in logs.values().flatten() {
        let body = event.body();
        if body.stream.kind.as_str() != hub::STREAM {
            continue;
        }
        let Ok(HubEvent::Claimed(claimed)) = HubEvent::decode(&body.schema, &body.payload) else {
            return Err(format!("{:?} wrote a claim that doesn't decode", body.origin_device));
        };
        let (device, position) = (body.origin_device, body.origin_seq.get());
        claims.push(Claim { event: body.event_id, device, position, claimed });
    }
    Ok(claims)
}

/// Checks the hub's terms (ADR-0022): every replica holds the winning claim, whose device alone
/// serves as the hub, and the chain of terms is whole. Returns the chain.
fn terms(
    run: &mut Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
) -> Result<Vec<Term>, String> {
    let chain = hub::chain(&claims(logs)?);
    let Some(&winning) = chain.first() else {
        return Err("no one claimed the hub's role".to_owned());
    };
    let epochs: Vec<u64> = chain.iter().map(|term| term.epoch.get()).collect();
    if epochs != (1..=winning.epoch.get()).rev().collect::<Vec<u64>>() {
        return Err(format!("the chain of terms isn't whole: its epochs are {epochs:?}"));
    }
    for (&n, node) in &run.nodes {
        let held = store_of(run, n)?.term().map_err(text)?;
        if held != Some(winning) {
            return Err(format!("node {n} holds the winning claim as {held:?}, not {winning:?}"));
        }
        let serving = node.replicator.as_ref().is_some_and(keel_sync::Replicator::serving);
        if serving != (device(n) == winning.device) {
            return Err(format!("node {n} serving: {serving}, the hub being {:?}", winning.device));
        }
    }
    run.report.claims[1] = winning.epoch.get();
    Ok(chain)
}

/// Checks the numbering by the records that count on `chain`, and every replica's confirmations
/// and feed, against the logs.
fn sequencing(
    run: &mut Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
    chain: &[Term],
) -> Result<(), String> {
    // The (epoch, number) the records that count give, each event once.
    let mut numbers: BTreeMap<(Id<Device>, u64), (u64, u64)> = BTreeMap::new();
    let mut records: u64 = 0;
    for (origin, log) in logs {
        for event in log.iter().filter(|event| is_record(event)) {
            let record = record_of(event)?;
            let position = event.body().origin_seq.get();
            let counts = chain.iter().any(|term| {
                term.device == *origin && term.epoch.get() == record.epoch && term.counts(position)
            });
            if !counts {
                continue;
            }
            records = records.saturating_add(1);
            for (number, run_of) in record.numbered() {
                for position in run_of.from..=run_of.to {
                    let number = number.saturating_add(position.saturating_sub(run_of.from));
                    if numbers.insert((run_of.device, position), (record.epoch, number)).is_some() {
                        return Err(format!("{:?} {position} is numbered twice", run_of.device));
                    }
                }
            }
        }
    }
    // Every event but a record is numbered, each device's log in order, a forked device's aside:
    // which version of its log a hub numbers depends on which it holds.
    for (origin, log) in logs.iter().filter(|(origin, _)| !forked.contains(*origin)) {
        let mut last = (0, 0);
        for event in log.iter().filter(|event| !is_record(event)) {
            let position = event.body().origin_seq.get();
            let Some(&number) = numbers.get(&(*origin, position)) else {
                return Err(format!("{origin:?} {position} is never numbered"));
            };
            if number <= last {
                return Err(format!(
                    "{origin:?} {position} is numbered {number:?}, after {last:?}"
                ));
            }
            last = number;
        }
    }
    // Each epoch's numbers are gapless from 1.
    let mut epochs: BTreeMap<u64, Vec<(u64, Id<Device>, u64)>> = BTreeMap::new();
    for (&(origin, position), &(epoch, number)) in &numbers {
        epochs.entry(epoch).or_default().push((number, origin, position));
    }
    for (epoch, numbered) in &mut epochs {
        numbered.sort_unstable();
        let count = u64::try_from(numbered.len()).unwrap_or(u64::MAX);
        if !numbered.iter().map(|(number, _, _)| *number).eq(1..=count) {
            return Err(format!("epoch {epoch}'s numbers have gaps"));
        }
    }
    let count = u64::try_from(numbers.len()).unwrap_or(u64::MAX);
    run.report.sequenced = [records, count];
    // Every replica confirms each event so, a forked device's aside, and holds every log
    // confirmed to its end.
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for (origin, log) in logs.iter().filter(|(origin, _)| !forked.contains(*origin)) {
            for event in log {
                let position = event.body().origin_seq.get();
                let expected = numbers
                    .get(&(*origin, position))
                    .map(|&(epoch, number)| StoreSeq { epoch, number });
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
        // Each epoch's feed: its confirmed events in number order.
        for (&epoch, numbered) in &epochs {
            let feed: Vec<(u64, Id<Device>, u64)> = store
                .sequenced(epoch, 0, u32::MAX)
                .map_err(text)?
                .into_iter()
                .map(|entry| {
                    let body = entry.event.body();
                    (entry.number, body.origin_device, body.origin_seq.get())
                })
                .filter(|(_, origin, _)| !forked.contains(origin))
                .collect();
            let expected: Vec<(u64, Id<Device>, u64)> = numbered
                .iter()
                .filter(|(_, origin, _)| !forked.contains(origin))
                .copied()
                .collect();
            if feed != expected {
                return Err(format!("node {n}'s feed of epoch {epoch} differs from the numbering"));
            }
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

/// The terms answers were written in, as the run saw each written: (node, epoch).
type Terms = BTreeMap<Id<Event>, (u8, u64)>;

/// The term `answer` was written in.
fn term_of(terms: &Terms, answer: &SignedEvent) -> Result<(u8, u64), String> {
    let id = answer.body().event_id;
    terms.get(&id).copied().ok_or_else(|| format!("answer {id:?} was never seen written"))
}

/// Checks the answers to requests for orders against the logs, the orders' folds on the hub, and
/// every replica's view of each order's ownership; counts what the run did.
fn ownership(
    run: &mut Run,
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    forked: &BTreeSet<Id<Device>>,
    chain: &[Term],
) -> Result<(), String> {
    let events = ownership_events(logs)?;
    // The orders a forked device wrote to are left out, since replicas hold different versions
    // of its requests, and of the orders' events.
    let touched = touched_by_forks(run, forked)?;
    let answers = answers(&events, &run.answer_terms, &touched)?;
    grants_in_order(logs, &events, &run.answer_terms)?;
    let orders: BTreeSet<Id<Aggregate>> = logs
        .values()
        .flatten()
        .map(|event| &event.body().stream)
        .filter(|stream| stream.kind.as_str() == OrderEvent::STREAM)
        .map(|stream| stream.id)
        .collect();
    let hub_node = if chain.first().is_some_and(|term| term.device == device(STANDBY)) {
        STANDBY
    } else {
        HUB
    };
    let hub = store_of(run, hub_node)?;
    let unforked: BTreeSet<Id<Aggregate>> = orders.difference(&touched).copied().collect();
    let folds = folds_on_hub(hub, &unforked, &events, &run.answer_terms)?;
    // Every replica agrees with the hub on each order's owner and lease, and no request waits.
    for &n in run.nodes.keys() {
        let store = store_of(run, n)?;
        for order in &unforked {
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

/// Checks that every request has an answer, and no term answers one twice; that each answer is a
/// candidate's, after the request it names, on the same order; and that a grant gives the
/// requesting device the lease after the request's, in the epoch of the term it was written in.
/// Counts the requests, the grants, and the refusals for each reason. The orders `touched` are
/// left out.
fn answers(
    events: &Owning<'_>,
    terms: &Terms,
    touched: &BTreeSet<Id<Aggregate>>,
) -> Result<[u64; 5], String> {
    let mut answered: BTreeMap<Id<Event>, BTreeSet<(u8, u64)>> = BTreeMap::new();
    let mut answers = [0_u64; 5];
    for (decoded, answer) in events.values() {
        let (request, refusal) = match decoded {
            OrderEvent::OwnershipGranted(granted) => (granted.request, None),
            OrderEvent::OwnershipRefused { request, refusal } => (*request, Some(*refusal)),
            _ => continue,
        };
        let body = answer.body();
        if touched.contains(&body.stream.id) {
            continue;
        }
        if body.origin_device != device(HUB) && body.origin_device != device(STANDBY) {
            return Err(format!("{:?} answered request {request:?}", body.origin_device));
        }
        let term = term_of(terms, answer)?;
        let Some((OrderEvent::OwnershipRequested { lease }, asked)) = events.get(&request) else {
            return Err(format!("{term:?} answered {request:?}, which no one holds as a request"));
        };
        let asked = asked.body();
        if asked.stream != body.stream || asked.hlc >= body.hlc {
            return Err(format!("{term:?}'s answer to {request:?} isn't after it on its order"));
        }
        if let OrderEvent::OwnershipGranted(granted) = decoded
            && (granted.device != asked.origin_device
                || Some(granted.lease) != lease.next()
                || granted.epoch.get() != term.1)
        {
            return Err(format!("{term:?}'s grant for {request:?} is wrong: {granted:?}"));
        }
        if !answered.entry(request).or_default().insert(term) {
            return Err(format!("{term:?} answered request {request:?} twice"));
        }
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
    for (id, (decoded, request)) in events {
        if matches!(decoded, OrderEvent::OwnershipRequested { .. })
            && !touched.contains(&request.body().stream.id)
        {
            if !answered.contains_key(id) {
                return Err(format!("request {id:?} has no answer"));
            }
            answers[0] = answers[0].saturating_add(1);
        }
    }
    Ok(answers)
}

/// Checks that each term's grants for each order name each lease once, in increasing order in its
/// candidate's log.
fn grants_in_order(
    logs: &BTreeMap<Id<Device>, Vec<SignedEvent>>,
    events: &Owning<'_>,
    terms: &Terms,
) -> Result<(), String> {
    let mut granted: BTreeMap<((u8, u64), Id<Aggregate>), Lease> = BTreeMap::new();
    for candidate in [HUB, STANDBY] {
        for event in logs.get(&device(candidate)).into_iter().flatten() {
            let body = event.body();
            let Some((OrderEvent::OwnershipGranted(grant), _)) = events.get(&body.event_id) else {
                continue;
            };
            let term = term_of(terms, event)?;
            if let Some(before) = granted.insert((term, body.stream.id), grant.lease)
                && before >= grant.lease
            {
                return Err(format!(
                    "{term:?} granted lease {} of order {:?} after lease {}",
                    grant.lease.get(),
                    body.stream.id,
                    before.get()
                ));
            }
        }
    }
    Ok(())
}

/// Checks that, in each order's fold on the hub, a stale grant only ever follows an override, or
/// another term's grant, of the lease it replaced: a term grants each lease once, so only those,
/// which the term hadn't heard of when it granted, can have taken that lease first. Counts the
/// overrides that applied, the stale overrides, the stale grants, and the events recorded
/// without ownership.
fn folds_on_hub(
    hub: &SimStore,
    orders: &BTreeSet<Id<Aggregate>>,
    events: &Owning<'_>,
    terms: &Terms,
) -> Result<[u64; 4], String> {
    let mut counts = [0_u64; 4];
    for &order in orders {
        let folded = hub.load(Order::new(order.cast())).map_err(text)?;
        let stale: BTreeSet<Id<Event>> = folded
            .conflicts()
            .iter()
            .filter(|conflict| conflict.kind == ConflictKind::StaleGrant)
            .map(|conflict| conflict.event)
            .collect();
        // The grants of the order that applied: each lease, and the term that granted it.
        let mut applied: BTreeSet<(Lease, (u8, u64))> = BTreeSet::new();
        for (id, (decoded, event)) in events {
            if let OrderEvent::OwnershipGranted(grant) = decoded
                && event.body().stream.id == order
                && !stale.contains(id)
            {
                applied.insert((grant.lease, term_of(terms, event)?));
            }
        }
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
                    let Some((OrderEvent::OwnershipGranted(grant), event)) =
                        events.get(&conflict.event)
                    else {
                        return Err(format!("order {order:?}: a stale grant that isn't one"));
                    };
                    let term = term_of(terms, event)?;
                    let replaced = grant.lease.get().checked_sub(1).and_then(Lease::new);
                    let overrode = replaced.is_some_and(|lease| overridden.contains(&lease));
                    let granted =
                        applied.iter().any(|&(lease, other)| lease == grant.lease && other != term);
                    if !overrode && !granted {
                        return Err(format!(
                            "order {order:?}: {term:?}'s grant of lease {} is stale, and no \
                             override or other term's grant of the lease before it came first",
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
