//! The simulator's one source of randomness: SplitMix64, seeded, so a seed replays exactly.

use keel_types::{Entropy, SeededEntropy};

/// A seeded random number generator.
#[derive(Debug)]
pub(crate) struct Rng(SeededEntropy);

impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(SeededEntropy::new(seed))
    }

    /// The next 64 random bits.
    pub(crate) fn next(&mut self) -> u64 {
        // SplitMix64 can't fail; the error exists for entropy from the operating system.
        self.0.next_u64().unwrap_or(0)
    }

    /// A number from 0 up to, but not including, `bound`; 0 if `bound` is 0.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next().checked_rem(bound).unwrap_or(0)
    }

    /// A number from `low` up to, but not including, `high`; `low` if the range is empty.
    pub(crate) fn between(&mut self, low: i64, high: i64) -> i64 {
        let span = high.checked_sub(low).and_then(|span| u64::try_from(span).ok()).unwrap_or(0);
        let offset = i64::try_from(self.below(span)).unwrap_or(0);
        low.saturating_add(offset)
    }

    /// True `per_mille` times in a thousand.
    pub(crate) fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }

    /// One of `items`, if there are any.
    pub(crate) fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let count = u64::try_from(items.len()).ok()?;
        items.get(usize::try_from(self.below(count)).ok()?)
    }
}
