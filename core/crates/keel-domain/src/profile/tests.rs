//! Known answers for location profiles: the demo café's items rung with their modifiers, every
//! way choices are refused, every rule a profile is checked against, and its encoding.

use core::fmt::Write as _;
use core::num::{NonZeroU8, NonZeroU32};

use keel_events::cbor::{Map, Value};
use keel_types::{Currency, Id, Money};

use super::*;
use crate::order::{ChosenModifier, Placement, Prefix};
use crate::refs::{ModifierGroup, Variant};

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
        (|d| d.catalog.items[1].id = d.catalog.items[0].id, Duplicate("items")),
        (
            |d| d.catalog.items[1].variants[0].id = d.catalog.items[0].variants[0].id,
            Duplicate("variants"),
        ),
        (|d| d.catalog.groups[1].id = d.catalog.groups[0].id, Duplicate("modifier groups")),
        (
            |d| d.catalog.groups.swap(3, 4),
            Invalid("modifier groups not in the ascending order of their identifiers"),
        ),
        (
            |d| d.catalog.groups[1].modifiers[0].id = d.catalog.groups[0].modifiers[0].id,
            Duplicate("modifiers"),
        ),
        (|d| d.team[1].id = d.team[0].id, Duplicate("team members")),
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
            Invalid("a time zone not named as the database names it"),
        ),
        (
            |d| d.business_day.time_zone = "Mars/Olympus".to_owned(),
            Invalid("a business day policy that doesn't exist"),
        ),
        (|d| d.business_day.cutoff_hour = 24, Invalid("a business day policy that doesn't exist")),
        (|d| d.rules.taxes.push(d.rules.taxes[0].clone()), Duplicate("taxes")),
        (
            |d| d.rules.taxes[0].rate = keel_types::Rate::from_basis_points(-1),
            Invalid("a negative tax rate"),
        ),
        (|d| d.rules.taxes[0].categories.clear(), Invalid("a tax with no tax categories")),
        (|d| d.rules.taxes[0].name = String::new(), Invalid("a tax without a valid name")),
        (
            |d| {
                let mode = keel_types::RoundingMode::HalfAwayFromZero;
                d.cash_rounding = Some(RoundingRule::new(mode, NonZeroU32::MIN));
            },
            Invalid("cash rounded to the minor unit, which rounds none"),
        ),
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
        decode_changed(|e| *entry(e, 1) = Value::Unsigned(3)).map(|_| ()),
        Err(ProfileError::Format(3))
    );
    assert_eq!(
        decode_changed(|e| e.push((Value::Unsigned(13), Value::Null))).map(|_| ()),
        Err(ProfileError::Payload(PayloadError::UnknownField("13".to_owned())))
    );
    // Format 1 has no roles: their key is unknown to it. Format 2 has them.
    assert_eq!(
        decode_changed(|e| *entry(e, 1) = Value::Unsigned(1)).map(|_| ()),
        Err(ProfileError::Payload(PayloadError::UnknownField("12".to_owned())))
    );
    assert_eq!(
        decode_changed(|e| e.retain(|(k, _)| *k != Value::Unsigned(12))).map(|_| ()),
        Err(ProfileError::Payload(PayloadError::Missing("roles")))
    );
    assert_eq!(
        decode_changed(|e| *entry(e, 12) = Value::Array(Vec::new())).map(|_| ()),
        Err(ProfileError::Invalid("a format-2 profile with no roles"))
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
/// independently of this codec, and its PINs' hashes with `argon2-cffi`, the reference
/// implementation's bindings. In format 2 since ADR-0029: its catalog's and rules' versions are
/// as they were in format 1.
#[test]
fn the_demo_is_pinned_byte_for_byte() {
    let profile = demo().unwrap();
    let bytes = profile.encode();
    assert_eq!(bytes.len(), 2632);
    assert_eq!(
        hex(&sha256(&bytes)),
        "f18e3f2d63758eee2275263b0659631f7c14f63c3cf7c5eb521b202e888cf202"
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

#[test]
fn free_applications_skip_modifiers_that_cost_nothing() {
    // Vanilla made free of charge: the syrup group's one free application goes to the caramel,
    // the cheapest that costs something, rather than being spent on the vanilla.
    let profile = changed(|d| d.catalog.groups[2].modifiers[0].price = usd(0)).unwrap();
    let rung = profile.ring(id(0x402), &[choice(0x207), choice(0x206)]).unwrap();
    assert_eq!(
        rung.modifiers,
        vec![chosen(0x206, "Vanilla", 1, 0), chosen(0x207, "Caramel", 1, 0)]
    );
    // Two vanillas and a caramel: still the caramel.
    let rung = profile.ring(id(0x402), &[choice(0x207), times(0x206, 2)]).unwrap();
    assert_eq!(
        rung.modifiers,
        vec![chosen(0x206, "Vanilla", 2, 0), chosen(0x207, "Caramel", 1, 0)]
    );
    // With nothing chosen that costs something, nothing is free, and nothing appears twice.
    let rung = profile.ring(id(0x402), &[times(0x206, 2)]).unwrap();
    assert_eq!(rung.modifiers, vec![chosen(0x206, "Vanilla", 2, 0)]);
}

#[test]
fn a_line_records_its_items_tax_category() {
    let rung = ring(0x40a, &[]).unwrap();
    assert_eq!(rung.item.tax_category, id(0x11));
    assert_eq!(rung.item.name.as_str(), "Coffee beans, 12 oz");
}

#[test]
fn placeholder_time_zones_are_refused() {
    let zone =
        |name: &'static str| move |d: &mut ProfileData| d.business_day.time_zone = name.into();
    assert_eq!(
        changed(zone("Factory")).map(|_| ()),
        Err(ProfileError::Invalid("a placeholder time zone"))
    );
    for unknown in ["Etc/Unknown", "etc/unknown"] {
        assert_eq!(
            changed(zone(unknown)).map(|_| ()),
            Err(ProfileError::Invalid("a business day policy that doesn't exist")),
            "{unknown}"
        );
    }
    // Zones of a fixed offset, and the database's other names for zones, are places' own.
    for name in ["UTC", "Etc/GMT+5", "US/Eastern", "America/Toronto"] {
        assert!(changed(zone(name)).is_ok(), "{name}");
    }
}

/// `n` distinct names.
fn names(n: usize, prefix: &str) -> Vec<Name> {
    (0..n).map(|i| Name::new(&format!("{prefix} {i}")).unwrap()).collect()
}

/// `n` identifiers, from `first` up.
fn ids<T>(first: u64, n: usize) -> Vec<Id<T>> {
    (0..u64::try_from(n).unwrap()).map(|i| id(first.checked_add(i).unwrap())).collect()
}

#[test]
#[expect(clippy::too_many_lines, reason = "a case for every limit")]
fn every_count_has_a_limit() {
    // Each case: a change that reaches the limit, which is accepted, and one past it, refused.
    type Change = Box<dyn Fn(&mut ProfileData, usize)>;
    let cases: Vec<(usize, Change, &str)> = vec![
        (
            MAX_VARIANTS,
            Box::new(|d, n| {
                // The latte's small and large, and more sizes.
                let item = &mut d.catalog.items[1];
                let template = item.variants[0].clone();
                let more = n - item.variants.len();
                item.variants.extend(
                    ids(0x10_0000, more).into_iter().zip(names(more, "Size")).map(|(id, name)| {
                        CatalogVariant { id, name: Some(name), ..template.clone() }
                    }),
                );
            }),
            "more variants of an item than it holds",
        ),
        (
            MAX_PER_GROUP,
            Box::new(|d, n| {
                let group = &mut d.catalog.groups[0];
                let template = group.modifiers[0].clone();
                group.modifiers = ids(0x10_0000, n)
                    .into_iter()
                    .map(|id| CatalogModifier { id, ..template.clone() })
                    .collect();
            }),
            "more modifiers in a group than it holds",
        ),
        (
            MAX_PER_GROUP,
            Box::new(|d, n| {
                let template = d.catalog.groups[7].clone();
                let extra: Vec<Id<ModifierGroup>> = ids(0x10_0000, n);
                for (at, group) in extra.iter().enumerate() {
                    let modifier = id(0x20_0000 + u64::try_from(at).unwrap());
                    let mut copy = template.clone();
                    copy.id = *group;
                    copy.modifiers[0].id = modifier;
                    d.catalog.groups.push(copy);
                }
                d.catalog.items[5].groups = extra;
            }),
            "more modifier groups offered than allowed",
        ),
        (
            MAX_PER_GROUP,
            Box::new(|d, n| {
                let template = d.catalog.groups[7].clone();
                let extra: Vec<Id<ModifierGroup>> = ids(0x10_0000, n);
                for (at, group) in extra.iter().enumerate() {
                    let modifier = id(0x20_0000 + u64::try_from(at).unwrap());
                    let mut copy = template.clone();
                    copy.id = *group;
                    copy.modifiers[0].id = modifier;
                    d.catalog.groups.push(copy);
                }
                d.catalog.groups[0].modifiers[0].groups = extra;
            }),
            "more modifier groups offered than allowed",
        ),
        (
            MAX_PAGES,
            Box::new(|d, n| d.menu.pages = vec![d.menu.pages[0].clone(); n]),
            "more menu pages than allowed",
        ),
        (
            MAX_BUTTONS,
            Box::new(|d, n| d.menu.pages[0].buttons = vec![d.menu.pages[0].buttons[0]; n]),
            "more buttons on a page than allowed",
        ),
        (
            MAX_TEAM,
            Box::new(|d, n| {
                let template = d.team[0].clone();
                d.team = ids(0x10_0000, n)
                    .into_iter()
                    .map(|id| Member { id, ..template.clone() })
                    .collect();
            }),
            "more team members than allowed",
        ),
        (
            MAX_TAXES,
            Box::new(|d, n| {
                let template = d.rules.taxes[0].clone();
                d.rules.taxes = ids(0x10_0000, n)
                    .into_iter()
                    .map(|id| keel_pricing::Tax { id, ..template.clone() })
                    .collect();
            }),
            "more taxes than allowed",
        ),
        (
            MAX_CATEGORIES,
            Box::new(|d, n| d.rules.taxes[0].categories = ids(0x10_0000, n)),
            "a tax with more tax categories than allowed",
        ),
        (
            MAX_ADDRESS,
            Box::new(|d, n| d.address = names(n, "Line")),
            "more than four address lines",
        ),
        (
            MAX_ROLES,
            Box::new(|d, n| {
                let template = d.roles[0].clone();
                d.roles = ids(0x10_0000, n)
                    .into_iter()
                    .map(|id| Role { id, ..template.clone() })
                    .collect();
                for member in &mut d.team {
                    member.role = Some(d.roles[0].id);
                }
            }),
            "more roles than allowed",
        ),
    ];
    for (index, (limit, change, refusal)) in cases.into_iter().enumerate() {
        if let Err(error) = changed(|d| change(d, limit)) {
            panic!("case {index} at its limit: {error}");
        }
        assert_eq!(
            changed(|d| change(d, limit + 1)).map(|_| ()),
            Err(ProfileError::Invalid(refusal)),
            "case {index} past its limit"
        );
    }
}

#[test]
fn a_catalog_past_its_limits_is_refused() {
    // Past each limit, by copies: the counts are checked before anything else in the catalog.
    let over =
        |change: fn(&mut Catalog)| changed(|d| change(&mut d.catalog)).map(|_| ()).unwrap_err();
    assert_eq!(
        over(|c| c.items = vec![c.items[0].clone(); MAX_ITEMS + 1]),
        ProfileError::Invalid("more items than a catalog holds")
    );
    assert_eq!(
        over(|c| c.groups = vec![c.groups[7].clone(); MAX_GROUPS + 1]),
        ProfileError::Invalid("more modifier groups than a catalog holds")
    );
    assert_eq!(
        over(|c| c.items[0].variants = vec![c.items[0].variants[0].clone(); MAX_ALL_VARIANTS + 1]),
        ProfileError::Invalid("more variants than a catalog holds")
    );
    assert_eq!(
        over(|c| {
            let modifiers = vec![c.groups[7].modifiers[0].clone(); MAX_PER_GROUP];
            let groups = MAX_ALL_MODIFIERS / MAX_PER_GROUP + 1;
            c.groups = vec![CatalogGroup { modifiers, ..c.groups[7].clone() }; groups];
        }),
        ProfileError::Invalid("more modifiers than a catalog holds")
    );
}

/// The largest catalog a profile holds: [`MAX_ITEMS`] items with [`MAX_ALL_VARIANTS`] variants
/// in all, and [`MAX_GROUPS`] groups with [`MAX_ALL_MODIFIERS`] modifiers, checked, encoded and
/// decoded.
#[test]
#[ignore = "an exhaustive sweep: CI runs it"]
fn the_largest_catalog_is_accepted() {
    let variants_each = MAX_ALL_VARIANTS / MAX_ITEMS;
    let modifiers_each = MAX_ALL_MODIFIERS / MAX_GROUPS;
    let profile = changed(|d| {
        let item = d.catalog.items[1].clone();
        let group = d.catalog.groups[2].clone();
        let sizes = names(variants_each, "Size");
        let mut next = 0x100_0000_u64;
        let mut fresh = || {
            next += 1;
            next
        };
        d.catalog.groups = (0..MAX_GROUPS)
            .map(|_| CatalogGroup {
                id: id(fresh()),
                modifiers: (0..modifiers_each)
                    .map(|_| CatalogModifier { id: id(fresh()), ..group.modifiers[0].clone() })
                    .collect(),
                ..group.clone()
            })
            .collect();
        let offered = d.catalog.groups[0].id;
        d.catalog.items = (0..MAX_ITEMS)
            .map(|_| CatalogItem {
                id: id(fresh()),
                variants: sizes
                    .iter()
                    .map(|size| CatalogVariant {
                        id: id(fresh()),
                        name: Some(size.clone()),
                        price: usd(450),
                    })
                    .collect(),
                groups: vec![offered],
                ..item.clone()
            })
            .collect();
        d.menu.pages[0].buttons = vec![d.catalog.items[0].variants[0].id];
        d.menu.pages.truncate(1);
    })
    .unwrap();
    let decoded = Profile::decode(&profile.encode()).unwrap();
    assert_eq!(decoded.catalog_version(), profile.catalog_version());
    assert_eq!(decoded.data().catalog.items.len(), MAX_ITEMS);
}

/// A bakery in Toronto, which writes every optional field the demo leaves out: cash rounding, a
/// tax by dining, a group with a minimum and free applications, every prefix, and modifiers
/// chosen under a modifier in a group that repeats. `tests/golden/profile.py` builds it too.
#[expect(clippy::too_many_lines, reason = "the whole profile, field by field")]
fn golden_bakery() -> Profile {
    use Prefix::{Extra, Light, No, Plain, Side, Sub};
    use keel_pricing::{Dining, Rules, Tax, TaxRounding, TaxScope};
    use keel_types::{Rate, RoundingMode, RoundingRule};
    let cad = |minor: i64| Money::from_minor(minor, Currency::CAD);
    let name = |text: &str| Name::new(text).unwrap();
    let modifier =
        |n: u64, title: &str, prefix: Prefix, minor: i64, groups: &[u64]| CatalogModifier {
            id: id(n),
            name: name(title),
            prefix,
            price: cad(minor),
            groups: groups.iter().map(|&g| id(g)).collect(),
        };
    let group = |n: u64, title: &str, (min, max, free): (u8, u8, u8), repeat: bool, modifiers| {
        CatalogGroup {
            id: id(n),
            name: name(title),
            min,
            max: NonZeroU8::new(max).unwrap(),
            free,
            repeat,
            modifiers,
        }
    };
    let item = |n: u64,
                title: &str,
                category: u64,
                variants: &[(u64, Option<&str>, i64)],
                groups: &[u64]| {
        CatalogItem {
            id: id(n),
            name: name(title),
            tax_category: id(category),
            variants: variants
                .iter()
                .map(|&(v, title, minor)| CatalogVariant {
                    id: id(v),
                    name: title.map(name),
                    price: cad(minor),
                })
                .collect(),
            groups: groups.iter().map(|&g| id(g)).collect(),
        }
    };
    let tax = |n: u64, title: &str, rate: &str, categories: &[u64], dining| Tax {
        id: id(n),
        name: title.to_owned(),
        rate: Rate::from_fraction(rate.parse().unwrap()),
        categories: categories.iter().map(|&c| id(c)).collect(),
        dining,
    };
    let page = |title: &str, buttons: &[u64]| Page {
        name: name(title),
        buttons: buttons.iter().map(|&b| id(b)).collect(),
    };
    let groups = vec![
        group(0x111, "Warming", (0, 1, 0), false, vec![modifier(0x221, "Warmed", Plain, 0, &[])]),
        group(
            0x112,
            "Filling",
            (1, 2, 1),
            false,
            vec![
                modifier(0x222, "Almond paste", Plain, 150, &[]),
                modifier(0x223, "Chocolate", Plain, 100, &[]),
                modifier(0x224, "Filling", No, 0, &[]),
            ],
        ),
        group(
            0x113,
            "Toppings",
            (0, 5, 2),
            true,
            vec![
                modifier(0x225, "Icing sugar", Extra, 0, &[0x114]),
                modifier(0x226, "Crème fraîche", Side, 175, &[]),
                modifier(0x227, "Berries", Sub, 225, &[]),
            ],
        ),
        group(
            0x114,
            "Amount",
            (0, 1, 0),
            false,
            vec![
                modifier(0x228, "Dusting", Light, 0, &[]),
                modifier(0x229, "Snowfall", Extra, 25, &[]),
            ],
        ),
    ];
    let items = vec![
        item(0x311, "Croissant", 0x14, &[(0x411, None, 395)], &[0x111, 0x112]),
        item(
            0x312,
            "Café au lait",
            0x13,
            &[(0x412, Some("Bol"), 550), (0x413, Some("Tasse"), 425)],
            &[],
        ),
        item(
            0x313,
            "Tarte aux fruits",
            0x14,
            &[(0x414, Some("Slice"), 650), (0x415, Some("Whole"), 4800)],
            &[0x113],
        ),
        item(0x314, "Quiche", 0x12, &[(0x416, None, 1195)], &[0x111]),
    ];
    Profile::new(ProfileData {
        location: id(0xb4ce),
        name: name("Boulangerie Keel"),
        address: vec![name("88 Queen Street West"), name("Toronto, ON M5H 2N2"), name("Canada")],
        currency: Currency::CAD,
        business_day: BusinessDay {
            time_zone: "America/Toronto".to_owned(),
            cutoff_hour: 3,
            cutoff_minute: 30,
        },
        cash_rounding: Some(RoundingRule::new(
            RoundingMode::HalfAwayFromZero,
            NonZeroU32::new(5).unwrap(),
        )),
        rules: Rules {
            extension: RoundingMode::HalfEven,
            discounts: RoundingMode::Floor,
            taxes: vec![
                tax(0x21, "HST", "0.13", &[0x12, 0x13], None),
                tax(0x22, "HST, baked goods eaten here", "0.13", &[0x14], Some(Dining::OnPremises)),
                tax(0x23, "Container levy", "0.005", &[0x12, 0x13, 0x14], Some(Dining::ToGo)),
            ],
            tax_rounding: TaxRounding { scope: TaxScope::Line, mode: RoundingMode::HalfEven },
        },
        catalog: Catalog { items, groups },
        menu: Menu {
            pages: vec![
                page("Viennoiserie", &[0x411]),
                page("Café", &[0x412, 0x413]),
                page("Pâtisserie", &[0x414, 0x415, 0x416]),
            ],
        },
        team: vec![
            Member { id: id(0x511), name: name("Camille"), role: None, pin: None },
            Member { id: id(0x512), name: name("Jo"), role: None, pin: None },
            Member { id: id(0x513), name: name("Noor"), role: None, pin: None },
        ],
        roles: Vec::new(),
    })
    .unwrap()
}

/// Pinned from `tests/golden/profile.py`, which builds the bakery with Python's `cbor2` too.
#[test]
fn the_bakery_is_pinned_byte_for_byte() {
    let profile = golden_bakery();
    let bytes = profile.encode();
    assert_eq!(bytes.len(), 1615);
    assert_eq!(
        hex(&sha256(&bytes)),
        "66c42a5951ffa375e62de140b9e6f034fce04f33eeaf065af8fc2e32eddcc3a2"
    );
    assert_eq!(
        hex(profile.catalog_version().as_bytes()),
        "535d5a072e788fe94e597ca94d8e0855c04651dad182f2627091b0ad03be2615"
    );
    assert_eq!(
        hex(profile.rules_version().as_bytes()),
        "367cd5c948f34116fe53e616d5835bcb7e0302ce75019691a5caf516fab5e328"
    );
    let decoded = Profile::decode(&bytes).unwrap();
    assert_eq!(decoded.data(), profile.data());
    assert_eq!(decoded.encode(), bytes);
}

#[test]
fn the_bakerys_tarts_ring_with_toppings_under_toppings() {
    // Two toppings free, the cheapest that cost something: the crème fraîche, and one of the
    // two berries. Icing sugar costs nothing and uses none, with a snowfall of it chosen under.
    let sugar = Choice { choices: vec![choice(0x229)], ..choice(0x225) };
    let rung = golden_bakery().ring(id(0x414), &[times(0x227, 2), choice(0x226), sugar]).unwrap();
    let cad = |minor| Money::from_minor(minor, Currency::CAD);
    let entry = |n, name: &str, prefix, quantity, minor| ChosenModifier {
        prefix,
        unit_price: cad(minor),
        ..chosen(n, name, quantity, 0)
    };
    let mut icing = entry(0x225, "Icing sugar", Prefix::Extra, 1, 0);
    icing.modifiers = vec![entry(0x229, "Snowfall", Prefix::Extra, 1, 25)];
    assert_eq!(
        rung.modifiers,
        vec![
            icing,
            entry(0x226, "Crème fraîche", Prefix::Side, 1, 0),
            entry(0x227, "Berries", Prefix::Sub, 1, 225),
            entry(0x227, "Berries", Prefix::Sub, 1, 0),
        ]
    );
    assert_eq!(rung.item.name.as_str(), "Tarte aux fruits (Slice)");
    assert_eq!(rung.item.tax_category, id(0x14));
}
