//! Known answers for location profiles: the demo café's items rung with their modifiers, every
//! way choices are refused, every rule a profile is checked against, and its encoding.

use core::fmt::Write as _;
use core::num::NonZeroU8;

use keel_events::cbor::{Map, Value};
use keel_types::{Currency, Id, Money};

use super::*;
use crate::order::{ChosenModifier, Placement, Prefix};
use crate::refs::Variant;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn choice(modifier: u64) -> Choice {
    Choice::of(id(modifier))
}

fn times(modifier: u64, quantity: u8) -> Choice {
    Choice { modifier: id(modifier), quantity: NonZeroU8::new(quantity).unwrap(), choices: vec![] }
}

/// A chosen modifier: `quantity` of modifier `n`, named `name`, at `cents` each.
fn chosen(n: u64, name: &str, quantity: u8, cents: i64) -> ChosenModifier {
    ChosenModifier {
        modifier: id(n),
        name: Name::new(name).unwrap(),
        prefix: Prefix::Plain,
        quantity: NonZeroU8::new(quantity).unwrap(),
        placement: Placement::Whole,
        unit_price: usd(cents),
        modifiers: vec![],
    }
}

fn ring(variant: u64, choices: &[Choice]) -> Result<Rung, RingError> {
    demo().unwrap().ring(id(variant), choices)
}

#[test]
fn a_variant_rings_with_its_snapshot() {
    let profile = demo().unwrap();
    let rung = profile.ring(id(0x403), &[]).unwrap();
    assert_eq!(rung.item.variant, id::<Variant>(0x403));
    assert_eq!(rung.item.name.as_str(), "Latte (Large)");
    assert_eq!(rung.item.unit_price, usd(525));
    assert_eq!(rung.item.tax_category, id(0x10));
    assert_eq!(rung.item.catalog_version, profile.catalog_version());
    assert!(rung.modifiers.is_empty());
    assert_eq!(ring(0x401, &[]).unwrap().item.name.as_str(), "Espresso");
}

#[test]
fn modifiers_ring_in_the_catalogs_order_whatever_order_they_were_chosen_in() {
    let expected = vec![
        chosen(0x202, "Oat milk", 1, 75),
        ChosenModifier { prefix: Prefix::Extra, ..chosen(0x205, "Shot", 2, 95) },
    ];
    let rung = ring(0x403, &[times(0x205, 2), choice(0x202)]).unwrap();
    assert_eq!(rung.modifiers, expected);
    let rung = ring(0x403, &[choice(0x202), times(0x205, 2)]).unwrap();
    assert_eq!(rung.modifiers, expected);
}

#[test]
fn free_applications_go_to_the_lowest_priced_ties_to_the_first_listed() {
    // Syrup: one free, every syrup at USD 0.60. Vanilla is listed first.
    let rung = ring(0x402, &[choice(0x207), choice(0x206)]).unwrap();
    assert_eq!(
        rung.modifiers,
        vec![chosen(0x206, "Vanilla", 1, 0), chosen(0x207, "Caramel", 1, 60)]
    );
    // Two vanillas and a hazelnut: one vanilla free, so vanilla appears twice, charged first.
    let rung = ring(0x402, &[choice(0x208), times(0x206, 2)]).unwrap();
    assert_eq!(
        rung.modifiers,
        vec![
            chosen(0x206, "Vanilla", 1, 60),
            chosen(0x206, "Vanilla", 1, 0),
            chosen(0x208, "Hazelnut", 1, 60),
        ]
    );
}

#[test]
fn modifiers_chosen_under_a_modifier_ring_under_it() {
    let hash_brown = Choice { choices: vec![choice(0x212), choice(0x211)], ..choice(0x210) };
    let rung = ring(0x408, &[choice(0x20e), hash_brown]).unwrap();
    let mut side = chosen(0x210, "Hash brown", 1, 200);
    // Sauce: one free, both at USD 0.25, ketchup listed first.
    side.modifiers = vec![chosen(0x211, "Ketchup", 1, 0), chosen(0x212, "Hot sauce", 1, 25)];
    assert_eq!(rung.modifiers, vec![chosen(0x20e, "Fried", 1, 0), side]);
}

#[test]
fn choices_that_break_the_catalogs_rules_are_refused() {
    let cases = [
        // No such variant.
        (0x4ff, vec![], RingError::UnknownVariant(id(0x4ff))),
        // Milk isn't offered for a bagel; sauce is offered only under a hash brown.
        (0x407, vec![choice(0x20a), choice(0x202)], RingError::NotOffered(id(0x202))),
        (0x408, vec![choice(0x20d), choice(0x211)], RingError::NotOffered(id(0x211))),
        (0x4ff, vec![choice(0x2ff)], RingError::UnknownVariant(id(0x4ff))),
        (0x402, vec![choice(0x2ff)], RingError::NotOffered(id(0x2ff))),
        // Chosen twice at one level.
        (0x402, vec![choice(0x205), choice(0x205)], RingError::ChosenTwice(id(0x205))),
        // Oat milk applied twice, where each milk applies once.
        (0x402, vec![times(0x202, 2)], RingError::NotRepeatable(id(0x202))),
        // A bagel needs a spread; a sandwich an egg.
        (0x407, vec![], RingError::TooFew { group: id(0x104), min: 1 }),
        (0x408, vec![], RingError::TooFew { group: id(0x105), min: 1 }),
        // One milk at most; four extra shots at most.
        (
            0x402,
            vec![choice(0x202), choice(0x203)],
            RingError::TooMany { group: id(0x101), max: NonZeroU8::new(1).unwrap() },
        ),
        (
            0x402,
            vec![times(0x205, 5)],
            RingError::TooMany { group: id(0x102), max: NonZeroU8::new(4).unwrap() },
        ),
    ];
    for (variant, choices, expected) in cases {
        assert_eq!(ring(variant, &choices), Err(expected), "{variant:x} {choices:?}");
    }
    // A refusal below a modifier is the refusal.
    let fruit = Choice { choices: vec![choice(0x211)], ..choice(0x20f) };
    assert_eq!(ring(0x408, &[choice(0x20d), fruit]), Err(RingError::NotOffered(id(0x211))));
    // Exactly the bounds ring.
    assert!(ring(0x402, &[times(0x205, 4)]).is_ok());
    assert!(ring(0x407, &[choice(0x20c)]).is_ok());
}

#[test]
fn the_demo_round_trips_through_its_encoding() {
    let profile = demo().unwrap();
    let bytes = profile.encode();
    let decoded = Profile::decode(&bytes).unwrap();
    assert_eq!(decoded.data(), profile.data());
    assert_eq!(decoded.encode(), bytes);
    assert_eq!(decoded.catalog_version(), profile.catalog_version());
    assert_eq!(decoded.rules_version(), profile.rules_version());
}

#[test]
fn versions_change_with_what_they_cover_and_nothing_else() {
    let profile = demo().unwrap();
    let mut data = profile.data().clone();
    data.name = Name::new("Keel Café West").unwrap();
    let renamed = Profile::new(data.clone()).unwrap();
    assert_eq!(renamed.catalog_version(), profile.catalog_version());
    assert_eq!(renamed.rules_version(), profile.rules_version());
    data.catalog.items[0].variants[0].price = usd(350);
    let repriced = Profile::new(data.clone()).unwrap();
    assert_ne!(repriced.catalog_version(), profile.catalog_version());
    assert_eq!(repriced.rules_version(), profile.rules_version());
    data.rules.taxes[0].rate = keel_types::Rate::from_basis_points(800);
    let retaxed = Profile::new(data).unwrap();
    assert_ne!(retaxed.rules_version(), profile.rules_version());
}

/// The demo, changed by `change`, and checked.
fn changed(change: impl FnOnce(&mut ProfileData)) -> Result<Profile, ProfileError> {
    let mut data = demo().unwrap().data().clone();
    change(&mut data);
    Profile::new(data)
}

#[test]
fn a_profile_that_breaks_a_rule_is_refused() {
    use ProfileError::{Duplicate, Invalid, Unknown};
    type Change = fn(&mut ProfileData);
    let cases: Vec<(Change, ProfileError)> = vec![
        (|d| d.catalog.items[1].id = d.catalog.items[0].id, Duplicate("item")),
        (
            |d| d.catalog.items[1].variants[0].id = d.catalog.items[0].variants[0].id,
            Duplicate("variant"),
        ),
        (|d| d.catalog.groups[1].id = d.catalog.groups[0].id, Duplicate("modifier group")),
        (
            |d| d.catalog.groups[1].modifiers[0].id = d.catalog.groups[0].modifiers[0].id,
            Duplicate("modifier"),
        ),
        (|d| d.team[1].id = d.team[0].id, Duplicate("team member")),
        (|d| d.catalog.items[0].groups.push(id(0x1ff)), Unknown("modifier group")),
        (|d| d.catalog.groups[0].modifiers[0].groups.push(id(0x1ff)), Unknown("modifier group")),
        (|d| d.menu.pages[0].buttons.push(id(0x4ff)), Unknown("variant")),
        // The sauce group offered under its own ketchup.
        (
            |d| d.catalog.groups[6].modifiers[0].groups.push(id(0x107)),
            Invalid("a modifier group nested within itself"),
        ),
        (
            |d| d.catalog.groups[0].min = 2,
            Invalid("a modifier group whose minimum is over its maximum"),
        ),
        (
            |d| d.catalog.groups[2].free = 4,
            Invalid("a modifier group with more free than its maximum"),
        ),
        (
            |d| {
                d.catalog.groups[4].max = NonZeroU8::new(3).unwrap();
                d.catalog.groups[4].min = 3;
            },
            Invalid("a modifier group whose minimum can't be met"),
        ),
        (|d| d.catalog.groups[0].modifiers.clear(), Invalid("a modifier group with no modifiers")),
        (|d| d.catalog.items[0].variants.clear(), Invalid("an item with no variants")),
        (
            |d| d.catalog.items[1].variants[1].name = None,
            Invalid("an item with several variants, one of them unnamed or named twice"),
        ),
        (
            |d| d.catalog.items[1].variants[1].name = d.catalog.items[1].variants[0].name.clone(),
            Invalid("an item with several variants, one of them unnamed or named twice"),
        ),
        (
            |d| d.catalog.items[1].variants[1].name = Some(Name::new(&"L".repeat(200)).unwrap()),
            Invalid("a variant whose full name is too long"),
        ),
        (|d| d.catalog.items[0].variants[0].price = usd(-1), Invalid("a negative price")),
        (
            |d| d.catalog.groups[0].modifiers[1].price = Money::from_minor(75, Currency::EUR),
            Invalid("a price in another currency than the location's"),
        ),
        (
            |d| d.catalog.items[0].groups.push(id(0x102)),
            Invalid("a modifier group offered twice in one place"),
        ),
        (|d| d.menu.pages[0].buttons.clear(), Invalid("a menu page with no buttons")),
        (|d| d.team.clear(), Invalid("no team members")),
        (
            |d| d.address = vec![Name::new("A line").unwrap(); 5],
            Invalid("more than four address lines"),
        ),
        (
            |d| d.currency = Currency::from_code("HRK").unwrap(),
            Invalid("a currency that isn't circulating"),
        ),
        (
            |d| d.business_day.time_zone = "america/new_york".to_owned(),
            Invalid("a time zone by another name than its own"),
        ),
        (
            |d| d.business_day.time_zone = "Mars/Olympus".to_owned(),
            Invalid("a business day policy that doesn't exist"),
        ),
        (|d| d.business_day.cutoff_hour = 24, Invalid("a business day policy that doesn't exist")),
        (|d| d.rules.taxes.push(d.rules.taxes[0].clone()), Duplicate("tax")),
        (
            |d| d.rules.taxes[0].rate = keel_types::Rate::from_basis_points(-1),
            Invalid("a negative tax rate"),
        ),
        (
            |d| d.rules.taxes[0].categories.clear(),
            Invalid("a tax with no tax categories, or one twice"),
        ),
        (|d| d.rules.taxes[0].name = String::new(), Invalid("a tax without a valid name")),
    ];
    for (index, (change, expected)) in cases.into_iter().enumerate() {
        assert_eq!(changed(change).map(|_| ()), Err(expected), "case {index}");
    }
}

#[test]
fn modifier_groups_nest_four_deep_and_no_deeper() {
    // Toasting (depth 1) offers milk under its modifier, milk offers syrup, syrup offers
    // spread: four deep.
    let nest = |d: &mut ProfileData| {
        d.catalog.groups[7].modifiers[0].groups.push(id(0x101));
        d.catalog.groups[0].modifiers[0].groups.push(id(0x103));
        d.catalog.groups[2].modifiers[0].groups.push(id(0x104));
    };
    assert!(changed(nest).is_ok());
    let deeper = |d: &mut ProfileData| {
        nest(d);
        d.catalog.groups[3].modifiers[0].groups.push(id(0x105));
    };
    assert_eq!(
        changed(deeper).map(|_| ()),
        Err(ProfileError::Invalid("modifier groups nested too deep"))
    );
}

/// The demo's encoding, as a CBOR value, changed by `change`, then decoded.
fn decode_changed(change: impl FnOnce(&mut Vec<(Value, Value)>)) -> Result<Profile, ProfileError> {
    let bytes = demo().unwrap().encode();
    let value = cbor::decode(&bytes).unwrap();
    let mut entries: Vec<(Value, Value)> =
        value.as_map().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    change(&mut entries);
    Profile::decode(&Value::Map(Map::from_entries(entries).unwrap()).encode())
}

/// The value of the entry with key `key`.
fn entry(entries: &mut [(Value, Value)], key: u64) -> &mut Value {
    &mut entries.iter_mut().find(|(k, _)| *k == Value::Unsigned(key)).unwrap().1
}

#[test]
fn encodings_other_than_the_profiles_own_are_refused() {
    assert_eq!(
        decode_changed(|e| *entry(e, 1) = Value::Unsigned(2)).map(|_| ()),
        Err(ProfileError::Format(2))
    );
    assert_eq!(
        decode_changed(|e| e.push((Value::Unsigned(12), Value::Null))).map(|_| ()),
        Err(ProfileError::Payload(PayloadError::UnknownField("12".to_owned())))
    );
    assert_eq!(
        decode_changed(|e| e.retain(|(k, _)| *k != Value::Unsigned(11))).map(|_| ()),
        Err(ProfileError::Payload(PayloadError::Missing("team")))
    );
    // A catalog with a field it doesn't define.
    let refused = decode_changed(|e| {
        let catalog = entry(e, 9);
        let mut entries: Vec<_> =
            catalog.as_map().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.push((Value::Unsigned(3), Value::Null));
        *catalog = Value::Map(Map::from_entries(entries).unwrap());
    });
    assert_eq!(refused.map(|_| ()), Err(ProfileError::Payload(PayloadError::Invalid("catalog"))));
    // A tax rate written with a trailing zero.
    let refused = decode_changed(|e| {
        let rules = entry(e, 8);
        let mut entries: Vec<_> =
            rules.as_map().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let taxes = &mut entries.iter_mut().find(|(k, _)| *k == Value::Unsigned(3)).unwrap().1;
        let tax = taxes.as_array().unwrap()[0].as_map().unwrap();
        let mut tax: Vec<_> = tax.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        tax.iter_mut().find(|(k, _)| *k == Value::Unsigned(3)).unwrap().1 = Value::from("0.088750");
        *taxes = Value::Array(vec![Value::Map(Map::from_entries(tax).unwrap())]);
        *rules = Value::Map(Map::from_entries(entries).unwrap());
    });
    assert_eq!(
        refused.map(|_| ()),
        Err(ProfileError::Payload(PayloadError::Invalid("pricing rules")))
    );
    // Bytes after the profile.
    let mut bytes = demo().unwrap().encode();
    bytes.push(0);
    assert!(matches!(Profile::decode(&bytes), Err(ProfileError::Cbor(_))));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Pinned from `tests/golden/profile.py`, which builds the demo profile with Python's `cbor2`,
/// independently of this codec.
#[test]
fn the_demo_is_pinned_byte_for_byte() {
    let profile = demo().unwrap();
    let bytes = profile.encode();
    assert_eq!(bytes.len(), 2367);
    assert_eq!(
        hex(&sha256(&bytes)),
        "d7b4ebe3f1d7e6e717702e613275048a268599fcf996f6ac7279e865c0fd1be7"
    );
    assert_eq!(
        hex(profile.catalog_version().as_bytes()),
        "92b47a4d0bf077d94f33b6c3cc7c712c7db7e89132d6a583b045121b425eea76"
    );
    assert_eq!(
        hex(profile.rules_version().as_bytes()),
        "80b9f0662b46e15c2ac965c8bf382e4e0f0f49656ba53efa7ca4d57fe86e22cf"
    );
}

#[test]
fn a_taxs_dining_rule_round_trips() {
    for dining in [keel_pricing::Dining::OnPremises, keel_pricing::Dining::ToGo] {
        let profile = changed(|d| d.rules.taxes[0].dining = Some(dining)).unwrap();
        let decoded = Profile::decode(&profile.encode()).unwrap();
        assert_eq!(decoded.data().rules.taxes[0].dining, Some(dining));
        assert_ne!(profile.rules_version(), demo().unwrap().rules_version());
    }
}
