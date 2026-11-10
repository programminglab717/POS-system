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
//! It draws the operating system's randomness for keys and identifiers, and reads the system's
//! clock for the certificates' validity: it is a tool, never part of what a device runs.
//!
//! The CA may issue only devices' certificates (its path length is 0) and is valid for 5 years; a
//! device's certificate is for client authentication, and the hub's for server authentication
//! too, valid for 397 days from a day before it was made. The Device CA's certificates will live
//! 24 to 72 hours and be renewed (security §2); until a device can renew, a pilot's kits are
//! provisioned again within the year. Secret files are written readable by their owner alone.

use core::num::{NonZeroU8, NonZeroU16};
use core::time::Duration;
use std::path::Path;

use keel_domain::codec::Name;
use keel_domain::profile::Profile;
use keel_domain::roster::{
    CloudAddress, HubAddress, Roster, RosterData, RosterDevice, RosterError,
};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_net::{Credentials, device_name};
use keel_types::{Clock, Entropy, Id, IdGenerator, OsEntropy, SystemClock};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
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
    /// The location's cloud, if it has one (ADR-0028).
    pub cloud: Option<CloudSpec>,
}

/// Where a location's cloud listens, and the name its certificate carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudSpec {
    /// Its host: a DNS name or an address.
    pub host: String,
    /// Its port.
    pub port: NonZeroU16,
    /// The name its certificate carries.
    pub name: String,
}

/// The name a location's cloud is known by in its roster.
pub const CLOUD_NAME: &str = "Cloud";

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
    /// Each device's kit, in the roster's order, but for the cloud's.
    pub kits: Vec<Kit>,
    /// The cloud's kit, for the cell (ADR-0028), if the location has a cloud.
    pub cloud: Option<Kit>,
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
    /// The system's clock is before 1970, or too far ahead to add a certificate's life to.
    #[error("the system's clock can't date a certificate")]
    Clock,
}

/// How long the CA's certificate is valid.
const CA_LIFE: Duration = Duration::from_secs(5 * 365 * 86_400);

/// How long a device's certificate is valid: the longest a public CA issues.
const DEVICE_LIFE: Duration = Duration::from_secs(397 * 86_400);

/// How long before it was made a certificate is valid from, for a device whose clock is behind.
const SKEW: Duration = Duration::from_secs(86_400);

/// `params` made valid from [`SKEW`] before `now` for `life`.
fn valid(
    mut params: CertificateParams,
    now: time::OffsetDateTime,
    life: Duration,
) -> Result<CertificateParams, ProvisionError> {
    let offset = |by: Duration| time::Duration::try_from(by).map_err(|_| ProvisionError::Clock);
    params.not_before = now.checked_sub(offset(SKEW)?).ok_or(ProvisionError::Clock)?;
    params.not_after = now.checked_add(offset(life)?).ok_or(ProvisionError::Clock)?;
    Ok(params)
}

/// The system's clock, as a certificate dates.
fn now() -> Result<time::OffsetDateTime, ProvisionError> {
    let seconds = SystemClock.now().as_millis().div_euclid(1000);
    time::OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| ProvisionError::Clock)
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

/// A device of the roster named `name`, with `hub` its priority as the hub, its identifier and
/// signing key drawn: the roster's entry, and the key's secret.
fn draw(
    ids: &mut IdGenerator<OsEntropy>,
    entropy: &mut OsEntropy,
    name: &str,
    hub: Option<NonZeroU8>,
) -> Result<(RosterDevice, [u8; 32]), ProvisionError> {
    let id: Id<Device> = ids
        .generate(SystemClock.now())
        .map_err(|error| ProvisionError::Entropy(error.to_string()))?;
    let signing_key: [u8; 32] = random(entropy)?;
    let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &signing_key)
        .map_err(|error| ProvisionError::Entropy(error.to_string()))?;
    let entry = RosterDevice {
        id,
        name: Name::new(name).map_err(|_| ProvisionError::Name(name.to_owned()))?,
        key: signer.public_key().clone(),
        hub,
    };
    Ok((entry, signing_key))
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
    let now = now()?;
    let ca_key = KeyPair::generate()?;
    let mut ca_params = valid(CertificateParams::new(Vec::<String>::new())?, now, CA_LIFE)?;
    // It issues devices' certificates, and no CA below it.
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, format!("Keel location {}", profile.data().location));
    let ca = ca_params.self_signed(&ca_key)?.der().to_vec();
    let ca_key_der = ca_key.serialize_der();
    let issuer = Issuer::new(ca_params, ca_key);
    let mut devices = Vec::new();
    let mut secrets = Vec::new();
    for device in &spec.devices {
        let (entry, signing_key) = draw(&mut ids, &mut entropy, &device.name, device.hub)?;
        secrets.push((entry.id, device.name.clone(), device.hub.is_some(), signing_key));
        devices.push(entry);
    }
    // The cloud is a device of the roster that never hosts the hub.
    let mut cloud = None;
    if let Some(spec_cloud) = &spec.cloud {
        let (entry, signing_key) = draw(&mut ids, &mut entropy, CLOUD_NAME, None)?;
        let address = CloudAddress {
            device: entry.id,
            host: spec_cloud.host.clone(),
            port: spec_cloud.port,
            name: spec_cloud.name.clone(),
        };
        devices.push(entry);
        cloud = Some((address, signing_key));
    }
    // A roster lists its devices in the ascending order of their identifiers.
    devices.sort_by_key(|device| device.id.to_bytes());
    secrets.sort_by_key(|(id, _, _, _)| id.to_bytes());
    let roster = Roster::new(RosterData {
        location: profile.data().location,
        devices,
        hub: HubAddress {
            host: spec.hub_host.clone(),
            port: spec.hub_port,
            name: spec.hub_name.clone(),
        },
        cloud: cloud.as_ref().map(|(address, _)| address.clone()),
    })?;
    // Each device's certificate names it, and the hub's name for one that may host the hub, or
    // the cloud's for the cloud: each serves what it names.
    let mut kit = |id: Id<Device>, name: &str, serves: Option<&str>, signing_key: [u8; 32]| {
        let mut names = vec![device_name(id)];
        let mut usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        if let Some(served) = serves {
            names.push(served.to_owned());
            usages.push(ExtendedKeyUsagePurpose::ServerAuth);
        }
        let key = KeyPair::generate()?;
        let mut params = valid(CertificateParams::new(names)?, now, DEVICE_LIFE)?;
        params.distinguished_name.push(DnType::CommonName, name);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = usages;
        params.use_authority_key_identifier_extension = true;
        let certificate = params.signed_by(&key, &issuer)?.der().to_vec();
        Ok::<_, ProvisionError>(Kit {
            device: id,
            name: name.to_owned(),
            signing_key,
            store_key: random(&mut entropy)?,
            tls_key: key.serialize_der(),
            certificate,
            ca: ca.clone(),
            roster: roster.encode(),
            profile: spec.profile.clone(),
        })
    };
    let mut kits = Vec::new();
    for (id, name, hub, signing_key) in secrets {
        kits.push(kit(id, &name, hub.then_some(spec.hub_name.as_str()), signing_key)?);
    }
    let cloud = match cloud {
        Some((address, signing_key)) => {
            Some(kit(address.device, CLOUD_NAME, Some(&address.name), signing_key)?)
        }
        None => None,
    };
    Ok(Provisioned { ca, ca_key: ca_key_der, roster, kits, cloud })
}

/// The files of a kit, by name.
const FILES: [&str; 8] =
    ["device", "signing.key", "store.key", "tls.key", "tls.crt", "ca.crt", "roster", "profile"];

/// The files of a kit that hold its secrets.
const SECRETS: [&str; 3] = ["signing.key", "store.key", "tls.key"];

/// Writes `bytes` to `path`, readable and writable by its owner alone where the system has
/// owners, whatever the file was before.
///
/// # Errors
/// [`ProvisionError::Io`] if the file can't be written.
pub fn write_secret(path: &Path, bytes: &[u8]) -> Result<(), ProvisionError> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        options.mode(0o600);
        // A file there already keeps its mode when opened: narrow it before writing.
        if path.exists() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Makes `dir` and its parents, `dir` itself entered by its owner alone where the system has
/// owners.
///
/// # Errors
/// [`ProvisionError::Io`] if it can't be made.
pub fn private_dir(dir: &Path) -> Result<(), ProvisionError> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

impl Kit {
    /// The device's credentials for `keel-net`.
    pub fn credentials(&self) -> Credentials {
        Credentials {
            ca: CertificateDer::from(self.ca.clone()),
            chain: vec![CertificateDer::from(self.certificate.clone())],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.tls_key.clone())),
        }
    }

    /// Writes the kit's files into `dir`, which it makes, entered by its owner alone: `device`
    /// (the identifier's text), `signing.key`, `store.key`, `tls.key` (each readable by its owner
    /// alone), `tls.crt`, `ca.crt`, `roster` and `profile`.
    ///
    /// # Errors
    /// [`ProvisionError::Io`] if a file can't be written.
    pub fn write(&self, dir: &Path) -> Result<(), ProvisionError> {
        private_dir(dir)?;
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
            if SECRETS.contains(file) {
                write_secret(&dir.join(file), bytes)?;
            } else {
                std::fs::write(dir.join(file), bytes)?;
            }
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
