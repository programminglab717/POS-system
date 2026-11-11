//! Known answers for staff: PINs hashed as the reference implementation hashes them, a role's
//! limits at their bounds, the rules a role keeps, who may approve what, and the approval's
//! pinned payload.

use keel_events::cbor::Map;
use keel_events::envelope::SchemaName;
use keel_types::Currency;

use super::*;
use crate::profile::{Profile, ProfileError, demo};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// Known answers from `argon2-cffi`, the Argon2 reference implementation's bindings, with
/// ADR-0029's parameters:
/// `hash_secret_raw(pin, salt, time_cost=2, memory_cost=19456, parallelism=1, hash_len=32,
/// type=Type.ID, version=19)`.
#[test]
fn a_pin_hashes_as_the_reference_implementation_hashes_it() {
    let counting: [u8; 16] = core::array::from_fn(|n| u8::try_from(n).unwrap());
    let cases = [
        ("1234", counting, "fd2f78617f352bccb028644658898796b04bbedaa07d06ee028bbf0fef35f068"),
        (
            "1234",
            *b"keel-demo-alex-1",
            "53cf2a9809dd196b7db47604539ae28b634d54ae60e02b65b96056e9bcbb264a",
        ),
        (
            "2468",
            *b"keel-demo-sam--2",
            "0d9cf591ce3b8333c77ceeb02d1c098bf8189104954b4bbb6777bf415a3cba49",
        ),
    ];
    for (pin, salt, hash) in cases {
        let hashed = PinHash::new(pin, salt).unwrap();
        assert_eq!(hashed.hash().as_slice(), bytes(hash).as_slice(), "{pin}");
        assert_eq!(hashed.salt(), &salt);
        assert!(hashed.verifies(pin));
    }
}

#[test]
fn a_pin_is_4_to_8_digits_and_only_its_own_verifies() {
    let salt = [7; 16];
    for good in ["0000", "1234", "12345678"] {
        assert!(PinHash::new(good, salt).is_ok(), "{good}");
    }
    for bad in ["", "123", "123456789", "12a4", "12 34", " 1234", "١٢٣٤", "1234\n"] {
        assert_eq!(PinHash::new(bad, salt), Err(PinError::Malformed), "{bad:?}");
    }
    let pin = PinHash::new("1234", salt).unwrap();
    for wrong in ["1235", "4321", "12340", "123", "", "1234 "] {
        assert!(!pin.verifies(wrong), "{wrong:?}");
    }
    // Another salt is another hash.
    assert_ne!(PinHash::new("1234", [8; 16]).unwrap(), pin);
    assert_eq!(format!("{pin:?}"), "PinHash(..)", "nothing of it in a log line");
}

fn discount(most_amount: Option<u64>, most_percent: Option<u32>) -> Role {
    Role {
        id: id(0x601),
        name: Name::new("Cashier").unwrap(),
        permissions: vec![Permission { action: Action::DiscountApply, most_amount, most_percent }],
    }
}

fn asks(amount: Option<i64>, percent: Option<u32>) -> Request {
    Request { action: Action::DiscountApply, amount: amount.map(usd), percent }
}

#[test]
fn a_role_permits_up_to_its_limits_and_no_further() {
    let role = discount(Some(500), Some(1_000));
    assert!(role.permits(&asks(Some(500), Some(1_000))), "at both limits");
    assert!(role.permits(&asks(Some(0), None)));
    assert!(!role.permits(&asks(Some(501), None)), "a cent over");
    assert!(!role.permits(&asks(None, Some(1_001))), "a basis point over");
    // An amount counts by its size: a refund of -$6 is $6.
    assert!(!role.permits(&asks(Some(-501), None)));
    assert!(role.permits(&asks(Some(-500), None)));
    // Unlimited.
    let unlimited = discount(None, None);
    assert!(unlimited.permits(&asks(Some(i64::MAX), Some(MAX_PERCENT))));
    assert!(unlimited.permits(&asks(Some(i64::MIN), None)));
    // An action the role doesn't list.
    assert!(!unlimited.permits(&Request::of(Action::OrderVoid)));
}

#[test]
fn a_role_lists_each_action_once_with_the_limits_it_has() {
    let may = |action| Permission { action, most_amount: None, most_percent: None };
    let role = |permissions| Role { id: id(0x601), name: Name::new("R").unwrap(), permissions };
    assert_eq!(role(Action::ALL.iter().copied().map(may).collect()).check(), Ok(()));
    assert_eq!(
        role(vec![may(Action::LineRemove), may(Action::LineRemove)]).check(),
        Err("a role that lists an action twice")
    );
    let limited = |action, most_amount, most_percent| {
        role(vec![Permission { action, most_amount, most_percent }]).check()
    };
    assert_eq!(
        limited(Action::OrderVoid, Some(1), None),
        Err("an amount limit on an action that has none")
    );
    assert_eq!(
        limited(Action::RefundIssue, None, Some(1)),
        Err("a percentage limit on an action that has none")
    );
    assert_eq!(limited(Action::DiscountApply, None, Some(MAX_PERCENT)), Ok(()));
    assert_eq!(
        limited(Action::DiscountApply, None, Some(MAX_PERCENT + 1)),
        Err("a percentage limit over 100%")
    );
    assert_eq!(limited(Action::RefundIssue, Some(i64::MAX.unsigned_abs()), None), Ok(()));
    assert_eq!(
        limited(Action::RefundIssue, Some(i64::MAX.unsigned_abs() + 1), None),
        Err("an amount limit beyond any amount")
    );
}

#[test]
fn actions_keep_their_codes_and_names() {
    let pinned = [
        (Action::OrderVoid, 0, "order.void"),
        (Action::LineRemove, 1, "line.remove"),
        (Action::DiscountApply, 2, "discount.apply"),
        (Action::PriceOverride, 3, "price.override"),
        (Action::RefundIssue, 4, "refund.issue"),
        (Action::DrawerNoSale, 5, "drawer.no_sale"),
        (Action::DrawerVariance, 6, "drawer.variance"),
        (Action::DrawerPaidOut, 7, "drawer.paid_out"),
        (Action::ReceiptReprint, 8, "receipt.reprint"),
        (Action::ItemAvailability, 9, "item.availability"),
    ];
    assert_eq!(pinned.len(), Action::ALL.len());
    for (action, code, name) in pinned {
        assert_eq!((action.code(), action.name()), (code, name));
        assert_eq!(Action::named(name), Some(action));
        assert_eq!(Action::from_code(code), Some(action));
    }
    assert_eq!(Action::named("order.refund"), None);
    assert_eq!(Action::from_code(10), None);
}

#[test]
fn a_cashier_asks_a_manager_who_never_approves_their_own_request() {
    let profile = demo().unwrap();
    let (alex, sam) = (id(0x501), id(0x502));
    let void = Request::of(Action::OrderVoid);
    assert!(profile.permits(alex, &void));
    assert!(!profile.permits(sam, &void), "a cashier doesn't void alone");
    assert!(profile.may_approve(alex, sam, &void));
    assert!(!profile.may_approve(sam, alex, &void), "a cashier can't approve it");
    assert!(!profile.may_approve(alex, alex, &void), "nor a manager their own");
    // Within the cashier's limits, a discount needs no one; beyond them, a manager.
    let small =
        Request { action: Action::DiscountApply, amount: Some(usd(500)), percent: Some(1_000) };
    let large = Request { amount: Some(usd(501)), ..small };
    assert!(profile.permits(sam, &small));
    assert!(!profile.permits(sam, &large));
    assert!(profile.may_approve(alex, sam, &large));
    // Someone the team doesn't have may do nothing, and approve nothing.
    let stranger = id(0x5ff);
    assert!(!profile.permits(stranger, &small));
    assert!(!profile.may_approve(stranger, sam, &large));
    // PINs.
    assert!(profile.verifies_pin(alex, "1234"));
    assert!(profile.verifies_pin(sam, "2468"));
    assert!(!profile.verifies_pin(sam, "1234"));
    assert!(!profile.verifies_pin(sam, ""));
    assert!(!profile.verifies_pin(stranger, "1234"));
}

#[test]
fn a_profile_without_roles_lets_every_member_do_everything_with_no_pin() {
    let mut data = demo().unwrap().data().clone();
    data.roles.clear();
    for member in &mut data.team {
        member.role = None;
        member.pin = None;
    }
    let profile = Profile::new(data).unwrap();
    let (alex, sam) = (id(0x501), id(0x502));
    for action in Action::ALL {
        assert!(profile.permits(sam, &Request::of(*action)), "{}", action.name());
    }
    assert!(profile.verifies_pin(sam, ""));
    assert!(!profile.verifies_pin(sam, "2468"), "no PIN, so none is theirs");
    assert!(!profile.permits(id(0x5ff), &Request::of(Action::LineRemove)));
    assert!(profile.may_approve(alex, sam, &Request::of(Action::OrderVoid)));
    // It is written as format 1, and reads back the same.
    let bytes = profile.encode();
    assert_eq!(bytes.get(..3), Some([0xaa, 0x01, 0x01].as_slice()), "a map of 10 keys, format 1");
    assert_eq!(Profile::decode(&bytes).unwrap().encode(), bytes);
}

#[test]
fn a_team_with_roles_has_a_role_and_a_pin_each() {
    let changed = |change: &dyn Fn(&mut crate::profile::ProfileData)| {
        let mut data = demo().unwrap().data().clone();
        change(&mut data);
        Profile::new(data).map(|_| ())
    };
    assert_eq!(
        changed(&|data| data.team[1].role = None),
        Err(ProfileError::Invalid("a team member with no role"))
    );
    assert_eq!(
        changed(&|data| data.team[1].pin = None),
        Err(ProfileError::Invalid("a team member with no PIN"))
    );
    assert_eq!(
        changed(&|data| data.team[1].role = Some(id(0x6ff))),
        Err(ProfileError::Unknown("role"))
    );
    assert_eq!(
        changed(&|data| data.roles[1].id = data.roles[0].id),
        Err(ProfileError::Duplicate("roles"))
    );
    assert_eq!(
        changed(&|data| {
            let first = data.roles[1].permissions[0];
            data.roles[1].permissions.push(first);
        }),
        Err(ProfileError::Invalid("a role that lists an action twice"))
    );
    // Without roles, a member can't have one, or a PIN.
    assert_eq!(
        changed(&|data| {
            data.roles.clear();
            data.team[0].role = None;
            data.team[1].role = None;
            data.team[1].pin = None;
        }),
        Err(ProfileError::Invalid("a role or PIN in a profile without roles"))
    );
}

/// Pinned payloads, each as Python's `cbor2` encodes the documented key table, canonically.
#[test]
fn the_approval_payload_is_pinned() {
    let pinned = [
        (
            Granted::new(
                id(0x501),
                Request {
                    action: Action::DiscountApply,
                    amount: Some(usd(500)),
                    percent: Some(1_500),
                },
                Some(id(0x901)),
            )
            .unwrap(),
            "a501500192f0c1000070008000000000000501020203821901f463555344041905dc05500192f0c1000070008000000000000901",
        ),
        (
            Granted::new(id(0x501), Request::of(Action::OrderVoid), None).unwrap(),
            "a201500192f0c10000700080000000000005010200",
        ),
    ];
    for (granted, hex) in pinned {
        let event = ApprovalEvent::Granted(granted);
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), "approval.granted");
        assert_eq!(schema.version.get(), 1);
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice());
        assert_eq!(ApprovalEvent::decode(&schema, &payload), Ok(event));
    }
}

#[test]
fn the_registry_lists_the_schema_once_and_refuses_what_breaks_a_rule() {
    assert_eq!(ApprovalEvent::SCHEMAS, &[GRANTED]);
    assert_eq!(ApprovalEvent::STREAM, "approval");
    let schema = |name: &str, version: u32| SchemaRef {
        name: SchemaName::new(name).unwrap(),
        version: version.try_into().unwrap(),
    };
    let map = |entries: Vec<(u64, Value)>| {
        Value::Map(
            Map::from_entries(entries.into_iter().map(|(k, v)| (Value::Unsigned(k), v))).unwrap(),
        )
    };
    let approver = Field::to_value(&id::<TeamMember>(0x501));
    let ok = map(vec![(1, approver.clone()), (2, Value::Unsigned(0))]);
    assert!(ApprovalEvent::from_value(&schema("approval.granted", 1), &ok).is_ok());
    for (name, version) in [("approval.granted", 2), ("approval.denied", 1)] {
        assert_eq!(
            ApprovalEvent::from_value(&schema(name, version), &ok),
            Err(DecodeError::UnknownSchema),
            "{name} v{version}"
        );
    }
    let usd5 = Field::to_value(&usd(500));
    let refused = [
        // An amount on an action without one.
        map(vec![(1, approver.clone()), (2, Value::Unsigned(0)), (3, usd5.clone())]),
        // A percentage on an action without one, and one over 100%.
        map(vec![(1, approver.clone()), (2, Value::Unsigned(4)), (4, Value::Unsigned(10))]),
        map(vec![(1, approver.clone()), (2, Value::Unsigned(2)), (4, Value::Unsigned(10_001))]),
        // An action this kernel doesn't know, and an unknown key.
        map(vec![(1, approver.clone()), (2, Value::Unsigned(10))]),
        map(vec![(1, approver.clone()), (2, Value::Unsigned(0)), (6, Value::Unsigned(0))]),
        // No approver.
        map(vec![(2, Value::Unsigned(0))]),
    ];
    for payload in refused {
        assert!(
            matches!(
                ApprovalEvent::from_value(&schema("approval.granted", 1), &payload),
                Err(DecodeError::Malformed(_))
            ),
            "{payload:?}"
        );
    }
    // A refund's amount is its own.
    let refund = map(vec![(1, approver), (2, Value::Unsigned(4)), (3, usd5)]);
    assert!(ApprovalEvent::from_value(&schema("approval.granted", 1), &refund).is_ok());
}
