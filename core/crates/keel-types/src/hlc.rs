//! Hybrid logical clocks: timestamps that respect causality even when wall clocks disagree.
//!
//! An [`Hlc`] pairs a wall-clock time in milliseconds with a logical counter. An [`HlcClock`]
//! issues HLCs that
//! - always increase, even when the wall clock jumps backwards;
//! - stay close to physical time;
//! - move past every HLC observed from another device, so an effect is always stamped after its
//!   cause.
//!
//! The algorithm is from Kulkarni, Demirbas, Madappa, Avva and Leone, "Logical Physical Clocks"
//! (2014). HLCs from different devices can be equal; events break such ties by device and
//! sequence number, which is why an HLC carries no node identifier.

use core::fmt;
use core::time::Duration;

use crate::time::Timestamp;

/// A hybrid logical clock value: 48 bits of wall time in milliseconds since the Unix epoch and
/// a 16-bit logical counter, packed into a `u64` that orders like the pair.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc(u64);

impl Hlc {
    /// The latest wall time an HLC can hold: 2^48 − 1 ms after the epoch, in the year 10889.
    pub const MAX_WALL_MS: u64 = 0xFFFF_FFFF_FFFF;
    /// The smallest HLC: the epoch, counter zero.
    pub const ZERO: Hlc = Hlc(0);

    /// An HLC from its wall time in milliseconds and its logical counter.
    ///
    /// # Errors
    /// [`HlcError::OutOfRange`] if `wall_ms` exceeds [`Hlc::MAX_WALL_MS`].
    pub const fn new(wall_ms: u64, logical: u16) -> Result<Hlc, HlcError> {
        if wall_ms > Hlc::MAX_WALL_MS {
            return Err(HlcError::OutOfRange);
        }
        let [_, _, w0, w1, w2, w3, w4, w5] = wall_ms.to_be_bytes();
        let [l0, l1] = logical.to_be_bytes();
        Ok(Hlc(u64::from_be_bytes([w0, w1, w2, w3, w4, w5, l0, l1])))
    }

    /// An HLC from its packed form. Every `u64` is a valid HLC.
    pub const fn from_u64(packed: u64) -> Hlc {
        Hlc(packed)
    }

    /// The packed form, for storage: it orders exactly like the HLC.
    pub const fn to_u64(self) -> u64 {
        self.0
    }

    /// The wall time, in milliseconds since the Unix epoch.
    pub const fn wall_ms(self) -> u64 {
        let [w0, w1, w2, w3, w4, w5, _, _] = self.0.to_be_bytes();
        u64::from_be_bytes([0, 0, w0, w1, w2, w3, w4, w5])
    }

    /// The logical counter, which orders HLCs with the same wall time.
    pub const fn logical(self) -> u16 {
        let [_, _, _, _, _, _, l0, l1] = self.0.to_be_bytes();
        u16::from_be_bytes([l0, l1])
    }
}

impl fmt::Display for Hlc {
    /// The wall time as an RFC 3339 timestamp, then the counter: `2026-09-27T14:03:00.123000Z#5`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wall =
            i64::try_from(self.wall_ms()).ok().and_then(|ms| Timestamp::from_millis(ms).ok());
        match wall {
            Some(wall) => write!(f, "{wall}#{}", self.logical()),
            // Beyond the year 9999: show the raw milliseconds.
            None => write!(f, "{}ms#{}", self.wall_ms(), self.logical()),
        }
    }
}

impl fmt::Debug for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hlc({self})")
    }
}

/// Issues hybrid logical clock values for one device.
///
/// The clock doesn't read time itself: every call takes the current physical time, which keeps
/// it deterministic. Its state is the last HLC it issued; persist it (for example with the
/// event log) and restore it with [`HlcClock::resume`], so HLCs keep increasing across restarts.
#[derive(Clone, Debug)]
pub struct HlcClock {
    last: Hlc,
    max_forward_drift_ms: u64,
}

impl HlcClock {
    /// A new clock. Remote HLCs more than `max_forward_drift` ahead of physical time are
    /// rejected by [`HlcClock::observe`]: a device with a broken clock must not drag everyone
    /// else's clocks into the future.
    pub fn new(max_forward_drift: Duration) -> HlcClock {
        HlcClock::resume(Hlc::ZERO, max_forward_drift)
    }

    /// A clock continuing after `last`, the last HLC issued before a restart.
    pub fn resume(last: Hlc, max_forward_drift: Duration) -> HlcClock {
        let max_forward_drift_ms = u64::try_from(max_forward_drift.as_millis()).unwrap_or(u64::MAX);
        HlcClock { last, max_forward_drift_ms }
    }

    /// The last HLC issued.
    pub const fn last(&self) -> Hlc {
        self.last
    }

    /// Issues an HLC for a local event, such as recording a sale, at physical time `now`.
    ///
    /// # Errors
    /// [`HlcError::Exhausted`] only if the clock has reached the year 10889.
    pub fn tick(&mut self, now: Timestamp) -> Result<Hlc, HlcError> {
        let physical = physical_ms(now);
        let (last_wall, last_logical) = (self.last.wall_ms(), self.last.logical());
        let next = if physical > last_wall {
            Hlc::new(physical, 0)?
        } else {
            advance(last_wall, last_logical)?
        };
        self.last = next;
        Ok(next)
    }

    /// Takes in an HLC received from another device at physical time `now`, and issues an HLC
    /// for the receipt: later than both the remote HLC and everything issued so far.
    ///
    /// # Errors
    /// [`HlcError::ClockDrift`] if `remote` is too far ahead of `now` (the clock is left
    /// unchanged), [`HlcError::Exhausted`] only if the clock has reached the year 10889.
    pub fn observe(&mut self, remote: Hlc, now: Timestamp) -> Result<Hlc, HlcError> {
        let physical = physical_ms(now);
        let remote_wall = remote.wall_ms();
        if remote_wall > physical.saturating_add(self.max_forward_drift_ms) {
            return Err(HlcError::ClockDrift {
                remote,
                ahead_ms: remote_wall.saturating_sub(physical),
                max_forward_drift_ms: self.max_forward_drift_ms,
            });
        }
        let (last_wall, last_logical) = (self.last.wall_ms(), self.last.logical());
        let wall = last_wall.max(remote_wall).max(physical);
        let next = if wall == last_wall && wall == remote_wall {
            advance(wall, last_logical.max(remote.logical()))?
        } else if wall == last_wall {
            advance(wall, last_logical)?
        } else if wall == remote_wall {
            advance(wall, remote.logical())?
        } else {
            Hlc::new(wall, 0)?
        };
        self.last = next;
        Ok(next)
    }
}

/// Physical time in whole milliseconds, clamped to what an HLC can hold. A clock before 1970
/// reads as the epoch.
fn physical_ms(now: Timestamp) -> u64 {
    u64::try_from(now.as_millis()).unwrap_or(0).min(Hlc::MAX_WALL_MS)
}

/// The HLC just after `(wall, logical)`: the next counter value, or, if the counter is full,
/// the next millisecond. At 65,536 events in one millisecond, running a millisecond ahead of
/// physical time is the price of never repeating a value.
fn advance(wall: u64, logical: u16) -> Result<Hlc, HlcError> {
    match logical.checked_add(1) {
        Some(logical) => Hlc::new(wall, logical),
        None => Hlc::new(wall.checked_add(1).ok_or(HlcError::Exhausted)?, 0)
            .map_err(|_| HlcError::Exhausted),
    }
}

/// Errors from hybrid logical clocks.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HlcError {
    /// A remote HLC was too far ahead of local physical time: the remote clock (or ours) is
    /// wrong. The event still counts; only the local clock refuses to jump ahead.
    #[error("remote clock is {ahead_ms} ms ahead, beyond the {max_forward_drift_ms} ms limit")]
    ClockDrift {
        /// The HLC that was rejected.
        remote: Hlc,
        /// How far ahead of local physical time it was.
        ahead_ms: u64,
        /// The configured limit.
        max_forward_drift_ms: u64,
    },
    /// The clock can't advance any further (it has reached the year 10889).
    #[error("hybrid logical clock exhausted")]
    Exhausted,
    /// A wall time beyond what an HLC can hold.
    #[error("wall time out of range for a hybrid logical clock")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRIFT: Duration = Duration::from_secs(60);

    fn ms(millis: i64) -> Timestamp {
        Timestamp::from_millis(millis).unwrap()
    }

    fn hlc(wall_ms: u64, logical: u16) -> Hlc {
        Hlc::new(wall_ms, logical).unwrap()
    }

    #[test]
    fn packing_orders_like_the_pair() {
        let value = hlc(1_790_517_780_123, 5);
        assert_eq!(value.wall_ms(), 1_790_517_780_123);
        assert_eq!(value.logical(), 5);
        assert_eq!(Hlc::from_u64(value.to_u64()), value);
        assert!(hlc(10, u16::MAX) < hlc(11, 0));
        assert!(hlc(10, 1) < hlc(10, 2));
        assert_eq!(Hlc::new(Hlc::MAX_WALL_MS, u16::MAX), Ok(Hlc::from_u64(u64::MAX)));
        assert_eq!(Hlc::new(Hlc::MAX_WALL_MS + 1, 0), Err(HlcError::OutOfRange));
        assert_eq!(value.to_string(), "2026-09-27T14:03:00.123000Z#5");
        assert_eq!(Hlc::from_u64(u64::MAX).to_string(), "281474976710655ms#65535");
    }

    #[test]
    fn tick_follows_physical_time_and_never_repeats() {
        let mut clock = HlcClock::new(DRIFT);
        assert_eq!(clock.tick(ms(1_000)), Ok(hlc(1_000, 0)));
        assert_eq!(clock.tick(ms(1_000)), Ok(hlc(1_000, 1)));
        assert_eq!(clock.tick(ms(1_005)), Ok(hlc(1_005, 0)));
        // The wall clock jumps back: the HLC holds its wall time and counts.
        assert_eq!(clock.tick(ms(900)), Ok(hlc(1_005, 1)));
        assert_eq!(clock.last(), hlc(1_005, 1));
        // Before 1970 reads as the epoch.
        let mut early = HlcClock::new(DRIFT);
        // A fresh clock's last value is ZERO, and every tick is strictly greater.
        assert_eq!(early.tick(ms(-5)), Ok(hlc(0, 1)));
    }

    #[test]
    fn a_full_counter_moves_to_the_next_millisecond() {
        let mut clock = HlcClock::resume(hlc(1_000, u16::MAX), DRIFT);
        assert_eq!(clock.tick(ms(1_000)), Ok(hlc(1_001, 0)));
        let mut end = HlcClock::resume(Hlc::from_u64(u64::MAX), DRIFT);
        assert_eq!(end.tick(ms(1_000)), Err(HlcError::Exhausted));
        assert_eq!(end.last(), Hlc::from_u64(u64::MAX));
    }

    #[test]
    fn observe_moves_past_the_remote_clock() {
        let mut clock = HlcClock::resume(hlc(1_000, 3), DRIFT);
        // Remote ahead of us and of physical time: adopt its wall time, count past it.
        assert_eq!(clock.observe(hlc(2_000, 7), ms(1_500)), Ok(hlc(2_000, 8)));
        // Same wall time on both sides: count past the larger counter.
        assert_eq!(clock.observe(hlc(2_000, 20), ms(1_500)), Ok(hlc(2_000, 21)));
        // Remote behind: keep our wall time, count up.
        assert_eq!(clock.observe(hlc(1_000, 50), ms(1_500)), Ok(hlc(2_000, 22)));
        // Physical time ahead of both: adopt it.
        assert_eq!(clock.observe(hlc(2_500, 9), ms(3_000)), Ok(hlc(3_000, 0)));
    }

    #[test]
    fn observe_rejects_a_remote_clock_too_far_ahead() {
        let mut clock = HlcClock::resume(hlc(1_000, 3), DRIFT);
        let far_ahead = hlc(1_000 + 60_001, 0);
        assert_eq!(
            clock.observe(far_ahead, ms(1_000)),
            Err(HlcError::ClockDrift {
                remote: far_ahead,
                ahead_ms: 60_001,
                max_forward_drift_ms: 60_000
            })
        );
        assert_eq!(clock.last(), hlc(1_000, 3), "a rejected observation changes nothing");
        // Exactly at the limit is accepted.
        assert_eq!(clock.observe(hlc(61_000, 0), ms(1_000)), Ok(hlc(61_000, 1)));
    }
}
