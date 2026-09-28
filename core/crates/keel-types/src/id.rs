//! Identifiers: typed, time-ordered UUIDv7s.
//!
//! Every entity is identified by a UUIDv7 (RFC 9562): 48 bits of Unix time in milliseconds,
//! then counter and random bits. Devices create IDs offline, without coordinating, and IDs
//! sort by creation time, which keeps database indexes compact.
//!
//! [`Id<T>`] tags a UUID with the kind of entity it identifies, so an order's ID can't be passed
//! where a customer's is expected. Note that a UUIDv7 reveals when it was created.

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::str::FromStr;

use uuid::{Uuid, Variant};

use crate::time::Timestamp;

/// The identifier of an entity of type `T`: a UUIDv7.
///
/// `T` is only a marker; `Id<T>` is `Copy`, `Send` and `Sync` whatever `T` is.
pub struct Id<T> {
    uuid: Uuid,
    entity: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// An ID from a UUID, which must be a version 7 UUID with the RFC 9562 variant.
    ///
    /// # Errors
    /// [`IdError::NotVersion7`] or [`IdError::WrongVariant`] otherwise.
    pub fn from_uuid(uuid: Uuid) -> Result<Id<T>, IdError> {
        if uuid.get_version_num() != 7 {
            return Err(IdError::NotVersion7 { version: uuid.get_version_num() });
        }
        if uuid.get_variant() != Variant::RFC4122 {
            return Err(IdError::WrongVariant);
        }
        Ok(Id { uuid, entity: PhantomData })
    }

    /// An ID from its 16 bytes, in network (big-endian) order.
    ///
    /// # Errors
    /// As [`Id::from_uuid`].
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Id<T>, IdError> {
        Id::from_uuid(Uuid::from_bytes(bytes))
    }

    /// Parses the canonical hyphenated form, such as `0192f0c1-9c4a-7d3e-8b5a-3f1e2d4c5b6a`.
    /// Letters may be in either case; braces, `urn:uuid:` prefixes and unhyphenated forms are
    /// rejected.
    ///
    /// # Errors
    /// [`IdError::InvalidFormat`] if the text isn't a hyphenated UUID, otherwise as
    /// [`Id::from_uuid`].
    pub fn parse(text: &str) -> Result<Id<T>, IdError> {
        let hyphenated = text.len() == 36
            && text.bytes().enumerate().all(|(index, byte)| match index {
                8 | 13 | 18 | 23 => byte == b'-',
                _ => byte.is_ascii_hexdigit(),
            });
        if !hyphenated {
            return Err(IdError::InvalidFormat(text.to_owned()));
        }
        let uuid = Uuid::try_parse(text).map_err(|_| IdError::InvalidFormat(text.to_owned()))?;
        Id::from_uuid(uuid)
    }

    /// The underlying UUID.
    pub const fn as_uuid(&self) -> Uuid {
        self.uuid
    }

    /// The 16 bytes, in network (big-endian) order.
    pub const fn to_bytes(self) -> [u8; 16] {
        *self.uuid.as_bytes()
    }

    /// The same ID, tagged with another entity type. For the few entities identified by another
    /// entity's ID, such as an aggregate identified by its event stream's ID.
    pub const fn cast<U>(self) -> Id<U> {
        Id { uuid: self.uuid, entity: PhantomData }
    }

    /// The creation time embedded in the ID, in milliseconds since the Unix epoch.
    pub const fn timestamp_ms(self) -> u64 {
        let [t0, t1, t2, t3, t4, t5, ..] = *self.uuid.as_bytes();
        u64::from_be_bytes([0, 0, t0, t1, t2, t3, t4, t5])
    }

    /// Assembles a UUIDv7 from its fields. Only the low 12 bits of `counter` and the low 62
    /// bits of `random` are used.
    fn from_fields(timestamp_ms: u64, counter: u16, random: u64) -> Id<T> {
        let [_, _, t0, t1, t2, t3, t4, t5] = timestamp_ms.to_be_bytes();
        let [c0, c1] = counter.to_be_bytes();
        let [r0, r1, r2, r3, r4, r5, r6, r7] = random.to_be_bytes();
        let bytes = [
            t0,
            t1,
            t2,
            t3,
            t4,
            t5,
            0x70 | (c0 & 0x0F), // version 7, then the counter's top 4 bits
            c1,
            0x80 | (r0 & 0x3F), // variant 0b10, then 62 random bits
            r1,
            r2,
            r3,
            r4,
            r5,
            r6,
            r7,
        ];
        Id { uuid: Uuid::from_bytes(bytes), entity: PhantomData }
    }
}

// Implemented by hand: deriving would needlessly require `T` itself to implement each trait.
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<T> fmt::Display for Id<T> {
    /// The canonical form: lowercase, hyphenated.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.uuid.hyphenated(), f)
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = core::any::type_name::<T>();
        let short_name = type_name.rsplit("::").next().unwrap_or(type_name);
        write!(f, "Id<{short_name}>({self})")
    }
}

impl<T> FromStr for Id<T> {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Id<T>, IdError> {
        Id::parse(text)
    }
}

/// A source of random bits for identifiers.
pub trait Entropy {
    /// 64 random bits.
    ///
    /// # Errors
    /// [`EntropyError`] if the source failed, for example if the operating system's generator
    /// was unavailable.
    fn next_u64(&mut self) -> Result<u64, EntropyError>;
}

/// Deterministic pseudo-random bits from a seed, for tests and simulations: the same seed gives
/// the same identifiers. Its output is predictable, so never use it in production.
///
/// The generator is SplitMix64 (Steele, Lea and Flood, 2014).
#[derive(Clone, Debug)]
pub struct SeededEntropy {
    state: u64,
}

impl SeededEntropy {
    /// A generator starting from `seed`.
    pub const fn new(seed: u64) -> SeededEntropy {
        SeededEntropy { state: seed }
    }
}

impl Entropy for SeededEntropy {
    fn next_u64(&mut self) -> Result<u64, EntropyError> {
        // Wrapping arithmetic is the algorithm, not an accident.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ mixed.wrapping_shr(30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ mixed.wrapping_shr(27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Ok(mixed ^ mixed.wrapping_shr(31))
    }
}

/// The operating system's cryptographically secure random number generator.
#[cfg(feature = "os")]
#[derive(Clone, Copy, Debug, Default)]
pub struct OsEntropy;

#[cfg(feature = "os")]
impl Entropy for OsEntropy {
    fn next_u64(&mut self) -> Result<u64, EntropyError> {
        getrandom::u64().map_err(|error| EntropyError(error.to_string()))
    }
}

/// The latest time a UUIDv7's 48-bit field can hold, in milliseconds: in the year 10889.
const MAX_TIMESTAMP_MS: u64 = 0xFFFF_FFFF_FFFF;
/// The largest value of the 12-bit counter.
const MAX_COUNTER: u16 = 0x0FFF;
/// The counter is reseeded with 11 random bits: its top bit starts clear, leaving room to count.
const COUNTER_SEED_MASK: u64 = 0x07FF;

/// Generates monotonic UUIDv7s, following RFC 9562, section 6.2, method 1.
///
/// Each ID holds:
/// - 48 bits of Unix time in milliseconds;
/// - a 12-bit counter, reseeded every millisecond with 11 random bits and incremented for each
///   further ID in the same millisecond;
/// - 62 fresh random bits.
///
/// The IDs from one generator strictly increase, even if the clock goes backwards (the last
/// millisecond is kept, and the counter keeps counting) or the counter fills up (the time moves
/// on by a millisecond).
#[derive(Clone, Debug)]
pub struct IdGenerator<E> {
    entropy: E,
    /// The time and counter of the last ID.
    last: Option<(u64, u16)>,
}

impl<E: Entropy> IdGenerator<E> {
    /// A generator drawing random bits from `entropy`.
    pub const fn new(entropy: E) -> IdGenerator<E> {
        IdGenerator { entropy, last: None }
    }

    /// A new ID for an entity created at `now`.
    ///
    /// # Errors
    /// [`IdError::Entropy`] if the entropy source failed, [`IdError::Exhausted`] only if IDs
    /// have reached the end of UUIDv7 time (the year 10889).
    pub fn generate<T>(&mut self, now: Timestamp) -> Result<Id<T>, IdError> {
        let now_ms = u64::try_from(now.as_millis()).unwrap_or(0).min(MAX_TIMESTAMP_MS);
        let (timestamp_ms, counter) = match self.last {
            // The same millisecond, or the clock went backwards: stay put and count.
            Some((last_ms, last_counter)) if now_ms <= last_ms => {
                if let Some(counter) =
                    last_counter.checked_add(1).filter(|&counter| counter <= MAX_COUNTER)
                {
                    (last_ms, counter)
                } else {
                    // The counter is full: move on to the next millisecond.
                    let next_ms = last_ms.checked_add(1).filter(|&ms| ms <= MAX_TIMESTAMP_MS);
                    (next_ms.ok_or(IdError::Exhausted)?, self.counter_seed()?)
                }
            }
            _ => (now_ms, self.counter_seed()?),
        };
        let random = self.entropy.next_u64().map_err(IdError::Entropy)?;
        self.last = Some((timestamp_ms, counter));
        Ok(Id::from_fields(timestamp_ms, counter, random))
    }

    fn counter_seed(&mut self) -> Result<u16, IdError> {
        let bits = self.entropy.next_u64().map_err(IdError::Entropy)? & COUNTER_SEED_MASK;
        u16::try_from(bits).map_err(|_| IdError::Exhausted)
    }
}

/// The operating system's entropy failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("entropy source failed: {0}")]
pub struct EntropyError(pub String);

/// Errors from identifiers.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdError {
    /// Text that isn't a hyphenated UUID.
    #[error("invalid identifier {0:?}: expected a hyphenated UUID")]
    InvalidFormat(String),
    /// A UUID of another version.
    #[error("identifier is a version {version} UUID, not version 7")]
    NotVersion7 {
        /// The UUID's version.
        version: usize,
    },
    /// A UUID without the RFC 9562 variant bits.
    #[error("identifier doesn't have the RFC 9562 variant")]
    WrongVariant,
    /// The entropy source failed.
    #[error(transparent)]
    Entropy(EntropyError),
    /// No more identifiers can be created: UUIDv7 time ends in the year 10889.
    #[error("identifier space exhausted")]
    Exhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Order;
    #[derive(Debug)]
    struct Customer;

    fn ms(millis: i64) -> Timestamp {
        Timestamp::from_millis(millis).unwrap()
    }

    #[test]
    fn seeded_entropy_matches_the_splitmix64_reference() {
        let mut entropy = SeededEntropy::new(1_234_567);
        let expected: [u64; 5] = [
            6_457_827_717_110_365_317,
            3_203_168_211_198_807_973,
            9_817_491_932_198_370_423,
            4_593_380_528_125_082_431,
            16_408_922_859_458_223_821,
        ];
        for value in expected {
            assert_eq!(entropy.next_u64(), Ok(value));
        }
    }

    #[test]
    fn layout_follows_rfc_9562() {
        let id: Id<Order> = Id::from_fields(0x0192_F0C1_9C4A, 0x0ABC, u64::MAX);
        assert_eq!(id.to_string(), "0192f0c1-9c4a-7abc-bfff-ffffffffffff");
        assert_eq!(id.timestamp_ms(), 0x0192_F0C1_9C4A);
        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(id.as_uuid().get_variant(), Variant::RFC4122);
        let id: Id<Order> = Id::from_fields(0, 0, 0);
        assert_eq!(id.to_string(), "00000000-0000-7000-8000-000000000000");
    }

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let mut generator = IdGenerator::new(SeededEntropy::new(42));
        let now = ms(1_790_517_780_123);
        let first: Id<Order> = generator.generate(now).unwrap();
        let second: Id<Order> = generator.generate(now).unwrap();
        let mut again = IdGenerator::new(SeededEntropy::new(42));
        assert_eq!(again.generate::<Order>(now), Ok(first));
        assert_eq!(again.generate::<Order>(now), Ok(second));
        assert_eq!(first.timestamp_ms(), 1_790_517_780_123);
        assert!(first < second);
    }

    #[test]
    fn ids_increase_within_a_millisecond_and_when_the_clock_goes_back() {
        let mut generator = IdGenerator::new(SeededEntropy::new(7));
        let mut previous: Id<Order> = generator.generate(ms(1_000)).unwrap();
        // 5000 IDs in one millisecond overflow the 12-bit counter at least once.
        for _ in 0..5_000 {
            let next = generator.generate(ms(1_000)).unwrap();
            assert!(next > previous);
            previous = next;
        }
        assert!(previous.timestamp_ms() > 1_000, "the counter overflowed into the next ms");
        // The clock jumps back an hour: IDs keep increasing.
        let after_jump = generator.generate::<Order>(ms(1_000 - 3_600_000)).unwrap();
        assert!(after_jump > previous);
        assert!(after_jump.timestamp_ms() >= previous.timestamp_ms());
    }

    #[test]
    fn exhaustion_is_an_error_not_a_panic() {
        let mut generator = IdGenerator::new(SeededEntropy::new(1));
        generator.last = Some((MAX_TIMESTAMP_MS, MAX_COUNTER));
        assert_eq!(generator.generate::<Order>(ms(0)), Err(IdError::Exhausted));
    }

    #[test]
    fn parsing_is_strict() {
        let text = "0192f0c1-9c4a-7abc-bfff-ffffffffffff";
        let id: Id<Order> = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
        assert_eq!(Id::<Order>::parse(&text.to_uppercase()), Ok(id));
        assert_eq!(Id::<Order>::from_bytes(id.to_bytes()), Ok(id));
        for bad in [
            "",
            "0192f0c19c4a7abcbfffffffffffffff",
            "{0192f0c1-9c4a-7abc-bfff-ffffffffffff}",
            "urn:uuid:0192f0c1-9c4a-7abc-bfff-ffffffffffff",
            "0192f0c1-9c4a-7abc-bfff-fffffffffffg",
            "0192f0c1+9c4a-7abc-bfff-ffffffffffff",
        ] {
            assert_eq!(
                Id::<Order>::parse(bad),
                Err(IdError::InvalidFormat(bad.to_owned())),
                "{bad}"
            );
        }
        // A version 4 UUID, and a version 7 one with the wrong variant.
        assert_eq!(
            Id::<Order>::parse("0192f0c1-9c4a-4abc-bfff-ffffffffffff"),
            Err(IdError::NotVersion7 { version: 4 })
        );
        assert_eq!(
            Id::<Order>::parse("0192f0c1-9c4a-7abc-cfff-ffffffffffff"),
            Err(IdError::WrongVariant)
        );
    }

    #[test]
    fn debug_names_the_entity() {
        let id: Id<Order> = Id::from_fields(0, 0, 0);
        assert_eq!(format!("{id:?}"), "Id<Order>(00000000-0000-7000-8000-000000000000)");
        let cast: Id<Customer> = id.cast();
        assert_eq!(cast.to_bytes(), id.to_bytes());
        assert_eq!(format!("{cast:?}"), "Id<Customer>(00000000-0000-7000-8000-000000000000)");
    }

    #[cfg(feature = "os")]
    #[test]
    fn os_entropy_produces_distinct_ids() {
        let mut generator = IdGenerator::new(OsEntropy);
        let a = generator.generate::<Order>(ms(1_000)).unwrap();
        let b = generator.generate::<Order>(ms(1_000)).unwrap();
        assert_ne!(a, b);
    }
}
