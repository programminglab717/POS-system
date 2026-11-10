//! The certificates a location is provisioned with (ADR-0026, decision 5), read back with the
//! verifier `keel-net`'s TLS uses: a device's for client authentication alone, the hub's for
//! server authentication too, each valid from a day before it was made for 397 days; a CA that
//! may issue no CA below it; and the kits' secret files readable by their owner alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU16};
use core::time::Duration;
use std::path::PathBuf;

use keel_provision::{CloudSpec, DeviceSpec, Kit, Provisioned, Spec, provision};
use rustls_pki_types::{CertificateDer, UnixTime};
use webpki::{EndEntityCert, KeyUsage};

fn spec() -> Spec {
    Spec {
        profile: keel_domain::profile::demo().unwrap().encode(),
        hub_host: "127.0.0.1".into(),
        hub_port: NonZeroU16::new(7443).unwrap(),
        hub_name: "hub.keel-cafe.local".into(),
        cloud: None,
        devices: vec![
            DeviceSpec { name: "Back office".into(), hub: NonZeroU8::new(1) },
            DeviceSpec { name: "Front counter".into(), hub: None },
        ],
    }
}

fn kit<'a>(provisioned: &'a Provisioned, name: &str) -> &'a Kit {
    provisioned.kits.iter().find(|kit| kit.name == name).unwrap()
}

fn now() -> Duration {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()
}

/// Whether `certificate` verifies against `ca`, through `intermediates`, for `usage` at `at`.
fn verifies(
    certificate: &[u8],
    ca: &[u8],
    intermediates: &[CertificateDer<'_>],
    usage: KeyUsage,
    at: Duration,
) -> Result<(), webpki::Error> {
    let certificate = CertificateDer::from(certificate);
    let ca = CertificateDer::from(ca);
    let anchor = webpki::anchor_from_trusted_cert(&ca).unwrap();
    let provider = rustls::crypto::ring::default_provider();
    EndEntityCert::try_from(&certificate)?
        .verify_for_usage(
            provider.signature_verification_algorithms.all,
            &[anchor],
            intermediates,
            UnixTime::since_unix_epoch(at),
            usage,
            None,
            None,
        )
        .map(|_| ())
}

#[test]
fn a_device_s_certificate_is_for_what_it_does() {
    let provisioned = provision(&spec()).unwrap();
    let (hub, register) = (kit(&provisioned, "Back office"), kit(&provisioned, "Front counter"));
    let ca = &provisioned.ca;
    let now = now();
    // Every device authenticates as a client; only the hub as a server.
    for kit in [hub, register] {
        assert_eq!(verifies(&kit.certificate, ca, &[], KeyUsage::client_auth(), now), Ok(()));
    }
    assert_eq!(verifies(&hub.certificate, ca, &[], KeyUsage::server_auth(), now), Ok(()));
    assert!(
        matches!(
            verifies(&register.certificate, ca, &[], KeyUsage::server_auth(), now),
            Err(webpki::Error::RequiredEkuNotFoundContext(_))
        ),
        "a register's certificate can't serve as the hub's"
    );
}

#[test]
fn a_location_with_a_cloud_has_the_cloud_s_kit_beside_its_devices() {
    let without = spec();
    let spec = Spec {
        cloud: Some(CloudSpec {
            host: "203.0.113.10".into(),
            port: NonZeroU16::new(8443).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
        ..spec()
    };
    let provisioned = provision(&spec).unwrap();
    let cloud = provisioned.cloud.as_ref().unwrap();
    // The roster names the cloud: a device of its own that never hosts the hub, and not among
    // the kits a location's devices are given.
    let named = provisioned.roster.cloud().unwrap();
    assert_eq!(named.device, cloud.device);
    assert_eq!((named.host.as_str(), named.port.get()), ("203.0.113.10", 8443));
    assert_eq!(provisioned.roster.device(cloud.device).unwrap().hub, None);
    assert!(provisioned.kits.iter().all(|kit| kit.device != cloud.device));
    assert_eq!(provisioned.kits.len(), 2);
    for kit in provisioned.kits.iter().chain([cloud]) {
        assert_eq!(kit.roster, provisioned.roster.encode(), "{}", kit.name);
    }
    // Its certificate serves the cloud's name, and the device's, and nothing of the hub's.
    let now = now();
    for usage in [KeyUsage::client_auth(), KeyUsage::server_auth()] {
        assert_eq!(verifies(&cloud.certificate, &provisioned.ca, &[], usage, now), Ok(()));
    }
    let certificate = CertificateDer::from(cloud.certificate.as_slice());
    let parsed = EndEntityCert::try_from(&certificate).unwrap();
    let names: Vec<_> = parsed.valid_dns_names().collect();
    assert_eq!(names, [keel_net::device_name(cloud.device).as_str(), "cell.keel-cafe.example"]);
    // A location without a cloud has no cloud's kit.
    assert!(provision(&without).unwrap().cloud.is_none());
}

#[test]
fn a_certificate_is_valid_for_397_days_from_the_day_before() {
    let provisioned = provision(&spec()).unwrap();
    let register = kit(&provisioned, "Front counter");
    let check = |at: Duration| {
        verifies(&register.certificate, &provisioned.ca, &[], KeyUsage::client_auth(), at)
    };
    let (day, now) = (Duration::from_secs(86_400), now());
    // A device whose clock is hours behind still takes it.
    assert_eq!(check(now.checked_sub(day / 2).unwrap()), Ok(()));
    assert_eq!(check(now + day * 396), Ok(()));
    assert!(matches!(
        check(now.checked_sub(day * 2).unwrap()),
        Err(webpki::Error::CertNotValidYet { .. })
    ));
    assert!(matches!(check(now + day * 398), Err(webpki::Error::CertExpired { .. })));
}

/// Whether `der` holds `part`.
fn holds(der: &[u8], part: &[u8]) -> bool {
    der.windows(part.len()).any(|window| window == part)
}

#[test]
fn the_ca_issues_devices_certificates_alone() {
    let provisioned = provision(&spec()).unwrap();
    // Basic constraints, a CA with a path length of 0: SEQUENCE { TRUE, 0 }. `rustls-webpki`
    // reads a trust anchor's name and key alone, so a chain through a CA below it would be
    // taken there: the constraint is for other verifiers, and the CA's key, kept off the
    // devices, is what keeps every certificate the location's.
    let constraints = [0x30, 0x06, 0x01, 0x01, 0xff, 0x02, 0x01, 0x00];
    // Key usage, certificates and revocation lists signed: BIT STRING, bits 5 and 6.
    let usage = [0x03, 0x02, 0x01, 0x06];
    assert!(holds(&provisioned.ca, &constraints), "the CA's path length is 0");
    assert!(holds(&provisioned.ca, &usage), "the CA signs certificates and revocations alone");
    // A device's certificate is no CA, and signs alone: BIT STRING, bit 0.
    let signs = [0x03, 0x02, 0x07, 0x80];
    for kit in &provisioned.kits {
        assert!(holds(&kit.certificate, &signs), "{} signs alone", kit.name);
        // No basic constraints at all: OBJECT IDENTIFIER 2.5.29.19.
        assert!(!holds(&kit.certificate, &[0x06, 0x03, 0x55, 0x1d, 0x13]), "{} is no CA", kit.name);
    }
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn a_kit_s_secrets_are_its_owner_s_alone() {
    use std::os::unix::fs::PermissionsExt as _;
    let provisioned = provision(&spec()).unwrap();
    let dir = Dir(std::env::temp_dir().join(format!("keel-kit-modes-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    let path = dir.0.join("kit");
    // A secret file there already, readable by anyone, is narrowed too.
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("tls.key"), b"old").unwrap();
    std::fs::set_permissions(path.join("tls.key"), std::fs::Permissions::from_mode(0o644)).unwrap();
    provisioned.kits[0].write(&path).unwrap();
    let mode =
        |file: &str| std::fs::metadata(path.join(file)).unwrap().permissions().mode() & 0o777;
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
    for secret in ["signing.key", "store.key", "tls.key"] {
        assert_eq!(mode(secret), 0o600, "{secret}");
    }
    assert_eq!(&Kit::read(&path).unwrap(), &provisioned.kits[0]);
}
