//! The frames replicas exchange (ADR-0019, ADR-0020, ADR-0022): `have`, what a replica holds;
//! `events`, a batch of signed events; `durable`, how far the store's durable replica holds; and
//! `heartbeat`, what a replica says of itself and the Store Hub.
//!
//! Each frame is one canonical CBOR array beginning with the protocol version:
//!
//! - `[1, 0, location, [[device, position], ...], acked, asks]`: `have`. The location is the 16
//!   bytes of its identifier; each device is listed once, by its 16 bytes in ascending order,
//!   with the last position of its log the sender holds (a device the sender holds nothing of
//!   isn't listed); `acked` is the number of the last batch the sender received from the frame's
//!   recipient, 0 if none; and `asks`, a boolean, is true when the sender asks for the
//!   recipient's `have` in return, having heard none from it since it started.
//! - `[1, 1, batch, [event, ...]]`: `events`. The batch's number, and the events as their
//!   devices signed and the sender stored them, each device's in order.
//! - `[1, 2, location, [[device, position], ...]]`: `durable`. How far into each device's log the
//!   location's durable replica, the cloud, holds, as far as the sender knows: the durable-ack
//!   watermark. Listed as in `have`.
//! - `[1, 3, location, priority, epoch, hub, acting, beat]`: `heartbeat`, sent to each peer every
//!   heartbeat period. The sender's priority as hub, from 0, which it can't be now, to 255; the
//!   term of the winning claim it holds: its epoch, from 1 to 2^63 − 1, and its hub, the 16 bytes
//!   of the device's identifier, or 0 and `null` if it holds none; whether the sender is that
//!   hub, acting as one, a boolean; and that hub's beat, a number the hub raises with every period
//!   it acts: the sender's own if it acts, else the latest it had from the hub directly in its
//!   periods of silence, `null` if none. A replica that holds no claim gives no beat, and one
//!   that acts always does.

use std::collections::BTreeMap;

use keel_events::cbor::{self, Value};
use keel_events::envelope::{Device, Location};
use keel_types::Id;

/// The protocol version this kernel speaks.
pub const PROTOCOL: u64 = 1;

/// The largest frame a replica sends or accepts, in bytes. A batch of events stays well below
/// it, since no event is larger than [`keel_events::event::MAX_EVENT_BYTES`].
pub const MAX_FRAME: usize = 1 << 20;

/// The kind of a `have` frame.
const HAVE: u64 = 0;
/// The kind of an `events` frame.
const EVENTS: u64 = 1;
/// The kind of a `durable` frame.
const DURABLE: u64 = 2;
/// The kind of a `heartbeat` frame.
const HEARTBEAT: u64 = 3;

/// The largest epoch a heartbeat names: the largest a claim can (ADR-0022).
const MAX_EPOCH: u64 = (1 << 63) - 1;

/// How far into each device's log a replica holds: for each device, the last position it holds,
/// with every position before it. A device the replica holds nothing of isn't listed.
pub type VersionVector = BTreeMap<Id<Device>, u64>;

/// A frame, as replicas exchange it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    /// What the sender holds.
    Have(Have),
    /// A batch of events.
    Events(Events),
    /// How far the durable replica holds.
    Durable(Durable),
    /// What a replica says of itself and the Store Hub.
    Heartbeat(Heartbeat),
}

/// What a replica holds, and the last batch it received from the frame's recipient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Have {
    /// The location whose events the sender holds.
    pub location: Id<Location>,
    /// The sender's version vector. Entries of 0 aren't sent.
    pub vv: VersionVector,
    /// The number of the last batch the sender received from the recipient; 0 if none.
    pub acked: u64,
    /// Whether the sender asks for the recipient's `have` in return: it has heard none from the
    /// recipient since it started.
    pub asks: bool,
}

/// A batch of events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Events {
    /// The batch's number, which the recipient's next `have` acknowledges.
    pub batch: u64,
    /// The events, as stored: each device's in order.
    pub events: Vec<Vec<u8>>,
}

/// How far into each device's log the location's durable replica holds, as far as the sender
/// knows: the durable-ack watermark (ADR-0020).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Durable {
    /// The location.
    pub location: Id<Location>,
    /// The watermark. Entries of 0 aren't sent.
    pub vv: VersionVector,
}

/// What a replica says of itself and the Store Hub, every heartbeat period (ADR-0022).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    /// The location.
    pub location: Id<Location>,
    /// The sender's priority as hub: 0 if it can't be the hub now.
    pub priority: u8,
    /// The epoch of the winning claim the sender holds: 0 if it holds none.
    pub epoch: u64,
    /// The device whose claim that is, the term's hub: `None` exactly when the epoch is 0.
    pub hub: Option<Id<Device>>,
    /// Whether the sender is that hub, acting as one.
    pub acting: bool,
    /// That hub's beat, which it raises with every period it acts: the sender's own if it acts
    /// as the hub, else the latest it had from the hub directly, in its periods of silence.
    /// `None` if it had none; always while it acts, and never while it holds no claim.
    pub beat: Option<u64>,
}

/// Why a frame was dropped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FrameError {
    /// The frame is larger than [`MAX_FRAME`].
    #[error("the frame is larger than {MAX_FRAME} bytes")]
    TooLarge,
    /// The frame isn't one canonical CBOR data item.
    #[error("the frame isn't canonical CBOR")]
    Cbor,
    /// The frame is for a protocol version this kernel doesn't speak.
    #[error("protocol version {0} isn't spoken here")]
    Version(u64),
    /// The frame's contents aren't a `have`, `events`, `durable` or `heartbeat` frame.
    #[error("the frame is malformed")]
    Malformed,
}

impl Frame {
    /// The frame, encoded.
    pub fn encode(&self) -> Vec<u8> {
        let value = match self {
            Frame::Have(have) => Value::Array(vec![
                Value::Unsigned(PROTOCOL),
                Value::Unsigned(HAVE),
                Value::Bytes(have.location.to_bytes().to_vec()),
                vv_value(&have.vv),
                Value::Unsigned(have.acked),
                Value::Bool(have.asks),
            ]),
            Frame::Durable(durable) => Value::Array(vec![
                Value::Unsigned(PROTOCOL),
                Value::Unsigned(DURABLE),
                Value::Bytes(durable.location.to_bytes().to_vec()),
                vv_value(&durable.vv),
            ]),
            Frame::Events(events) => Value::Array(vec![
                Value::Unsigned(PROTOCOL),
                Value::Unsigned(EVENTS),
                Value::Unsigned(events.batch),
                Value::Array(events.events.iter().cloned().map(Value::Bytes).collect()),
            ]),
            Frame::Heartbeat(heartbeat) => Value::Array(vec![
                Value::Unsigned(PROTOCOL),
                Value::Unsigned(HEARTBEAT),
                Value::Bytes(heartbeat.location.to_bytes().to_vec()),
                Value::Unsigned(u64::from(heartbeat.priority)),
                Value::Unsigned(heartbeat.epoch),
                heartbeat.hub.map_or(Value::Null, |hub| Value::Bytes(hub.to_bytes().to_vec())),
                Value::Bool(heartbeat.acting),
                heartbeat.beat.map_or(Value::Null, Value::Unsigned),
            ]),
        };
        value.encode()
    }

    /// Decodes a frame.
    ///
    /// # Errors
    /// [`FrameError`] saying why the frame isn't one this kernel accepts.
    pub fn decode(bytes: &[u8]) -> Result<Frame, FrameError> {
        if bytes.len() > MAX_FRAME {
            return Err(FrameError::TooLarge);
        }
        let Value::Array(items) = cbor::decode(bytes).map_err(|_| FrameError::Cbor)? else {
            return Err(FrameError::Malformed);
        };
        let mut items = items.into_iter();
        let version = items.next().and_then(|item| item.as_u64()).ok_or(FrameError::Malformed)?;
        if version != PROTOCOL {
            return Err(FrameError::Version(version));
        }
        let kind = items.next().and_then(|item| item.as_u64());
        let rest: Vec<Value> = items.collect();
        match (kind, rest.as_slice()) {
            (Some(HAVE), [location, vv, acked, asks]) => Ok(Frame::Have(Have {
                location: id(location)?,
                vv: version_vector(vv)?,
                acked: acked.as_u64().ok_or(FrameError::Malformed)?,
                asks: asks.as_bool().ok_or(FrameError::Malformed)?,
            })),
            (Some(DURABLE), [location, vv]) => {
                Ok(Frame::Durable(Durable { location: id(location)?, vv: version_vector(vv)? }))
            }
            (Some(HEARTBEAT), [location, priority, epoch, hub, acting, beat]) => {
                let epoch = epoch.as_u64().filter(|&epoch| epoch <= MAX_EPOCH);
                let epoch = epoch.ok_or(FrameError::Malformed)?;
                let acting = acting.as_bool().ok_or(FrameError::Malformed)?;
                // A term has a hub, and no term none; a hub acts only with a beat, and only a
                // replica holding a term gives one.
                let hub = match hub {
                    Value::Null if epoch == 0 => None,
                    hub if epoch > 0 => Some(id(hub)?),
                    _ => return Err(FrameError::Malformed),
                };
                let beat = match beat {
                    Value::Null if !acting => None,
                    beat if epoch > 0 => Some(beat.as_u64().ok_or(FrameError::Malformed)?),
                    _ => return Err(FrameError::Malformed),
                };
                Ok(Frame::Heartbeat(Heartbeat {
                    location: id(location)?,
                    priority: priority
                        .as_u64()
                        .and_then(|priority| u8::try_from(priority).ok())
                        .ok_or(FrameError::Malformed)?,
                    epoch,
                    hub,
                    acting,
                    beat,
                }))
            }
            (Some(EVENTS), [batch, events]) => Ok(Frame::Events(Events {
                batch: batch.as_u64().ok_or(FrameError::Malformed)?,
                events: events
                    .as_array()
                    .ok_or(FrameError::Malformed)?
                    .iter()
                    .map(|event| event.as_bytes().map(<[u8]>::to_vec).ok_or(FrameError::Malformed))
                    .collect::<Result<_, _>>()?,
            })),
            _ => Err(FrameError::Malformed),
        }
    }
}

/// The encoding of `vv`: each device listed once, in ascending order, with a position from 1.
fn vv_value(vv: &VersionVector) -> Value {
    Value::Array(
        vv.iter()
            .filter(|(_, position)| **position > 0)
            .map(|(device, position)| {
                Value::Array(vec![
                    Value::Bytes(device.to_bytes().to_vec()),
                    Value::Unsigned(*position),
                ])
            })
            .collect(),
    )
}

/// The identifier in `value`, 16 bytes.
fn id<T>(value: &Value) -> Result<Id<T>, FrameError> {
    let bytes: [u8; 16] =
        value.as_bytes().and_then(|bytes| bytes.try_into().ok()).ok_or(FrameError::Malformed)?;
    Id::from_bytes(bytes).map_err(|_| FrameError::Malformed)
}

/// The version vector in `value`: devices in ascending order, each once, with positions from 1.
fn version_vector(value: &Value) -> Result<VersionVector, FrameError> {
    let mut vv = VersionVector::new();
    for entry in value.as_array().ok_or(FrameError::Malformed)? {
        let [device, position] = entry.as_array().ok_or(FrameError::Malformed)? else {
            return Err(FrameError::Malformed);
        };
        let device: Id<Device> = id(device)?;
        let position = position.as_u64().filter(|position| *position > 0);
        let position = position.ok_or(FrameError::Malformed)?;
        if vv.last_key_value().is_some_and(|(last, _)| *last >= device) {
            return Err(FrameError::Malformed);
        }
        vv.insert(device, position);
    }
    Ok(vv)
}
