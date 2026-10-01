//! Known answers for frames (ADR-0019, ADR-0020, ADR-0022): pinned byte for byte, as Python's
//! `cbor2` encodes them, and every way a frame is refused.

use keel_types::Id;

use crate::frame::{Durable, Events, Frame, FrameError, Have, Heartbeat, MAX_FRAME, VersionVector};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// [`id`]`(n)` as CBOR: a byte string of 16 bytes (header 0x50), then the bytes.
fn id_cbor(n: u8) -> Vec<u8> {
    vec![0x50, 0x01, 0x92, 0xf0, 0xc1, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0, 0, 0, 0, 0, n]
}

fn vv(entries: &[(u64, u64)]) -> VersionVector {
    entries.iter().map(|&(device, position)| (id(device), position)).collect()
}

#[test]
fn a_have_frame_is_pinned_byte_for_byte() {
    let have =
        Frame::Have(Have { location: id(0x10), vv: vv(&[(1, 3), (2, 300)]), acked: 5, asks: true });
    // [1, 0, location, [[device 1, 3], [device 2, 300]], 5, true]
    let mut expected = vec![0x86, 0x01, 0x00];
    expected.extend(id_cbor(0x10));
    expected.extend([0x82, 0x82]);
    expected.extend(id_cbor(1));
    expected.push(0x03);
    expected.push(0x82);
    expected.extend(id_cbor(2));
    expected.extend([0x19, 0x01, 0x2c]);
    expected.extend([0x05, 0xf5]);
    assert_eq!(have.encode(), expected);
    assert_eq!(Frame::decode(&expected), Ok(have));
}

#[test]
fn an_events_frame_is_pinned_byte_for_byte() {
    let events = Frame::Events(Events { batch: 7, events: vec![b"abc".to_vec(), vec![0]] });
    // [1, 1, 7, [h'616263', h'00']]
    let expected = [0x84, 0x01, 0x01, 0x07, 0x82, 0x43, b'a', b'b', b'c', 0x41, 0x00];
    assert_eq!(events.encode(), expected);
    assert_eq!(Frame::decode(&expected), Ok(events));
}

#[test]
fn a_durable_frame_is_pinned_byte_for_byte() {
    let durable = Frame::Durable(Durable { location: id(0x10), vv: vv(&[(1, 3), (2, 300)]) });
    // [1, 2, location, [[device 1, 3], [device 2, 300]]]
    let mut expected = vec![0x84, 0x01, 0x02];
    expected.extend(id_cbor(0x10));
    expected.extend([0x82, 0x82]);
    expected.extend(id_cbor(1));
    expected.push(0x03);
    expected.push(0x82);
    expected.extend(id_cbor(2));
    expected.extend([0x19, 0x01, 0x2c]);
    assert_eq!(durable.encode(), expected);
    assert_eq!(Frame::decode(&expected), Ok(durable));
}

#[test]
fn a_heartbeat_frame_is_pinned_byte_for_byte() {
    let heartbeat = |priority, epoch, hub: Option<u64>, acting, beat| {
        let hub = hub.map(id);
        Frame::Heartbeat(Heartbeat { location: id(0x10), priority, epoch, hub, acting, beat })
    };
    let pinned = |parts: &[&[u8]]| {
        let mut expected = vec![0x88, 0x01, 0x03];
        expected.extend(id_cbor(0x10));
        for part in parts {
            expected.extend(*part);
        }
        expected
    };
    for (frame, expected) in [
        // [1, 3, location, 2, 300, device 1, true, 5000]: of priority 2, device 1 acts as the hub
        // of epoch 300, at its beat 5000.
        (
            heartbeat(2, 300, Some(1), true, Some(5000)),
            pinned(&[&[0x02, 0x19, 0x01, 0x2c], &id_cbor(1), &[0xf5, 0x19, 0x13, 0x88]]),
        ),
        // [1, 3, location, 0, 0, null, false, null]: it can't be the hub, holds no claim, and
        // gives no beat.
        (heartbeat(0, 0, None, false, None), pinned(&[&[0x00, 0x00, 0xf6, 0xf4, 0xf6]])),
        // [1, 3, location, 255, 2^63 − 1, device 2, false, 2^64 − 1]: the largest of each, the
        // beat of device 2, the hub, passed on.
        (
            heartbeat(255, (1 << 63) - 1, Some(2), false, Some(u64::MAX)),
            pinned(&[
                &[0x18, 0xff, 0x1b, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
                &id_cbor(2),
                &[0xf4, 0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            ]),
        ),
        // [1, 3, location, 1, 7, device 2, false, null]: device 2 is the hub of epoch 7, and the
        // sender has had no beat of it.
        (
            heartbeat(1, 7, Some(2), false, None),
            pinned(&[&[0x01, 0x07], &id_cbor(2), &[0xf4, 0xf6]]),
        ),
    ] {
        assert_eq!(frame.encode(), expected);
        assert_eq!(Frame::decode(&expected), Ok(frame));
    }
}

#[test]
fn an_empty_have_and_an_empty_batch_round_trip() {
    for frame in [
        Frame::Have(Have { location: id(0x10), vv: VersionVector::new(), acked: 0, asks: false }),
        Frame::Events(Events { batch: 0, events: Vec::new() }),
        Frame::Durable(Durable { location: id(0x10), vv: VersionVector::new() }),
        Frame::Heartbeat(Heartbeat {
            location: id(0x10),
            priority: 0,
            epoch: 0,
            hub: None,
            acting: false,
            beat: None,
        }),
    ] {
        assert_eq!(Frame::decode(&frame.encode()), Ok(frame));
    }
}

#[test]
fn a_device_held_up_to_position_0_isnt_sent() {
    let have = |vv| Frame::Have(Have { location: id(0x10), vv, acked: 0, asks: false });
    let decoded = Frame::decode(&have(vv(&[(1, 0), (2, 4)])).encode()).unwrap();
    assert_eq!(decoded, have(vv(&[(2, 4)])));
}

/// A `have` frame from its parts, encoded as given, canonical or not in its contents; it
/// doesn't ask for a `have` in return.
fn have_with(location: Vec<u8>, entries: Vec<Vec<u8>>, acked: u8) -> Vec<u8> {
    let mut frame = vec![0x86, 0x01, 0x00];
    frame.extend(location);
    frame.push(0x80 | u8::try_from(entries.len()).unwrap());
    for entry in entries {
        frame.extend(entry);
    }
    frame.extend([acked, 0xf4]);
    frame
}

/// An entry of a version vector: `[device, position]`, the position below 24.
fn entry(device: Vec<u8>, position: u8) -> Vec<u8> {
    let mut entry = vec![0x82];
    entry.extend(device);
    entry.push(position);
    entry
}

#[test]
fn frames_are_refused_for_what_they_are() {
    for (name, frame, error) in refused_frames() {
        assert_eq!(Frame::decode(&frame), Err(error), "{name}");
    }
}

/// Frames that are refused, and why.
fn refused_frames() -> Vec<(&'static str, Vec<u8>, FrameError)> {
    let mut refused = vec![
        ("too large", vec![0; MAX_FRAME + 1], FrameError::TooLarge),
        ("not CBOR", vec![0xff], FrameError::Cbor),
        ("two items", vec![0x01, 0x01], FrameError::Cbor),
        ("not canonical: 1 in two bytes", vec![0x81, 0x18, 0x01], FrameError::Cbor),
        ("not an array", vec![0x01], FrameError::Malformed),
        ("empty", vec![0x80], FrameError::Malformed),
        ("version 2", vec![0x82, 0x02, 0x00], FrameError::Version(2)),
        ("no kind", vec![0x81, 0x01], FrameError::Malformed),
        ("unknown kind", vec![0x82, 0x01, 0x09], FrameError::Malformed),
        (
            "a batch number that isn't a number",
            vec![0x84, 0x01, 0x01, 0x41, 0x07, 0x80],
            FrameError::Malformed,
        ),
        (
            "an event that isn't bytes",
            vec![0x84, 0x01, 0x01, 0x07, 0x81, 0x63, b'a', b'b', b'c'],
            FrameError::Malformed,
        ),
        (
            "events that aren't an array",
            vec![0x84, 0x01, 0x01, 0x07, 0x41, 0x00],
            FrameError::Malformed,
        ),
    ];
    refused.extend(refused_haves());
    refused.extend(refused_durables());
    refused.extend(refused_heartbeats());
    refused
}

/// A `heartbeat` frame, `[1, 3, location, priority, epoch, hub, acting, beat]`, with `location`
/// and each of `parts` as given.
fn heartbeat_with(location: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let items = parts.len().checked_add(3).and_then(|items| u8::try_from(items).ok());
    let mut frame = vec![0x80 | items.unwrap(), 0x01, 0x03];
    frame.extend(location);
    for part in parts {
        frame.extend(*part);
    }
    frame
}

/// The first 15 bytes of the identifier `id`, as a byte string.
fn short(id: &[u8]) -> Vec<u8> {
    let mut short = vec![0x4f];
    short.extend(&id[1..16]);
    short
}

/// `heartbeat` frames that are refused for their shape or a field out of range, and why.
fn refused_heartbeats() -> Vec<(&'static str, Vec<u8>, FrameError)> {
    let (location, hub) = (id_cbor(0x10), id_cbor(1));
    let (priority, epoch, acting, beat): (&[u8], &[u8], &[u8], &[u8]) =
        (&[0x02], &[0x01], &[0xf5], &[0x07]);
    let at = |parts: &[&[u8]]| heartbeat_with(&location, parts);
    let mut refused = vec![
        ("a heartbeat without its beat", at(&[priority, epoch, &hub, acting])),
        ("a heartbeat with an extra item", at(&[priority, epoch, &hub, acting, beat, &[0x00]])),
        (
            "a heartbeat's location of 15 bytes",
            heartbeat_with(&short(&location), &[priority, epoch, &hub, acting, beat]),
        ),
        ("a priority of 256", at(&[&[0x19, 0x01, 0x00], epoch, &hub, acting, beat])),
        ("a priority of −1", at(&[&[0x20], epoch, &hub, acting, beat])),
        (
            "an epoch of 2^63",
            at(&[priority, &[0x1b, 0x80, 0, 0, 0, 0, 0, 0, 0], &hub, acting, beat]),
        ),
        ("an epoch that isn't a number", at(&[priority, &[0x41, 0x01], &hub, acting, beat])),
        ("a hub of 15 bytes", at(&[priority, epoch, &short(&hub), acting, beat])),
        ("acting that isn't a boolean", at(&[priority, epoch, &hub, &[0x01], beat])),
        ("a beat of −1", at(&[priority, epoch, &hub, acting, &[0x20]])),
        ("a beat that is a boolean", at(&[priority, epoch, &hub, &[0xf4], &[0xf4]])),
    ];
    refused.extend(refused_heartbeat_terms());
    refused.into_iter().map(|(name, frame)| (name, frame, FrameError::Malformed)).collect()
}

/// `heartbeat` frames refused for what they say of the sender's term: a term has a hub and no
/// term has one; acting needs a beat; and a beat, a term.
fn refused_heartbeat_terms() -> Vec<(&'static str, Vec<u8>)> {
    let (location, hub) = (id_cbor(0x10), id_cbor(1));
    let (priority, epoch, acting, beat): (&[u8], &[u8], &[u8], &[u8]) =
        (&[0x02], &[0x01], &[0xf5], &[0x07]);
    let (null, no, zero): (&[u8], &[u8], &[u8]) = (&[0xf6], &[0xf4], &[0x00]);
    let at = |parts: &[&[u8]]| heartbeat_with(&location, parts);
    vec![
        ("a term without its hub", at(&[priority, epoch, null, no, null])),
        ("a hub without a term", at(&[priority, zero, &hub, no, null])),
        ("acting without a term", at(&[priority, zero, null, acting, beat])),
        ("a beat without a term", at(&[priority, zero, null, no, beat])),
        ("acting without a beat", at(&[priority, epoch, &hub, acting, null])),
    ]
}

/// `durable` frames that are refused, and why: as a `have`'s parts are.
fn refused_durables() -> Vec<(&'static str, Vec<u8>, FrameError)> {
    let durable = |location: &[u8], entries: &[Vec<u8>]| {
        let mut frame = vec![0x84, 0x01, 0x02];
        frame.extend(location);
        frame.push(0x80 | u8::try_from(entries.len()).unwrap());
        for entry in entries {
            frame.extend(entry);
        }
        frame
    };
    let location = id_cbor(0x10);
    vec![
        (
            "a durable frame without its version vector",
            {
                let mut frame = durable(&location, &[]);
                frame[0] = 0x83;
                frame.pop();
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a durable frame with an extra item",
            {
                let mut frame = durable(&location, &[]);
                frame[0] = 0x85;
                frame.push(0x00);
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a durable frame's location of 15 bytes",
            {
                let mut short = vec![0x4f];
                short.extend(&location[1..16]);
                durable(&short, &[])
            },
            FrameError::Malformed,
        ),
        (
            "a durable frame's devices out of order",
            durable(&location, &[entry(id_cbor(2), 1), entry(id_cbor(1), 1)]),
            FrameError::Malformed,
        ),
        (
            "a durable frame's device at position 0",
            durable(&location, &[entry(id_cbor(1), 0)]),
            FrameError::Malformed,
        ),
    ]
}

/// `have` frames that are refused, and why.
fn refused_haves() -> Vec<(&'static str, Vec<u8>, FrameError)> {
    let location = id_cbor(0x10);
    vec![
        (
            "a have without its acknowledgement",
            {
                let mut frame = vec![0x84, 0x01, 0x00];
                frame.extend(location.clone());
                frame.push(0x80);
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a have without asks",
            {
                let mut frame = have_with(location.clone(), Vec::new(), 0);
                frame[0] = 0x85;
                frame.pop();
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a have whose asks isn't a boolean",
            {
                let mut frame = have_with(location.clone(), Vec::new(), 0);
                frame.pop();
                frame.push(0x01);
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a have with an extra item",
            {
                let mut frame = have_with(location.clone(), Vec::new(), 0);
                frame[0] = 0x87;
                frame.push(0x00);
                frame
            },
            FrameError::Malformed,
        ),
        (
            "a location of 15 bytes",
            {
                let mut short = vec![0x4f];
                short.extend(&location[1..16]);
                have_with(short, Vec::new(), 0)
            },
            FrameError::Malformed,
        ),
        (
            "a location that isn't a UUIDv7",
            have_with(
                {
                    let mut v4 = location.clone();
                    v4[7] = 0x40;
                    v4
                },
                Vec::new(),
                0,
            ),
            FrameError::Malformed,
        ),
        (
            "devices out of order",
            have_with(location.clone(), vec![entry(id_cbor(2), 1), entry(id_cbor(1), 1)], 0),
            FrameError::Malformed,
        ),
        (
            "a device twice",
            have_with(location.clone(), vec![entry(id_cbor(1), 1), entry(id_cbor(1), 2)], 0),
            FrameError::Malformed,
        ),
        (
            "a device at position 0",
            have_with(location.clone(), vec![entry(id_cbor(1), 0)], 0),
            FrameError::Malformed,
        ),
        (
            "an entry of three items",
            have_with(
                location.clone(),
                vec![{
                    let mut three = entry(id_cbor(1), 1);
                    three[0] = 0x83;
                    three.push(0x01);
                    three
                }],
                0,
            ),
            FrameError::Malformed,
        ),
    ]
}
