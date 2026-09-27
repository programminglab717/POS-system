//! Verifying events received from other devices: that the device enrolled at the location signed
//! them, and that the device was still trusted at their position in its log.
//!
//! Receiving an event takes two checks. [`DeviceRegistry::verify`] checks the event on its own:
//! its encoding, its signature by the origin device's enrolled key, its location, and whether
//! the device has been revoked. [`LogHead::link`](crate::log::LogHead::link) then checks where
//! the event fits in the device's log. An event that fails either check is quarantined and
//! reported, never applied.

use std::collections::BTreeMap;

use keel_types::Id;

use crate::cose::CoseError;
use crate::envelope::{Device, Location};
use crate::event::{EventError, SignedEvent, UnverifiedEvent};
use crate::hash::EventHash;
use crate::keys::PublicKey;

/// The devices a replica accepts events from: their keys, locations and revocations.
#[derive(Clone, Debug, Default)]
pub struct DeviceRegistry {
    devices: BTreeMap<Id<Device>, DeviceRecord>,
}

/// An enrolled device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRecord {
    location: Id<Location>,
    key: PublicKey,
    revocation: Option<Revocation>,
}

impl DeviceRecord {
    /// The location the device is enrolled at. Its events must be for this location.
    pub const fn location(&self) -> Id<Location> {
        self.location
    }

    /// The device's key, which signs every event in its log.
    pub const fn key(&self) -> &PublicKey {
        &self.key
    }

    /// The device's revocation, if it has been revoked.
    pub const fn revocation(&self) -> Option<Revocation> {
        self.revocation
    }
}

/// The revocation of a device: the events it signed after `after_seq` are no longer trusted.
///
/// A revocation cuts the device's log at a position, not at a time. A stolen device can set its
/// clock back, so a cut-off time could be evaded; but it can't change the events that replicas
/// already hold, and `last_trusted` pins the event at the cut, so a history forged with the
/// stolen key before the cut is caught as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revocation {
    /// The last trusted sequence number. 0 trusts none of the device's events.
    pub after_seq: u64,
    /// The hash of the event at `after_seq`, or [`EventHash::ZERO`] when `after_seq` is 0.
    pub last_trusted: EventHash,
}

impl DeviceRegistry {
    /// An empty registry.
    pub fn new() -> DeviceRegistry {
        DeviceRegistry::default()
    }

    /// Enrolls `device` at `location`, with its key. Enrolling it again with the same location
    /// and key changes nothing.
    ///
    /// # Errors
    /// [`RegistryError::AlreadyEnrolled`] if the device is enrolled with another location or key.
    pub fn enroll(
        &mut self,
        device: Id<Device>,
        location: Id<Location>,
        key: PublicKey,
    ) -> Result<(), RegistryError> {
        match self.devices.get(&device) {
            Some(record) if record.location == location && record.key == key => Ok(()),
            Some(_) => Err(RegistryError::AlreadyEnrolled(device)),
            None => {
                self.devices.insert(device, DeviceRecord { location, key, revocation: None });
                Ok(())
            }
        }
    }

    /// Revokes `device`. A device revoked more than once keeps the earliest cut, so replicas
    /// agree whatever order they learn of revocations in, and a revocation can only tighten.
    ///
    /// # Errors
    /// [`RegistryError::InvalidRevocation`] if `last_trusted` is zero when `after_seq` isn't, or
    /// the reverse; [`RegistryError::UnknownDevice`] if the device isn't enrolled; and
    /// [`RegistryError::ConflictingRevocation`] if it was already revoked at the same position
    /// with a different last trusted event.
    pub fn revoke(
        &mut self,
        device: Id<Device>,
        revocation: Revocation,
    ) -> Result<(), RegistryError> {
        if (revocation.after_seq == 0) != (revocation.last_trusted == EventHash::ZERO) {
            return Err(RegistryError::InvalidRevocation);
        }
        let record = self.devices.get_mut(&device).ok_or(RegistryError::UnknownDevice(device))?;
        match record.revocation {
            Some(existing) if existing.after_seq < revocation.after_seq => Ok(()),
            Some(existing) if existing.after_seq == revocation.after_seq => {
                if existing.last_trusted == revocation.last_trusted {
                    Ok(())
                } else {
                    Err(RegistryError::ConflictingRevocation(device))
                }
            }
            _ => {
                record.revocation = Some(revocation);
                Ok(())
            }
        }
    }

    /// The record of `device`, if it is enrolled.
    pub fn get(&self, device: Id<Device>) -> Option<&DeviceRecord> {
        self.devices.get(&device)
    }

    /// Decodes a received event and checks that its origin device signed it, for the location
    /// the device is enrolled at, at a position in its log where the device was still trusted.
    ///
    /// # Errors
    /// [`Rejection`] saying which check failed.
    pub fn verify(&self, bytes: &[u8]) -> Result<SignedEvent, Rejection> {
        let event = UnverifiedEvent::decode(bytes).map_err(Rejection::Malformed)?;
        let device = event.origin_device();
        let record = self.devices.get(&device).ok_or(Rejection::UnknownDevice(device))?;
        let event = event.verify(&record.key).map_err(Rejection::Signature)?;
        let body = event.body();
        if body.location != record.location {
            return Err(Rejection::WrongLocation);
        }
        if let Some(revocation) = record.revocation {
            let seq = body.origin_seq.get();
            let trusted = seq < revocation.after_seq
                || (seq == revocation.after_seq && event.hash() == revocation.last_trusted);
            if !trusted {
                return Err(Rejection::Revoked { after_seq: revocation.after_seq });
            }
        }
        Ok(event)
    }
}

/// Why a received event was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Rejection {
    /// The bytes aren't a well-formed event.
    #[error("malformed event: {0}")]
    Malformed(EventError),
    /// The origin device isn't enrolled.
    #[error("unknown device {0}")]
    UnknownDevice(Id<Device>),
    /// The event isn't signed by the origin device's key.
    #[error("bad signature: {0}")]
    Signature(CoseError),
    /// The event is for a location other than the one the device is enrolled at.
    #[error("the device is enrolled at another location")]
    WrongLocation,
    /// The device has been revoked, and the event isn't in the trusted part of its log.
    #[error("the device was revoked after event {after_seq}")]
    Revoked {
        /// The last trusted sequence number.
        after_seq: u64,
    },
}

/// Why the registry couldn't be changed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The device is already enrolled, with another location or key.
    #[error("device {0} is already enrolled with another location or key")]
    AlreadyEnrolled(Id<Device>),
    /// The device isn't enrolled.
    #[error("unknown device {0}")]
    UnknownDevice(Id<Device>),
    /// The device was already revoked at the same position, with a different last trusted event.
    #[error("device {0} was already revoked at the same position with another last trusted event")]
    ConflictingRevocation(Id<Device>),
    /// A revocation's last trusted hash must be zero exactly when it trusts no events.
    #[error("a revocation's last trusted hash must be zero exactly when it trusts no events")]
    InvalidRevocation,
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use keel_types::{Hlc, SeededEntropy, Timestamp};

    use super::*;
    use crate::event::golden;
    use crate::keys::{SignatureAlgorithm, Signer as _, SoftwareSigner};
    use crate::log::{EventDraft, LogConfig, LogHead, LogWriter};

    /// Three events from the golden device, and a registry that knows it.
    fn setup() -> (DeviceRegistry, Vec<SignedEvent>) {
        let body = golden::body();
        let config = LogConfig {
            device: body.origin_device,
            location: body.location,
            head: LogHead::EMPTY,
            latest_hlc: Hlc::ZERO,
            max_forward_drift: Duration::from_secs(60),
        };
        let mut writer = LogWriter::new(config, golden::signer(), SeededEntropy::new(1));
        let draft = EventDraft {
            stream: body.stream,
            schema: body.schema,
            business_date: body.business_date,
            actor: body.actor,
            approval: None,
            causation: None,
            correlation: None,
            payload: body.payload,
        };
        let events = (1..=3_u64)
            .map(|second| {
                let now = Timestamp::from_millis(1_790_517_780_000).unwrap();
                let now = now.checked_add(Duration::from_secs(second)).unwrap();
                writer.prepare(draft.clone(), now).unwrap().commit()
            })
            .collect();
        let mut registry = DeviceRegistry::new();
        registry
            .enroll(body.origin_device, body.location, golden::signer().public_key().clone())
            .unwrap();
        (registry, events)
    }

    fn device() -> Id<Device> {
        golden::body().origin_device
    }

    #[test]
    fn events_from_enrolled_devices_verify() {
        let (registry, events) = setup();
        for event in &events {
            assert_eq!(registry.verify(&event.to_bytes()).as_ref(), Ok(event));
        }
        let record = registry.get(device()).unwrap();
        assert_eq!(record.location(), golden::body().location);
        assert_eq!(record.key(), golden::signer().public_key());
        assert_eq!(record.revocation(), None);
    }

    #[test]
    fn events_are_rejected_for_what_they_are() {
        let (registry, events) = setup();
        let bytes = events[0].to_bytes();
        assert!(matches!(registry.verify(&bytes[1..]), Err(Rejection::Malformed(_))));
        assert_eq!(DeviceRegistry::new().verify(&bytes), Err(Rejection::UnknownDevice(device())));

        // The device is enrolled with another key.
        let other_key = SoftwareSigner::from_secret(SignatureAlgorithm::Es256, &[8; 32]).unwrap();
        let mut other = DeviceRegistry::new();
        other.enroll(device(), golden::body().location, other_key.public_key().clone()).unwrap();
        assert_eq!(other.verify(&bytes), Err(Rejection::Signature(CoseError::KeyMismatch)));

        // The device is enrolled at another location.
        let mut elsewhere = DeviceRegistry::new();
        let location = Id::parse("0192f0c1-0000-7000-8000-0000000000ff").unwrap();
        elsewhere.enroll(device(), location, golden::signer().public_key().clone()).unwrap();
        assert_eq!(elsewhere.verify(&bytes), Err(Rejection::WrongLocation));
    }

    #[test]
    fn revocation_cuts_the_log_at_a_position() {
        let (mut registry, events) = setup();
        let revocation = Revocation { after_seq: 2, last_trusted: events[1].hash() };
        registry.revoke(device(), revocation).unwrap();
        assert_eq!(registry.get(device()).unwrap().revocation(), Some(revocation));
        assert!(registry.verify(&events[0].to_bytes()).is_ok());
        assert!(registry.verify(&events[1].to_bytes()).is_ok());
        assert_eq!(
            registry.verify(&events[2].to_bytes()),
            Err(Rejection::Revoked { after_seq: 2 })
        );

        // Another event at the cut, signed with the device's key: a forgery.
        let (_, mut forged) = setup();
        let body = forged[1].body().clone();
        let mut altered = body;
        altered.payload = crate::envelope::Payload::new(&crate::cbor::Value::Null).unwrap();
        forged[1] = SignedEvent::sign(altered, &golden::signer()).unwrap();
        assert_eq!(
            registry.verify(&forged[1].to_bytes()),
            Err(Rejection::Revoked { after_seq: 2 })
        );

        // Revoking before any event trusts none.
        let (mut registry, events) = setup();
        registry
            .revoke(device(), Revocation { after_seq: 0, last_trusted: EventHash::ZERO })
            .unwrap();
        assert_eq!(
            registry.verify(&events[0].to_bytes()),
            Err(Rejection::Revoked { after_seq: 0 })
        );
    }

    #[test]
    fn revocations_only_tighten() {
        let (mut registry, events) = setup();
        let at = |index: usize| Revocation {
            after_seq: u64::try_from(index + 1).unwrap(),
            last_trusted: events[index].hash(),
        };
        registry.revoke(device(), at(1)).unwrap();
        registry.revoke(device(), at(2)).unwrap();
        assert_eq!(registry.get(device()).unwrap().revocation(), Some(at(1)));
        registry.revoke(device(), at(0)).unwrap();
        assert_eq!(registry.get(device()).unwrap().revocation(), Some(at(0)));
        registry.revoke(device(), at(0)).unwrap();
        let conflicting = Revocation { after_seq: 1, last_trusted: events[1].hash() };
        assert_eq!(
            registry.revoke(device(), conflicting),
            Err(RegistryError::ConflictingRevocation(device()))
        );
        assert_eq!(registry.get(device()).unwrap().revocation(), Some(at(0)));
    }

    #[test]
    fn registry_changes_are_checked() {
        let (mut registry, events) = setup();
        let key = golden::signer().public_key().clone();
        let location = golden::body().location;
        registry.enroll(device(), location, key.clone()).unwrap();
        let other_key = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[8; 32]).unwrap();
        assert_eq!(
            registry.enroll(device(), location, other_key.public_key().clone()),
            Err(RegistryError::AlreadyEnrolled(device()))
        );
        let other_location = Id::parse("0192f0c1-0000-7000-8000-0000000000ff").unwrap();
        assert_eq!(
            registry.enroll(device(), other_location, key),
            Err(RegistryError::AlreadyEnrolled(device()))
        );
        let stranger = Id::parse("0192f0c1-0000-7000-8000-0000000000fe").unwrap();
        let revocation = Revocation { after_seq: 1, last_trusted: events[0].hash() };
        assert_eq!(
            registry.revoke(stranger, revocation),
            Err(RegistryError::UnknownDevice(stranger))
        );
        for invalid in [
            Revocation { after_seq: 0, last_trusted: events[0].hash() },
            Revocation { after_seq: 1, last_trusted: EventHash::ZERO },
        ] {
            assert_eq!(registry.revoke(device(), invalid), Err(RegistryError::InvalidRevocation));
        }
        assert_eq!(registry.get(device()).unwrap().revocation(), None);
        assert_eq!(registry.get(stranger), None);
    }
}
