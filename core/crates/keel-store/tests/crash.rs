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
//! After each crash, the parent reopens the store and checks that:
//! - every acknowledged write is stored, and so is the write in flight when the crash came
//!   after its commit; no other write is;
//! - each write is stored whole or not at all;
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

use keel_events::event::SignedEvent;
use keel_events::log::{Link, LogHead};
use keel_events::verify::DeviceRegistry;
use keel_store::{Faults, Point, Received, Store, StoreError};
use keel_types::{Hlc, SeededEntropy};
use support::{OWN, PEERS, Scratch, at, config, device, draft, here, key, next_event, registry};

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
        Point::Began => "began",
        Point::Stored => "stored",
        Point::Committing => "committing",
        Point::Committed => "committed",
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
    let mut store =
        Store::open_with_faults(path, config(), key(OWN), SeededEntropy::new(11), faults).unwrap();
    let registry = registry();
    let peer = peer_log(writes);
    let mut received = 0;
    for i in 0..writes {
        let now = at(20_000 + i64::try_from(i).unwrap() * 100);
        let incoming = &peer[received..received + receives(i)];
        store
            .write(|w| {
                for k in 0..appends(i) {
                    w.append(draft(1 + u8::try_from(i % 3).unwrap(), note(i, k)), now)?;
                }
                for event in incoming {
                    let outcome = w.receive(&event.to_bytes(), &registry, now)?;
                    assert!(matches!(outcome, Received::Stored(_)));
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

/// Checks the store in `scratch` after a crash, given the peer's log: it holds exactly the first
/// `written` writes of the workload, for a `written` in `candidates`; every log verifies; and
/// the device carries on. Returns `written`.
fn check_store(scratch: &Scratch, peer: &[SignedEvent], candidates: &[usize]) -> usize {
    let mut store =
        Store::open(scratch.db(), config(), key(OWN), SeededEntropy::new(4242)).unwrap();
    let own = store.log(device(OWN), 0, 10_000).unwrap();
    let theirs = store.log(device(PEER), 0, 10_000).unwrap();
    // Whole writes only: the stored events are exactly those of the first `written` writes.
    let written = candidates
        .iter()
        .copied()
        .find(|&written| {
            let own_count: usize = (0..written).map(appends).sum();
            let their_count: usize = (0..written).map(receives).sum();
            own.len() == own_count && theirs.len() == their_count
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
    // Every log verifies.
    let registry: DeviceRegistry = registry();
    for log in [&own, &theirs] {
        let mut head = LogHead::EMPTY;
        for event in log {
            assert_eq!(registry.verify(&event.to_bytes()).as_ref(), Ok(event));
            assert_eq!(head.link(event), Ok(Link::Next));
            head = LogHead::of(event);
        }
    }
    // The clock came back: at least every stored event's.
    let latest = own.iter().chain(&theirs).map(|event| event.body().hlc).max().unwrap_or(Hlc::ZERO);
    assert!(store.clock() >= latest, "the clock {:?} is behind {latest:?}", store.clock());
    // The device carries on its log.
    let next = store.write(|w| w.append(draft(1, 999), at(20_000))).unwrap();
    assert_eq!(store.head(device(OWN)).unwrap(), LogHead::of(&next));
    let before = own.last().map_or(LogHead::EMPTY, LogHead::of);
    assert_eq!(before.link(&next), Ok(Link::Next));
    assert!(next.body().hlc > latest);
    written
}

/// How often a workload of `writes` writes reaches `point`.
fn occurrences(point: Point, writes: usize) -> usize {
    match point {
        Point::Migrating => 1,
        Point::Stored => (0..writes).map(|i| appends(i) + receives(i)).sum(),
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
                // Nothing of the interrupted migration was kept.
                let db = rusqlite::Connection::open(scratch.db()).unwrap();
                let version: i64 =
                    db.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
                assert_eq!(version, 0, "an interrupted migration left version {version}");
            }
            // A crash after a write committed leaves it stored though never acknowledged.
            let expected = if point == Point::Committed { acked + 1 } else { acked };
            let written = check_store(&scratch, &peer, &[expected]);
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
        check_store(&scratch, &peer, &candidates);
    }
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
    assert_eq!(check_store(&scratch, &peer_log(WRITES_EACH), &[WRITES_EACH]), WRITES_EACH);
}
