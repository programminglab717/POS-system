//! Time: timestamps and injectable clocks.
//!
//! Kernel code never reads the system clock itself. It asks a [`Clock`]: production code
//! passes the operating system's clock ([`SystemClock`]), while tests and simulations pass a
//! [`ManualClock`] they control. The same inputs therefore always produce the same outputs.

use core::fmt;
use core::str::FromStr;
use core::sync::atomic::{AtomicI64, Ordering};
use core::time::Duration;

const MICROS_PER_MILLI: i64 = 1000;
const NANOS_PER_MICRO: i32 = 1000;

/// An instant, in microseconds since the Unix epoch (1970-01-01T00:00:00Z), in UTC.
///
/// Timestamps run from 0001-01-01T00:00:00Z to 9999-12-29T23:59:59.999999Z. Every one of them
/// can be written in RFC 3339 and converted to local time in any time zone. Microseconds match
/// the precision databases store.
///
/// A timestamp is a device's reading of its wall clock, and wall clocks drift, jump and
/// disagree. Kernel code orders events with hybrid logical clocks and sequence numbers, never
/// with timestamps alone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The earliest timestamp: 0001-01-01T00:00:00Z.
    pub const MIN: Timestamp = Timestamp(-62_135_596_800_000_000);
    /// The latest timestamp: 9999-12-29T23:59:59.999999Z.
    pub const MAX: Timestamp = Timestamp(253_402_127_999_999_999);
    /// 1970-01-01T00:00:00Z.
    pub const UNIX_EPOCH: Timestamp = Timestamp(0);

    /// A timestamp from microseconds since the Unix epoch.
    ///
    /// # Errors
    /// [`TimestampError::OutOfRange`] outside [`Timestamp::MIN`] to [`Timestamp::MAX`].
    pub fn from_micros(micros: i64) -> Result<Timestamp, TimestampError> {
        if (Timestamp::MIN.0..=Timestamp::MAX.0).contains(&micros) {
            Ok(Timestamp(micros))
        } else {
            Err(TimestampError::OutOfRange)
        }
    }

    /// A timestamp from milliseconds since the Unix epoch.
    ///
    /// # Errors
    /// [`TimestampError::OutOfRange`] outside [`Timestamp::MIN`] to [`Timestamp::MAX`].
    pub fn from_millis(millis: i64) -> Result<Timestamp, TimestampError> {
        let micros = millis.checked_mul(MICROS_PER_MILLI).ok_or(TimestampError::OutOfRange)?;
        Timestamp::from_micros(micros)
    }

    /// Microseconds since the Unix epoch.
    pub const fn as_micros(self) -> i64 {
        self.0
    }

    /// Milliseconds since the Unix epoch, rounded down: toward the past, also before 1970.
    pub const fn as_millis(self) -> i64 {
        self.0.div_euclid(MICROS_PER_MILLI)
    }

    /// `self + duration`, with the duration truncated to whole microseconds.
    ///
    /// Returns `None` past [`Timestamp::MAX`].
    pub fn checked_add(self, duration: Duration) -> Option<Timestamp> {
        let micros = i64::try_from(duration.as_micros()).ok()?;
        Timestamp::from_micros(self.0.checked_add(micros)?).ok()
    }

    /// `self - duration`, with the duration truncated to whole microseconds.
    ///
    /// Returns `None` before [`Timestamp::MIN`].
    pub fn checked_sub(self, duration: Duration) -> Option<Timestamp> {
        let micros = i64::try_from(duration.as_micros()).ok()?;
        Timestamp::from_micros(self.0.checked_sub(micros)?).ok()
    }

    /// The time elapsed from `earlier` to `self`, or `None` if `earlier` is later.
    pub fn duration_since(self, earlier: Timestamp) -> Option<Duration> {
        let micros = u64::try_from(self.0.checked_sub(earlier.0)?).ok()?;
        Some(Duration::from_micros(micros))
    }

    /// The same instant as a `jiff` timestamp, for calendar and time zone arithmetic.
    pub(crate) fn to_jiff(self) -> Result<jiff::Timestamp, TimestampError> {
        // Always succeeds: jiff's range, -9999-01-02 to 9999-12-30, contains ours.
        jiff::Timestamp::from_microsecond(self.0).map_err(|_| TimestampError::OutOfRange)
    }

    /// A timestamp from a `jiff` timestamp, which must be a whole number of microseconds.
    pub(crate) fn from_jiff(timestamp: jiff::Timestamp) -> Result<Timestamp, TimestampError> {
        if timestamp.subsec_nanosecond().checked_rem(NANOS_PER_MICRO) != Some(0) {
            return Err(TimestampError::TooPrecise);
        }
        Timestamp::from_micros(timestamp.as_microsecond())
    }
}

impl fmt::Display for Timestamp {
    /// Canonical RFC 3339 in UTC, always with six fractional digits, so the text sorts like the
    /// timestamps: `2026-09-27T14:03:00.123456Z`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = jiff::tz::Offset::UTC.to_datetime(self.to_jiff().map_err(|_| fmt::Error)?);
        let micros = time.subsec_nanosecond().checked_div(NANOS_PER_MICRO).ok_or(fmt::Error)?;
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{micros:06}Z",
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second(),
        )
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({self})")
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    /// Parses an RFC 3339 timestamp with any UTC offset, such as `2026-09-27T14:03:00Z` or
    /// `2026-09-27T16:03:00.5+02:00`. Fractions finer than a microsecond are rejected rather
    /// than rounded.
    fn from_str(text: &str) -> Result<Timestamp, TimestampError> {
        let timestamp: jiff::Timestamp =
            text.parse().map_err(|_| TimestampError::InvalidFormat(text.to_owned()))?;
        Timestamp::from_jiff(timestamp)
    }
}

/// Errors from creating a [`Timestamp`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TimestampError {
    /// Outside [`Timestamp::MIN`] to [`Timestamp::MAX`].
    #[error("timestamp out of range: 0001-01-01 to 9999-12-29 (UTC) are supported")]
    OutOfRange,
    /// Text that isn't an RFC 3339 timestamp.
    #[error("invalid RFC 3339 timestamp {0:?}")]
    InvalidFormat(String),
    /// A fraction of a second finer than a microsecond.
    #[error("timestamp is more precise than a microsecond")]
    TooPrecise,
}

/// A source of the current time.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> Timestamp;
}

/// A clock that only moves when told to, for tests and deterministic simulation. It can also
/// jump backwards, as real clocks do when corrected.
#[derive(Debug)]
pub struct ManualClock {
    micros: AtomicI64,
}

impl ManualClock {
    /// A clock reading `start`.
    pub const fn new(start: Timestamp) -> ManualClock {
        ManualClock { micros: AtomicI64::new(start.0) }
    }

    /// Sets the clock, forwards or backwards.
    pub fn set(&self, now: Timestamp) {
        self.micros.store(now.0, Ordering::SeqCst);
    }

    /// Moves the clock forward by `duration` and returns the new time.
    ///
    /// # Errors
    /// [`TimestampError::OutOfRange`] if the new time would be past [`Timestamp::MAX`]; the
    /// clock is then left unchanged.
    pub fn advance(&self, duration: Duration) -> Result<Timestamp, TimestampError> {
        let mut advanced = Err(TimestampError::OutOfRange);
        // `fetch_update` retries if another thread moved the clock meanwhile.
        let _previous = self.micros.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |micros| {
            advanced = Timestamp(micros).checked_add(duration).ok_or(TimestampError::OutOfRange);
            advanced.as_ref().ok().map(|timestamp| timestamp.0)
        });
        advanced
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.micros.load(Ordering::SeqCst))
    }
}

/// The operating system's wall clock.
///
/// Readings outside [`Timestamp::MIN`] to [`Timestamp::MAX`] (a badly broken clock) are clamped
/// to that range.
#[cfg(feature = "os")]
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

#[cfg(feature = "os")]
impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        use std::time::{SystemTime, UNIX_EPOCH};
        let micros = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_micros()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_micros())
                .ok()
                .and_then(i64::checked_neg)
                .unwrap_or(i64::MIN),
        };
        Timestamp(micros.clamp(Timestamp::MIN.0, Timestamp::MAX.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn range_is_printable_and_convertible_everywhere() {
        assert_eq!(Timestamp::MIN.to_string(), "0001-01-01T00:00:00.000000Z");
        assert_eq!(Timestamp::MAX.to_string(), "9999-12-29T23:59:59.999999Z");
        // Both ends convert to civil time at the most extreme offsets jiff supports.
        for end in [Timestamp::MIN, Timestamp::MAX] {
            let instant = end.to_jiff().unwrap();
            for seconds in [-93_599, 93_599] {
                let offset = jiff::tz::Offset::from_seconds(seconds).unwrap();
                let _local = offset.to_datetime(instant);
            }
        }
        assert_eq!(Timestamp::UNIX_EPOCH.to_string(), "1970-01-01T00:00:00.000000Z");
        let before_min = Timestamp::MIN.as_micros().checked_sub(1).unwrap();
        let after_max = Timestamp::MAX.as_micros().checked_add(1).unwrap();
        assert_eq!(Timestamp::from_micros(before_min), Err(TimestampError::OutOfRange));
        assert_eq!(Timestamp::from_micros(after_max), Err(TimestampError::OutOfRange));
        assert_eq!(Timestamp::from_millis(i64::MAX), Err(TimestampError::OutOfRange));
    }

    #[test]
    fn display_and_parse() {
        let instant = at("2026-09-27T14:03:00.123456Z");
        assert_eq!(instant.to_string(), "2026-09-27T14:03:00.123456Z");
        assert_eq!(instant.as_micros(), 1_790_517_780_123_456);
        // Offsets are converted to UTC; short fractions are fine.
        assert_eq!(at("2026-09-27T16:03:00.123456+02:00"), instant);
        assert_eq!(at("2026-09-27T14:03:00.5Z").to_string(), "2026-09-27T14:03:00.500000Z");
        assert_eq!(format!("{instant:?}"), "Timestamp(2026-09-27T14:03:00.123456Z)");
        assert_eq!(
            "2026-09-27T14:03:00.1234567Z".parse::<Timestamp>(),
            Err(TimestampError::TooPrecise)
        );
        for text in ["", "2026-09-27", "2026-09-27T14:03:00", "yesterday", "1790517780"] {
            assert_eq!(
                text.parse::<Timestamp>(),
                Err(TimestampError::InvalidFormat(text.to_owned())),
                "{text:?}"
            );
        }
    }

    #[test]
    fn milliseconds_round_toward_the_past() {
        assert_eq!(Timestamp::from_micros(1_999).unwrap().as_millis(), 1);
        assert_eq!(Timestamp::from_micros(-1).unwrap().as_millis(), -1);
        assert_eq!(Timestamp::from_micros(-1_000).unwrap().as_millis(), -1);
        assert_eq!(Timestamp::from_micros(-1_001).unwrap().as_millis(), -2);
        assert_eq!(at("1969-12-31T23:59:59.999999Z").as_millis(), -1);
    }

    #[test]
    fn duration_arithmetic() {
        let start = at("2026-09-27T14:03:00Z");
        let later = start.checked_add(Duration::from_millis(1_500)).unwrap();
        assert_eq!(later, at("2026-09-27T14:03:01.5Z"));
        assert_eq!(later.duration_since(start), Some(Duration::from_millis(1_500)));
        assert_eq!(start.duration_since(later), None);
        assert_eq!(later.checked_sub(Duration::from_millis(1_500)), Some(start));
        // Sub-microsecond parts are truncated.
        assert_eq!(start.checked_add(Duration::from_nanos(999)), Some(start));
        assert_eq!(Timestamp::MAX.checked_add(Duration::from_micros(1)), None);
        assert_eq!(Timestamp::MIN.checked_sub(Duration::from_micros(1)), None);
        assert_eq!(start.checked_add(Duration::MAX), None);
    }

    #[test]
    fn manual_clock() {
        let clock = ManualClock::new(at("2026-09-27T14:03:00Z"));
        assert_eq!(clock.now(), at("2026-09-27T14:03:00Z"));
        assert_eq!(clock.advance(Duration::from_secs(60)), Ok(at("2026-09-27T14:04:00Z")));
        assert_eq!(clock.now(), at("2026-09-27T14:04:00Z"));
        clock.set(at("2026-09-27T14:00:00Z"));
        assert_eq!(clock.now(), at("2026-09-27T14:00:00Z"));
        clock.set(Timestamp::MAX);
        assert_eq!(clock.advance(Duration::from_micros(1)), Err(TimestampError::OutOfRange));
        assert_eq!(clock.now(), Timestamp::MAX);
    }

    #[cfg(feature = "os")]
    #[test]
    fn system_clock_reads_a_plausible_time() {
        let now = SystemClock.now();
        assert!(now > at("2024-01-01T00:00:00Z"), "{now}");
        assert!(now < at("2100-01-01T00:00:00Z"), "{now}");
    }
}
