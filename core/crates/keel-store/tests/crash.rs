//! Crash-safety tests: a child process works through a fixed sequence of writes and dies partway,
//! then the parent reopens the store and checks it.
//!
//! The child is this test binary, run again with [`CHILD`] set. After each write returns it
//! prints [`ACK`] and the write's number, so the parent knows which writes the store
//! acknowledged. It dies:
//! - at a chosen [`Point`] of a write, at each time the workload reaches that point in turn: the
//!   store's fault hook aborts the process there;
//! - or killed by the parent, a moment after a chosen write's acknowledgement, which also lands
//!   inside SQLite's own work.
//!
//! Each write appends events, one of them to a real order, receives another device's events, and
//! moves effects along: it enqueues one, starts the one before, and finishes the one before that.
//! Halfway, the store changes its key.
//!
//! After each crash, the parent reopens the store and checks that:
//! - exactly one key opens it: the old one before the rekey committed, the new one after;
//! - it knows it wasn't closed cleanly, and its full check finds nothing wrong;
//! - every acknowledged write is stored, and so is the write in flight when the crash came
//!   after its commit; no other write is;
//! - each write is stored whole or not at all, its projections and effects included: the
//!   projections equal a rebuild from the stored events, and the outbox holds exactly the
//!   effects of the writes stored;
//! - every stored log verifies: signatures, sequence numbers, links and clocks;
//! - the device's clock came back, and its writer continues its log;
//! - after a crash in a migration, the database holds nothing of it.
//!
//! Power loss also loses what the operating system hadn't written to disk yet; that needs a
//! simulated disk, which keel-sim brings (ADR-0016).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::print_stdout,
    reason = "test code: a broken assumption should fail loudly, and the child reports on stdout"
)]

mod support;

use core::sync::atomic::{AtomicU32, Ordering};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::time::Duration;

use keel_domain::order::{Line, Order};
use keel_events::event::SignedEvent;
use keel_events::keys::SoftwareSigner;
use keel_events::log::{Link, LogHead};
use keel_events::verify::DeviceRegistry;
use keel_store::{
    Effect, EffectKind, EffectState, Faults, OrderState, Point, Received, Store, StoreError,
};
use keel_types::{Hlc, Id, SeededEntropy};
use support::{
    OWN, PEERS, Scratch, at, config, created, device, domain_draft, draft, here, id, line_added,
    next_event, projection_rows, raw_reader, registry, signer, store_key_of,
};

/// Set in the child: the directory of the store it works on.
const CHILD: &str = "KEEL_STORE_CRASH_DIR";
/// Set in the child: the point to die at, and at which time it reaches it.
const POINT: &str = "KEEL_STORE_CRASH_POINT";
const NTH: &str = "KEEL_STORE_CRASH_NTH";
/// Set in the child: how many writes to make.
const WRITES: &str = "KEEL_STORE_CRASH_WRITES";
/// What the child prints after each write returns. The test harness may print on the same line
/// first, so the parent looks for it anywhere in a line.
const ACK: &str = "keel-store acknowledged write ";
/// What the child prints at the end, for each point: how often it reached it.
const REACHED: &str = "keel-store reached ";
/// What the child prints just before it aborts at a point.
const DIES: &str = "keel-store dies at ";
/// What the child prints just before it changes the store's key, and after, with how long it
/// took in microseconds.
const REKEYING: &str = "keel-store rekeying";
const REKEYED: &str = "keel-store rekeyed in ";

/// What the child prints at the end: SQLCipher's log level, which is the process's. The child is
/// a process of its own, where only the store sets it.
const LOG_LEVEL: &str = "keel-store cipher log level ";

/// The store's key, before and after the rekey: each byte of it.
const OLD_KEY: u8 = 0x4B;
const NEW_KEY: u8 = 0x4C;

/// The store changes its key before this write.
fn rekey_before(writes: usize) -> usize {
    writes / 2
}

/// The peer whose events the workload receives.
const PEER: u8 = PEERS[0];

/// Write `i` appends this many of the device's own events.
fn appends(i: usize) -> usize {
    1 + i % 3
}

/// Write `i` receives this many of the peer's events, the next ones in its log.
fn receives(i: usize) -> usize {
    i % 3
}

/// The note on the `k`-th event write `i` appends: which write it came from.
fn note(i: usize, k: usize) -> u64 {
    u64::try_from(i * 10 + k).unwrap()
}

/// The order the workload rings up: the first write creates it, and each write adds a line.
fn order() -> Id<Order> {
    id(0x7000)
}

fn line(i: usize) -> Id<Line> {
    id(0x1_0000 + u64::try_from(i).unwrap())
}

/// Write `i` appends this many events to the order.
fn order_events(i: usize) -> usize {
    if i == 0 { 2 } else { 1 }
}

/// The key of the effect write `i` enqueues.
fn effect_key(i: usize) -> Vec<u8> {
    format!("effect {i}").into_bytes()
}

/// Where the effect write `k` enqueued is after `written` writes: write `k + 1` starts it, and
/// write `k + 2` finishes it.
fn effect_state(k: usize, written: usize) -> EffectState {
    if k + 2 < written {
        EffectState::Done
    } else if k + 1 < written {
        EffectState::Running
    } else {
        EffectState::Pending
    }
}

/// The peer's log, as long as `writes` writes need.
fn peer_log(writes: usize) -> Vec<SignedEvent> {
    let count: usize = (0..writes).map(receives).sum();
    let mut head = LogHead::EMPTY;
    (0..count)
        .map(|j| {
            let event = next_event(PEER, here(), head, at(1_000 + i64::try_from(j).unwrap()), 1, 0);
            head = LogHead::of(&event);
            event
        })
        .collect()
}

fn point_name(point: Point) -> &'static str {
    match point {
        Point::Migrating => "migrating",
        Point::Rebuilding => "rebuilding",
        Point::Began => "began",
        Point::Stored => "stored",
        Point::Committing => "committing",
        Point::Committed => "committed",
        Point::Rekeying => "rekeying",
        Point::Rekeyed => "rekeyed",
        _ => panic!("a new point: {point:?}"),
    }
}

/// How often the child reached each point, in [`Point::ALL`]'s order. Each child is a process of
/// its own, so each counts from zero.
static TIMES: [AtomicU32; Point::ALL.len()] = [const { AtomicU32::new(0) }; Point::ALL.len()];

/// Counts the points the store reaches, and aborts the process at the `nth` time it reaches
/// `point`, if given as `Some((point, nth))`.
struct AbortAt(Option<(Point, u32)>);

impl Faults for AbortAt {
    fn proceed(&mut self, point: Point) -> bool {
        let index = Point::ALL.iter().position(|&each| each == point).unwrap();
        let times = TIMES[index].fetch_add(1, Ordering::Relaxed) + 1;
        if self.0 == Some((point, times)) {
            println!("{DIES}{} {times}", point_name(point));
            std::process::abort();
        }
        true
    }
}

/// The workload, run in the child process.
#[test]
#[ignore = "run by the crash tests, in a child process"]
fn crash_child() {
    let Ok(dir) = std::env::var(CHILD) else { return };
    let crash = std::env::var(POINT).ok().map(|name| {
        let point = Point::ALL.into_iter().find(|&point| point_name(point) == name).unwrap();
        (point, std::env::var(NTH).unwrap().parse().unwrap())
    });
    let writes: usize = std::env::var(WRITES).unwrap().parse().unwrap();
    let faults = Box::new(AbortAt(crash));
    let path = std::path::Path::new(&dir).join("store.db");
    let key = store_key_of(OLD_KEY);
    let mut store =
        Store::open_with_faults(path, key, config(), signer(OWN), SeededEntropy::new(11), faults)
            .unwrap();
    let registry = registry();
    let peer = peer_log(writes);
    let mut received = 0;
    for i in 0..writes {
        if i == rekey_before(writes) {
            println!("{REKEYING}");
            let started = std::time::Instant::now();
            store.rekey(store_key_of(NEW_KEY)).unwrap();
            println!("{REKEYED}{}", started.elapsed().as_micros());
        }
        let now = at(20_000 + i64::try_from(i).unwrap() * 100);
        let incoming = &peer[received..received + receives(i)];
        store
            .write(|w| {
                for k in 0..appends(i) {
                    w.append(draft(1 + u8::try_from(i % 3).unwrap(), note(i, k)), now)?;
                }
                if i == 0 {
                    w.append(domain_draft(order().cast(), &created(0x200, 2), 0), now)?;
                }
                let added =
                    w.append(domain_draft(order().cast(), &line_added(line(i), 450), 0), now)?;
                for event in incoming {
                    let outcome = w.receive(&event.to_bytes(), &registry, now)?;
                    assert!(matches!(outcome, Received::Stored(_)));
                }
                let effect = Effect {
                    key: effect_key(i),
                    kind: EffectKind::new("print.kitchen").unwrap(),
                    payload: note(i, 0).to_be_bytes().to_vec(),
                    cause: Some(added.body().event_id),
                };
                w.enqueue(&effect, now)?;
                if i >= 1 {
                    w.start(&effect_key(i - 1), now)?;
                }
                if i >= 2 {
                    w.finish(&effect_key(i - 2))?;
                }
                Ok::<(), StoreError>(())
            })
            .unwrap();
        received += receives(i);
        println!("{ACK}{i}");
    }
    for (point, times) in Point::ALL.into_iter().zip(&TIMES) {
        println!("{REACHED}{} {}", point_name(point), times.load(Ordering::Relaxed));
    }
    let any = rusqlite::Connection::open_in_memory().unwrap();
    let level: String = any.query_row("PRAGMA cipher_log_level", [], |row| row.get(0)).unwrap();
    println!("{LOG_LEVEL}{level}");
}

/// Runs the child on `scratch` for `writes` writes, dying at `crash` if given.
fn spawn_child(
    scratch: &Scratch,
    writes: usize,
    crash: Option<(Point, u32)>,
) -> std::process::Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["crash_child", "--exact", "--ignored", "--nocapture", "--test-threads", "1"])
        .env(CHILD, scratch.path())
        .env(WRITES, writes.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some((point, nth)) = crash {
        command.env(POINT, point_name(point)).env(NTH, nth.to_string());
    }
    command.spawn().unwrap()
}

/// The write a line of the child's output acknowledges, if it does.
fn acknowledges(line: &str) -> Option<usize> {
    line.split_once(ACK).map(|(_, n)| n.trim().parse().unwrap())
}

/// How many writes the child acknowledged, given its output.
fn acknowledged(stdout: &str) -> usize {
    let acked: Vec<usize> = stdout.lines().filter_map(acknowledges).collect();
    assert_eq!(acked, (0..acked.len()).collect::<Vec<_>>(), "acknowledged in order");
    acked.len()
}

/// How often the child reached each point, in [`Point::ALL`]'s order, given its output.
fn reached(stdout: &str) -> Vec<(String, usize)> {
    stdout
        .lines()
        .filter_map(|line| line.split_once(REACHED))
        .map(|(_, rest)| {
            let (name, times) = rest.trim().split_once(' ').unwrap();
            (name.to_owned(), times.parse().unwrap())
        })
        .collect()
}

/// The keys that may open the store after a child printed `stdout`: the old one until the rekey
/// began, the new one once it returned, and either in between.
fn keys_after(stdout: &str) -> &'static [u8] {
    if stdout.contains(REKEYED) {
        &[NEW_KEY]
    } else if stdout.contains(REKEYING) {
        &[OLD_KEY, NEW_KEY]
    } else {
        &[OLD_KEY]
    }
}

type TestStore = Store<SoftwareSigner, SeededEntropy>;

/// Opens the store in `scratch` with each key: exactly one must open it, one of `keys`. Returns
/// the store, and the key that opened it.
fn open_with_one_key(scratch: &Scratch, keys: &[u8]) -> (TestStore, u8) {
    let open = |key: u8| {
        Store::open(
            scratch.db(),
            store_key_of(key),
            config(),
            signer(OWN),
            SeededEntropy::new(4242),
        )
    };
    match (open(OLD_KEY), open(NEW_KEY)) {
        (Ok(store), Err(StoreError::KeyRejected)) if keys.contains(&OLD_KEY) => (store, OLD_KEY),
        (Err(StoreError::KeyRejected), Ok(store)) if keys.contains(&NEW_KEY) => (store, NEW_KEY),
        (old, new) => panic!(
            "with the old key {:?}, with the new {:?}, where {keys:?} may open it",
            old.map(|_| ()),
            new.map(|_| ())
        ),
    }
}

/// Checks the store in `scratch` after a crash, or after a whole workload if not `crashed`,
/// given the peer's log: exactly one of `keys` opens it; it holds exactly the first `written`
/// writes of the workload, for a `written` in `candidates`; every log verifies; and the device
/// carries on. Returns `written`, and the key that opened the store.
fn check_store(
    scratch: &Scratch,
    peer: &[SignedEvent],
    candidates: &[usize],
    keys: &[u8],
    crashed: bool,
) -> (usize, u8) {
    let (mut store, key) = open_with_one_key(scratch, keys);
    assert_eq!(store.recovered(), crashed, "whether the store was closed cleanly");
    assert_eq!(store.check().unwrap(), [], "the full check");
    let all_own = store.log(device(OWN), 0, 10_000).unwrap();
    let (own, ordered): (Vec<SignedEvent>, Vec<SignedEvent>) = all_own
        .iter()
        .cloned()
        .partition(|event| event.body().schema.name.as_str() == "order.noted");
    let theirs = store.log(device(PEER), 0, 10_000).unwrap();
    // Whole writes only: the stored events are exactly those of the first `written` writes.
    let written = candidates
        .iter()
        .copied()
        .find(|&written| {
            let own_count: usize = (0..written).map(appends).sum();
            let order_count: usize = (0..written).map(order_events).sum();
            let their_count: usize = (0..written).map(receives).sum();
            own.len() == own_count && ordered.len() == order_count && theirs.len() == their_count
        })
        .unwrap_or_else(|| {
            panic!(
                "{} own and {} received events aren't the first {candidates:?} writes",
                own.len(),
                theirs.len()
            )
        });
    let notes: Vec<u64> =
        (0..written).flat_map(|i| (0..appends(i)).map(move |k| note(i, k))).collect();
    let stored_notes: Vec<u64> =
        own.iter().map(|event| event.body().payload.value().unwrap().as_u64().unwrap()).collect();
    assert_eq!(stored_notes, notes, "the device's own events, in order");
    assert_eq!(theirs, peer[..theirs.len()], "the peer's events, in order");
    // The projections are what the stored events make: a rebuild changes nothing. The order has a
    // line for each write.
    let kept = projection_rows(scratch, key);
    store.rebuild_projections().unwrap();
    assert_eq!(projection_rows(scratch, key), kept, "the projections disagree with the events");
    match store.order(order()).unwrap() {
        None => assert_eq!(written, 0),
        Some(summary) => {
            assert_eq!(summary.state, OrderState::Active);
            assert_eq!(summary.live_lines, u64::try_from(written).unwrap());
        }
    }
    // The outbox holds each stored write's effect, as far as the writes after it moved it.
    let effects: Vec<(Vec<u8>, EffectState)> = store
        .effects()
        .unwrap()
        .into_iter()
        .map(|queued| (queued.effect.key, queued.state))
        .collect();
    let expected: Vec<(Vec<u8>, EffectState)> =
        (0..written).map(|k| (effect_key(k), effect_state(k, written))).collect();
    assert_eq!(effects, expected, "the outbox after {written} writes");
    // Every log verifies.
    let registry: DeviceRegistry = registry();
    for log in [&all_own, &theirs] {
        let mut head = LogHead::EMPTY;
        for event in log {
            assert_eq!(registry.verify(&event.to_bytes()).as_ref(), Ok(event));
            assert_eq!(head.link(event), Ok(Link::Next));
            head = LogHead::of(event);
        }
    }
    // The clock came back: at least every stored event's.
    let latest =
        all_own.iter().chain(&theirs).map(|event| event.body().hlc).max().unwrap_or(Hlc::ZERO);
    assert!(store.clock() >= latest, "the clock {:?} is behind {latest:?}", store.clock());
    // The device carries on its log.
    let next = store.write(|w| w.append(draft(1, 999), at(20_000))).unwrap();
    assert_eq!(store.head(device(OWN)).unwrap(), LogHead::of(&next));
    let before = all_own.last().map_or(LogHead::EMPTY, LogHead::of);
    assert_eq!(before.link(&next), Ok(Link::Next));
    assert!(next.body().hlc > latest);
    (written, key)
}

/// How often a workload of `writes` writes reaches `point`.
fn occurrences(point: Point, writes: usize) -> usize {
    match point {
        // A new store migrates, and builds its projections, once; the workload changes its key
        // once.
        Point::Migrating | Point::Rebuilding | Point::Rekeying | Point::Rekeyed => 1,
        Point::Stored => (0..writes).map(|i| appends(i) + order_events(i) + receives(i)).sum(),
        _ => writes,
    }
}

/// A crash at every point of every write, each in turn.
#[test]
fn a_crash_at_any_point_keeps_every_acknowledged_write_whole() {
    const WRITES_EACH: usize = 6;
    let peer = peer_log(WRITES_EACH);
    for point in Point::ALL {
        for nth in 1..=occurrences(point, WRITES_EACH) {
            let scratch = Scratch::new("crash");
            let child =
                spawn_child(&scratch, WRITES_EACH, Some((point, u32::try_from(nth).unwrap())));
            let output = child.wait_with_output().unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            // It died where it was told to, not of something else.
            assert!(!output.status.success(), "the child lived past {point:?} {nth}");
            let last = stdout.lines().last().unwrap_or_default();
            assert!(last.ends_with(&format!("{DIES}{} {nth}", point_name(point))), "{last}");
            let acked = acknowledged(&stdout);
            if point == Point::Migrating {
                // Nothing of the interrupted migration was kept. Looking can't tidy the WAL.
                let db = raw_reader(&scratch.db(), OLD_KEY);
                let version: i64 =
                    db.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
                assert_eq!(version, 0, "an interrupted migration left version {version}");
            }
            // A crash after a write committed leaves it stored though never acknowledged.
            let expected = if point == Point::Committed { acked + 1 } else { acked };
            // A crash before the rekey leaves the old key, and after it the new one.
            let keys = match point {
                Point::Rekeying => &[OLD_KEY],
                Point::Rekeyed => &[NEW_KEY],
                _ => keys_after(&stdout),
            };
            let (written, _) = check_store(&scratch, &peer, &[expected], keys, true);
            assert_eq!(written, expected, "{point:?} {nth}");
        }
    }
}

/// A child killed at moments spread over its writes: each write is still whole, and every
/// acknowledged one is stored.
#[test]
fn a_kill_at_any_moment_keeps_every_acknowledged_write_whole() {
    const WRITES_EACH: usize = 200;
    const KILLS: usize = 32;
    let peer = peer_log(WRITES_EACH);
    // A fixed sequence of moments: after a write's acknowledgement, a delay of up to about two
    // writes' time, so that kills land anywhere in a write, SQLite's own work included.
    let mut state: u64 = 0x5EED;
    let mut next = move |bound: u64| {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    for _ in 0..KILLS {
        let after = usize::try_from(next(u64::try_from(WRITES_EACH - 1).unwrap())).unwrap();
        let delay = Duration::from_micros(next(2_000));
        let scratch = Scratch::new("kill");
        let mut child = spawn_child(&scratch, WRITES_EACH, None);
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut output = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "the child ended early");
            output.push_str(&line);
            if acknowledges(&line) == Some(after) {
                break;
            }
        }
        std::thread::sleep(delay);
        let _ = child.kill();
        stdout.read_to_string(&mut output).unwrap();
        let _ = child.wait().unwrap();
        let acked = acknowledged(&output);
        // The kill may land after a commit and before its acknowledgement.
        let candidates = if acked == WRITES_EACH { vec![acked] } else { vec![acked, acked + 1] };
        check_store(&scratch, &peer, &candidates, keys_after(&output), acked < WRITES_EACH);
    }
}

/// A child killed at moments spread over its rekey: exactly one key opens the store, and it holds
/// every acknowledged write.
#[test]
fn a_kill_during_a_rekey_leaves_the_store_under_one_key_or_the_other() {
    const WRITES_EACH: usize = 200;
    const KILLS: u32 = 16;
    let peer = peer_log(WRITES_EACH);
    // How long the rekey takes, from a child that isn't killed.
    let timed = Scratch::new("rekey-time");
    let whole = spawn_child(&timed, WRITES_EACH, None);
    let stdout = String::from_utf8(whole.wait_with_output().unwrap().stdout).unwrap();
    let took: u64 = stdout
        .lines()
        .find_map(|line| line.split_once(REKEYED))
        .map(|(_, micros)| micros.trim().parse().unwrap())
        .unwrap();
    let mut opened_by = [0; 2];
    for kill in 0..KILLS {
        // Delays spread from the rekey's start to half again its length.
        let delay = Duration::from_micros(took * 3 * u64::from(kill) / (2 * u64::from(KILLS)));
        let scratch = Scratch::new("rekey-kill");
        let mut child = spawn_child(&scratch, WRITES_EACH, None);
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut output = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "the child ended early");
            output.push_str(&line);
            if line.contains(REKEYING) {
                break;
            }
        }
        std::thread::sleep(delay);
        let _ = child.kill();
        stdout.read_to_string(&mut output).unwrap();
        let _ = child.wait().unwrap();
        let acked = acknowledged(&output);
        let candidates = if acked == WRITES_EACH { vec![acked] } else { vec![acked, acked + 1] };
        let (_, key) =
            check_store(&scratch, &peer, &candidates, keys_after(&output), acked < WRITES_EACH);
        opened_by[usize::from(key == NEW_KEY)] += 1;
    }
    println!(
        "rekey of {took} µs: {} kills left the old key, {} the new",
        opened_by[0], opened_by[1]
    );
}

/// Without a crash the workload runs to the end, and reaches each point as often as the crash
/// test assumes, so that the crash test crashes at every one.
#[test]
fn a_whole_workload_reaches_each_point_as_often_as_the_crash_test_assumes() {
    const WRITES_EACH: usize = 6;
    let scratch = Scratch::new("whole");
    let output = spawn_child(&scratch, WRITES_EACH, None).wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(acknowledged(&stdout), WRITES_EACH);
    let expected: Vec<(String, usize)> = Point::ALL
        .into_iter()
        .map(|point| (point_name(point).to_owned(), occurrences(point, WRITES_EACH)))
        .collect();
    assert_eq!(reached(&stdout), expected);
    // The store turned SQLCipher's logging off: it reports what goes wrong itself.
    let level = stdout.lines().find_map(|line| line.split_once(LOG_LEVEL)).map(|(_, level)| level);
    assert_eq!(level, Some("NONE"));
    let checked = check_store(&scratch, &peer_log(WRITES_EACH), &[WRITES_EACH], &[NEW_KEY], false);
    assert_eq!(checked, (WRITES_EACH, NEW_KEY));
}
