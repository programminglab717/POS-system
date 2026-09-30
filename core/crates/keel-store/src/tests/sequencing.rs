//! Known answers for sequencing (ADR-0020): what the hub numbers and how, confirmation by hash,
//! the feed, and records that disagree.

use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_events::envelope::Component;
use keel_events::hash::EventHash;

use super::*;
use crate::{Sequenced, StoreSeq};
use std::collections::BTreeMap;

/// The hub's epoch in these tests.
const EPOCH: u64 = 1;

/// The store of device `n`, at `path`.
fn open_as(path: &Path, n: u8) -> TestStore {
    let (device, signer) = device(n);
    let config = StoreConfig { device, ..config() };
    Store::open(path, key(), config, signer, SeededEntropy::new(u64::from(n))).unwrap()
}

/// Receives `events`, in order, in one write.
fn take(store: &mut TestStore, events: &[&SignedEvent]) {
    store
        .write(|w| {
            for event in events {
                let received = w.receive(&event.to_bytes(), &registry(), at(5_000))?;
                assert!(matches!(received, Received::Stored(_)), "{received:?}");
            }
            Ok::<_, StoreError>(())
        })
        .unwrap();
}

/// The record `event` holds.
fn record(event: &SignedEvent) -> Assigned {
    let body = event.body();
    let Ok(SequenceEvent::Assigned(record)) = SequenceEvent::decode(&body.schema, &body.payload)
    else {
        panic!("not a record: {event:?}");
    };
    record
}

/// Each run of `record`: its device, first and last positions.
fn spans(record: &Assigned) -> Vec<(Id<Device>, u64, u64)> {
    record.runs.iter().map(|run| (run.device, run.from, run.to)).collect()
}

/// Number `number` of the tests' epoch.
fn seq(number: u64) -> StoreSeq {
    StoreSeq { epoch: EPOCH, number }
}

#[test]
fn the_hub_numbers_what_it_received_in_the_order_it_did() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 6), log_of(3, here(), 3));
    take(&mut hub, &[&two[0], &two[1], &two[2], &three[0]]);
    let later = EventDraft { business_date: "2026-09-29".parse().unwrap(), ..draft(1, 1) };
    let own = append(&mut hub, vec![later], at(6_000));
    take(&mut hub, &[&three[1], &two[3], &two[4]]);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    assert_eq!(written.len(), 1);
    let first = record(&written[0]);
    let (hub_id, d2, d3) = (own[0].body().origin_device, device(2).0, device(3).0);
    assert_eq!((first.epoch, first.first), (EPOCH, 1));
    assert_eq!(spans(&first), [(d2, 1, 3), (d3, 1, 1), (hub_id, 1, 1), (d3, 2, 2), (d2, 4, 5)]);
    // Each run is pinned by its last event's hash.
    assert_eq!(first.runs[0].last, two[2].hash());
    assert_eq!(first.runs[4].last, two[4].hash());
    // The record is the hub's own event, of a stream of its own, by the sequencer, dated as the
    // latest event it numbers.
    let body = written[0].body();
    assert_eq!((body.origin_device, body.origin_seq.get()), (hub_id, 2));
    assert_eq!(body.stream.kind.as_str(), "sequence");
    assert_eq!(body.actor, Actor::System(Component::new("sequencer").unwrap()));
    assert_eq!(body.business_date, "2026-09-29".parse().unwrap());
    // Nothing new, nothing written.
    assert_eq!(hub.sequence(EPOCH, at(7_500)).unwrap(), Vec::new());
    // Later events follow on, the hub's own record left out.
    take(&mut hub, &[&two[5], &three[2]]);
    let more = append(&mut hub, vec![draft(1, 2)], at(8_000));
    let written = hub.sequence(EPOCH, at(9_000)).unwrap();
    let second = record(&written[0]);
    assert_eq!(second.first, 9);
    assert_eq!(spans(&second), [(d2, 6, 6), (d3, 3, 3), (hub_id, 3, 3)]);
    assert_eq!(more[0].body().origin_seq.get(), 3);
    assert_eq!(hub.store_seq(hub_id, 3).unwrap(), Some(seq(11)));
    assert!(hub.check().unwrap().is_empty());
}

#[test]
fn a_record_ends_where_a_device_does_not_follow_on_or_it_is_full() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    // Device 2 wrote a record of its own between its events, which the hub leaves out.
    let (two_id, two_signer) = device(2);
    let mut writer = LogWriter::new(
        LogConfig {
            device: two_id,
            location: here(),
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: Duration::from_secs(60),
        },
        two_signer,
        SeededEntropy::new(2),
    );
    let its_record = Assigned::new(
        EPOCH,
        40,
        vec![Run { device: device(4).0, from: 1, to: 1, last: EventHash::from_bytes([7; 32]) }],
    )
    .unwrap();
    let (schema, payload) = SequenceEvent::Assigned(its_record).encode().unwrap();
    let record_draft = EventDraft {
        stream: StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(0x9000) },
        schema,
        payload,
        ..draft(1, 0)
    };
    let two: Vec<SignedEvent> = [draft(1, 1), record_draft, draft(1, 3)]
        .into_iter()
        .zip(1_i64..)
        .map(|(draft, k)| writer.prepare(draft, at(k)).unwrap().commit())
        .collect();
    take(&mut hub, &[&two[0], &two[1], &two[2]]);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    let records: Vec<Assigned> = written.iter().map(record).collect();
    assert_eq!(records.len(), 2);
    assert_eq!((spans(&records[0]), records[0].first), (vec![(two_id, 1, 1)], 1));
    assert_eq!((spans(&records[1]), records[1].first), (vec![(two_id, 3, 3)], 2));

    // Two devices taking turns, an event each: a record holds 1,024 runs, and the next the rest.
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 513), log_of(3, here(), 512));
    let turns: Vec<&SignedEvent> =
        two.iter().zip(three.iter()).flat_map(|(a, b)| [a, b]).chain(two.last()).collect();
    take(&mut hub, &turns);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    let records: Vec<Assigned> = written.iter().map(record).collect();
    assert_eq!(records.iter().map(|record| record.runs.len()).collect::<Vec<_>>(), [1024, 1]);
    assert_eq!((records[1].first, records[1].runs[0].device), (1025, two_id));
    assert_eq!(hub.store_seq(two_id, 513).unwrap(), Some(seq(1025)));
}

#[test]
fn an_event_is_confirmed_once_a_record_covers_it_with_its_hash() {
    let (hub_dir, other_dir) = (TempDir::new(), TempDir::new());
    let mut hub = open(&hub_dir.db());
    let two = log_of(2, here(), 3);
    take(&mut hub, &[&two[0], &two[1], &two[2]]);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    let hub_id = own().0;
    let d2 = device(2).0;
    // Another replica holds the record before the last event it covers: nothing is confirmed
    // but the record.
    let mut other = open_as(&other_dir.db(), 3);
    take(&mut other, &[&two[0], &written[0], &two[1]]);
    assert_eq!(other.store_seq(d2, 1).unwrap(), None);
    assert_eq!(other.confirmed().unwrap(), [(hub_id, 1)].into());
    assert_eq!(other.sequenced(EPOCH, 0, 10).unwrap(), Vec::new());
    // Once it holds that event, the run confirms every event it covers.
    take(&mut other, &[&two[2]]);
    for position in 1..=3 {
        assert_eq!(other.store_seq(d2, position).unwrap(), Some(seq(position)));
        assert_eq!(hub.store_seq(d2, position).unwrap(), Some(seq(position)));
    }
    assert_eq!(other.store_seq(d2, 4).unwrap(), None);
    assert_eq!(other.confirmed().unwrap(), [(hub_id, 1), (d2, 3)].into());
    assert_eq!(other.confirmed().unwrap(), hub.confirmed().unwrap());
    assert!(other.check().unwrap().is_empty());
}

/// Device 2's log, and another version of it that its device forked at `at`: the same events
/// before, and another from there on.
fn forked(count: usize, at_position: usize) -> (Vec<SignedEvent>, Vec<SignedEvent>) {
    let one = log_of(2, here(), count);
    let (device, signer) = device(2);
    let base = &one[at_position.checked_sub(2).unwrap()];
    let mut writer = LogWriter::new(
        LogConfig {
            device,
            location: here(),
            head: LogHead::of(base),
            latest_hlc: base.body().hlc,
            max_forward_drift: Duration::from_secs(60),
        },
        signer,
        SeededEntropy::new(99),
    );
    let mut other: Vec<SignedEvent> = one[..at_position.checked_sub(1).unwrap()].to_vec();
    for k in at_position..=count {
        let k = i64::try_from(k).unwrap();
        other.push(
            writer.prepare(draft(2, 99), at(3_000_i64.checked_add(k).unwrap())).unwrap().commit(),
        );
    }
    (one, other)
}

#[test]
fn a_forked_version_of_a_log_is_never_confirmed() {
    let (hub_dir, other_dir) = (TempDir::new(), TempDir::new());
    let (one, other_version) = forked(3, 3);
    assert_eq!(one[1], other_version[1]);
    assert_ne!(one[2], other_version[2]);
    let mut hub = open(&hub_dir.db());
    take(&mut hub, &[&one[0], &one[1], &one[2]]);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    // A replica holding the other version finds the run's hash isn't its event's: the hub's
    // numbers aren't for its version, even for the events both versions share.
    let mut other = open_as(&other_dir.db(), 3);
    take(&mut other, &[&other_version[0], &other_version[1], &other_version[2], &written[0]]);
    let d2 = device(2).0;
    for position in 1..=3 {
        assert_eq!(other.store_seq(d2, position).unwrap(), None);
    }
    assert!(!other.confirmed().unwrap().contains_key(&d2));
    assert_eq!(other.sequenced(EPOCH, 0, 10).unwrap(), Vec::new());
}

#[test]
fn the_feed_gives_confirmed_events_in_number_order() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 3), log_of(3, here(), 2));
    take(&mut hub, &[&two[0], &three[0], &two[1]]);
    hub.sequence(EPOCH, at(7_000)).unwrap();
    take(&mut hub, &[&three[1], &two[2]]);
    hub.sequence(EPOCH, at(8_000)).unwrap();
    let numbered = |feed: Vec<Sequenced>| -> Vec<(u64, SignedEvent)> {
        feed.into_iter().map(|entry| (entry.number, entry.event)).collect()
    };
    let all = [&two[0], &three[0], &two[1], &three[1], &two[2]];
    let expected: Vec<(u64, SignedEvent)> =
        (1..).zip(all.iter().map(|&event| event.clone())).collect();
    assert_eq!(numbered(hub.sequenced(EPOCH, 0, 100).unwrap()), expected);
    // A page: after number 1, two events, across both records.
    assert_eq!(numbered(hub.sequenced(EPOCH, 1, 2).unwrap()), expected[1..3].to_vec());
    assert_eq!(numbered(hub.sequenced(EPOCH, 2, 2).unwrap()), expected[2..4].to_vec());
    assert_eq!(hub.sequenced(EPOCH, 5, 10).unwrap(), Vec::new());
    assert_eq!(hub.sequenced(EPOCH, 0, 0).unwrap(), Vec::new());
    assert_eq!(hub.sequenced(2, 0, 10).unwrap(), Vec::new());
}

#[test]
fn where_records_disagree_an_events_first_number_counts() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let two = log_of(2, here(), 2);
    take(&mut hub, &[&two[0], &two[1]]);
    hub.sequence(EPOCH, at(7_000)).unwrap();
    // Device 4 numbers the same events in a later epoch, and the second again, earlier in it.
    let (four, four_signer) = device(4);
    let mut writer = LogWriter::new(
        LogConfig {
            device: four,
            location: here(),
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: Duration::from_secs(60),
        },
        four_signer,
        SeededEntropy::new(4),
    );
    let d2 = device(2).0;
    let records = [
        Assigned::new(2, 7, vec![Run { device: d2, from: 1, to: 2, last: two[1].hash() }]),
        Assigned::new(2, 3, vec![Run { device: d2, from: 2, to: 2, last: two[1].hash() }]),
        // A run within one numbered earlier, which reaches past it.
        Assigned::new(2, 20, vec![Run { device: d2, from: 1, to: 1, last: two[0].hash() }]),
    ];
    let events: Vec<SignedEvent> = records
        .into_iter()
        .zip(0x9100_u64..)
        .map(|(record, n)| {
            let (schema, payload) = SequenceEvent::Assigned(record.unwrap()).encode().unwrap();
            let stream = StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(n) };
            let draft = EventDraft { stream, schema, payload, ..draft(1, 0) };
            writer.prepare(draft, at(9_000)).unwrap().commit()
        })
        .collect();
    take(&mut hub, &[&events[0], &events[1], &events[2]]);
    assert_eq!(hub.store_seq(d2, 1).unwrap(), Some(seq(1)));
    assert_eq!(hub.store_seq(d2, 2).unwrap(), Some(seq(2)));
    // Epoch 2's feed leaves out what epoch 1 numbered first.
    assert_eq!(hub.sequenced(2, 0, 10).unwrap(), Vec::new());
    // The hub numbers after the last position any record covers.
    assert_eq!(hub.sequence(EPOCH, at(9_500)).unwrap(), Vec::new());
    assert!(hub.check().unwrap().is_empty());
    // In epoch 2, the hub's own numbers start at 1, whatever another device's records say.
    let more = log_of(2, here(), 3);
    take(&mut hub, &[&more[2]]);
    let written = hub.sequence(2, at(10_000)).unwrap();
    assert_eq!((record(&written[0]).epoch, record(&written[0]).first), (2, 1));
}

#[test]
fn a_confirmed_stretch_counts_once_the_log_is_confirmed_up_to_it() {
    let dir = TempDir::new();
    let mut replica = open_as(&dir.db(), 3);
    let two = log_of(2, here(), 3);
    take(&mut replica, &[&two[0], &two[1], &two[2]]);
    // Device 4's record numbers the third event alone: it is confirmed, and the first two
    // aren't, so the confirmed start of the log is empty.
    let (four, four_signer) = device(4);
    let mut writer = LogWriter::new(
        LogConfig {
            device: four,
            location: here(),
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: Duration::from_secs(60),
        },
        four_signer,
        SeededEntropy::new(4),
    );
    let d2 = device(2).0;
    let third =
        Assigned::new(EPOCH, 3, vec![Run { device: d2, from: 3, to: 3, last: two[2].hash() }]);
    let (schema, payload) = SequenceEvent::Assigned(third.unwrap()).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(0x9200) };
    let event = writer.prepare(EventDraft { stream, schema, payload, ..draft(1, 0) }, at(9_000));
    take(&mut replica, &[&event.unwrap().commit()]);
    assert_eq!(replica.store_seq(d2, 3).unwrap(), Some(seq(3)));
    assert_eq!(replica.store_seq(d2, 2).unwrap(), None);
    assert_eq!(replica.confirmed().unwrap(), [(four, 1)].into());
}

#[test]
fn the_confirmed_start_of_a_log_counts_records_and_stops_at_the_first_provisional_event() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let hub_id = own().0;
    append(&mut hub, vec![draft(1, 1)], at(6_000));
    assert_eq!(hub.confirmed().unwrap(), BTreeMap::new());
    hub.sequence(EPOCH, at(6_500)).unwrap();
    append(&mut hub, vec![draft(1, 2)], at(7_000));
    // The hub's sale, then its record, then a sale no record covers yet.
    assert_eq!(hub.confirmed().unwrap(), [(hub_id, 2)].into());
    hub.sequence(EPOCH, at(7_500)).unwrap();
    assert_eq!(hub.confirmed().unwrap(), [(hub_id, 4)].into());
    assert_eq!(hub.store_seq(hub_id, 3).unwrap(), Some(seq(2)));
    // Records aren't numbered.
    assert_eq!(hub.store_seq(hub_id, 2).unwrap(), None);
}

#[test]
fn sequencing_is_a_write_of_its_own_and_an_interrupted_one_leaves_nothing() {
    let dir = TempDir::new();
    let faults = Box::new(RefuseAt { point: Point::Stored, nth: 3, seen: 0 });
    let mut hub =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults)
            .unwrap();
    let two = log_of(2, here(), 2);
    take(&mut hub, &[&two[0], &two[1]]);
    // The record's append is the third event stored: refused.
    assert!(matches!(hub.sequence(EPOCH, at(7_000)), Err(StoreError::Interrupted(Point::Stored))));
    assert_eq!(hub.head(own().0).unwrap(), LogHead::EMPTY);
    assert_eq!(hub.store_seq(device(2).0, 1).unwrap(), None);
    let written = hub.sequence(EPOCH, at(7_500)).unwrap();
    assert_eq!(spans(&record(&written[0])), [(device(2).0, 1, 2)]);
    assert_eq!(record(&written[0]).first, 1);
}

#[test]
fn each_epoch_numbers_from_1() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let two = log_of(2, here(), 3);
    take(&mut hub, &[&two[0], &two[1]]);
    hub.sequence(EPOCH, at(7_000)).unwrap();
    take(&mut hub, &[&two[2]]);
    let written = hub.sequence(3, at(8_000)).unwrap();
    assert_eq!((record(&written[0]).epoch, record(&written[0]).first), (3, 1));
    assert_eq!(hub.store_seq(device(2).0, 3).unwrap(), Some(StoreSeq { epoch: 3, number: 1 }));
    for epoch in [0, 1 << 63] {
        assert!(matches!(hub.sequence(epoch, at(9_500)), Err(StoreError::OutOfRange("an epoch"))));
    }
}

#[test]
fn the_check_finds_a_run_that_isnt_what_its_record_says() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 2), log_of(3, here(), 1));
    take(&mut hub, &[&two[0], &three[0], &two[1]]);
    let written = hub.sequence(EPOCH, at(7_000)).unwrap();
    assert!(hub.check().unwrap().is_empty());
    drop(hub);
    // The record's last run, changed behind the store's back.
    let db = raw(&dir.db());
    db.execute("UPDATE sequence SET number = 4 WHERE run = 2", []).unwrap();
    drop(db);
    let mut hub = open(&dir.db());
    let stream = written[0].body().stream.id.to_bytes().to_vec();
    assert_eq!(hub.check().unwrap(), [Problem::Projection { projection: "sequence", stream }]);
    hub.rebuild_projections().unwrap();
    assert!(hub.check().unwrap().is_empty());
    assert_eq!(hub.store_seq(device(2).0, 2).unwrap(), Some(seq(3)));
}
