//! Known answers for publications: the pinned payloads, the registry, each rule a chunk and a
//! publication keep, cutting a profile into chunks and joining them again, and every way a
//! publication's chunks may fail to make its profile.

use keel_events::cbor::{Map, Value};
use keel_events::envelope::{SchemaName, SchemaRef};
use keel_types::{Currency, Id, Money};

use super::*;
use crate::codec::{Name, PayloadError};
use crate::profile::{CatalogItem, CatalogVariant, demo};
use crate::schema::{DecodeError, DomainEvent};

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// A version of 32 bytes counting from 0.
fn counting() -> ProfileVersion {
    ProfileVersion::from_bytes(core::array::from_fn(|n| u8::try_from(n).unwrap()))
}

/// The demo café's profile with `items` more items: about 80 bytes each.
fn large(items: u64) -> Profile {
    let mut data = demo().unwrap().data().clone();
    let food = data.catalog.items[0].tax_category;
    for n in 0..items {
        data.catalog.items.push(CatalogItem {
            id: id(n.checked_add(0x100_0000).unwrap()),
            name: Name::new(&format!("Item {n}")).unwrap(),
            tax_category: food,
            variants: vec![CatalogVariant {
                id: id(n.checked_add(0x200_0000).unwrap()),
                name: None,
                price: Money::from_minor(100, Currency::USD),
            }],
            groups: Vec::new(),
            allergens: std::collections::BTreeSet::new(),
        });
    }
    Profile::new(data).unwrap()
}

/// Pinned payloads, each as Python's `cbor2` encodes the documented key table, canonically.
#[test]
fn the_payload_format_is_pinned() {
    let version = counting();
    let pinned = [
        (
            PublicationEvent::Chunk(Chunk::new(version, 0, b"keel".to_vec()).unwrap()),
            "profile.chunk",
            "a3015820000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f020003446b65656c",
        ),
        (
            PublicationEvent::Published(Published::new(version, 2300, 1).unwrap()),
            "profile.published",
            "a3015820000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f021908fc0301",
        ),
        (
            PublicationEvent::Published(Published::new(version, 196_609, 2).unwrap()),
            "profile.published",
            "a3015820000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f021a000300010302",
        ),
    ];
    for (event, name, hex) in pinned {
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), name);
        assert_eq!(schema.version.get(), 1);
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice(), "{name}");
        assert_eq!(PublicationEvent::decode(&schema, &payload), Ok(event));
    }
}

#[test]
fn a_version_shows_as_64_hexadecimal_digits() {
    assert_eq!(
        counting().to_hex(),
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    );
    assert_eq!(ProfileVersion::from_bytes([0xab; 32]).to_hex(), "ab".repeat(32));
}

#[test]
fn the_registry_lists_each_schema_once() {
    assert_eq!(PublicationEvent::SCHEMAS, &[CHUNK, PUBLISHED]);
    assert_eq!(PublicationEvent::STREAM, "profile");
    for schema in PublicationEvent::SCHEMAS {
        assert!(schema.to_ref().is_ok());
        assert!(schema.name.starts_with("profile."));
    }
    let payload = Published::new(counting(), 10, 1).map(PublicationEvent::Published).unwrap();
    let value = payload.to_value();
    for (name, version) in [("profile.published", 2), ("profile.chunk", 2), ("hub.claimed", 1)] {
        let schema = SchemaRef {
            name: SchemaName::new(name).unwrap(),
            version: version.try_into().unwrap(),
        };
        assert_eq!(
            PublicationEvent::from_value(&schema, &value),
            Err(DecodeError::UnknownSchema),
            "{name} v{version}"
        );
    }
}

#[test]
fn a_chunk_holds_some_bytes_and_no_more_than_a_chunk_may() {
    let version = counting();
    assert_eq!(Chunk::new(version, 0, Vec::new()), Err(PayloadError::Invalid("bytes")));
    assert!(Chunk::new(version, 0, vec![0; MAX_CHUNK]).is_ok());
    assert_eq!(Chunk::new(version, 0, vec![0; MAX_CHUNK + 1]), Err(PayloadError::Invalid("bytes")));
    assert!(Chunk::new(version, MAX_CHUNKS - 1, vec![1]).is_ok());
    assert_eq!(Chunk::new(version, MAX_CHUNKS, vec![1]), Err(PayloadError::Invalid("index")));
    // A chunk of the most bytes, in an event, fits what an event may be with room to spare.
    let (_, payload) =
        PublicationEvent::Chunk(Chunk::new(version, 0, vec![0xff; MAX_CHUNK]).unwrap())
            .encode()
            .unwrap();
    assert!(payload.as_bytes().len() < keel_events::event::MAX_EVENT_BYTES - 16 * 1024);
}

#[test]
fn a_publication_has_as_many_chunks_as_its_size_takes() {
    let version = counting();
    let chunk = u64::try_from(MAX_CHUNK).unwrap();
    assert_eq!(Published::new(version, 0, 0), Err(PayloadError::Invalid("size")));
    assert!(Published::new(version, 1, 1).is_ok());
    assert!(Published::new(version, chunk, 1).is_ok());
    assert_eq!(Published::new(version, chunk, 2), Err(PayloadError::Invalid("chunks")));
    assert_eq!(Published::new(version, chunk + 1, 1), Err(PayloadError::Invalid("chunks")));
    assert!(Published::new(version, chunk + 1, 2).is_ok());
    assert!(Published::new(version, MAX_PROFILE, MAX_CHUNKS).is_ok());
    assert_eq!(
        Published::new(version, MAX_PROFILE + 1, MAX_CHUNKS + 1),
        Err(PayloadError::Invalid("size"))
    );
}

#[test]
fn a_payload_that_breaks_a_rule_is_refused() {
    let chunk = SchemaRef {
        name: SchemaName::new("profile.chunk").unwrap(),
        version: 1.try_into().unwrap(),
    };
    let published = SchemaRef {
        name: SchemaName::new("profile.published").unwrap(),
        version: 1.try_into().unwrap(),
    };
    let map = |entries: Vec<(u64, Value)>| {
        Value::Map(
            Map::from_entries(entries.into_iter().map(|(k, v)| (Value::Unsigned(k), v))).unwrap(),
        )
    };
    let version = Value::Bytes(counting().as_bytes().to_vec());
    let cases = [
        // No bytes.
        (
            &chunk,
            map(vec![(1, version.clone()), (2, Value::Unsigned(0)), (3, Value::Bytes(vec![]))]),
        ),
        // A version that isn't 32 bytes.
        (
            &chunk,
            map(vec![
                (1, Value::Bytes(vec![0; 31])),
                (2, Value::Unsigned(0)),
                (3, Value::Bytes(vec![1])),
            ]),
        ),
        // An unknown key.
        (
            &chunk,
            map(vec![
                (1, version.clone()),
                (2, Value::Unsigned(0)),
                (3, Value::Bytes(vec![1])),
                (4, Value::Unsigned(0)),
            ]),
        ),
        // Chunks the size doesn't take.
        (
            &published,
            map(vec![(1, version.clone()), (2, Value::Unsigned(10)), (3, Value::Unsigned(2))]),
        ),
        // No size.
        (&published, map(vec![(1, version), (3, Value::Unsigned(1))])),
    ];
    for (schema, payload) in cases {
        assert!(
            matches!(
                PublicationEvent::from_value(schema, &payload),
                Err(DecodeError::Malformed(_))
            ),
            "{payload:?}"
        );
    }
}

/// The encoding of the profile [`assemble`] makes: profiles compare by their encodings.
fn joined<'a>(
    published: &Published,
    chunks: impl IntoIterator<Item = &'a Chunk>,
) -> Result<Vec<u8>, PublicationError> {
    assemble(published, chunks).map(|profile| profile.encode())
}

/// The publication and the chunks of `events`.
fn split(events: Vec<PublicationEvent>) -> (Published, Vec<Chunk>) {
    let mut chunks = Vec::new();
    let mut published = None;
    for event in events {
        match event {
            PublicationEvent::Chunk(chunk) => chunks.push(chunk),
            PublicationEvent::Published(publication) => published = Some(publication),
        }
    }
    (published.unwrap(), chunks)
}

#[test]
fn a_profile_published_joins_again_to_itself() {
    // The demo café's profile in one chunk; a larger one in three, cut at 192 KiB.
    for profile in [demo().unwrap(), large(6_000)] {
        let events = publish(&profile).unwrap();
        assert!(matches!(events.last(), Some(PublicationEvent::Published(_))), "published last");
        let (published, chunks) = split(events);
        let encoded = profile.encode();
        assert_eq!(published.version(), version(&profile));
        assert_eq!(published.size(), u64::try_from(encoded.len()).unwrap());
        assert_eq!(published.chunks(), u64::try_from(chunks.len()).unwrap());
        for (index, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index(), u64::try_from(index).unwrap());
            assert_eq!(chunk.version(), published.version());
        }
        // In any order, and with a chunk held twice.
        let mut shuffled: Vec<&Chunk> = chunks.iter().rev().collect();
        shuffled.push(&chunks[0]);
        assert_eq!(joined(&published, shuffled), Ok(profile.encode()));
    }
    let (published, chunks) = split(publish(&large(6_000)).unwrap());
    assert_eq!(published.chunks(), 3);
    assert_eq!(chunks[0].bytes().len(), MAX_CHUNK);
    assert_eq!(chunks[1].bytes().len(), MAX_CHUNK);
}

#[test]
fn chunks_that_dont_make_the_profile_are_refused() {
    let (published, chunks) = split(publish(&large(6_000)).unwrap());
    // One missing.
    assert_eq!(joined(&published, [&chunks[0], &chunks[2]]), Err(PublicationError::Missing(1)));
    // Another version's chunks are no part of it.
    let (other, others) = split(publish(&demo().unwrap()).unwrap());
    assert_eq!(joined(&published, &others), Err(PublicationError::Missing(0)));
    assert_eq!(joined(&other, chunks.iter().chain(&others)), Ok(demo().unwrap().encode()));
    // Two of one index that differ.
    let mut changed = chunks[1].bytes().to_vec();
    changed[0] ^= 1;
    let twin = Chunk::new(published.version(), 1, changed).unwrap();
    assert_eq!(
        joined(&published, chunks.iter().chain([&twin])),
        Err(PublicationError::Conflicting(1))
    );
    // A changed chunk alone: the chunks no longer hash to the version.
    assert_eq!(joined(&published, [&chunks[0], &twin, &chunks[2]]), Err(PublicationError::Hash));
    // A chunk of the wrong length, though the bytes join to the size.
    let (head, tail) = chunks[0].bytes().split_at(10);
    let short = Chunk::new(published.version(), 0, head.to_vec()).unwrap();
    let mut long = tail.to_vec();
    long.extend_from_slice(chunks[1].bytes());
    let long = Chunk::new(published.version(), 1, long[..MAX_CHUNK].to_vec()).unwrap();
    assert_eq!(joined(&published, [&short, &long, &chunks[2]]), Err(PublicationError::Length(0)));
    // Chunks that hash to the version but aren't a profile.
    let junk = vec![0xff; 100];
    let version = ProfileVersion::from_bytes(sha256(&junk));
    let junked = Published::new(version, 100, 1).unwrap();
    let chunk = Chunk::new(version, 0, junk).unwrap();
    assert!(matches!(assemble(&junked, [&chunk]), Err(PublicationError::Profile(_))));
}
