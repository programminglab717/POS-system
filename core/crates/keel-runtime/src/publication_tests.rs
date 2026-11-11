//! Known answers for taking publications that a runtime's own log holds, written event by event
//! as the cloud's: one of another location, one missing a chunk, and one whose team no longer
//! has the member signed in.

use core::num::{NonZeroU8, NonZeroU16};

use keel_domain::codec::Name;
use keel_domain::profile::{Profile, demo};
use keel_domain::publication::{self, PublicationEvent};
use keel_domain::roster::{CloudAddress, HubAddress, Roster, RosterData, RosterDevice};
use keel_events::envelope::{Actor, Component, Device};
use keel_events::keys::{SignatureAlgorithm, Signer, SoftwareSigner};
use keel_types::Clock;

use crate::runtime::{Meta, append};
use crate::tests::{Scratch, TestRuntime, demo_with, id, register};

/// The demo café's roster, the runtime's device (0xd1) its cloud, beside a hub.
fn roster() -> Roster {
    let key = |seed: u8| {
        SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[seed; 32])
            .unwrap()
            .public_key()
            .clone()
    };
    Roster::new(RosterData {
        location: demo().unwrap().data().location,
        devices: vec![
            RosterDevice {
                id: id(0xd0),
                name: Name::new("Back office").unwrap(),
                key: key(1),
                hub: NonZeroU8::new(1),
            },
            RosterDevice {
                id: id::<Device>(0xd1),
                name: Name::new("Cloud").unwrap(),
                key: key(7),
                hub: None,
            },
        ],
        hub: HubAddress {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(7443).unwrap(),
            name: "hub.keel-cafe.local".into(),
        },
        cloud: Some(CloudAddress {
            device: id(0xd1),
            host: "203.0.113.10".into(),
            port: NonZeroU16::new(8443).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
    })
    .unwrap()
}

/// `events` written to the runtime's log in a stream of their own, as the cloud writes them.
fn write(runtime: &mut TestRuntime, events: &[PublicationEvent]) {
    let now = runtime.clock.now();
    let meta = Meta {
        now,
        business_date: runtime.profile.business_day().business_date_of(now).unwrap(),
        actor: Actor::System(Component::new("cloud").unwrap()),
        correlation: runtime.ids.generate(now).unwrap(),
        approval: None,
    };
    let stream = runtime.ids.generate(now).unwrap();
    runtime
        .store
        .write(|w| {
            for event in events {
                append(w, stream, event, &meta)?;
            }
            Ok::<_, crate::RuntimeError>(())
        })
        .unwrap();
}

fn cloud(scratch: &Scratch) -> TestRuntime {
    let mut runtime = register(scratch, demo().unwrap());
    runtime.start_sync(roster()).unwrap();
    runtime
}

#[test]
fn a_publication_of_another_location_is_never_taken() {
    let scratch = Scratch::new();
    let mut runtime = cloud(&scratch);
    let before = runtime.profile_version();
    let elsewhere = demo_with(|data| data.location = id(0xbad));
    write(&mut runtime, &publication::publish(&elsewhere).unwrap());
    assert!(!runtime.take_publications().unwrap());
    assert_eq!(runtime.profile_version(), before);
    // A later one of its own location is.
    let espresso =
        demo_with(|data| crate::tests::variant(data, 0x401).price = crate::tests::usd(410));
    write(&mut runtime, &publication::publish(&espresso).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.profile_version(), publication::version(&espresso));
    // Looked at once: nothing more to take.
    assert!(!runtime.take_publications().unwrap());
}

#[test]
fn a_publication_missing_a_chunk_is_never_taken() {
    let scratch = Scratch::new();
    let mut runtime = cloud(&scratch);
    let before = runtime.profile_version();
    let espresso =
        demo_with(|data| crate::tests::variant(data, 0x401).price = crate::tests::usd(410));
    let mut events = publication::publish(&espresso).unwrap();
    events.remove(0);
    write(&mut runtime, &events);
    assert!(!runtime.take_publications().unwrap());
    assert_eq!(runtime.profile_version(), before);
    // Its chunk written later, in another stream, makes no publication of it.
    let chunk = publication::publish(&espresso).unwrap().remove(0);
    write(&mut runtime, &[chunk]);
    assert!(!runtime.take_publications().unwrap());
    assert_eq!(runtime.profile().encode(), demo().unwrap().encode());
}

#[test]
fn a_member_the_new_team_doesnt_have_is_signed_out() {
    let scratch = Scratch::new();
    let mut runtime = cloud(&scratch);
    runtime.sign_in(id(0x501), "1234").unwrap();
    let without: Profile = demo_with(|data| data.team.retain(|member| member.id != id(0x501)));
    write(&mut runtime, &publication::publish(&without).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.signed_in(), None);
    // One it still has stays signed in.
    let other = without.data().team[0].id;
    runtime.sign_in(other, "2468").unwrap();
    let espresso = demo_with(|data| {
        data.team.retain(|member| member.id != id(0x501));
        crate::tests::variant(data, 0x401).price = crate::tests::usd(410);
    });
    write(&mut runtime, &publication::publish(&espresso).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.signed_in(), Some(other));
}

/// Found in review: a member signed in stayed so when their PIN or role changed under them.
#[test]
fn a_member_whose_pin_or_role_changes_signs_in_again() {
    let scratch = Scratch::new();
    let mut runtime = cloud(&scratch);
    runtime.sign_in(id(0x502), "2468").unwrap();
    // A price changed: nothing of Sam's, who stays.
    let espresso =
        demo_with(|data| crate::tests::variant(data, 0x401).price = crate::tests::usd(410));
    write(&mut runtime, &publication::publish(&espresso).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.signed_in(), Some(id(0x502)));
    // A new PIN for Sam.
    let new_pin = demo_with(|data| {
        crate::tests::variant(data, 0x401).price = crate::tests::usd(410);
        data.team[1].pin = Some(keel_domain::staff::PinHash::new("8642", [3; 16]).unwrap());
    });
    write(&mut runtime, &publication::publish(&new_pin).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.signed_in(), None);
    // Sam made a manager.
    runtime.sign_in(id(0x502), "8642").unwrap();
    let promoted = demo_with(|data| {
        crate::tests::variant(data, 0x401).price = crate::tests::usd(410);
        data.team[1].pin = Some(keel_domain::staff::PinHash::new("8642", [3; 16]).unwrap());
        data.team[1].role = data.team[0].role;
    });
    write(&mut runtime, &publication::publish(&promoted).unwrap());
    assert!(runtime.take_publications().unwrap());
    assert_eq!(runtime.signed_in(), None);
}
