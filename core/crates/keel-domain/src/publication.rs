//! Publishing a location's profile (ADR-0028, decision 3): the cloud cuts the profile's encoding
//! into chunks and writes each as an event, then a `profile.published` event naming the version
//! they make, all in a stream of its own, of kind [`STREAM`], in the cloud's own log. Replication
//! carries them to every device, which takes the publication once it holds every chunk
//! ([`assemble`]).
//!
//! | Schema | Keys |
//! |---|---|
//! | `profile.chunk` | 1 version, 2 index, 3 bytes |
//! | `profile.published` | 1 version, 2 size, 3 chunks |
//!
//! A version is the SHA-256 of the profile's encoding, 32 bytes ([`ProfileVersion`]). Beyond the
//! types:
//! - a chunk holds from 1 to [`MAX_CHUNK`] bytes, and its index is below [`MAX_CHUNKS`];
//! - a publication's size is from 1 to [`MAX_PROFILE`] bytes, and it has as many chunks as the
//!   size takes, every one but the last [`MAX_CHUNK`] bytes long;
//! - the chunks join, in the order of their indexes, to the size and the hash the publication
//!   names, and decode to a profile.
//!
//! Who may publish, and which publication a device takes, are the runtime's to decide: the
//! roster's cloud, and the latest that passes every check.

use std::collections::BTreeMap;

use keel_events::cbor::Value;
use keel_events::envelope::SchemaRef;
use keel_events::hash::sha256;

use crate::codec::{Field, Fields, PayloadError, ProfileVersion, Record};
use crate::profile::{Profile, ProfileError};
use crate::schema::{DecodeError, DomainEvent, SchemaId};

#[cfg(test)]
mod tests;

/// The kind of stream a publication is.
pub const STREAM: &str = "profile";

/// The most bytes in one chunk: 192 KiB, so a chunk's event stays below the 256 KiB an event may
/// be, with room for its envelope.
pub const MAX_CHUNK: usize = 192 * 1024;

/// [`MAX_CHUNK`], as sizes are counted.
const CHUNK_SIZE: u64 = 192 * 1024;

/// The largest profile that may be published: 64 MiB.
pub const MAX_PROFILE: u64 = 64 * 1024 * 1024;

/// The most chunks a publication may have: as many as [`MAX_PROFILE`] takes.
pub const MAX_CHUNKS: u64 = MAX_PROFILE.div_ceil(CHUNK_SIZE);

const CHUNK: SchemaId = SchemaId { name: "profile.chunk", version: 1 };
const PUBLISHED: SchemaId = SchemaId { name: "profile.published", version: 1 };

/// A piece of a profile's encoding.
#[derive(Clone, PartialEq, Eq)]
pub struct Chunk {
    version: ProfileVersion,
    index: u64,
    bytes: Vec<u8>,
}

impl core::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Chunk")
            .field("version", &self.version)
            .field("index", &self.index)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl Chunk {
    /// The chunk at `index` of `version`, holding `bytes`.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] if it holds no bytes or more than [`MAX_CHUNK`], or its index is
    /// [`MAX_CHUNKS`] or more.
    pub fn new(version: ProfileVersion, index: u64, bytes: Vec<u8>) -> Result<Chunk, PayloadError> {
        if bytes.is_empty() || bytes.len() > MAX_CHUNK {
            return Err(PayloadError::Invalid("bytes"));
        }
        if index >= MAX_CHUNKS {
            return Err(PayloadError::Invalid("index"));
        }
        Ok(Chunk { version, index, bytes })
    }

    /// The version it is a piece of.
    pub const fn version(&self) -> ProfileVersion {
        self.version
    }

    /// Where it goes, from 0.
    pub const fn index(&self) -> u64 {
        self.index
    }

    /// Its bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A publication: the version its chunks make, how long it is and how many chunks it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Published {
    version: ProfileVersion,
    size: u64,
    chunks: u64,
}

impl Published {
    /// The publication of `version`, `size` bytes long in `chunks` chunks.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] if the size is 0 or more than [`MAX_PROFILE`], or it isn't in
    /// as many chunks as it takes.
    pub fn new(version: ProfileVersion, size: u64, chunks: u64) -> Result<Published, PayloadError> {
        if size == 0 || size > MAX_PROFILE {
            return Err(PayloadError::Invalid("size"));
        }
        if chunks != size.div_ceil(CHUNK_SIZE) {
            return Err(PayloadError::Invalid("chunks"));
        }
        Ok(Published { version, size, chunks })
    }

    /// The version.
    pub const fn version(&self) -> ProfileVersion {
        self.version
    }

    /// How many bytes the profile's encoding is.
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// How many chunks it is in.
    pub const fn chunks(&self) -> u64 {
        self.chunks
    }
}

/// What can happen in a publication's stream: its chunks are written, then it is published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicationEvent {
    /// A piece of the profile's encoding.
    Chunk(Chunk),
    /// The publication, written after its chunks.
    Published(Published),
}

/// Whether `schema` is `profile.published`'s, one this kernel decodes: a reader of the log finds
/// publications without decoding every chunk.
pub fn is_published(schema: &SchemaRef) -> bool {
    PUBLISHED.matches(schema)
}

/// The version of `profile`: the SHA-256 of its encoding.
pub fn version(profile: &Profile) -> ProfileVersion {
    ProfileVersion::from_bytes(sha256(&profile.encode()))
}

/// `profile` as a publication's events, in the order they are written: its chunks, then the
/// publication.
///
/// # Errors
/// [`PayloadError::Invalid`] if its encoding is longer than [`MAX_PROFILE`].
pub fn publish(profile: &Profile) -> Result<Vec<PublicationEvent>, PayloadError> {
    let bytes = profile.encode();
    let version = ProfileVersion::from_bytes(sha256(&bytes));
    let size = u64::try_from(bytes.len()).map_err(|_| PayloadError::Invalid("size"))?;
    let mut events = Vec::new();
    let mut chunks = 0_u64;
    for piece in bytes.chunks(MAX_CHUNK) {
        events.push(PublicationEvent::Chunk(Chunk::new(version, chunks, piece.to_vec())?));
        chunks = chunks.checked_add(1).ok_or(PayloadError::Invalid("chunks"))?;
    }
    events.push(PublicationEvent::Published(Published::new(version, size, chunks)?));
    Ok(events)
}

/// Why a publication can't be taken.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PublicationError {
    /// A chunk it names isn't held yet.
    #[error("chunk {0} isn't held")]
    Missing(u64),
    /// Two chunks of one index differ.
    #[error("two chunks {0} differ")]
    Conflicting(u64),
    /// A chunk is of another length than its place takes.
    #[error("chunk {0} is of the wrong length")]
    Length(u64),
    /// The chunks don't hash to the version.
    #[error("the chunks don't hash to the version")]
    Hash,
    /// The profile they make doesn't decode, or breaks a rule.
    #[error("the profile: {0}")]
    Profile(#[from] ProfileError),
}

/// The profile `published` names, from `chunks`, which may hold chunks of other versions and the
/// same chunk more than once.
///
/// # Errors
/// [`PublicationError`] if a chunk is missing, two of one index differ, one is of the wrong
/// length, they don't hash to the version, or the profile doesn't decode.
pub fn assemble<'a>(
    published: &Published,
    chunks: impl IntoIterator<Item = &'a Chunk>,
) -> Result<Profile, PublicationError> {
    let mut pieces: BTreeMap<u64, &[u8]> = BTreeMap::new();
    for chunk in chunks {
        if chunk.version != published.version || chunk.index >= published.chunks {
            continue;
        }
        if let Some(held) = pieces.insert(chunk.index, &chunk.bytes)
            && held != chunk.bytes.as_slice()
        {
            return Err(PublicationError::Conflicting(chunk.index));
        }
    }
    let last = published.chunks.saturating_sub(1);
    let tail = published.size.saturating_sub(last.saturating_mul(CHUNK_SIZE));
    let mut bytes = Vec::new();
    for index in 0..published.chunks {
        let piece = pieces.get(&index).ok_or(PublicationError::Missing(index))?;
        let length = if index == last { tail } else { CHUNK_SIZE };
        if u64::try_from(piece.len()).ok() != Some(length) {
            return Err(PublicationError::Length(index));
        }
        bytes.extend_from_slice(piece);
    }
    if sha256(&bytes) != *published.version.as_bytes() {
        return Err(PublicationError::Hash);
    }
    Ok(Profile::decode(&bytes)?)
}

/// An index, a size or a count, as encoded: the event's rules judge its value.
struct Number(u64);

impl Field for Number {
    fn to_value(&self) -> Value {
        Value::Unsigned(self.0)
    }

    fn from_value(value: &Value) -> Option<Number> {
        value.as_u64().map(Number)
    }
}

/// A chunk's bytes, as encoded.
struct Bytes(Vec<u8>);

impl Field for Bytes {
    fn to_value(&self) -> Value {
        Value::Bytes(self.0.clone())
    }

    fn from_value(value: &Value) -> Option<Bytes> {
        value.as_bytes().map(|bytes| Bytes(bytes.to_vec()))
    }
}

impl DomainEvent for PublicationEvent {
    const STREAM: &'static str = STREAM;

    const SCHEMAS: &'static [SchemaId] = &[CHUNK, PUBLISHED];

    fn schema(&self) -> SchemaId {
        match self {
            PublicationEvent::Chunk(_) => CHUNK,
            PublicationEvent::Published(_) => PUBLISHED,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            PublicationEvent::Chunk(chunk) => Record::default()
                .field(1, &chunk.version)
                .field(2, &Number(chunk.index))
                .field(3, &Bytes(chunk.bytes.clone()))
                .build(),
            PublicationEvent::Published(published) => Record::default()
                .field(1, &published.version)
                .field(2, &Number(published.size))
                .field(3, &Number(published.chunks))
                .build(),
        }
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<PublicationEvent, DecodeError> {
        let mut fields = Fields::read(payload)?;
        if CHUNK.matches(schema) {
            let version: ProfileVersion = fields.required(1, "version")?;
            let index: Number = fields.required(2, "index")?;
            let bytes: Bytes = fields.required(3, "bytes")?;
            fields.finish()?;
            Ok(PublicationEvent::Chunk(Chunk::new(version, index.0, bytes.0)?))
        } else if PUBLISHED.matches(schema) {
            let version: ProfileVersion = fields.required(1, "version")?;
            let size: Number = fields.required(2, "size")?;
            let chunks: Number = fields.required(3, "chunks")?;
            fields.finish()?;
            Ok(PublicationEvent::Published(Published::new(version, size.0, chunks.0)?))
        } else {
            Err(DecodeError::UnknownSchema)
        }
    }
}
