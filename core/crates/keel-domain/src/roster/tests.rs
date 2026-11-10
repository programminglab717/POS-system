//! Known answers for rosters: every rule a roster is checked against, its encoding pinned, every
//! way an encoding is refused, and the registry it gives.

use core::fmt::Write as _;
use core::num::{NonZeroU8, NonZeroU16};

use keel_events::cbor::{Map, Value};
use keel_events::hash::sha256;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};

use super::*;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn key(algorithm: SignatureAlgorithm, seed: u8) -> PublicKey {
    SoftwareSigner::from_secret(algorithm, &[seed; 32]).unwrap().public_key().clone()
}

fn device(n: u64, name: &str, seed: u8, hub: Option<u8>) -> RosterDevice {
    RosterDevice {
        id: id(n),
        name: Name::new(name).unwrap(),
        key: key(SignatureAlgorithm::EdDsa, seed),
        hub: hub.and_then(NonZeroU8::new),
    }
}

/// The demo café's roster: its hub, a standby, and a register that never hosts the hub.
fn demo() -> RosterData {
    RosterData {
        location: id(0xc0fe),
        devices: vec![
            device(0xd0, "Back office", 1, Some(2)),
            RosterDevice {
                key: key(SignatureAlgorithm::Es256, 2),
                ..device(0xd1, "Front counter", 2, Some(1))
            },
            device(0xd2, "Patio", 3, None),
        ],
        hub: HubAddress {
            host: "192.168.1.20".to_owned(),
            port: NonZeroU16::new(7443).unwrap(),
            name: "hub.keel-cafe.local".to_owned(),
        },
        cloud: None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[test]
fn a_roster_round_trips_and_gives_its_registry() {
    let roster = Roster::new(demo()).unwrap();
    let bytes = roster.encode();
    let decoded = Roster::decode(&bytes).unwrap();
    assert_eq!(decoded, roster);
    assert_eq!(decoded.encode(), bytes);
    assert_eq!(roster.device(id(0xd1)).unwrap().name.as_str(), "Front counter");
    assert_eq!(roster.device(id(0xd9)), None);
    let registry = roster.registry();
    for device in &roster.data().devices {
        let record = registry.get(device.id).unwrap();
        assert_eq!((record.location(), record.key()), (id(0xc0fe), &device.key));
    }
}

/// Pinned from `tests/golden/roster.py`, which builds the demo roster with Python's `cbor2` from
/// the same keys.
#[test]
fn the_demo_roster_is_pinned_byte_for_byte() {
    let roster = Roster::new(demo()).unwrap();
    // The keys the script writes in, which Python's `cryptography` derives from the same seeds.
    let keys: Vec<String> =
        roster.data().devices.iter().map(|device| hex(&device.key.to_bytes())).collect();
    assert_eq!(keys[0], "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c");
    assert_eq!(keys[2], "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1");
    let bytes = roster.encode();
    assert_eq!(bytes.len(), 304);
    assert_eq!(
        hex(&sha256(&bytes)),
        "5d8a7efae8658ec992afb3559167df4c4aa4ec2792006db37419ef2d1f4c2b19"
    );
}

#[test]
fn a_roster_that_breaks_a_rule_is_refused() {
    type Change = fn(&mut RosterData);
    const NAME: &str = "a hub name that isn't a DNS name a hub may carry";
    let cases: Vec<(Change, &str)> = vec![
        (|d| d.devices.clear(), "a roster with no devices"),
        (
            |d| d.devices.swap(0, 1),
            "devices not each once, in the ascending order of their identifiers",
        ),
        (
            |d| d.devices[1].id = d.devices[0].id,
            "devices not each once, in the ascending order of their identifiers",
        ),
        (|d| d.devices[2].key = d.devices[0].key.clone(), "two devices sharing a key"),
        (
            |d| d.devices.iter_mut().for_each(|device| device.hub = None),
            "no device that may host the hub",
        ),
        (|d| d.hub.host = String::new(), "a hub host that isn't a name or an address"),
        (|d| d.hub.host = "hub .local".to_owned(), "a hub host that isn't a name or an address"),
        (|d| d.hub.host = "h".repeat(254), "a hub host that isn't a name or an address"),
        (|d| d.hub.name = "Hub.local".to_owned(), NAME),
        (|d| d.hub.name = "-hub".to_owned(), NAME),
        (|d| d.hub.name = "hub.".to_owned(), NAME),
        (|d| d.hub.name = String::new(), NAME),
        (|d| d.hub.name = "h".repeat(254), NAME),
        // What `keel-net` couldn't check a certificate for: an empty label, a label too long, a
        // label's edge a hyphen, an address, and a device's name.
        (|d| d.hub.name = "hub..local".to_owned(), NAME),
        (|d| d.hub.name = format!("{}.local", "h".repeat(64)), NAME),
        (|d| d.hub.name = "hub-.local".to_owned(), NAME),
        (|d| d.hub.name = "hub.-local".to_owned(), NAME),
        (|d| d.hub.name = "192.168.1.20".to_owned(), NAME),
        (|d| d.hub.name = "hub.local.2".to_owned(), NAME),
        (|d| d.hub.name = "hub.devices.keel".to_owned(), NAME),
        (|d| d.hub.name = "hub_1.local".to_owned(), NAME),
    ];
    for (index, (change, why)) in cases.into_iter().enumerate() {
        let mut data = demo();
        change(&mut data);
        assert_eq!(Roster::new(data), Err(RosterError::Invalid(why)), "case {index}");
    }
    // At the limits.
    let mut data = demo();
    data.hub.name = format!("{0}.{0}.{0}.{1}", "a".repeat(63), "b".repeat(61));
    assert_eq!(data.hub.name.len(), MAX_HOST);
    data.hub.host = "b".repeat(MAX_HOST);
    assert!(Roster::new(data).is_ok());
    // A name of one label, digits inside a name, and an IPv6 address for a host.
    for (name, host) in [("hub", "::1"), ("hub-1.10.local", "fe80::1%eth0"), ("1hub.keel", "h")] {
        let mut data = demo();
        data.hub.name = name.to_owned();
        data.hub.host = host.to_owned();
        assert!(Roster::new(data).is_ok(), "{name} at {host}");
    }
    let mut data = demo();
    let template = data.devices[2].clone();
    data.devices = (0..MAX_DEVICES)
        .map(|n| RosterDevice {
            id: id(0x10_0000 + u64::try_from(n).unwrap()),
            key: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &sha256(&n.to_be_bytes()))
                .unwrap()
                .public_key()
                .clone(),
            ..template.clone()
        })
        .collect();
    data.devices[0].hub = NonZeroU8::new(1);
    assert!(Roster::new(data.clone()).is_ok());
    data.devices.push(RosterDevice {
        id: id(0x20_0000),
        key: key(SignatureAlgorithm::EdDsa, 9),
        ..template
    });
    assert_eq!(Roster::new(data), Err(RosterError::Invalid("more devices than a roster holds")));
}

/// The demo's encoding as a CBOR value, changed by `change`, then decoded.
fn decode_changed(change: impl FnOnce(&mut Vec<(Value, Value)>)) -> Result<Roster, RosterError> {
    let value = cbor::decode(&Roster::new(demo()).unwrap().encode()).unwrap();
    let mut entries = value.as_map().unwrap().clone().into_entries();
    change(&mut entries);
    Roster::decode(&Value::Map(Map::from_entries(entries).unwrap()).encode())
}

/// The first device's key, as the encoding writes it, replaced by `key`.
fn with_key(entries: &mut [(Value, Value)], key: Value) {
    let devices = &mut entries.iter_mut().find(|(k, _)| *k == Value::Unsigned(3)).unwrap().1;
    let mut list = devices.as_array().unwrap().to_vec();
    let mut first = list[0].as_map().unwrap().clone().into_entries();
    first.iter_mut().find(|(k, _)| *k == Value::Unsigned(3)).unwrap().1 = key;
    list[0] = Value::Map(Map::from_entries(first).unwrap());
    *devices = Value::Array(list);
}

#[test]
fn encodings_other_than_a_rosters_own_are_refused() {
    assert_eq!(
        decode_changed(|e| e[0].1 = Value::Unsigned(3)).map(|_| ()),
        Err(RosterError::Format(3))
    );
    // Format 2 needs its cloud, and format 1 can't have one.
    assert!(matches!(
        decode_changed(|e| e[0].1 = Value::Unsigned(2)),
        Err(RosterError::Payload(_))
    ));
    assert!(matches!(
        decode_changed(|e| e.push((Value::Unsigned(5), Value::Null))),
        Err(RosterError::Payload(_))
    ));
    assert!(matches!(
        decode_changed(|e| e.retain(|(k, _)| *k != Value::Unsigned(4))),
        Err(RosterError::Payload(_))
    ));
    // A key under an algorithm COSE gives another number, or none.
    let ed = key(SignatureAlgorithm::EdDsa, 1).to_bytes();
    for algorithm in [-7, -9, 0, 7] {
        let bad = Value::Array(vec![Value::integer(algorithm), Value::Bytes(ed.clone())]);
        assert!(
            matches!(decode_changed(|e| with_key(e, bad)), Err(RosterError::Payload(_))),
            "{algorithm}"
        );
    }
    // A P-256 key written compressed, which is the same key with another encoding.
    let es = key(SignatureAlgorithm::Es256, 2).to_bytes();
    let mut compressed = vec![2 + (es[64] & 1)];
    compressed.extend_from_slice(&es[1..33]);
    let bad = Value::Array(vec![Value::integer(-7), Value::Bytes(compressed)]);
    assert!(matches!(decode_changed(|e| with_key(e, bad)), Err(RosterError::Payload(_))));
    // The genuine key, written as it is, decodes.
    let good = Value::Array(vec![Value::integer(-8), Value::Bytes(ed)]);
    assert!(decode_changed(|e| with_key(e, good)).is_ok());
    let mut bytes = Roster::new(demo()).unwrap().encode();
    bytes.push(0);
    assert!(matches!(Roster::decode(&bytes), Err(RosterError::Cbor(_))));
}

/// The demo café's roster naming its cloud, a device of its own that never hosts the hub.
fn with_cloud() -> RosterData {
    let mut data = demo();
    data.devices.insert(0, device(0xcc, "Cloud", 4, None));
    data.cloud = Some(CloudAddress {
        device: id(0xcc),
        host: "203.0.113.10".to_owned(),
        port: NonZeroU16::new(8443).unwrap(),
        name: "cell.keel-cafe.example".to_owned(),
    });
    data
}

/// Pinned from `tests/golden/roster.py`, which builds it with Python's `cbor2` in format 2.
#[test]
fn a_roster_naming_its_cloud_is_pinned_in_format_2() {
    let roster = Roster::new(with_cloud()).unwrap();
    assert_eq!(
        hex(&roster.data().devices[0].key.to_bytes()),
        "ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c"
    );
    let bytes = roster.encode();
    assert_eq!(bytes.len(), 429);
    assert_eq!(
        hex(&sha256(&bytes)),
        "7e0cf766e817877300927808285818835efdad15d3614c5dfc13f71ce5365e61"
    );
    assert_eq!(Roster::decode(&bytes).unwrap(), roster);
    assert_eq!(roster.cloud().unwrap().device, id(0xcc));
    // Without a cloud, the roster is format 1, as before format 2.
    let plain = cbor::decode(&Roster::new(demo()).unwrap().encode()).unwrap();
    assert_eq!(plain.as_map().unwrap().get(&Value::Unsigned(1)), Some(&Value::Unsigned(1)));
}

#[test]
fn a_cloud_that_breaks_a_rule_is_refused() {
    type Change = fn(&mut RosterData);
    let cases: Vec<(Change, &str)> = vec![
        (|d| d.cloud.as_mut().unwrap().device = id(0xee), "a cloud that isn't one of the devices"),
        (|d| d.devices[0].hub = NonZeroU8::new(9), "a cloud that may host the hub"),
        (
            |d| d.cloud.as_mut().unwrap().host = String::new(),
            "a cloud host that isn't a name or an address",
        ),
        (
            |d| d.cloud.as_mut().unwrap().host = "a b".to_owned(),
            "a cloud host that isn't a name or an address",
        ),
        (
            |d| d.cloud.as_mut().unwrap().name = "hub.keel-cafe.local".to_owned(),
            "a cloud name that isn't a DNS name a cloud may carry, or is the hub's",
        ),
        (
            |d| d.cloud.as_mut().unwrap().name = "10.0.0.1".to_owned(),
            "a cloud name that isn't a DNS name a cloud may carry, or is the hub's",
        ),
        (
            |d| d.cloud.as_mut().unwrap().name = "cell.devices.keel".to_owned(),
            "a cloud name that isn't a DNS name a cloud may carry, or is the hub's",
        ),
    ];
    for (index, (change, why)) in cases.into_iter().enumerate() {
        let mut data = with_cloud();
        change(&mut data);
        assert_eq!(Roster::new(data), Err(RosterError::Invalid(why)), "case {index}");
    }
    // Format 1 with a cloud's field is refused, as a field it doesn't have.
    assert!(matches!(
        decode_changed(|e| e.push((
            Value::Unsigned(5),
            Value::Map(
                Map::from_entries(vec![(Value::Unsigned(1), Value::Bytes(vec![0; 16]))]).unwrap()
            )
        ))),
        Err(RosterError::Payload(_))
    ));
}
