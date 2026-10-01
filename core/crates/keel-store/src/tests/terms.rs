//! Known answers for hub terms (ADR-0022): claiming the hub's role, the chain of terms the
//! claims make, the records each term's cut fences, a split brain healing, a store that is
//! behind, and the check.

use core::num::NonZeroU8;

use keel_domain::hub::{Claimed, Epoch, HubEvent, Term};
use keel_domain::schema::DomainEvent;
use keel_domain::sequence::{Assigned, Run, SequenceEvent};
use keel_events::envelope::Component;
use keel_events::hash::EventHash;

use super::*;
use crate::StoreSeq;

/// The store of device `n`, at `path`.
fn open_as(path: &Path, n: u8) -> TestStore {
    let (device, signer) = device(n);
    let config = StoreConfig { device, ..config() };
    Store::open(path, key(), config, signer, SeededEntropy::new(u64::from(n))).unwrap()
}

/// Receives `events`, in order, in one write, each the next of its device's log or one the
/// store holds already.
fn take(store: &mut TestStore, events: &[SignedEvent]) {
    store
        .write(|w| {
            for event in events {
                let received = w.receive(&event.to_bytes(), &registry(), at(5_000))?;
                assert!(
                    matches!(received, Received::Stored(_) | Received::Duplicate),
                    "{received:?}"
                );
            }
            Ok::<_, StoreError>(())
        })
        .unwrap();
}

/// Everything `from` holds of each of `devices`' logs.
fn logs(from: &TestStore, devices: &[u8]) -> Vec<SignedEvent> {
    devices.iter().flat_map(|&n| from.log(device(n).0, 0, 1_000).unwrap()).collect()
}

fn claimed(event: &SignedEvent) -> Claimed {
    let body = event.body();
    let Ok(HubEvent::Claimed(claimed)) = HubEvent::decode(&body.schema, &body.payload) else {
        panic!("not a claim: {event:?}");
    };
    claimed
}

fn epoch(n: u64) -> Epoch {
    Epoch::new(n).unwrap()
}

fn seq(epoch: u64, number: u64) -> StoreSeq {
    StoreSeq { epoch, number }
}

fn priority(n: u8) -> NonZeroU8 {
    NonZeroU8::new(n).unwrap()
}

#[test]
fn a_store_claims_the_first_epoch_and_then_is_the_hub() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    assert_eq!(store.term().unwrap(), None);
    assert_eq!(store.terms().unwrap(), []);
    // Not the hub, it neither numbers nor answers.
    assert!(matches!(store.sequence(at(50)), Err(StoreError::NotHub)));
    assert!(matches!(store.answer_requests(at(50)), Err(StoreError::NotHub)));
    let claim = store.claim(priority(7), at(100)).unwrap().unwrap();
    assert_eq!(claimed(&claim), Claimed::new(Epoch::FIRST, priority(7), None).unwrap());
    // A claim is the claimant's own event, of a stream of its own, by the hub. An empty store
    // dates it by the UTC calendar.
    let body = claim.body();
    assert_eq!((body.origin_device, body.origin_seq.get()), (own().0, 1));
    assert_eq!(body.stream.kind.as_str(), "hub");
    assert_eq!(body.actor, Actor::System(Component::new("hub").unwrap()));
    assert_eq!(body.business_date, "2026-09-28".parse().unwrap());
    let term = Term {
        epoch: Epoch::FIRST,
        device: own().0,
        claim: body.event_id,
        after: 1,
        through: None,
    };
    assert_eq!(store.term().unwrap(), Some(term));
    assert_eq!(store.terms().unwrap(), [term]);
    // The hub claims nothing more.
    assert_eq!(store.claim(priority(7), at(200)).unwrap(), None);
    assert_eq!(store.head(own().0).unwrap(), LogHead::of(&claim));
    assert_eq!(store.sequence(at(300)).unwrap().len(), 1);
    assert!(store.check().unwrap().is_empty());
}

#[test]
fn a_claim_is_dated_as_the_latest_event_the_store_stored() {
    let dir = TempDir::new();
    let mut store = open_as(&dir.db(), 2);
    let mut writer = writer_of(3);
    let dated = |date: &str| EventDraft { business_date: date.parse().unwrap(), ..draft(1, 1) };
    let events = vec![
        writer.prepare(dated("2026-10-02"), at(1_000)).unwrap().commit(),
        writer.prepare(dated("2026-09-30"), at(1_001)).unwrap().commit(),
    ];
    take(&mut store, &events);
    let claim = claim(&mut store, at(2_000));
    assert_eq!(claim.body().business_date, "2026-09-30".parse().unwrap());
}

#[test]
fn a_successor_fences_what_the_hub_it_succeeds_wrote_after_it_claimed() {
    let dir = TempDir::new();
    let (hub_id, two_id, three_id) = (own().0, device(2).0, device(3).0);
    let three = log_of(3, here(), 3);
    // The hub numbers its claim and device 3's first two events.
    let mut hub = open_hub(&dir.db());
    take(&mut hub, &three[..2]);
    hub.sequence(at(1_100)).unwrap();
    // The standby holds that, and device 3's third event, and claims epoch 2.
    let mut standby = open_as(&dir.file("standby.db"), 2);
    take(&mut standby, &hub.log(hub_id, 0, 10).unwrap());
    take(&mut standby, &three);
    let succession = claim(&mut standby, at(2_000));
    let cut = claimed(&succession).succeeds.unwrap().cuts;
    assert_eq!(claimed(&succession).epoch, epoch(2));
    assert_eq!(cut.iter().map(|cut| (cut.device, cut.position)).collect::<Vec<_>>(), [(hub_id, 2)]);
    // Unaware, the hub numbers the third event too, in epoch 1, past the standby's cut.
    take(&mut hub, &three[2..]);
    hub.sequence(at(2_100)).unwrap();
    assert_eq!(hub.store_seq(three_id, 3).unwrap(), Some(seq(1, 4)));
    // The standby numbers it in epoch 2, with its own claim.
    standby.sequence(at(2_200)).unwrap();
    assert_eq!(standby.store_seq(three_id, 3).unwrap(), Some(seq(2, 1)));
    assert_eq!(standby.store_seq(two_id, 1).unwrap(), Some(seq(2, 2)));
    // A replica holding everything counts the hub's records only as far as the cut.
    let mut other = open_as(&dir.file("other.db"), 4);
    take(&mut other, &logs(&hub, &[1, 3]));
    take(&mut other, &logs(&standby, &[2]));
    for store in [&mut standby, &mut other] {
        assert_eq!(store.store_seq(hub_id, 1).unwrap(), Some(seq(1, 1)));
        assert_eq!(store.store_seq(three_id, 2).unwrap(), Some(seq(1, 3)));
        assert_eq!(store.store_seq(three_id, 3).unwrap(), Some(seq(2, 1)));
        let feed: Vec<u64> =
            store.sequenced(1, 0, 10).unwrap().iter().map(|entry| entry.number).collect();
        assert_eq!(feed, [1, 2, 3]);
        assert!(store.check().unwrap().is_empty());
    }
    assert_eq!(other.terms().unwrap(), standby.terms().unwrap());
    // The hub learns of the standby's claim: it isn't the hub, and its number for the third
    // event is withdrawn, until it holds the standby's.
    take(&mut hub, &standby.log(two_id, 0, 1).unwrap());
    assert_eq!(hub.term().unwrap().map(|term| term.device), Some(two_id));
    assert!(matches!(hub.sequence(at(3_000)), Err(StoreError::NotHub)));
    assert_eq!(hub.store_seq(three_id, 3).unwrap(), None);
    take(&mut hub, &logs(&standby, &[2]));
    assert_eq!(hub.store_seq(three_id, 3).unwrap(), Some(seq(2, 1)));
    assert_eq!(hub.confirmed().unwrap(), other.confirmed().unwrap());
}

#[test]
fn a_store_behind_the_chain_cannot_claim_until_it_catches_up() {
    let dir = TempDir::new();
    let hub_id = own().0;
    let mut hub = open_hub(&dir.db());
    take(&mut hub, &log_of(3, here(), 2));
    hub.sequence(at(1_100)).unwrap();
    let mut standby = open_as(&dir.file("standby.db"), 2);
    take(&mut standby, &hub.log(hub_id, 0, 10).unwrap());
    claim(&mut standby, at(2_000));
    // Holding the standby's claim, but not the claim it succeeds: the chain isn't whole.
    let mut late = open_as(&dir.file("late.db"), 4);
    take(&mut late, &logs(&standby, &[2]));
    assert!(matches!(late.claim(priority(1), at(3_000)), Err(StoreError::Behind)));
    // Holding the hub's claim, but not its record, which the standby's cut keeps counting.
    take(&mut late, &hub.log(hub_id, 0, 1).unwrap());
    assert_eq!(late.terms().unwrap().len(), 2);
    assert!(matches!(late.claim(priority(1), at(3_100)), Err(StoreError::Behind)));
    // Caught up, it claims epoch 3.
    take(&mut late, &hub.log(hub_id, 1, 10).unwrap());
    let claim = claim(&mut late, at(3_200));
    assert_eq!(claimed(&claim).epoch, epoch(3));
    assert_eq!(late.head(device(4).0).unwrap(), LogHead::of(&claim));
}

#[test]
fn of_two_claims_of_one_epoch_one_wins_and_the_other_side_is_numbered_again() {
    let dir = TempDir::new();
    let (two_id, three_id, four_id) = (device(2).0, device(3).0, device(4).0);
    let hub = open_hub(&dir.db());
    let hub_log = hub.log(own().0, 0, 10).unwrap();
    // The store splits: devices 2 and 3 each claim epoch 2, each on its side, at priorities 2
    // and 1, and each numbers what it holds there.
    let mut left = open_as(&dir.file("left.db"), 2);
    let mut right = open_as(&dir.file("right.db"), 3);
    take(&mut left, &hub_log);
    take(&mut right, &hub_log);
    left.claim(priority(2), at(2_000)).unwrap().unwrap();
    right.claim(priority(1), at(2_000)).unwrap().unwrap();
    let four = log_of(4, here(), 2);
    take(&mut right, &four);
    right.sequence(at(2_100)).unwrap();
    left.sequence(at(2_100)).unwrap();
    assert_eq!(right.store_seq(four_id, 2).unwrap(), Some(seq(2, 4)));
    // The split heals. The left's claim wins: the right numbers no more, and what it numbered
    // counts for nothing, until the left numbers it again.
    take(&mut right, &logs(&left, &[2]));
    take(&mut left, &logs(&right, &[3, 4]));
    assert!(matches!(right.sequence(at(3_000)), Err(StoreError::NotHub)));
    assert_eq!(right.store_seq(four_id, 2).unwrap(), None);
    left.sequence(at(3_000)).unwrap();
    take(&mut right, &logs(&left, &[2]));
    for store in [&mut left, &mut right] {
        assert_eq!(store.term().unwrap().map(|term| term.device), Some(two_id));
        assert_eq!(store.store_seq(four_id, 2).unwrap().map(|seq| seq.epoch), Some(2));
        assert!(store.store_seq(three_id, 1).unwrap().is_some());
        assert!(store.check().unwrap().is_empty());
    }
    assert_eq!(left.terms().unwrap(), right.terms().unwrap());
    assert_eq!(left.confirmed().unwrap(), right.confirmed().unwrap());
    // Every event of the right's log but its records is confirmed: its claim and nothing else.
    assert_eq!(left.confirmed().unwrap().get(&three_id), Some(&2));
}

#[test]
fn a_hub_elected_again_numbers_what_only_a_record_of_its_cut_off_numbered() {
    let dir = TempDir::new();
    let hub_id = own().0;
    // Device 3's log holds a record between two events, so the hub's runs of it don't follow
    // on: the hub numbers device 3's first event in one record, and its third, then the hub's
    // own claim, in a second, written with it.
    let mut writer = writer_of(3);
    let stray = Assigned::new(
        9,
        1,
        vec![Run { device: device(4).0, from: 1, to: 1, last: EventHash::from_bytes([3; 32]) }],
    )
    .unwrap();
    let (schema, payload) = SequenceEvent::Assigned(stray).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(0x9300) };
    let three = vec![
        writer.prepare(draft(1, 1), at(1)).unwrap().commit(),
        writer
            .prepare(EventDraft { stream, schema, payload, ..draft(1, 0) }, at(2))
            .unwrap()
            .commit(),
        writer.prepare(draft(1, 3), at(3)).unwrap().commit(),
    ];
    let mut hub = open(&dir.db());
    take(&mut hub, &three);
    claim(&mut hub, at(100));
    let written = hub.sequence(at(200)).unwrap();
    assert_eq!(written.len(), 2);
    // The standby holds the first record but not the second, and claims epoch 2.
    let mut standby = open_as(&dir.file("standby.db"), 2);
    take(&mut standby, &three);
    take(&mut standby, &hub.log(hub_id, 0, 2).unwrap());
    claim(&mut standby, at(300));
    // The hub claims epoch 3. Its own claim, numbered only by the second record, which the
    // standby cut off, is pending again: so is device 3's third event.
    take(&mut hub, &logs(&standby, &[2]));
    claim(&mut hub, at(400));
    assert_eq!(hub.store_seq(hub_id, 1).unwrap(), None);
    hub.sequence(at(500)).unwrap();
    assert_eq!(hub.store_seq(hub_id, 1).unwrap().map(|seq| seq.epoch), Some(3));
    assert_eq!(hub.store_seq(device(3).0, 3).unwrap().map(|seq| seq.epoch), Some(3));
    assert_eq!(hub.sequence(at(600)).unwrap(), []);
    // Every event the hub holds is numbered, but records.
    let confirmed = hub.confirmed().unwrap();
    assert_eq!(confirmed.get(&hub_id), hub.version_vector().unwrap().get(&hub_id));
    assert_eq!(confirmed.get(&device(3).0), Some(&3));
    assert_eq!(confirmed.get(&device(2).0), Some(&1));
}

#[test]
fn a_record_before_its_terms_claim_counts_for_nothing() {
    let dir = TempDir::new();
    let mut hub = open_hub(&dir.db());
    let first = hub.log(own().0, 0, 1).unwrap().remove(0);
    let three = log_of(3, here(), 2);
    take(&mut hub, &three);
    // Device 4 numbers device 3's events in epoch 2 before it claims epoch 2, as no hub would.
    let mut writer = writer_of(4);
    let early = Assigned::new(
        2,
        1,
        vec![Run { device: device(3).0, from: 1, to: 2, last: three[1].hash() }],
    )
    .unwrap();
    let (schema, payload) = SequenceEvent::Assigned(early).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("sequence").unwrap(), id: id(0x9400) };
    let record = writer
        .prepare(EventDraft { stream, schema, payload, ..draft(1, 0) }, at(2_000))
        .unwrap()
        .commit();
    let succeeds = keel_domain::hub::Succession {
        previous: first.body().event_id,
        cuts: vec![keel_domain::hub::Cut { device: own().0, position: 1 }],
    };
    let claimed = Claimed::new(epoch(2), priority(1), Some(succeeds)).unwrap();
    let (schema, payload) = HubEvent::Claimed(claimed).encode().unwrap();
    let stream = StreamRef { kind: StreamKind::new("hub").unwrap(), id: id(0x9401) };
    let claim = writer
        .prepare(EventDraft { stream, schema, payload, ..draft(1, 0) }, at(2_001))
        .unwrap()
        .commit();
    take(&mut hub, &[record, claim]);
    assert_eq!(hub.term().unwrap().map(|term| (term.epoch, term.after)), Some((epoch(2), 2)));
    assert_eq!(hub.store_seq(device(3).0, 1).unwrap(), None);
    assert!(!hub.confirmed().unwrap().contains_key(&device(3).0));
}

#[test]
fn the_check_finds_a_chain_that_isnt_what_the_claims_make() {
    let dir = TempDir::new();
    let mut hub = open_hub(&dir.db());
    let mut standby = open_as(&dir.file("standby.db"), 2);
    take(&mut standby, &hub.log(own().0, 0, 10).unwrap());
    claim(&mut standby, at(2_000));
    take(&mut hub, &logs(&standby, &[2]));
    assert!(hub.check().unwrap().is_empty());
    let terms = hub.terms().unwrap();
    drop(hub);
    // The first term's cut, changed behind the store's back.
    let db = raw(&dir.db());
    db.execute("UPDATE terms SET through = 5 WHERE epoch = 1", []).unwrap();
    drop(db);
    let mut hub = open(&dir.db());
    assert_eq!(hub.check().unwrap(), [Problem::Terms]);
    hub.rebuild_projections().unwrap();
    assert!(hub.check().unwrap().is_empty());
    assert_eq!(hub.terms().unwrap(), terms);
}
