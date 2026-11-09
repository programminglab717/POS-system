//! The roster v0 (ADR-0026, decision 1): a location's devices, the keys they sign with, which may
//! host the Store Hub, and where the hub is, in one document every device and the hub is
//! provisioned with.
//!
//! Canonical CBOR, decoded as strictly as a profile is, format 1:
//!
//! | Key | Field | Rules |
//! |---|---|---|
//! | 1 | format | 1 |
//! | 2 | location | an identifier |
//! | 3 | devices | 1 to [`MAX_DEVICES`], in the ascending order of their identifiers' bytes |
//! | 4 | hub | `{1: host, 2: port, 3: name}` |
//!
//! A device is `{1: identifier, 2: name, 3: [algorithm, public key], 4: priority}`: its algorithm
//! by its COSE identifier, -7 for ES256 (its key a 65-byte SEC1 point) and -8 for EdDSA (32
//! bytes); its priority as the hub, 1 to 255, higher preferred, absent for a device that never
//! hosts it. No two devices share a key. The hub's host is a name or an address, its port 1 to
//! 65,535, and its name, lowercase letters, digits, dots and hyphens, is the one its certificate
//! carries, which devices check instead of an address.
//!
//! Until the cloud signs rosters (step 8), a roster is trusted as provisioned.

use core::num::{NonZeroU8, NonZeroU16};
use std::collections::BTreeSet;

use keel_events::cbor::{self, CborError, Value};
use keel_events::envelope::{Device, Location};
use keel_events::keys::{PublicKey, SignatureAlgorithm};
use keel_events::verify::DeviceRegistry;
use keel_types::Id;

use crate::codec::{Field, Fields, Name, PayloadError, Record};

/// The roster format this kernel reads and writes.
pub const FORMAT: u64 = 1;
/// The most devices a roster holds.
pub const MAX_DEVICES: usize = 1_000;
/// The longest host or hub name, in bytes, as DNS allows.
pub const MAX_HOST: usize = 253;

/// A device of the location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterDevice {
    /// Its identifier.
    pub id: Id<Device>,
    /// The name staff know it by, such as "Front counter".
    pub name: Name,
    /// The key it signs its events with.
    pub key: PublicKey,
    /// Its priority as the Store Hub, higher preferred; `None` if it never hosts it.
    pub hub: Option<NonZeroU8>,
}

/// Where the Store Hub listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubAddress {
    /// Its host: a name or an address.
    pub host: String,
    /// Its port.
    pub port: NonZeroU16,
    /// The name its certificate carries.
    pub name: String,
}

/// A roster's contents, unchecked: [`Roster::new`] checks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterData {
    /// The location.
    pub location: Id<Location>,
    /// Its devices, in the ascending order of their identifiers' bytes.
    pub devices: Vec<RosterDevice>,
    /// Where the hub listens.
    pub hub: HubAddress,
}

/// A location's roster, checked whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roster {
    data: RosterData,
}

/// Why a roster can't be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RosterError {
    /// The bytes aren't canonical CBOR.
    #[error("the roster isn't canonical CBOR: {0}")]
    Cbor(#[from] CborError),
    /// A roster format this kernel doesn't read.
    #[error("roster format {0} isn't one this kernel reads")]
    Format(u64),
    /// A field is missing, unknown or malformed.
    #[error(transparent)]
    Payload(#[from] PayloadError),
    /// A rule the roster breaks.
    #[error("{0}")]
    Invalid(&'static str),
}

impl Roster {
    /// `data`, checked: between one and [`MAX_DEVICES`] devices, in the ascending order of their
    /// identifiers' bytes, no two sharing a key, and at least one that may host the hub; and the
    /// hub's host and name valid.
    ///
    /// # Errors
    /// [`RosterError::Invalid`] for the first rule `data` breaks.
    pub fn new(data: RosterData) -> Result<Roster, RosterError> {
        if data.devices.is_empty() {
            return Err(RosterError::Invalid("a roster with no devices"));
        }
        if data.devices.len() > MAX_DEVICES {
            return Err(RosterError::Invalid("more devices than a roster holds"));
        }
        let ascending = data.devices.windows(2).all(|pair| match pair {
            [left, right] => left.id.to_bytes() < right.id.to_bytes(),
            _ => true,
        });
        if !ascending {
            return Err(RosterError::Invalid(
                "devices not each once, in the ascending order of their identifiers",
            ));
        }
        let keys: BTreeSet<Vec<u8>> =
            data.devices.iter().map(|device| device.key.to_bytes()).collect();
        if keys.len() != data.devices.len() {
            return Err(RosterError::Invalid("two devices sharing a key"));
        }
        if data.devices.iter().all(|device| device.hub.is_none()) {
            return Err(RosterError::Invalid("no device that may host the hub"));
        }
        let host = &data.hub.host;
        if host.is_empty()
            || host.len() > MAX_HOST
            || host.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(RosterError::Invalid("a hub host that isn't a name or an address"));
        }
        if !is_hub_name(&data.hub.name) {
            return Err(RosterError::Invalid("a hub name that isn't a DNS name a hub may carry"));
        }
        Ok(Roster { data })
    }

    /// The roster `bytes` encode, checked.
    ///
    /// # Errors
    /// [`RosterError`] if `bytes` aren't a roster of format [`FORMAT`] in canonical CBOR, or it
    /// breaks a rule ([`Roster::new`]).
    pub fn decode(bytes: &[u8]) -> Result<Roster, RosterError> {
        let value = cbor::decode(bytes)?;
        let mut fields = Fields::read(&value)?;
        let format: u64 = fields.required(1, "roster format")?;
        if format != FORMAT {
            return Err(RosterError::Format(format));
        }
        let data = RosterData {
            location: fields.required(2, "location")?,
            devices: fields.required(3, "devices")?,
            hub: fields.required(4, "hub")?,
        };
        fields.finish()?;
        Roster::new(data)
    }

    /// The roster's canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        Record::default()
            .field(1, &FORMAT)
            .field(2, &self.data.location)
            .field(3, &self.data.devices)
            .field(4, &self.data.hub)
            .build()
            .encode()
    }

    /// What the roster holds.
    pub const fn data(&self) -> &RosterData {
        &self.data
    }

    /// The device `device`, if the roster has it.
    pub fn device(&self, device: Id<Device>) -> Option<&RosterDevice> {
        self.data.devices.iter().find(|candidate| candidate.id == device)
    }

    /// The location's devices and their keys, which a store verifies the events it receives with.
    pub fn registry(&self) -> DeviceRegistry {
        let mut registry = DeviceRegistry::new();
        for device in &self.data.devices {
            // Identifiers are unique: enrolling can't conflict.
            let _ = registry.enroll(device.id, self.data.location, device.key.clone());
        }
        registry
    }
}

/// What every device's name in its certificate ends with, which no hub's name may.
const DEVICE_NAMES: &str = ".devices.keel";

/// Whether `name` is a hub's name, a DNS name a certificate can carry: 1 to [`MAX_HOST`] bytes of
/// labels joined by dots, each 1 to 63 lowercase letters, digits and hyphens, neither starting
/// nor ending with a hyphen; the last not all digits, so no address is taken for a name; and not
/// a device's name.
fn is_hub_name(name: &str) -> bool {
    let label_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && label.chars().all(label_char)
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    let last = name.rsplit('.').next().unwrap_or_default();
    !name.is_empty()
        && name.len() <= MAX_HOST
        && name.split('.').all(label)
        && !last.chars().all(|c| c.is_ascii_digit())
        && !name.ends_with(DEVICE_NAMES)
}

/// COSE's identifier of `algorithm`.
const fn cose(algorithm: SignatureAlgorithm) -> i64 {
    match algorithm {
        SignatureAlgorithm::Es256 => -7,
        SignatureAlgorithm::EdDsa => -8,
    }
}

impl Field for RosterDevice {
    fn to_value(&self) -> Value {
        let key = Value::Array(vec![
            Value::integer(cose(self.key.algorithm())),
            Value::Bytes(self.key.to_bytes()),
        ]);
        Record::default()
            .field(1, &self.id)
            .field(2, &self.name)
            .field(3, &Raw(key))
            .optional(4, self.hub.as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<RosterDevice> {
        let mut fields = Fields::read(value).ok()?;
        let id = fields.required(1, "device").ok()?;
        let name = fields.required(2, "device name").ok()?;
        let key = fields.required::<Raw>(3, "key").ok()?.0;
        let hub = fields.optional(4, "hub priority").ok()?;
        fields.finish().ok()?;
        let [algorithm, bytes] = key.as_array()? else { return None };
        let bytes = bytes.as_bytes()?;
        let key = match algorithm.as_i64()? {
            -7 => PublicKey::es256_from_sec1(bytes).ok()?,
            -8 => PublicKey::ed25519_from_bytes(bytes).ok()?,
            _ => return None,
        };
        // A key has one encoding: what was read must be what it writes.
        (key.to_bytes() == bytes).then_some(RosterDevice { id, name, key, hub })
    }
}

impl Field for HubAddress {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &Raw(Value::from(self.host.as_str())))
            .field(2, &self.port)
            .field(3, &Raw(Value::from(self.name.as_str())))
            .build()
    }

    fn from_value(value: &Value) -> Option<HubAddress> {
        let mut fields = Fields::read(value).ok()?;
        let host = fields.required::<Raw>(1, "hub host").ok()?.0;
        let port = fields.required(2, "hub port").ok()?;
        let name = fields.required::<Raw>(3, "hub name").ok()?.0;
        fields.finish().ok()?;
        Some(HubAddress {
            host: host.as_text()?.to_owned(),
            port,
            name: name.as_text()?.to_owned(),
        })
    }
}

/// Any value, as it is.
struct Raw(Value);

impl Field for Raw {
    fn to_value(&self) -> Value {
        self.0.clone()
    }

    fn from_value(value: &Value) -> Option<Raw> {
        Some(Raw(value.clone()))
    }
}

#[cfg(test)]
mod tests;
