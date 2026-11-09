//! The network over real TLS on the host (ADR-0026, decision 6): a hub and a device exchange
//! frames; a certificate from another CA, a device not in the roster, and a hub whose
//! certificate lacks the roster's name are each refused, as are a certificate naming two devices
//! or a wildcard, and a hub that names a device the roster doesn't let host it; a frame larger
//! than a frame may be closes the connection; both sides writing at once never stall each other;
//! a quiet connection stays up; and a device reconnects after the hub restarts.

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
use std::net::TcpListener;

use keel_domain::codec::Name;
use keel_domain::roster::{HubAddress, Roster, RosterData, RosterDevice};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_net::{Credentials, Event, Net, device_name};
use keel_types::Id;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

const HUB: u64 = 0xd0;
const REGISTER: u64 = 0xd1;
const HUB_NAME: &str = "hub.keel-cafe.local";

/// A CA: its certificate, and what signs with it.
struct Ca {
    certificate: CertificateDer<'static>,
    issuer: Issuer<'static, KeyPair>,
}

fn ca() -> Ca {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let certificate = params.self_signed(&key).unwrap().der().clone();
    Ca { certificate, issuer: Issuer::new(params, key) }
}

/// Credentials from `ca` for a certificate carrying `names`, trusting `trusted`.
fn credentials(ca: &Ca, trusted: &Ca, names: Vec<String>) -> Credentials {
    let key = KeyPair::generate().unwrap();
    let certificate = CertificateParams::new(names).unwrap().signed_by(&key, &ca.issuer).unwrap();
    Credentials {
        ca: trusted.certificate.clone(),
        chain: vec![certificate.der().clone()],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
    }
}

fn roster(port: u16) -> Roster {
    let device = |n: u64, seed: u8, hub: Option<u8>| RosterDevice {
        id: id(n),
        name: Name::new("A device").unwrap(),
        key: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[seed; 32])
            .unwrap()
            .public_key()
            .clone(),
        hub: hub.and_then(NonZeroU8::new),
    };
    Roster::new(RosterData {
        location: id(0xc0fe),
        devices: vec![device(HUB, 1, Some(1)), device(REGISTER, 2, None)],
        hub: HubAddress {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(port).unwrap(),
            name: HUB_NAME.into(),
        },
    })
    .unwrap()
}

fn hub_names() -> Vec<String> {
    vec![HUB_NAME.into(), device_name(id::<Device>(HUB))]
}

/// The first event within `wait` that `want` accepts, skipping others.
fn wait_for(net: &Net, wait: Duration, want: impl Fn(&Event) -> bool) -> Option<Event> {
    let until = std::time::Instant::now() + wait;
    while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
        match net.recv_timeout(left) {
            Some(event) if want(&event) => return Some(event),
            Some(_) => {}
            None => return None,
        }
    }
    None
}

fn bound() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

#[test]
fn a_device_and_its_hub_exchange_frames() {
    let ca = ca();
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    let connected = |n: u64| move |event: &Event| *event == Event::Connected(id(n));
    assert!(
        wait_for(&hub, Duration::from_secs(5), connected(REGISTER)).is_some(),
        "the hub sees the register"
    );
    assert!(
        wait_for(&device, Duration::from_secs(5), connected(HUB)).is_some(),
        "the register sees the hub"
    );
    device.send(id(HUB), b"have".to_vec());
    let frame =
        wait_for(&hub, Duration::from_secs(5), |event| matches!(event, Event::Frame { .. }));
    assert_eq!(frame, Some(Event::Frame { from: id(REGISTER), frame: b"have".to_vec() }));
    // The largest frame there may be crosses.
    let largest = vec![7_u8; keel_sync::MAX_FRAME];
    hub.send(id(REGISTER), largest.clone());
    let frame =
        wait_for(&device, Duration::from_secs(5), |event| matches!(event, Event::Frame { .. }));
    assert_eq!(frame, Some(Event::Frame { from: id(HUB), frame: largest }));
    // A frame for a device not connected is dropped.
    hub.send(id(0xd9), b"lost".to_vec());
    // A frame larger than that closes the connection; the device connects again.
    hub.send(id(REGISTER), vec![7_u8; keel_sync::MAX_FRAME + 1]);
    let dropped =
        wait_for(&hub, Duration::from_secs(5), |event| *event == Event::Disconnected(id(REGISTER)));
    assert!(dropped.is_some(), "a frame too large closes the connection");
    assert!(
        wait_for(&hub, Duration::from_secs(5), connected(REGISTER)).is_some(),
        "and the register comes back"
    );
}

#[test]
fn a_peer_that_cant_prove_who_it_is_is_refused() {
    let (ca, other) = (ca(), ca());
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let quiet = |net: &Net| {
        wait_for(net, Duration::from_secs(2), |event| matches!(event, Event::Connected(_)))
            .is_none()
    };
    // A certificate from another CA.
    let stranger =
        Net::device(roster(port), credentials(&other, &ca, vec![device_name(id(REGISTER))]))
            .unwrap();
    assert!(quiet(&hub) && quiet(&stranger));
    drop(stranger);
    // A device the roster doesn't have.
    let unknown =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(0xd9))])).unwrap();
    assert!(quiet(&hub) && quiet(&unknown));
    drop(unknown);
    // A certificate with no device's name.
    let nameless =
        Net::device(roster(port), credentials(&ca, &ca, vec!["till.local".into()])).unwrap();
    assert!(quiet(&hub) && quiet(&nameless));
    drop((nameless, hub));
    // A hub whose certificate lacks the roster's hub name.
    let (listener, port) = bound();
    let wrong = Net::hub(listener, roster(port), credentials(&ca, &ca, vec![device_name(id(HUB))]))
        .unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    assert!(quiet(&device) && quiet(&wrong));
}

#[test]
fn a_device_connects_again_after_its_hub_restarts() {
    let ca = ca();
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    let hub_connected = |event: &Event| *event == Event::Connected(id(HUB));
    assert!(wait_for(&device, Duration::from_secs(5), hub_connected).is_some());
    drop(hub);
    assert!(
        wait_for(&device, Duration::from_secs(5), |event| *event == Event::Disconnected(id(HUB)))
            .is_some()
    );
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    assert!(
        wait_for(&device, Duration::from_secs(10), hub_connected).is_some(),
        "back within the backoff"
    );
    device.send(id(HUB), b"again".to_vec());
    let frame =
        wait_for(&hub, Duration::from_secs(5), |event| matches!(event, Event::Frame { .. }));
    assert_eq!(frame, Some(Event::Frame { from: id(REGISTER), frame: b"again".to_vec() }));
}

#[test]
fn a_name_that_could_be_two_devices_is_refused() {
    let ca = ca();
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let quiet = |net: &Net| {
        wait_for(net, Duration::from_secs(2), |event| matches!(event, Event::Connected(_)))
            .is_none()
    };
    // Two devices' names.
    let names = vec![device_name(id(REGISTER)), device_name(id(HUB))];
    let both = Net::device(roster(port), credentials(&ca, &ca, names)).unwrap();
    assert!(quiet(&hub) && quiet(&both));
    drop(both);
    // A device's name beside a wildcard that stands for every device.
    let names = vec![device_name(id(REGISTER)), "*.devices.keel".into()];
    let wild = Net::device(roster(port), credentials(&ca, &ca, names)).unwrap();
    assert!(quiet(&hub) && quiet(&wild));
    drop((wild, hub));
    // A hub whose certificate names a device the roster doesn't let host it.
    let (listener, port) = bound();
    let names = vec![HUB_NAME.into(), device_name(id(REGISTER))];
    let usurper = Net::hub(listener, roster(port), credentials(&ca, &ca, names)).unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    assert!(quiet(&device), "the device refuses a hub that may not host");
    drop(usurper);
}

#[test]
fn both_sides_writing_at_once_never_stall() {
    const FRAMES: u8 = 40;
    let ca = ca();
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    assert!(
        wait_for(&device, Duration::from_secs(5), |event| *event == Event::Connected(id(HUB)))
            .is_some()
    );
    assert!(
        wait_for(&hub, Duration::from_secs(5), |event| *event == Event::Connected(id(REGISTER)))
            .is_some()
    );
    // Far more than the sockets' buffers hold, each way at once.
    for n in 0..FRAMES {
        hub.send(id(REGISTER), vec![n; keel_sync::MAX_FRAME]);
        device.send(id(HUB), vec![n; keel_sync::MAX_FRAME]);
    }
    let (mut to_device, mut to_hub) = (0_u8, 0_u8);
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while (to_device < FRAMES || to_hub < FRAMES) && std::time::Instant::now() < until {
        for (net, count) in [(&device, &mut to_device), (&hub, &mut to_hub)] {
            match net.recv_timeout(Duration::from_millis(10)) {
                Some(Event::Frame { frame, .. }) => {
                    assert_eq!(frame, vec![*count; keel_sync::MAX_FRAME], "in order, whole");
                    *count += 1;
                }
                Some(other) => panic!("the connection dropped: {other:?}"),
                None => {}
            }
        }
    }
    assert_eq!((to_device, to_hub), (FRAMES, FRAMES), "every frame crossed both ways");
}

#[test]
fn a_quiet_connection_stays_up() {
    let ca = ca();
    let (listener, port) = bound();
    let hub = Net::hub(listener, roster(port), credentials(&ca, &ca, hub_names())).unwrap();
    let device =
        Net::device(roster(port), credentials(&ca, &ca, vec![device_name(id(REGISTER))])).unwrap();
    assert!(
        wait_for(&device, Duration::from_secs(5), |event| *event == Event::Connected(id(HUB)))
            .is_some()
    );
    // Longer than a connection may stay silent: the pings keep it up.
    let dropped =
        wait_for(&device, Duration::from_secs(20), |event| matches!(event, Event::Disconnected(_)));
    assert_eq!(dropped, None, "the pings keep a quiet connection up");
    device.send(id(HUB), b"still".to_vec());
    let frame =
        wait_for(&hub, Duration::from_secs(5), |event| matches!(event, Event::Frame { .. }));
    assert_eq!(frame, Some(Event::Frame { from: id(REGISTER), frame: b"still".to_vec() }));
}
