//! Known answers for the replicator (ADR-0019), frame by frame, against the model replica.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use core::time::Duration;

use keel_events::event::SignedEvent;
use keel_sync::{Events, Frame, Have, Outgoing, Replicator, SyncConfig, VersionVector};
use keel_types::Timestamp;
use support::{Model, at, device, elsewhere, here, registry};

/// Devices 1 to 4, enrolled.
fn model(n: u8) -> Model {
    Model::new(n, registry(1..=4))
}

/// Small batches, so that a few events take several.
const CONFIG: SyncConfig = SyncConfig {
    round: Duration::from_secs(5),
    ack_timeout: Duration::from_secs(2),
    batch_events: 2,
    batch_bytes: 256 * 1024,
};

fn decoded(outgoing: &[Outgoing]) -> Vec<(u8, Frame)> {
    outgoing
        .iter()
        .map(|out| {
            let to = (1..=4).find(|n| device(*n) == out.to).unwrap();
            (to, Frame::decode(&out.frame).unwrap())
        })
        .collect()
}

fn vv(entries: &[(u8, u64)]) -> VersionVector {
    entries.iter().map(|&(n, position)| (device(n), position)).collect()
}

/// A peer's `have`, from a peer that has heard from the replica since it started.
fn have(entries: &[(u8, u64)], acked: u64) -> Vec<u8> {
    Frame::Have(Have { location: here(), vv: vv(entries), acked, asks: false }).encode()
}

/// The events of the batch in `frame`, and its number.
fn batch(frame: &Frame) -> (u64, Vec<SignedEvent>) {
    let Frame::Events(Events { batch, events }) = frame else { panic!("not a batch: {frame:?}") };
    let events = events.iter().map(|bytes| SignedEvent::from_stored(bytes).unwrap()).collect();
    (*batch, events)
}

fn positions(events: &[SignedEvent]) -> Vec<(u8, u64)> {
    events
        .iter()
        .map(|event| {
            let body = event.body();
            let n = (1..=4).find(|n| device(*n) == body.origin_device).unwrap();
            (n, body.origin_seq.get())
        })
        .collect()
}

/// Device `n`'s model holding `count` events of its own, appended a second apart.
fn with_events(n: u8, count: u64) -> Model {
    let mut model = model(n);
    for k in 1..=count {
        model.append(k, at(i64::try_from(k).unwrap() * 1000));
    }
    model
}

fn start(model: &mut Model, peers: &[u8], now: Timestamp) -> (Replicator, Vec<(u8, Frame)>) {
    let (replicator, out) =
        Replicator::start(model, peers.iter().map(|n| device(*n)), CONFIG, now).unwrap();
    (replicator, decoded(&out))
}

#[test]
fn starting_tells_each_peer_what_the_replica_holds() {
    let mut a = with_events(1, 3);
    let (replicator, out) = start(&mut a, &[2, 3, 1], at(10_000));
    // Its own device isn't a peer. Having heard from neither peer, it asks each for its `have`.
    let expected = Frame::Have(Have { location: here(), vv: vv(&[(1, 3)]), acked: 0, asks: true });
    assert_eq!(out, [(2, expected.clone()), (3, expected)]);
    assert_eq!(replicator.version_vector(), &vv(&[(1, 3)]));
    assert!(!replicator.settled());
    // Having heard from neither peer, it tells them again after the acknowledgement timeout.
    assert_eq!(replicator.next_tick(), Some(at(12_000)));
}

#[test]
fn a_replica_repeats_itself_sooner_until_it_hears_from_a_peer() {
    let mut a = with_events(1, 1);
    let (mut replicator, _) = start(&mut a, &[2], at(10_000));
    assert!(replicator.on_tick(&mut a, at(11_999)).unwrap().is_empty());
    let out = decoded(&replicator.on_tick(&mut a, at(12_000)).unwrap());
    assert!(matches!(out.as_slice(), [(2, Frame::Have(Have { asks: true, .. }))]), "{out:?}");
    // Once it has heard from the peer, a round apart, and asking for nothing.
    replicator.on_frame(&mut a, device(2), &have(&[(1, 1)], 0), at(12_500)).unwrap();
    assert_eq!(replicator.next_tick(), Some(at(17_000)));
    assert!(replicator.on_tick(&mut a, at(16_999)).unwrap().is_empty());
    let out = decoded(&replicator.on_tick(&mut a, at(17_000)).unwrap());
    assert!(matches!(out.as_slice(), [(2, Frame::Have(Have { asks: false, .. }))]), "{out:?}");
}

/// A replica that starts learns at once what a peer holds, so its device's log can settle
/// without waiting for the peer's round.
#[test]
fn a_replica_that_hasnt_heard_from_a_peer_asks_for_its_have_and_gets_it_at_once() {
    let now = at(10_000);
    let mut a = with_events(1, 2);
    let mut hub = with_events(3, 1);
    let (mut a_replicator, out) = start(&mut a, &[3], now);
    let (mut hub_replicator, _) = start(&mut hub, &[1], now);
    let [(3, asking)] = out.as_slice() else { panic!("{out:?}") };
    let out = hub_replicator.on_frame(&mut hub, device(1), &asking.encode(), now).unwrap();
    let out = decoded(&out);
    // The hub answers at once, asking nothing, since it has now heard from the device; then
    // it sends what the device lacks.
    let answer = Frame::Have(Have { location: here(), vv: vv(&[(3, 1)]), acked: 0, asks: false });
    assert_eq!(out[0], (1, answer.clone()));
    let [_, (1, frame)] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(positions(&batch(frame).1), [(3, 1)]);
    // The hub holds none of the device's log, so the answer settles it.
    a_replicator.on_frame(&mut a, device(3), &answer.encode(), now).unwrap();
    assert!(a_replicator.settled());
    // Having heard from the hub, the device asks no more.
    let out = decoded(&a_replicator.on_frame(&mut a, device(3), &frame.encode(), now).unwrap());
    assert!(matches!(out.as_slice(), [(3, Frame::Have(Have { asks: false, .. }))]), "{out:?}");
    // And a `have` that doesn't ask gets no `have` in return.
    let out = hub_replicator.on_frame(&mut hub, device(1), &have(&[(1, 2), (3, 1)], 0), now);
    assert!(out.unwrap().is_empty());
}

#[test]
fn a_peer_gets_what_it_lacks_one_acknowledged_batch_at_a_time() {
    let mut a = with_events(1, 5);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    // The peer holds the first event: batches start after it, two events at a time.
    let out = decoded(&replicator.on_frame(&mut a, device(2), &have(&[(1, 1)], 0), now).unwrap());
    let [(2, frame)] = out.as_slice() else { panic!("{out:?}") };
    let (first, events) = batch(frame);
    assert_eq!(positions(&events), [(1, 2), (1, 3)]);
    // Another `have` that doesn't acknowledge it sends nothing more.
    let out = replicator.on_frame(&mut a, device(2), &have(&[(1, 1)], 0), now).unwrap();
    assert!(out.is_empty(), "{out:?}");
    // Its acknowledgement sends the next batch, from where the peer now holds.
    let out = replicator.on_frame(&mut a, device(2), &have(&[(1, 3)], first), now).unwrap();
    let out = decoded(&out);
    let [(2, frame)] = out.as_slice() else { panic!("{out:?}") };
    let (second, events) = batch(frame);
    assert_eq!(second, first.wrapping_add(1));
    assert_eq!(positions(&events), [(1, 4), (1, 5)]);
    let out = replicator.on_frame(&mut a, device(2), &have(&[(1, 5)], second), now).unwrap();
    assert!(out.is_empty(), "the peer holds everything: {out:?}");
    assert_eq!(replicator.known(device(2)), Some(&vv(&[(1, 5)])));
    assert_eq!(replicator.stats().batches, 2);
    assert_eq!(replicator.stats().sent, 4);
}

#[test]
fn each_device_lagging_gets_a_share_of_a_batch() {
    let mut hub = model(3);
    for n in [1, 2] {
        let device_model = with_events(n, 3);
        for event in device_model.events() {
            let out = hub.receive_one(&event.to_bytes());
            assert!(matches!(out, keel_store::Received::Stored(_)));
        }
    }
    let now = at(10_000);
    let (mut replicator, _) = start(&mut hub, &[4], now);
    let out = decoded(&replicator.on_frame(&mut hub, device(4), &have(&[], 0), now).unwrap());
    let (_, events) = batch(&out[0].1);
    // Two events a batch, one from each device lagging.
    assert_eq!(positions(&events), [(1, 1), (2, 1)]);
}

/// A device restored from an older copy mustn't write until it has its own log back, so a batch
/// to a device carries its own log first, then shares the room left among the others.
#[test]
fn a_peers_own_log_comes_first_in_a_batch() {
    let mut hub = model(3);
    for n in [1, 2] {
        for event in with_events(n, 6).events() {
            hub.receive_one(&event.to_bytes());
        }
    }
    let now = at(10_000);
    // Device 1 holds four of its own events, and none of device 2's: two batches of its own.
    let (mut replicator, _) = start(&mut hub, &[1], now);
    let out = decoded(&replicator.on_frame(&mut hub, device(1), &have(&[(1, 4)], 0), now).unwrap());
    let (sent, events) = batch(&out[0].1);
    assert_eq!(positions(&events), [(1, 5), (1, 6)]);
    let out = replicator.on_frame(&mut hub, device(1), &have(&[(1, 6)], sent), now).unwrap();
    assert_eq!(positions(&batch(&decoded(&out)[0].1).1), [(2, 1), (2, 2)]);
    // Device 2 lacks one of its own: that first, then a share of the room left.
    let (mut replicator, _) = start(&mut hub, &[2], now);
    let out = decoded(&replicator.on_frame(&mut hub, device(2), &have(&[(2, 5)], 0), now).unwrap());
    assert_eq!(positions(&batch(&out[0].1).1), [(2, 6), (1, 1)]);
}

/// Seed 1645 of the simulator: a device that forked its log, writing as an island after a
/// rollback, can't take the hub's version of its own log. When that still came first, it filled
/// every batch, the device took none of each, and it never got another device's events. A peer's
/// own log that it refused takes a share like any other's.
#[test]
fn a_peers_own_log_that_it_refused_no_longer_comes_first() {
    let mut hub = model(3);
    for n in [1, 2] {
        for event in with_events(n, 6).events() {
            hub.receive_one(&event.to_bytes());
        }
    }
    let now = at(10_000);
    let (mut replicator, _) = start(&mut hub, &[1], now);
    let out = decoded(&replicator.on_frame(&mut hub, device(1), &have(&[(1, 4)], 0), now).unwrap());
    let (sent, events) = batch(&out[0].1);
    assert_eq!(positions(&events), [(1, 5), (1, 6)]);
    // The device takes none of it: a stall, and at the next round, a share for each device.
    replicator.on_frame(&mut hub, device(1), &have(&[(1, 4)], sent), now).unwrap();
    let out = decoded(&replicator.on_tick(&mut hub, at(15_000)).unwrap());
    let (sent, events) = batch(&out[1].1);
    assert_eq!(positions(&events), [(1, 5), (2, 1)]);
    // It takes the other device's, and gets more of them.
    let out = replicator.on_frame(&mut hub, device(1), &have(&[(1, 4), (2, 1)], sent), now);
    assert_eq!(positions(&batch(&decoded(&out.unwrap())[0].1).1), [(1, 5), (2, 2)]);
}

#[test]
fn a_batch_the_peer_took_none_of_waits_for_the_next_round() {
    let mut a = with_events(1, 3);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    let out = decoded(&replicator.on_frame(&mut a, device(2), &have(&[], 0), now).unwrap());
    let (sent, _) = batch(&out[0].1);
    // Acknowledged, but the peer holds none of it, as if it refused the first event.
    let out = replicator.on_frame(&mut a, device(2), &have(&[], sent), now).unwrap();
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(replicator.stats().stalls, 1);
    let later = at(12_000);
    assert!(replicator.on_tick(&mut a, later).unwrap().is_empty());
    // At the next round it tells the peer what it holds, and tries again.
    let round = at(15_000);
    let out = decoded(&replicator.on_tick(&mut a, round).unwrap());
    assert!(matches!(out[0], (2, Frame::Have(_))), "{out:?}");
    let (_, events) = batch(&out[1].1);
    assert_eq!(positions(&events), [(1, 1), (1, 2)]);
}

/// Seed 162 of the simulator: a replica stalled towards a peer that kept sending it batches,
/// each acknowledged at once. While acknowledgements counted as the replica telling the peer
/// what it holds, they held off the round that ends the stall, and the peer never got what it
/// lacked.
#[test]
fn a_peer_sending_batches_doesnt_hold_off_the_round_that_ends_a_stall() {
    let mut a = with_events(1, 3);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    let out = decoded(&replicator.on_frame(&mut a, device(2), &have(&[], 0), now).unwrap());
    let (sent, _) = batch(&out[0].1);
    replicator.on_frame(&mut a, device(2), &have(&[], sent), now).unwrap();
    assert_eq!(replicator.stats().stalls, 1);
    // The peer sends a batch every second or so, of an event the replica can't store yet; each
    // is acknowledged, and nothing else is sent.
    let gap = with_events(2, 3).events()[2].to_bytes();
    for (number, time) in [(1, 11_000), (2, 12_500), (3, 14_000), (4, 14_999)] {
        let frame = Frame::Events(Events { batch: number, events: vec![gap.clone()] }).encode();
        let out = decoded(&replicator.on_frame(&mut a, device(2), &frame, at(time)).unwrap());
        let expected =
            Frame::Have(Have { location: here(), vv: vv(&[(1, 3)]), acked: number, asks: false });
        assert_eq!(out, [(2, expected)], "at {time}");
        assert!(replicator.on_tick(&mut a, at(time)).unwrap().is_empty(), "at {time}");
    }
    assert_eq!(replicator.stats().gaps, 4);
    // The round still comes due a round after the last: the stall ends, and the batch goes again.
    assert_eq!(replicator.next_tick(), Some(at(15_000)));
    let out = decoded(&replicator.on_tick(&mut a, at(15_000)).unwrap());
    assert!(matches!(out[0], (2, Frame::Have(_))), "{out:?}");
    let (_, events) = batch(&out[1].1);
    assert_eq!(positions(&events), [(1, 1), (1, 2)]);
}

#[test]
fn a_batch_unacknowledged_in_time_is_taken_as_lost() {
    let mut a = with_events(1, 3);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    let out = decoded(&replicator.on_frame(&mut a, device(2), &have(&[], 0), now).unwrap());
    let (lost, _) = batch(&out[0].1);
    assert_eq!(replicator.next_tick(), Some(at(12_000)));
    assert!(replicator.on_tick(&mut a, at(11_999)).unwrap().is_empty());
    let out = decoded(&replicator.on_tick(&mut a, at(12_000)).unwrap());
    let [(2, frame)] = out.as_slice() else { panic!("{out:?}") };
    let (again, events) = batch(frame);
    assert_ne!(again, lost);
    assert_eq!(positions(&events), [(1, 1), (1, 2)]);
    assert_eq!(replicator.stats().timeouts, 1);
}

#[test]
fn received_events_are_acknowledged_and_passed_on_but_never_back() {
    let mut a = with_events(1, 2);
    let mut hub = model(3);
    let now = at(10_000);
    let (mut hub_replicator, _) = start(&mut hub, &[1, 4], now);
    // The hub knows what both peers hold: nothing.
    for n in [1, 4] {
        let out = hub_replicator.on_frame(&mut hub, device(n), &have(&[], 0), now).unwrap();
        assert!(out.is_empty(), "{out:?}");
    }
    let (mut a_replicator, _) = start(&mut a, &[3], now);
    let out = a_replicator.on_frame(&mut a, device(3), &have(&[], 0), now).unwrap();
    let frame = &out[0].frame;
    let (sent, _) = batch(&Frame::decode(frame).unwrap());
    let out = decoded(&hub_replicator.on_frame(&mut hub, device(1), frame, now).unwrap());
    // Acknowledged to the device, and passed on to the other peer, not back.
    let expected =
        Frame::Have(Have { location: here(), vv: vv(&[(1, 2)]), acked: sent, asks: false });
    assert_eq!(out[0], (1, expected));
    assert_eq!(out.len(), 2, "{out:?}");
    let (4, frame) = &out[1] else { panic!("{out:?}") };
    assert_eq!(positions(&batch(frame).1), [(1, 1), (1, 2)]);
    assert_eq!(hub_replicator.stats().stored, 2);
}

#[test]
fn a_replica_that_cant_store_is_sent_the_events_again() {
    let mut a = with_events(1, 2);
    let mut hub = model(3);
    let now = at(10_000);
    let (mut a_replicator, _) = start(&mut a, &[3], now);
    let (mut hub_replicator, _) = start(&mut hub, &[1], now);
    let out = a_replicator.on_frame(&mut a, device(3), &have(&[], 0), now).unwrap();
    hub.fail_next = true;
    assert!(hub_replicator.on_frame(&mut hub, device(1), &out[0].frame, now).is_err());
    assert!(hub.logs().is_empty());
    // The batch times out, and goes again.
    let out = a_replicator.on_tick(&mut a, at(12_000)).unwrap();
    let out = hub_replicator.on_frame(&mut hub, device(1), &out[0].frame, at(12_000)).unwrap();
    assert_eq!(hub.logs()[&device(1)].len(), 2);
    assert!(matches!(Frame::decode(&out[0].frame), Ok(Frame::Have(_))));
}

#[test]
fn frames_from_strangers_for_other_locations_or_undecodable_are_dropped() {
    let mut a = with_events(1, 2);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    let elsewhere_have =
        Frame::Have(Have { location: elsewhere(), vv: vv(&[]), acked: 0, asks: false });
    for (from, frame) in [
        (device(4), have(&[], 0)),
        (device(2), elsewhere_have.encode()),
        (device(2), vec![0xff]),
        (device(2), vec![0x82, 0x02, 0x00]),
    ] {
        assert!(replicator.on_frame(&mut a, from, &frame, now).unwrap().is_empty());
    }
    assert_eq!(replicator.stats().dropped, 4);
    assert_eq!(replicator.known(device(2)), None);
}

#[test]
fn a_devices_own_log_settles_once_a_peer_holds_no_more_of_it() {
    let now = at(10_000);
    // A peer that holds none of it: settled at once.
    let mut a = with_events(1, 2);
    let (mut replicator, _) = start(&mut a, &[3], now);
    replicator.on_frame(&mut a, device(3), &have(&[], 0), now).unwrap();
    assert!(replicator.settled());
    // A store that lost its last two events, which the hub holds: settled only once it holds
    // them again, and its next event follows them.
    let full = with_events(1, 4);
    let mut hub = model(3);
    for event in full.events() {
        hub.receive_one(&event.to_bytes());
    }
    let mut rolled_back = with_events(1, 2);
    let (mut replicator, _) = start(&mut rolled_back, &[3], now);
    let (mut hub_replicator, _) = start(&mut hub, &[1], now);
    let out = hub_replicator.on_frame(&mut hub, device(1), &have(&[(1, 2)], 0), now).unwrap();
    replicator.on_frame(&mut rolled_back, device(3), &have(&[(1, 4)], 0), now).unwrap();
    assert!(!replicator.settled(), "the hub holds more of its log");
    replicator.on_frame(&mut rolled_back, device(3), &out[0].frame, now).unwrap();
    assert!(replicator.settled());
    let next = rolled_back.append(5, at(20_000));
    assert_eq!(next.body().origin_seq.get(), 5);
    assert_eq!(next.body().prev_hash, full.events()[3].hash());
}

#[test]
fn a_clock_set_back_doesnt_stop_the_rounds() {
    let mut a = with_events(1, 1);
    let (mut replicator, _) = start(&mut a, &[2], at(10_000));
    let out = decoded(&replicator.on_tick(&mut a, at(9_000)).unwrap());
    assert!(matches!(out.as_slice(), [(2, Frame::Have(_))]), "{out:?}");
}

/// Something other than the replicator wrote to the store: what the replicator is told next
/// doesn't follow what it knew, so it reads again what the store holds, of every device.
#[test]
fn a_replicator_out_of_step_with_its_store_reads_it_again() {
    let now = at(10_000);
    let (other, another) = (with_events(3, 3), with_events(4, 1));
    // Two events appended, and another device's first stored, behind the replicator's back;
    // then it is told of the second event.
    let mut a = model(1);
    let (mut replicator, _) = start(&mut a, &[2], now);
    a.append(1, now);
    let second = a.append(2, now);
    a.receive_one(&other.events()[0].to_bytes());
    replicator.appended(&mut a, &[second], now).unwrap();
    assert_eq!(replicator.version_vector(), &vv(&[(1, 2), (3, 1)]));
    // More stored behind its back, then it receives the next event of device 3.
    a.receive_one(&other.events()[1].to_bytes());
    a.receive_one(&another.events()[0].to_bytes());
    let frame = Frame::Events(Events { batch: 1, events: vec![other.events()[2].to_bytes()] });
    replicator.on_frame(&mut a, device(2), &frame.encode(), now).unwrap();
    assert_eq!(replicator.version_vector(), &vv(&[(1, 2), (3, 3), (4, 1)]));
}

#[test]
fn appending_pushes_the_new_events_to_peers_lacking_them() {
    let mut a = with_events(1, 1);
    let now = at(10_000);
    let (mut replicator, _) = start(&mut a, &[2], now);
    replicator.on_frame(&mut a, device(2), &have(&[(1, 1)], 0), now).unwrap();
    let event = a.append(2, at(11_000));
    let out = decoded(&replicator.appended(&mut a, &[event], at(11_000)).unwrap());
    let [(2, frame)] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(positions(&batch(frame).1), [(1, 2)]);
    assert_eq!(replicator.version_vector(), &vv(&[(1, 2)]));
}
