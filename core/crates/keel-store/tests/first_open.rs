//! Stores opened at once on several threads, as the process's first use of SQLite: this test is
//! alone in its process, so that nothing has used SQLite before it.
//!
//! SQLite runs SQLCipher's initialization only after it has marked itself initialized and let
//! other threads on, so a store opened on another thread meanwhile had its key refused, and
//! failed to open ([`keel_store::init_sqlite`]). The race needs the thread initializing SQLite to
//! stop for a moment within a few instructions, so no test can force it. Before the fix, this
//! test failed in 44 of 2,000 runs, four at a time, and after it in none of 2,000.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

mod support;

use std::sync::{Arc, Barrier};
use std::thread;

use keel_store::Store;
use keel_types::SeededEntropy;
use support::{OWN, Scratch, config, signer, store_key};

#[test]
fn stores_opened_at_once_on_several_threads_all_open() {
    const THREADS: usize = 32;
    let barrier = Arc::new(Barrier::new(THREADS));
    let threads: Vec<_> = (0..THREADS)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let dir = Scratch::new("first-open");
                let (key, signer) = (store_key(), signer(OWN));
                barrier.wait();
                Store::open(dir.db(), key, config(), signer, SeededEntropy::new(1)).map(|_| ())
            })
        })
        .collect();
    let failed: Vec<_> =
        threads.into_iter().filter_map(|thread| thread.join().unwrap().err()).collect();
    assert!(failed.is_empty(), "{} of {THREADS} failed to open: {failed:?}", failed.len());
}
