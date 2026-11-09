//! # keel-provision
//!
//! The provisioning tool ([ADR-0026](../../../../docs/adr/0026-local-hub.md), decision 5), for
//! development and pilots until the Device CA (step 8): a location's development CA, and for each
//! device its identifier, its signing key, its TLS key and certificate, and the location's roster
//! and profile, all as files a device is provisioned with.
//!
//! [`provision`] makes them in memory, which the tests use; [`Kit::write`] writes a device's to a
//! directory, which `keel-hub` and the shells read with [`Kit::read`].
//!
//! It draws the operating system's randomness for keys and identifiers: it is a tool, never part
//! of what a device runs.

use core::num::{NonZeroU8, NonZeroU16};
use std::path::Path;

use keel_domain::codec::Name;
use keel_domain::profile::Profile;
use keel_domain::roster::{HubAddress, Roster, RosterData, RosterDevice, RosterError};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_net::{Credentials, device_name};
use keel_types::{Clock, Entropy, Id, IdGenerator, OsEntropy, SystemClock};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A device to provision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSpec {
    /// The name staff know it by.
    pub name: String,
    /// Its priority as the Store Hub, if it may host it.
    pub hub: Option<NonZeroU8>,
}

/// What a location's devices are provisioned with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    /// The location's profile.
    pub profile: Vec<u8>,
    /// Where the hub listens: a host and a port.
    pub hub_host: String,
    /// The port.
    pub hub_port: NonZeroU16,
    /// The name the hub's certificate carries.
    pub hub_name: String,
    /// The devices.
    pub devices: Vec<DeviceSpec>,
}

/// One device's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kit {
    /// The device.
    pub device: Id<Device>,
    /// Its name.
    pub name: String,
    /// Its signing key's secret: a software Ed25519 key's 32 bytes.
    pub signing_key: [u8; 32],
    /// Its store's key: 32 random bytes, which a shell keeps in the platform's keystore instead.
    pub store_key: [u8; 32],
    /// Its TLS key, PKCS #8 DER.
    pub tls_key: Vec<u8>,
    /// Its certificate, DER.
    pub certificate: Vec<u8>,
    /// The location's CA's certificate, DER.
    pub ca: Vec<u8>,
    /// The roster's encoding.
    pub roster: Vec<u8>,
    /// The profile's encoding.
    pub profile: Vec<u8>,
}

/// A location, provisioned.
pub struct Provisioned {
    /// The CA's certificate, DER.
    pub ca: Vec<u8>,
    /// The CA's key, PKCS #8 DER: keep it off the devices.
    pub ca_key: Vec<u8>,
    /// The roster.
    pub roster: Roster,
    /// Each device's kit, in the roster's order.
    pub kits: Vec<Kit>,
}

/// Why provisioning failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProvisionError {
    /// The profile doesn't decode.
    #[error("the profile: {0}")]
    Profile(String),
    /// A certificate couldn't be made.
    #[error("a certificate: {0}")]
    Certificate(#[from] rcgen::Error),
    /// The roster breaks a rule.
    #[error("the roster: {0}")]
    Roster(#[from] RosterError),
    /// A device's name isn't a name.
    #[error("{0:?} isn't a device's name")]
    Name(String),
    /// The operating system's randomness failed.
    #[error("randomness: {0}")]
    Entropy(String),
    /// A file couldn't be written or read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A file isn't what it should be.
    #[error("{0} is malformed")]
    Malformed(&'static str),
}

fn random<const N: usize>(entropy: &mut OsEntropy) -> Result<[u8; N], ProvisionError> {
    let mut bytes = [0_u8; N];
    for chunk in bytes.chunks_mut(8) {
        let word =
            entropy.next_u64().map_err(|error| ProvisionError::Entropy(error.to_string()))?;
        for (byte, value) in chunk.iter_mut().zip(word.to_be_bytes()) {
            *byte = value;
        }
    }
    Ok(bytes)
}

/// Provisions the location `spec` describes: a CA, and each device's identifier, keys,
/// certificate, the roster and the profile.
///
/// # Errors
/// [`ProvisionError`] if the profile doesn't decode, a name isn't one, the roster breaks a rule,
/// or a key or certificate can't be made.
pub fn provision(spec: &Spec) -> Result<Provisioned, ProvisionError> {
    let profile = Profile::decode(&spec.profile)
        .map_err(|error| ProvisionError::Profile(error.to_string()))?;
    let mut entropy = OsEntropy;
    let mut ids = IdGenerator::new(OsEntropy);
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new())?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, format!("Keel location {}", profile.data().location));
    let ca = ca_params.self_signed(&ca_key)?.der().to_vec();
    let ca_key_der = ca_key.serialize_der();
    let issuer = Issuer::new(ca_params, ca_key);
    let mut devices = Vec::new();
    let mut secrets = Vec::new();
    for device in &spec.devices {
        let id: Id<Device> = ids
            .generate(SystemClock.now())
            .map_err(|error| ProvisionError::Entropy(error.to_string()))?;
        let signing_key: [u8; 32] = random(&mut entropy)?;
        let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &signing_key)
            .map_err(|error| ProvisionError::Entropy(error.to_string()))?;
        devices.push(RosterDevice {
            id,
            name: Name::new(&device.name).map_err(|_| ProvisionError::Name(device.name.clone()))?,
            key: signer.public_key().clone(),
            hub: device.hub,
        });
        secrets.push((id, device, signing_key));
    }
    // A roster lists its devices in the ascending order of their identifiers.
    devices.sort_by_key(|device| device.id.to_bytes());
    secrets.sort_by_key(|(id, _, _)| id.to_bytes());
    let roster = Roster::new(RosterData {
        location: profile.data().location,
        devices,
        hub: HubAddress {
            host: spec.hub_host.clone(),
            port: spec.hub_port,
            name: spec.hub_name.clone(),
        },
    })?;
    let mut kits = Vec::new();
    for (id, device, signing_key) in secrets {
        let mut names = vec![device_name(id)];
        if device.hub.is_some() {
            names.push(spec.hub_name.clone());
        }
        let key = KeyPair::generate()?;
        let certificate = CertificateParams::new(names)?.signed_by(&key, &issuer)?.der().to_vec();
        kits.push(Kit {
            device: id,
            name: device.name.clone(),
            signing_key,
            store_key: random(&mut entropy)?,
            tls_key: key.serialize_der(),
            certificate,
            ca: ca.clone(),
            roster: roster.encode(),
            profile: spec.profile.clone(),
        });
    }
    Ok(Provisioned { ca, ca_key: ca_key_der, roster, kits })
}

/// The files of a kit, by name.
const FILES: [&str; 8] =
    ["device", "signing.key", "store.key", "tls.key", "tls.crt", "ca.crt", "roster", "profile"];

impl Kit {
    /// The device's credentials for `keel-net`.
    pub fn credentials(&self) -> Credentials {
        Credentials {
            ca: CertificateDer::from(self.ca.clone()),
            chain: vec![CertificateDer::from(self.certificate.clone())],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.tls_key.clone())),
        }
    }

    /// Writes the kit's files into `dir`, which it makes: `device` (the identifier's text),
    /// `signing.key`, `store.key`, `tls.key`, `tls.crt`, `ca.crt`, `roster` and `profile`.
    ///
    /// # Errors
    /// [`ProvisionError::Io`] if a file can't be written.
    pub fn write(&self, dir: &Path) -> Result<(), ProvisionError> {
        std::fs::create_dir_all(dir)?;
        let device = self.device.to_string();
        let contents: [&[u8]; 8] = [
            device.as_bytes(),
            &self.signing_key,
            &self.store_key,
            &self.tls_key,
            &self.certificate,
            &self.ca,
            &self.roster,
            &self.profile,
        ];
        for (file, bytes) in FILES.iter().zip(contents) {
            std::fs::write(dir.join(file), bytes)?;
        }
        std::fs::write(dir.join("name"), &self.name)?;
        Ok(())
    }

    /// The kit whose files are in `dir`.
    ///
    /// # Errors
    /// [`ProvisionError`] if a file is missing or malformed.
    pub fn read(dir: &Path) -> Result<Kit, ProvisionError> {
        let read = |file: &str| std::fs::read(dir.join(file));
        let key = |file: &'static str| -> Result<[u8; 32], ProvisionError> {
            read(file)?.try_into().map_err(|_| ProvisionError::Malformed(file))
        };
        let device =
            String::from_utf8(read("device")?).map_err(|_| ProvisionError::Malformed("device"))?;
        Ok(Kit {
            device: Id::parse(device.trim()).map_err(|_| ProvisionError::Malformed("device"))?,
            name: String::from_utf8(read("name")?)
                .map_err(|_| ProvisionError::Malformed("name"))?,
            signing_key: key("signing.key")?,
            store_key: key("store.key")?,
            tls_key: read("tls.key")?,
            certificate: read("tls.crt")?,
            ca: read("ca.crt")?,
            roster: read("roster")?,
            profile: read("profile")?,
        })
    }
}
