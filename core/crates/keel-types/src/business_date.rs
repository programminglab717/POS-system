//! Business dates: the day a sale is reported under.
//!
//! A bar open until 2 a.m. reports its after-midnight sales under the previous day. Each
//! location therefore has a [`BusinessDayPolicy`]: a time zone, and the local time (the cutoff)
//! at which one business day ends and the next begins. Business date `D` covers the instants
//! from `D` at the cutoff, local time, up to `D + 1` at the cutoff.
//!
//! Daylight saving time makes some local times happen twice and others not at all. A cutoff in
//! a repeated hour means its first occurrence; a cutoff in a skipped hour moves forward by the
//! length of the gap (the "compatible" rule of RFC 5545 and JavaScript's Temporal). So around a
//! transition a business day lasts 23 or 25 hours, and every instant belongs to exactly one
//! business date.
//!
//! Time zone rules come from the IANA database bundled into Keel, never from the device's
//! operating system, so every device computes the same business dates. Updating Keel updates
//! the rules.

use core::fmt;
use core::str::FromStr;

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;

use crate::time::Timestamp;

/// The calendar date that a business reports under.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BusinessDate(Date);

impl BusinessDate {
    /// A business date from its year (1 to 9999), month (1 to 12) and day of the month.
    ///
    /// # Errors
    /// [`BusinessDateError::InvalidDate`] if the date doesn't exist.
    pub fn new(year: i16, month: i8, day: i8) -> Result<BusinessDate, BusinessDateError> {
        if !(1..=9999).contains(&year) {
            return Err(BusinessDateError::InvalidDate);
        }
        Date::new(year, month, day).map(BusinessDate).map_err(|_| BusinessDateError::InvalidDate)
    }

    /// The year.
    pub fn year(self) -> i16 {
        self.0.year()
    }

    /// The month, 1 to 12.
    pub fn month(self) -> i8 {
        self.0.month()
    }

    /// The day of the month, 1 to 31.
    pub fn day(self) -> i8 {
        self.0.day()
    }

    /// The next business date.
    ///
    /// # Errors
    /// [`BusinessDateError::OutOfRange`] after 9999-12-31.
    pub fn next(self) -> Result<BusinessDate, BusinessDateError> {
        let next = self.0.tomorrow().map_err(|_| BusinessDateError::OutOfRange)?;
        BusinessDate::new(next.year(), next.month(), next.day())
            .map_err(|_| BusinessDateError::OutOfRange)
    }

    /// The previous business date.
    ///
    /// # Errors
    /// [`BusinessDateError::OutOfRange`] before 0001-01-01.
    pub fn previous(self) -> Result<BusinessDate, BusinessDateError> {
        let previous = self.0.yesterday().map_err(|_| BusinessDateError::OutOfRange)?;
        BusinessDate::new(previous.year(), previous.month(), previous.day())
            .map_err(|_| BusinessDateError::OutOfRange)
    }
}

impl fmt::Display for BusinessDate {
    /// ISO 8601: `2026-09-27`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year(), self.month(), self.day())
    }
}

impl fmt::Debug for BusinessDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BusinessDate({self})")
    }
}

impl FromStr for BusinessDate {
    type Err = BusinessDateError;

    /// Parses exactly `YYYY-MM-DD`.
    fn from_str(text: &str) -> Result<BusinessDate, BusinessDateError> {
        let invalid = || BusinessDateError::InvalidFormat(text.to_owned());
        let well_formed = text.len() == 10
            && text.bytes().enumerate().all(|(index, byte)| match index {
                4 | 7 => byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        if !well_formed {
            return Err(invalid());
        }
        let field = |range: core::ops::Range<usize>| text.get(range).ok_or_else(invalid);
        let year = field(0..4)?.parse().map_err(|_| invalid())?;
        let month = field(5..7)?.parse().map_err(|_| invalid())?;
        let day = field(8..10)?.parse().map_err(|_| invalid())?;
        BusinessDate::new(year, month, day)
    }
}

/// When business days begin at a location: its time zone, and the local time of day at which
/// one business day ends and the next begins.
#[derive(Clone, Debug)]
pub struct BusinessDayPolicy {
    time_zone_name: String,
    time_zone: TimeZone,
    cutoff: Time,
    cutoff_hour: u8,
    cutoff_minute: u8,
}

impl BusinessDayPolicy {
    /// A policy for the IANA time zone `time_zone` (such as `"America/New_York"`), with
    /// business days starting at `cutoff_hour:cutoff_minute` local time. A cutoff of 00:00
    /// makes business dates the local calendar dates.
    ///
    /// # Errors
    /// [`BusinessDateError::UnknownTimeZone`] if the time zone isn't in the bundled IANA
    /// database, [`BusinessDateError::InvalidCutoff`] unless the hour is 0 to 23 and the minute
    /// 0 to 59.
    pub fn new(
        time_zone: &str,
        cutoff_hour: u8,
        cutoff_minute: u8,
    ) -> Result<BusinessDayPolicy, BusinessDateError> {
        let invalid_cutoff =
            || BusinessDateError::InvalidCutoff { hour: cutoff_hour, minute: cutoff_minute };
        let hour = i8::try_from(cutoff_hour).map_err(|_| invalid_cutoff())?;
        let minute = i8::try_from(cutoff_minute).map_err(|_| invalid_cutoff())?;
        let cutoff = Time::new(hour, minute, 0, 0).map_err(|_| invalid_cutoff())?;
        let zone = TimeZone::get(time_zone)
            .map_err(|_| BusinessDateError::UnknownTimeZone(time_zone.to_owned()))?;
        let time_zone_name = zone.iana_name().unwrap_or(time_zone).to_owned();
        Ok(BusinessDayPolicy {
            time_zone_name,
            time_zone: zone,
            cutoff,
            cutoff_hour,
            cutoff_minute,
        })
    }

    /// The IANA time zone name.
    pub fn time_zone(&self) -> &str {
        &self.time_zone_name
    }

    /// The cutoff, as local `(hour, minute)`.
    pub fn cutoff(&self) -> (u8, u8) {
        (self.cutoff_hour, self.cutoff_minute)
    }

    /// The instant business date `date` begins: `date` at the cutoff, local time.
    ///
    /// # Errors
    /// [`BusinessDateError::OutOfRange`] if that instant is outside the timestamp range.
    pub fn start_of(&self, date: BusinessDate) -> Result<Timestamp, BusinessDateError> {
        let local = date.0.to_datetime(self.cutoff);
        let instant = self
            .time_zone
            .to_ambiguous_timestamp(local)
            .compatible()
            .map_err(|_| BusinessDateError::OutOfRange)?;
        Timestamp::from_jiff(instant).map_err(|_| BusinessDateError::OutOfRange)
    }

    /// The instants belonging to business date `date`: from its start (inclusive) to the next
    /// business date's start (exclusive). Around daylight saving transitions a window lasts 23
    /// or 25 hours; where a time zone skipped a whole calendar day, it may even be empty.
    ///
    /// # Errors
    /// [`BusinessDateError::OutOfRange`] if either end is outside the timestamp range.
    pub fn window(&self, date: BusinessDate) -> Result<(Timestamp, Timestamp), BusinessDateError> {
        Ok((self.start_of(date)?, self.start_of(date.next()?)?))
    }

    /// The business date that `instant` belongs to: the date `D` with
    /// `start_of(D) <= instant < start_of(D + 1)`.
    ///
    /// # Errors
    /// [`BusinessDateError::OutOfRange`] at the very ends of the timestamp range.
    pub fn business_date_of(&self, instant: Timestamp) -> Result<BusinessDate, BusinessDateError> {
        let jiff_instant = instant.to_jiff().map_err(|_| BusinessDateError::OutOfRange)?;
        let local = self.time_zone.to_datetime(jiff_instant).date();
        let mut date = BusinessDate::new(local.year(), local.month(), local.day())
            .map_err(|_| BusinessDateError::OutOfRange)?;
        // The local date is within a day or two of the business date: a day's start can move
        // past midnight when a skipped hour or day swallows the cutoff. Starts never decrease,
        // so stepping toward the instant finds the one window containing it.
        for _ in 0..8 {
            if instant < self.start_of(date)? {
                date = date.previous()?;
            } else if instant >= self.start_of(date.next()?)? {
                date = date.next()?;
            } else {
                return Ok(date);
            }
        }
        Err(BusinessDateError::OutOfRange)
    }
}

impl PartialEq for BusinessDayPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.time_zone_name == other.time_zone_name && self.cutoff == other.cutoff
    }
}

impl Eq for BusinessDayPolicy {}

/// Errors from business dates.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BusinessDateError {
    /// A date that doesn't exist, or is outside the years 1 to 9999.
    #[error("invalid business date")]
    InvalidDate,
    /// Text that isn't a `YYYY-MM-DD` date.
    #[error("invalid business date {0:?}: expected YYYY-MM-DD")]
    InvalidFormat(String),
    /// A time zone that isn't in the bundled IANA time zone database.
    #[error("unknown time zone {0:?}")]
    UnknownTimeZone(String),
    /// A cutoff that isn't a valid time of day.
    #[error("invalid cutoff {hour:02}:{minute:02}")]
    InvalidCutoff {
        /// The requested hour.
        hour: u8,
        /// The requested minute.
        minute: u8,
    },
    /// Outside the supported range of dates and timestamps.
    #[error("business date out of range")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> BusinessDate {
        text.parse().unwrap()
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn policy(zone: &str, hour: u8, minute: u8) -> BusinessDayPolicy {
        BusinessDayPolicy::new(zone, hour, minute).unwrap()
    }

    fn hours(window: (Timestamp, Timestamp)) -> u64 {
        window.1.duration_since(window.0).unwrap().as_secs() / 3600
    }

    #[test]
    fn after_midnight_sales_belong_to_the_previous_day() {
        let bar = policy("America/New_York", 4, 0);
        // 03:59 EDT on the 28th is still the 27th's business day; 04:00 starts the 28th.
        assert_eq!(bar.business_date_of(at("2026-09-28T03:59:59-04:00")), Ok(date("2026-09-27")));
        assert_eq!(bar.business_date_of(at("2026-09-28T04:00:00-04:00")), Ok(date("2026-09-28")));
        assert_eq!(bar.start_of(date("2026-09-28")), Ok(at("2026-09-28T08:00:00Z")));
        assert_eq!(bar.time_zone(), "America/New_York");
        assert_eq!(bar.cutoff(), (4, 0));
    }

    #[test]
    fn spring_forward_days_are_23_hours() {
        // New York, 2026-03-08: 02:00 EST jumps to 03:00 EDT.
        let bar = policy("America/New_York", 4, 0);
        let window = bar.window(date("2026-03-07")).unwrap();
        assert_eq!(window, (at("2026-03-07T04:00:00-05:00"), at("2026-03-08T04:00:00-04:00")));
        assert_eq!(hours(window), 23);
        // A cutoff inside the skipped hour moves forward by the gap: 02:30 becomes 03:30 EDT.
        let skipped = policy("America/New_York", 2, 30);
        assert_eq!(skipped.start_of(date("2026-03-08")), Ok(at("2026-03-08T03:30:00-04:00")));
        assert_eq!(hours(skipped.window(date("2026-03-07")).unwrap()), 24);
        assert_eq!(hours(skipped.window(date("2026-03-08")).unwrap()), 23);
    }

    #[test]
    fn fall_back_days_are_25_hours_and_a_repeated_cutoff_means_its_first_occurrence() {
        // New York, 2026-11-01: 02:00 EDT falls back to 01:00 EST, so 01:00–02:00 happens twice.
        let bar = policy("America/New_York", 4, 0);
        assert_eq!(hours(bar.window(date("2026-10-31")).unwrap()), 25);
        let repeated = policy("America/New_York", 1, 30);
        assert_eq!(repeated.start_of(date("2026-11-01")), Ok(at("2026-11-01T01:30:00-04:00")));
        // The second 01:15 is after the business day began at the first 01:30, even though
        // 01:15 is earlier than the cutoff on the wall clock.
        assert_eq!(
            repeated.business_date_of(at("2026-11-01T01:15:00-04:00")),
            Ok(date("2026-10-31"))
        );
        assert_eq!(
            repeated.business_date_of(at("2026-11-01T01:15:00-05:00")),
            Ok(date("2026-11-01"))
        );
        assert_eq!(hours(repeated.window(date("2026-11-01")).unwrap()), 25);
    }

    #[test]
    fn european_transitions() {
        // Berlin, 2026-03-29: 02:00 CET jumps to 03:00 CEST; 2026-10-25: 03:00 CEST falls back
        // to 02:00 CET. A 02:30 cutoff is skipped in March and repeated in October.
        let bakery = policy("Europe/Berlin", 2, 30);
        assert_eq!(bakery.start_of(date("2026-03-29")), Ok(at("2026-03-29T03:30:00+02:00")));
        assert_eq!(bakery.start_of(date("2026-10-25")), Ok(at("2026-10-25T02:30:00+02:00")));
        assert_eq!(
            bakery.business_date_of(at("2026-10-25T02:40:00+01:00")),
            Ok(date("2026-10-25"))
        );
        assert_eq!(
            bakery.business_date_of(at("2026-10-25T02:20:00+02:00")),
            Ok(date("2026-10-24"))
        );
    }

    #[test]
    fn a_skipped_calendar_day_has_an_empty_business_day() {
        // Samoa skipped 2011-12-30 entirely, moving from UTC-10 to UTC+14.
        let shop = policy("Pacific/Apia", 4, 0);
        let (start, end) = shop.window(date("2011-12-30")).unwrap();
        assert_eq!(start, end, "the 30th never happened, so it has no business");
        // 02:00 on the 31st is before the 31st's 04:00 cutoff: it belongs to the 29th, whose
        // business day ran until then. (The 30th, the previous calendar day, didn't exist.)
        assert_eq!(shop.business_date_of(at("2011-12-31T02:00:00+14:00")), Ok(date("2011-12-29")));
        assert_eq!(shop.business_date_of(at("2011-12-31T04:00:00+14:00")), Ok(date("2011-12-31")));
    }

    #[test]
    fn a_midnight_cutoff_in_utc_gives_calendar_dates() {
        let utc = policy("UTC", 0, 0);
        assert_eq!(utc.business_date_of(at("2026-09-27T00:00:00Z")), Ok(date("2026-09-27")));
        assert_eq!(utc.business_date_of(at("2026-09-27T23:59:59.999999Z")), Ok(date("2026-09-27")));
        assert_eq!(utc.business_date_of(Timestamp::MIN), Ok(date("0001-01-01")));
    }

    #[test]
    fn dates_parse_strictly() {
        assert_eq!(date("2026-09-27").to_string(), "2026-09-27");
        assert_eq!(date("0001-01-01").previous(), Err(BusinessDateError::OutOfRange));
        assert_eq!(date("9999-12-31").next(), Err(BusinessDateError::OutOfRange));
        assert_eq!(date("2028-02-28").next(), Ok(date("2028-02-29")));
        assert_eq!(BusinessDate::new(2026, 2, 29), Err(BusinessDateError::InvalidDate));
        assert_eq!(BusinessDate::new(0, 1, 1), Err(BusinessDateError::InvalidDate));
        assert_eq!(format!("{:?}", date("2026-09-27")), "BusinessDate(2026-09-27)");
        for bad in ["", "2026-9-27", "2026/09/27", "20260927", "2026-09-27T00:00", "+026-09-27"] {
            assert_eq!(
                bad.parse::<BusinessDate>(),
                Err(BusinessDateError::InvalidFormat(bad.to_owned())),
                "{bad:?}"
            );
        }
        assert_eq!("2026-02-30".parse::<BusinessDate>(), Err(BusinessDateError::InvalidDate));
    }

    #[test]
    fn policies_validate_their_inputs() {
        assert_eq!(
            BusinessDayPolicy::new("Mars/Olympus_Mons", 4, 0),
            Err(BusinessDateError::UnknownTimeZone("Mars/Olympus_Mons".to_owned()))
        );
        for (hour, minute) in [(24, 0), (12, 60), (255, 0)] {
            assert_eq!(
                BusinessDayPolicy::new("UTC", hour, minute),
                Err(BusinessDateError::InvalidCutoff { hour, minute })
            );
        }
        assert_eq!(policy("America/New_York", 4, 0), policy("America/New_York", 4, 0));
        assert_ne!(policy("America/New_York", 4, 0), policy("America/New_York", 5, 0));
    }
}
