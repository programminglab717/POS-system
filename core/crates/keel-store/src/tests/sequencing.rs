//! Known answers for sequencing (ADR-0020): what the hub numbers and how, confirmation by hash,
//! the feed, and records that disagree. The hub is the store that claimed the role (ADR-0022),
//! and its claim is numbered like any other event.

use keel_domain::hub::{Claimed, Cut, Epoch, HubEvent, Succession};
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

/// Device 4's claim of `epoch`, succeeding `previous` and cutting the hub's log at `cut`, or its
/// first claim, written by `writer`.
fn claim_of_four(
    writer: &mut LogWriter<SoftwareSigner, SeededEntropy>,
    epoch: u64,
    succeeds: Option<(&SignedEvent, u64)>,
    now: i64,
) -> SignedEvent {
    let succeeds = succeeds.map(|(previous, cut)| Succession {
        previous: previous.body().event_id,
        cuts: vec![Cut { device: own().0, position: cut }],
    });
    let claimed = Claimed::new(Epoch::new(epoch).unwrap(), NonZeroU8::MIN, succeeds).unwrap();
    let (schema, payload) = HubEvent::Claimed(claimed).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("hub").unwrap(), id: id(0x9050) };
    let draft = EventDraft { stream, schema, payload, ..draft(1, 0) };
    writer.prepare(draft, at(now)).unwrap().commit()
}

/// A sequencing record of `record`, written by `writer`, on stream `n`.
fn record_by(
    writer: &mut LogWriter<SoftwareSigner, SeededEntropy>,
    record: Assigned,
    n: u64,
    now: i64,
) -> SignedEvent {
    let (schema, payload) = SequenceEvent::Assigned(record).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(n) };
    let draft = EventDraft { stream, schema, payload, ..draft(1, 0) };
    writer.prepare(draft, at(now)).unwrap().commit()
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
    // The hub claims its role, and numbers its claim like any other event.
    let claimed = claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    assert_eq!(written.len(), 1);
    let first = record(&written[0]);
    let (hub_id, d2, d3) = (own[0].body().origin_device, device(2).0, device(3).0);
    assert_eq!((first.epoch, first.first), (EPOCH, 1));
    assert_eq!(
        spans(&first),
        [(d2, 1, 3), (d3, 1, 1), (hub_id, 1, 1), (d3, 2, 2), (d2, 4, 5), (hub_id, 2, 2)]
    );
    // Each run is pinned by its last event's hash.
    assert_eq!(first.runs[0].last, two[2].hash());
    assert_eq!(first.runs[4].last, two[4].hash());
    assert_eq!(first.runs[5].last, claimed.hash());
    // The record is the hub's own event, of a stream of its own, by the sequencer, dated as the
    // latest event it numbers.
    let body = written[0].body();
    assert_eq!((body.origin_device, body.origin_seq.get()), (hub_id, 3));
    assert_eq!(body.stream.kind.as_str(), "sequence");
    assert_eq!(body.actor, Actor::System(Component::new("sequencer").unwrap()));
    assert_eq!(body.business_date, "2026-09-29".parse().unwrap());
    // Nothing new, nothing written.
    assert_eq!(hub.sequence(at(7_500)).unwrap(), Vec::new());
    // Later events follow on, the hub's own record left out.
    take(&mut hub, &[&two[5], &three[2]]);
    let more = append(&mut hub, vec![draft(1, 2)], at(8_000));
    let written = hub.sequence(at(9_000)).unwrap();
    let second = record(&written[0]);
    assert_eq!(second.first, 10);
    assert_eq!(spans(&second), [(d2, 6, 6), (d3, 3, 3), (hub_id, 4, 4)]);
    assert_eq!(more[0].body().origin_seq.get(), 4);
    assert_eq!(hub.store_seq(hub_id, 4).unwrap(), Some(seq(12)));
    assert!(hub.check().unwrap().is_empty());
}

#[test]
fn a_record_ends_where_a_device_does_not_follow_on_or_it_is_full() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    // Device 2 wrote a record of its own between its events, of a term it never held: it counts
    // for nothing, and the hub leaves it out.
    let two_id = device(2).0;
    let mut writer = writer_of(2);
    let its_record = Assigned::new(
        EPOCH,
        40,
        vec![Run { device: device(4).0, from: 1, to: 1, last: EventHash::from_bytes([7; 32]) }],
    )
    .unwrap();
    let two = [
        writer.prepare(draft(1, 1), at(1)).unwrap().commit(),
        record_by(&mut writer, its_record, 0x9000, 2),
        writer.prepare(draft(1, 3), at(3)).unwrap().commit(),
    ];
    take(&mut hub, &[&two[0], &two[1], &two[2]]);
    claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    let records: Vec<Assigned> = written.iter().map(record).collect();
    assert_eq!(records.len(), 2);
    assert_eq!((spans(&records[0]), records[0].first), (vec![(two_id, 1, 1)], 1));
    assert_eq!((spans(&records[1]), records[1].first), (vec![(two_id, 3, 3), (own().0, 1, 1)], 2));

    // Two devices taking turns, an event each: a record holds 1,024 runs, and the next the rest.
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 513), log_of(3, here(), 512));
    let turns: Vec<&SignedEvent> =
        two.iter().zip(three.iter()).flat_map(|(a, b)| [a, b]).chain(two.last()).collect();
    take(&mut hub, &turns);
    claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    let records: Vec<Assigned> = written.iter().map(record).collect();
    assert_eq!(records.iter().map(|record| record.runs.len()).collect::<Vec<_>>(), [1024, 2]);
    assert_eq!((records[1].first, records[1].runs[0].device), (1025, two_id));
    assert_eq!(hub.store_seq(two_id, 513).unwrap(), Some(seq(1025)));
}

#[test]
fn an_event_is_confirmed_once_a_record_covers_it_with_its_hash() {
    let (hub_dir, other_dir) = (TempDir::new(), TempDir::new());
    let mut hub = open(&hub_dir.db());
    let two = log_of(2, here(), 3);
    take(&mut hub, &[&two[0], &two[1], &two[2]]);
    let claimed = claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    let hub_id = own().0;
    let d2 = device(2).0;
    // Another replica holds the record before the last event of device 2's run: of what it
    // numbers, only the hub's claim is confirmed.
    let mut other = open_as(&other_dir.db(), 3);
    take(&mut other, &[&two[0], &claimed, &written[0], &two[1]]);
    assert_eq!(other.store_seq(d2, 1).unwrap(), None);
    assert_eq!(other.store_seq(hub_id, 1).unwrap(), Some(seq(4)));
    assert_eq!(other.confirmed().unwrap(), [(hub_id, 2)].into());
    let feed = other.sequenced(EPOCH, 0, 10).unwrap();
    assert_eq!(feed, [Sequenced { number: 4, event: claimed.clone() }]);
    // Once it holds that event, the run confirms every event it covers.
    take(&mut other, &[&two[2]]);
    for position in 1..=3 {
        assert_eq!(other.store_seq(d2, position).unwrap(), Some(seq(position)));
        assert_eq!(hub.store_seq(d2, position).unwrap(), Some(seq(position)));
    }
    assert_eq!(other.store_seq(d2, 4).unwrap(), None);
    assert_eq!(other.confirmed().unwrap(), [(hub_id, 2), (d2, 3)].into());
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
    let claimed = claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    // A replica holding the other version finds the run's hash isn't its event's: the hub's
    // numbers aren't for its version, even for the events both versions share.
    let mut other = open_as(&other_dir.db(), 3);
    let held = [&other_version[0], &other_version[1], &other_version[2], &claimed, &written[0]];
    take(&mut other, &held);
    let d2 = device(2).0;
    for position in 1..=3 {
        assert_eq!(other.store_seq(d2, position).unwrap(), None);
    }
    assert!(!other.confirmed().unwrap().contains_key(&d2));
    let feed = other.sequenced(EPOCH, 0, 10).unwrap();
    assert_eq!(feed, [Sequenced { number: 4, event: claimed }]);
}

#[test]
fn the_feed_gives_confirmed_events_in_number_order() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 3), log_of(3, here(), 2));
    take(&mut hub, &[&two[0], &three[0], &two[1]]);
    let claimed = claim(&mut hub, at(6_500));
    hub.sequence(at(7_000)).unwrap();
    take(&mut hub, &[&three[1], &two[2]]);
    hub.sequence(at(8_000)).unwrap();
    let numbered = |feed: Vec<Sequenced>| -> Vec<(u64, SignedEvent)> {
        feed.into_iter().map(|entry| (entry.number, entry.event)).collect()
    };
    let all = [&two[0], &three[0], &two[1], &claimed, &three[1], &two[2]];
    let expected: Vec<(u64, SignedEvent)> =
        (1..).zip(all.iter().map(|&event| event.clone())).collect();
    assert_eq!(numbered(hub.sequenced(EPOCH, 0, 100).unwrap()), expected);
    // A page: after number 1, two events, across both records.
    assert_eq!(numbered(hub.sequenced(EPOCH, 1, 2).unwrap()), expected[1..3].to_vec());
    assert_eq!(numbered(hub.sequenced(EPOCH, 3, 2).unwrap()), expected[3..5].to_vec());
    assert_eq!(hub.sequenced(EPOCH, 6, 10).unwrap(), Vec::new());
    assert_eq!(hub.sequenced(EPOCH, 0, 0).unwrap(), Vec::new());
    assert_eq!(hub.sequenced(2, 0, 10).unwrap(), Vec::new());
}

#[test]
fn where_records_that_count_disagree_an_events_first_number_counts() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let two = log_of(2, here(), 2);
    take(&mut hub, &[&two[0], &two[1]]);
    let first_claim = claim(&mut hub, at(6_500));
    hub.sequence(at(7_000)).unwrap();
    // Device 4 claims epoch 2, holding the hub's log to its record, and numbers the same events
    // again, as no hub would; and the second again, earlier in its epoch.
    let mut writer = writer_of(4);
    let four_claim = claim_of_four(&mut writer, 2, Some((&first_claim, 2)), 9_000);
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
        .map(|(record, n)| record_by(&mut writer, record.unwrap(), n, 9_000))
        .collect();
    take(&mut hub, &[&four_claim, &events[0], &events[1], &events[2]]);
    assert_eq!(hub.store_seq(d2, 1).unwrap(), Some(seq(1)));
    assert_eq!(hub.store_seq(d2, 2).unwrap(), Some(seq(2)));
    // Epoch 2's feed leaves out what epoch 1 numbered first.
    assert_eq!(hub.sequenced(2, 0, 10).unwrap(), Vec::new());
    // The store holds the hub's role no more.
    assert!(matches!(hub.sequence(at(9_500)), Err(StoreError::NotHub)));
    assert!(hub.check().unwrap().is_empty());
    // It claims it again, in epoch 3, where its own numbers start at 1, whatever device 4's
    // records say: device 4's claim, which no record numbers, then device 2's third event and
    // the hub's claim.
    let more = log_of(2, here(), 3);
    take(&mut hub, &[&more[2]]);
    claim(&mut hub, at(10_000));
    let written = hub.sequence(at(10_500)).unwrap();
    assert_eq!((record(&written[0]).epoch, record(&written[0]).first), (3, 1));
    let four = device(4).0;
    assert_eq!(spans(&record(&written[0])), [(four, 1, 1), (d2, 3, 3), (own().0, 3, 3)]);
}

#[test]
fn a_confirmed_stretch_counts_once_the_log_is_confirmed_up_to_it() {
    let dir = TempDir::new();
    let mut replica = open_as(&dir.db(), 3);
    let two = log_of(2, here(), 3);
    take(&mut replica, &[&two[0], &two[1], &two[2]]);
    // Device 4, the hub, numbers the third event alone, and its own claim: they are confirmed,
    // and the first two aren't, so the confirmed start of device 2's log is empty.
    let mut writer = writer_of(4);
    let four_claim = claim_of_four(&mut writer, 1, None, 8_000);
    let (d2, four) = (device(2).0, device(4).0);
    let third = Assigned::new(
        EPOCH,
        3,
        vec![
            Run { device: d2, from: 3, to: 3, last: two[2].hash() },
            Run { device: four, from: 1, to: 1, last: four_claim.hash() },
        ],
    );
    let event = record_by(&mut writer, third.unwrap(), 0x9200, 9_000);
    take(&mut replica, &[&four_claim, &event]);
    assert_eq!(replica.store_seq(d2, 3).unwrap(), Some(seq(3)));
    assert_eq!(replica.store_seq(d2, 2).unwrap(), None);
    assert_eq!(replica.store_seq(four, 1).unwrap(), Some(seq(4)));
    assert_eq!(replica.confirmed().unwrap(), [(four, 2)].into());
}

#[test]
fn the_confirmed_start_of_a_log_counts_records_and_stops_at_the_first_provisional_event() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let hub_id = own().0;
    append(&mut hub, vec![draft(1, 1)], at(6_000));
    assert_eq!(hub.confirmed().unwrap(), BTreeMap::new());
    claim(&mut hub, at(6_200));
    hub.sequence(at(6_500)).unwrap();
    append(&mut hub, vec![draft(1, 2)], at(7_000));
    // The hub's sale, its claim, then its record, then a sale no record covers yet.
    assert_eq!(hub.confirmed().unwrap(), [(hub_id, 3)].into());
    hub.sequence(at(7_500)).unwrap();
    assert_eq!(hub.confirmed().unwrap(), [(hub_id, 5)].into());
    assert_eq!(hub.store_seq(hub_id, 4).unwrap(), Some(seq(3)));
    // Records aren't numbered.
    assert_eq!(hub.store_seq(hub_id, 3).unwrap(), None);
}

#[test]
fn sequencing_is_a_write_of_its_own_and_an_interrupted_one_leaves_nothing() {
    let dir = TempDir::new();
    let faults = Box::new(RefuseAt { point: Point::Stored, nth: 4, seen: 0 });
    let mut hub =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults)
            .unwrap();
    let two = log_of(2, here(), 2);
    take(&mut hub, &[&two[0], &two[1]]);
    let claimed = claim(&mut hub, at(6_500));
    // The record's append is the fourth event stored: refused.
    assert!(matches!(hub.sequence(at(7_000)), Err(StoreError::Interrupted(Point::Stored))));
    assert_eq!(hub.head(own().0).unwrap(), LogHead::of(&claimed));
    assert_eq!(hub.store_seq(device(2).0, 1).unwrap(), None);
    let written = hub.sequence(at(7_500)).unwrap();
    assert_eq!(spans(&record(&written[0])), [(device(2).0, 1, 2), (own().0, 1, 1)]);
    assert_eq!(record(&written[0]).first, 1);
}

#[test]
fn each_epoch_numbers_from_1() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let two = log_of(2, here(), 3);
    take(&mut hub, &[&two[0], &two[1]]);
    let first_claim = claim(&mut hub, at(6_500));
    hub.sequence(at(7_000)).unwrap();
    // Device 4 takes the role in epoch 2, holding the hub's log, and numbers nothing; then the
    // hub takes it back in epoch 3, and numbers from 1 again.
    let four_claim = claim_of_four(&mut writer_of(4), 2, Some((&first_claim, 2)), 7_200);
    take(&mut hub, &[&four_claim]);
    assert!(matches!(hub.sequence(at(7_500)), Err(StoreError::NotHub)));
    take(&mut hub, &[&two[2]]);
    claim(&mut hub, at(8_000));
    assert_eq!(hub.term().unwrap().map(|term| term.epoch.get()), Some(3));
    let written = hub.sequence(at(8_500)).unwrap();
    assert_eq!((record(&written[0]).epoch, record(&written[0]).first), (3, 1));
    let third = Some(StoreSeq { epoch: 3, number: 2 });
    assert_eq!(hub.store_seq(device(2).0, 3).unwrap(), third);
}

#[test]
fn the_check_finds_a_run_that_isnt_what_its_record_says() {
    let dir = TempDir::new();
    let mut hub = open(&dir.db());
    let (two, three) = (log_of(2, here(), 2), log_of(3, here(), 1));
    take(&mut hub, &[&two[0], &three[0], &two[1]]);
    claim(&mut hub, at(6_500));
    let written = hub.sequence(at(7_000)).unwrap();
    assert!(hub.check().unwrap().is_empty());
    drop(hub);
    // The record's third run, changed behind the store's back.
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
