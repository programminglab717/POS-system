//! Property tests of ringing (ADR-0023, decision 4): random catalogs and choices, aimed at each
//! group's bounds and at ties in price, rung against a model written separately. The model finds
//! every rule the choices break, where ringing stops at the first; and it shares out free
//! applications by picking the cheapest left, one at a time, where ringing sorts.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::NonZeroU8;

use keel_domain::codec::Name;
use keel_domain::order::{ChosenModifier, Placement, Prefix};
use keel_domain::profile::{
    Catalog, CatalogGroup, CatalogItem, CatalogModifier, CatalogVariant, Choice, Menu, Page,
    Profile, RingError, demo,
};
use keel_domain::refs::{Modifier, ModifierGroup, Variant};
use keel_types::{Currency, Id, Money};
use proptest::prelude::*;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

/// A catalog's shape, from which [`build`] makes the catalog: groups at levels 1 to 4, each
/// group's modifiers offering groups only from the level below, so nesting is never circular or
/// too deep.
/// A modifier group's shape: its level, `(min, max, free)`, whether its modifiers repeat, and
/// each modifier's price, with the groups (by index) it offers.
type GroupShape = (usize, (u8, u8, u8), bool, Vec<(i64, Vec<usize>)>);

#[derive(Clone, Debug)]
struct Shape {
    /// Each group: its level, `(min, max, free)`, whether modifiers repeat, and its modifiers'
    /// prices, with the groups (by index) each offers.
    groups: Vec<GroupShape>,
    /// Each item: its variants' prices, and the level-1 groups it offers.
    items: Vec<(Vec<i64>, Vec<usize>)>,
}

fn any_price() -> impl Strategy<Value = i64> {
    prop::sample::select(vec![0_i64, 25, 60, 75, 100])
}

/// A group's modifiers' prices, with ties: each modifier at the group's own price half the time.
fn any_prices(modifiers: usize) -> impl Strategy<Value = Vec<i64>> {
    (any_price(), prop::collection::vec((any::<bool>(), any_price()), modifiers)).prop_map(
        |(shared, prices)| {
            prices.into_iter().map(|(tied, price)| if tied { shared } else { price }).collect()
        },
    )
}

fn any_shape() -> impl Strategy<Value = Shape> {
    let group = (1_usize..=4, 1_usize..=4, any::<bool>(), 0_u8..=3, 0_u8..=4, 0_u8..=4)
        .prop_flat_map(|(level, modifiers, repeat, low, extra, free)| {
            let max = low.max(1).saturating_add(extra % 3);
            let min = if repeat { low } else { low.min(u8::try_from(modifiers).unwrap()) };
            let free = free.min(max);
            (
                any_prices(modifiers),
                prop::collection::vec(prop::collection::vec(0_usize..64, 0..=2), modifiers),
            )
                .prop_map(move |(prices, under)| {
                    let modifiers: Vec<(i64, Vec<usize>)> = prices.into_iter().zip(under).collect();
                    (level, (min, max, free), repeat, modifiers)
                })
        });
    (
        prop::collection::vec(group, 1..=7),
        prop::collection::vec(
            (prop::collection::vec(any_price(), 1..=3), prop::collection::vec(0_usize..64, 0..=3)),
            1..=3,
        ),
    )
        .prop_map(|(mut groups, items)| {
            // Each modifier offers groups from the level below, each once; an item, level-1 groups.
            let levels: Vec<usize> = groups.iter().map(|group| group.0).collect();
            let below = |level: usize, picks: &[usize]| -> Vec<usize> {
                let candidates: Vec<usize> =
                    (0..levels.len()).filter(|&g| levels[g] == level + 1).collect();
                let mut chosen: Vec<usize> = Vec::new();
                for pick in picks {
                    if let Some(&group) = candidates.get(pick % candidates.len().max(1))
                        && !chosen.contains(&group)
                    {
                        chosen.push(group);
                    }
                }
                chosen
            };
            for group in &mut groups {
                let level = group.0;
                for modifier in &mut group.3 {
                    modifier.1 = below(level, &modifier.1);
                }
            }
            let items =
                items.into_iter().map(|(variants, picks)| (variants, below(0, &picks))).collect();
            Shape { groups, items }
        })
}

/// The identifiers a shape's entries get.
fn group_id(group: usize) -> Id<ModifierGroup> {
    id(0x1000 + u64::try_from(group).unwrap())
}

fn modifier_id(group: usize, modifier: usize) -> Id<Modifier> {
    id(0x2000 + u64::try_from(group * 16 + modifier).unwrap())
}

fn variant_id(item: usize, variant: usize) -> Id<Variant> {
    id(0x4000 + u64::try_from(item * 16 + variant).unwrap())
}

const PREFIXES: [Prefix; 3] = [Prefix::Plain, Prefix::Extra, Prefix::No];

fn build(shape: &Shape) -> Profile {
    let groups = shape
        .groups
        .iter()
        .enumerate()
        .map(|(g, (_, (min, max, free), repeat, modifiers))| CatalogGroup {
            id: group_id(g),
            name: name(&format!("Group {g}")),
            min: *min,
            max: NonZeroU8::new(*max).unwrap(),
            free: *free,
            repeat: *repeat,
            modifiers: modifiers
                .iter()
                .enumerate()
                .map(|(m, (price, under))| CatalogModifier {
                    id: modifier_id(g, m),
                    name: name(&format!("Modifier {g}.{m}")),
                    prefix: PREFIXES[m % PREFIXES.len()],
                    price: Money::from_minor(*price, Currency::USD),
                    groups: under.iter().map(|&u| group_id(u)).collect(),
                })
                .collect(),
        })
        .collect();
    let items = shape
        .items
        .iter()
        .enumerate()
        .map(|(i, (variants, offered))| CatalogItem {
            id: id(0x3000 + u64::try_from(i).unwrap()),
            name: name(&format!("Item {i}")),
            tax_category: id(0x10),
            variants: variants
                .iter()
                .enumerate()
                .map(|(v, price)| CatalogVariant {
                    id: variant_id(i, v),
                    name: (variants.len() > 1).then(|| name(&format!("Size {v}"))),
                    price: Money::from_minor(*price, Currency::USD),
                })
                .collect(),
            groups: offered.iter().map(|&g| group_id(g)).collect(),
        })
        .collect();
    let mut data = demo().unwrap().data().clone();
    data.catalog = Catalog { items, groups };
    data.menu = Menu { pages: vec![Page { name: name("All"), buttons: vec![variant_id(0, 0)] }] };
    Profile::new(data).unwrap()
}

/// A cashier's choices for a line: half the time within every group's bounds, and otherwise
/// around them, sometimes with a modifier from elsewhere or one chosen twice.
fn any_choices(shape: Shape) -> impl Strategy<Value = (Shape, usize, usize, Vec<Choice>)> {
    let items = shape.items.len();
    (0..items, any::<u64>(), any::<bool>()).prop_map(move |(item, seed, valid)| {
        let variant = usize::try_from(seed % 3).unwrap() % shape.items[item].0.len();
        let mut rng = seed;
        let offered = shape.items[item].1.clone();
        let choices = choices_for(&shape, &offered, &mut rng, 0, valid);
        (shape.clone(), item, variant, choices)
    })
}

/// A small, seeded generator, so that nested choices follow from one seed.
fn next(rng: &mut u64) -> u64 {
    *rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *rng >> 33
}

fn choices_for(
    shape: &Shape,
    offered: &[usize],
    rng: &mut u64,
    depth: usize,
    valid: bool,
) -> Vec<Choice> {
    let mut choices = Vec::new();
    for &g in offered {
        let (_, (min, max, _), repeat, modifiers) = &shape.groups[g];
        // Applications within the bounds, or around them: one under the minimum to one over the
        // maximum.
        let slack = i64::from(!valid);
        let low = i64::from(*min) - slack;
        let high = i64::from(*max) + slack;
        let wanted =
            low + i64::try_from(next(rng) % u64::try_from(high - low + 1).unwrap()).unwrap();
        let mut left = wanted.max(0);
        let mut m = usize::try_from(next(rng)).unwrap() % modifiers.len();
        let mut tried = 0;
        while left > 0 && tried < modifiers.len() {
            let quantity = if *repeat || (!valid && next(rng).is_multiple_of(8)) {
                1 + next(rng) % 3
            } else {
                1
            };
            let quantity = i64::try_from(quantity).unwrap().min(left);
            let under = if depth < 4 {
                choices_for(shape, &modifiers[m].1, rng, depth + 1, valid)
            } else {
                vec![]
            };
            choices.push(Choice {
                modifier: modifier_id(g, m),
                quantity: NonZeroU8::new(u8::try_from(quantity).unwrap()).unwrap(),
                choices: under,
            });
            left -= quantity;
            m = (m + 1) % modifiers.len();
            tried += 1;
        }
    }
    // Now and then a modifier from a group not offered here, or one chosen twice.
    match if valid { 2 } else { next(rng) % 12 } {
        0 => {
            let g = usize::try_from(next(rng)).unwrap() % shape.groups.len();
            choices.push(Choice::of(modifier_id(g, 0)));
        }
        1 if !choices.is_empty() => {
            let again = choices[0].clone();
            choices.push(again);
        }
        _ => {}
    }
    // In any order.
    let len = choices.len();
    for i in (1..len).rev() {
        let j = usize::try_from(next(rng)).unwrap() % (i + 1);
        choices.swap(i, j);
    }
    choices
}

/// Every rule `choices` break, where `offered` are the groups offered.
fn violations(shape: &Shape, offered: &[usize], choices: &[Choice]) -> Vec<RingError> {
    let mut found = Vec::new();
    let locate = |modifier: Id<Modifier>| -> Option<(usize, usize)> {
        (0..shape.groups.len())
            .flat_map(|g| (0..shape.groups[g].3.len()).map(move |m| (g, m)))
            .find(|&(g, m)| modifier_id(g, m) == modifier)
    };
    for (at, choice) in choices.iter().enumerate() {
        match locate(choice.modifier) {
            Some((g, m)) if offered.contains(&g) => {
                if choices[..at].iter().any(|earlier| earlier.modifier == choice.modifier) {
                    found.push(RingError::ChosenTwice(choice.modifier));
                }
                if choice.quantity.get() > 1 && !shape.groups[g].2 {
                    found.push(RingError::NotRepeatable(choice.modifier));
                }
                found.extend(violations(shape, &shape.groups[g].3[m].1, &choice.choices));
            }
            _ => found.push(RingError::NotOffered(choice.modifier)),
        }
    }
    for &g in offered {
        let (_, (min, max, _), _, _) = &shape.groups[g];
        let applications: u32 = choices
            .iter()
            .filter(|choice| locate(choice.modifier).is_some_and(|(at, _)| at == g))
            .map(|choice| u32::from(choice.quantity.get()))
            .sum();
        if applications < u32::from(*min) {
            found.push(RingError::TooFew { group: group_id(g), min: *min });
        }
        if applications > u32::from(*max) {
            found.push(RingError::TooMany {
                group: group_id(g),
                max: NonZeroU8::new(*max).unwrap(),
            });
        }
    }
    found
}

/// The modifiers valid `choices` ring as.
fn model(shape: &Shape, offered: &[usize], choices: &[Choice]) -> Vec<ChosenModifier> {
    let mut rung = Vec::new();
    for &g in offered {
        let (_, (_, _, free), _, modifiers) = &shape.groups[g];
        // This group's choices, in its modifiers' order.
        let mut mine: Vec<(usize, &Choice)> = (0..modifiers.len())
            .filter_map(|m| {
                choices.iter().find(|c| c.modifier == modifier_id(g, m)).map(|c| (m, c))
            })
            .collect();
        mine.sort_by_key(|(m, _)| *m);
        // Free applications: the cheapest left, ties to the modifier listed first, one at a time.
        let mut left: Vec<u8> = mine.iter().map(|(_, c)| c.quantity.get()).collect();
        let mut free_of = vec![0_u8; mine.len()];
        for _ in 0..*free {
            let cheapest = (0..mine.len())
                .filter(|&k| left[k] > 0)
                .min_by_key(|&k| (modifiers[mine[k].0].0, mine[k].0));
            let Some(k) = cheapest else { break };
            left[k] -= 1;
            free_of[k] += 1;
        }
        for (k, (m, choice)) in mine.iter().enumerate() {
            let (price, under) = &modifiers[*m];
            let nested = model(shape, under, &choice.choices);
            let entry = |quantity: u8, cents: i64| ChosenModifier {
                modifier: modifier_id(g, *m),
                name: name(&format!("Modifier {g}.{m}")),
                prefix: PREFIXES[m % PREFIXES.len()],
                quantity: NonZeroU8::new(quantity).unwrap(),
                placement: Placement::Whole,
                unit_price: Money::from_minor(cents, Currency::USD),
                modifiers: nested.clone(),
            };
            if left[k] > 0 {
                rung.push(entry(left[k], *price));
            }
            if free_of[k] > 0 {
                rung.push(entry(free_of[k], 0));
            }
        }
    }
    rung
}

proptest! {
    /// Choices ring exactly when they break no rule, and as the model says; a refusal names a
    /// rule they break.
    #[test]
    fn choices_ring_as_the_model_says((shape, item, variant, choices) in any_shape().prop_flat_map(any_choices)) {
        let profile = build(&shape);
        let offered = &shape.items[item].1;
        let found = violations(&shape, offered, &choices);
        match profile.ring(variant_id(item, variant), &choices) {
            Ok(rung) => {
                prop_assert!(found.is_empty(), "rang despite {found:?}");
                prop_assert_eq!(rung.item.unit_price, Money::from_minor(shape.items[item].0[variant], Currency::USD));
                prop_assert_eq!(rung.item.catalog_version, profile.catalog_version());
                prop_assert_eq!(rung.modifiers, model(&shape, offered, &choices));
            }
            Err(error) => prop_assert!(found.contains(&error), "{error:?} isn't among {found:?}"),
        }
    }

    /// The catalog's version changes with the catalog, and a profile round-trips its encoding.
    #[test]
    fn a_random_catalog_round_trips(shape in any_shape()) {
        let profile = build(&shape);
        let decoded = Profile::decode(&profile.encode()).unwrap();
        prop_assert_eq!(decoded.data(), profile.data());
        prop_assert_eq!(decoded.catalog_version(), profile.catalog_version());
    }
}
