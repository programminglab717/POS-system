//! Property tests of location profiles (ADR-0023, decisions 3 and 4).
//!
//! - Ringing: random catalogs and choices, aimed at each group's bounds and at ties in price,
//!   rung against a model written separately. The model finds every rule the choices break, where
//!   ringing stops at the first; and it shares out free applications by picking the cheapest left
//!   that costs something, one at a time, where ringing sorts.
//! - Encoding: random profiles, every field of them drawn, round-trip their encoding; and every
//!   change to an encoding, at any node of it, is refused or decodes to a profile whose encoding is
//!   exactly the changed one, so that every profile has one encoding. Changes that must be refused
//!   (an unknown key, a required one taken away, a set out of order) are.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU32};

use keel_domain::codec::Name;
use keel_domain::order::{ChosenModifier, Placement, Prefix};
use keel_domain::profile::{
    BusinessDay, Catalog, CatalogGroup, CatalogItem, CatalogModifier, CatalogVariant, Choice,
    Member, Menu, Page, Profile, ProfileData, RingError, demo,
};
use keel_domain::refs::{Modifier, ModifierGroup, TaxCategory, Variant};
use keel_events::cbor::{self, Map, Value};
use keel_pricing::{Dining, Rules, Tax, TaxRounding, TaxScope};
use keel_types::{Currency, Decimal, Id, Money, Rate, RoundingMode, RoundingRule};
use proptest::prelude::*;
use proptest::sample::Index;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn name(text: &str) -> Name {
    Name::new(text).unwrap()
}

/// A modifier group's shape: its level, `(min, max, free)`, whether its modifiers repeat, and
/// each modifier's price, with the groups (by index) it offers.
type GroupShape = (usize, (u8, u8, u8), bool, Vec<(i64, Vec<usize>)>);

/// An item's shape: its variants' prices, its tax category (by index), and the level-1 groups
/// it offers.
type ItemShape = (Vec<i64>, u64, Vec<usize>);

/// A catalog's shape, from which [`catalog`] makes the catalog: groups at levels 1 to 4, each
/// group's modifiers offering groups only from the level below, so nesting is never circular or
/// too deep.
#[derive(Clone, Debug)]
struct Shape {
    groups: Vec<GroupShape>,
    items: Vec<ItemShape>,
}

/// Prices, few enough that they tie: nothing, a minor unit, a few common ones, and a large one.
fn any_price() -> impl Strategy<Value = i64> {
    prop::sample::select(vec![0_i64, 0, 1, 25, 60, 75, 100, 999_999_999])
}

/// A group's modifiers' prices, with ties: each modifier at the group's own price half the time.
fn any_prices(modifiers: usize) -> impl Strategy<Value = Vec<i64>> {
    (any_price(), prop::collection::vec((any::<bool>(), any_price()), modifiers)).prop_map(
        |(shared, prices)| {
            prices.into_iter().map(|(tied, price)| if tied { shared } else { price }).collect()
        },
    )
}

/// A group's `(min, max, free)`: a small maximum mostly, and now and then the most a group
/// allows, with free applications up to it.
fn any_bounds(modifiers: usize, repeat: bool) -> impl Strategy<Value = (u8, u8, u8)> {
    (0_u8..=3, 0_u8..=4, 0_u8..=8, 0_u8..8).prop_map(move |(low, extra, free, wide)| {
        let max = if wide == 0 { u8::MAX } else { low.max(1).saturating_add(extra % 3) };
        let min = if repeat { low } else { low.min(u8::try_from(modifiers).unwrap()) };
        let free = if wide == 0 && free == 8 { max } else { free.min(max) };
        (min, max, free)
    })
}

fn any_shape() -> impl Strategy<Value = Shape> {
    let group =
        (1_usize..=4, 1_usize..=6, any::<bool>()).prop_flat_map(|(level, modifiers, repeat)| {
            (
                any_bounds(modifiers, repeat),
                any_prices(modifiers),
                prop::collection::vec(prop::collection::vec(0_usize..64, 0..=2), modifiers),
            )
                .prop_map(move |(bounds, prices, under)| {
                    let modifiers: Vec<(i64, Vec<usize>)> = prices.into_iter().zip(under).collect();
                    (level, bounds, repeat, modifiers)
                })
        });
    let item = (
        prop::collection::vec(any_price(), 1..=3),
        0_u64..3,
        prop::collection::vec(0_usize..64, 0..=3),
    );
    (prop::collection::vec(group, 1..=8), prop::collection::vec(item, 1..=4)).prop_map(
        |(mut groups, items)| {
            // Each modifier offers groups from the level below, each once; an item, level-1
            // groups.
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
            let items = items
                .into_iter()
                .map(|(variants, category, picks)| (variants, category, below(0, &picks)))
                .collect();
            Shape { groups, items }
        },
    )
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

fn category_id(category: u64) -> Id<TaxCategory> {
    id(0x10 + category)
}

const PREFIXES: [Prefix; 6] =
    [Prefix::Plain, Prefix::Extra, Prefix::No, Prefix::Light, Prefix::Side, Prefix::Sub];

fn prefix(group: usize, modifier: usize) -> Prefix {
    PREFIXES[(group + modifier) % PREFIXES.len()]
}

fn modifier_name(group: usize, modifier: usize) -> Name {
    name(&format!("Modifier {group}.{modifier}"))
}

/// The name a line shows for an item's variant.
fn variant_name(shape: &Shape, item: usize, variant: usize) -> String {
    if shape.items[item].0.len() > 1 {
        format!("Item {item} (Size {variant})")
    } else {
        format!("Item {item}")
    }
}

/// The catalog a shape describes, priced in `currency`.
fn catalog(shape: &Shape, currency: Currency) -> Catalog {
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
                    name: modifier_name(g, m),
                    prefix: prefix(g, m),
                    price: Money::from_minor(*price, currency),
                    groups: under.iter().map(|&u| group_id(u)).collect(),
                })
                .collect(),
        })
        .collect();
    let items = shape
        .items
        .iter()
        .enumerate()
        .map(|(i, (variants, category, offered))| CatalogItem {
            id: id(0x3000 + u64::try_from(i).unwrap()),
            name: name(&format!("Item {i}")),
            tax_category: category_id(*category),
            variants: variants
                .iter()
                .enumerate()
                .map(|(v, price)| CatalogVariant {
                    id: variant_id(i, v),
                    name: (variants.len() > 1).then(|| name(&format!("Size {v}"))),
                    price: Money::from_minor(*price, currency),
                })
                .collect(),
            groups: offered.iter().map(|&g| group_id(g)).collect(),
        })
        .collect();
    Catalog { items, groups }
}

/// The demo's profile, with the catalog a shape describes.
fn build(shape: &Shape) -> Profile {
    let mut data = demo().unwrap().data().clone();
    data.catalog = catalog(shape, Currency::USD);
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
        let offered = shape.items[item].2.clone();
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
        // maximum. Wide groups are mostly asked for few, or for as many as they allow.
        let slack = i64::from(!valid);
        let low = i64::from(*min) - slack;
        let high = if *max == u8::MAX && !next(rng).is_multiple_of(4) {
            low + 3
        } else {
            i64::from(*max) + slack
        };
        let wanted =
            low + i64::try_from(next(rng) % u64::try_from(high - low + 1).unwrap()).unwrap();
        let mut left = wanted.max(0);
        let mut m = usize::try_from(next(rng)).unwrap() % modifiers.len();
        let mut tried = 0;
        while left > 0 && tried < modifiers.len() {
            let last = tried + 1 == modifiers.len();
            let quantity = if *repeat && (last || next(rng).is_multiple_of(4)) {
                // All that is left, as one choice can apply it.
                left.min(255)
            } else if *repeat || (!valid && next(rng).is_multiple_of(8)) {
                1 + i64::try_from(next(rng) % 3).unwrap()
            } else {
                1
            };
            let quantity = quantity.min(left);
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
        // Free applications: the cheapest left that costs something, ties to the modifier
        // listed first, one at a time.
        let mut left: Vec<u8> = mine.iter().map(|(_, c)| c.quantity.get()).collect();
        let mut free_of = vec![0_u8; mine.len()];
        for _ in 0..*free {
            let cheapest = (0..mine.len())
                .filter(|&k| left[k] > 0 && modifiers[mine[k].0].0 > 0)
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
                name: modifier_name(g, *m),
                prefix: prefix(g, *m),
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

/// Names, some of them beyond ASCII.
fn any_name() -> impl Strategy<Value = Name> {
    prop::sample::select(vec!["Keel", "Café", "Crème brûlée", "東京 2", "Ünïcödé, Ltd.", "A"])
        .prop_map(name)
}

const CURRENCIES: [Currency; 5] =
    [Currency::USD, Currency::CAD, Currency::EUR, Currency::JPY, Currency::KWD];

/// Time zones: canonical names, a fixed offset, another name for a zone, and zones with
/// half-hour and 45-minute offsets.
const ZONES: [&str; 7] = [
    "America/New_York",
    "Europe/Paris",
    "Asia/Kolkata",
    "Pacific/Chatham",
    "UTC",
    "Etc/GMT+5",
    "US/Eastern",
];

fn any_mode() -> impl Strategy<Value = RoundingMode> {
    prop::sample::select(RoundingMode::ALL.to_vec())
}

/// A tax rate: a decimal fraction with up to 28 digits after the point, zero included.
fn any_rate() -> impl Strategy<Value = Rate> {
    prop_oneof![
        (0_i64..=100_000, 0_u32..=6),
        (any::<i64>().prop_map(i64::unsigned_abs), 0_u32..=28)
            .prop_map(|(mantissa, scale)| (i64::try_from(mantissa / 2).unwrap(), scale)),
    ]
    .prop_map(|(mantissa, scale)| Rate::from_fraction(Decimal::new(mantissa, scale)))
}

fn any_taxes() -> impl Strategy<Value = Vec<Tax>> {
    let tax = (
        any_name(),
        any_rate(),
        prop::collection::btree_set(0_u64..6, 1..=4),
        prop::option::of(prop::sample::select(vec![Dining::OnPremises, Dining::ToGo])),
        0_u8..8,
    );
    prop::collection::vec(tax, 0..=4).prop_map(|taxes| {
        taxes
            .into_iter()
            .enumerate()
            .map(|(at, (name, rate, categories, dining, order))| {
                // A set, ascending: the identifiers end in the category's number. Now and then
                // out of order, which a profile refuses.
                let mut categories: Vec<_> = categories.into_iter().map(|c| id(0x10 + c)).collect();
                if order == 0 {
                    categories.reverse();
                }
                Tax {
                    id: id(0x20 + u64::try_from(at).unwrap()),
                    name: name.as_str().to_owned(),
                    rate,
                    categories,
                    dining,
                }
            })
            .collect()
    })
}

fn any_rules() -> impl Strategy<Value = Rules> {
    (any_mode(), any_mode(), any_taxes(), any::<bool>(), any_mode()).prop_map(
        |(extension, discounts, taxes, per_line, mode)| Rules {
            extension,
            discounts,
            taxes,
            tax_rounding: TaxRounding {
                scope: if per_line { TaxScope::Line } else { TaxScope::Document },
                mode,
            },
        },
    )
}

fn any_cash_rounding() -> impl Strategy<Value = Option<RoundingRule>> {
    let increment = prop_oneof![2_u32..=100, Just(u32::MAX)];
    prop::option::of((any_mode(), increment)).prop_map(|rule| {
        rule.map(|(mode, increment)| RoundingRule::new(mode, NonZeroU32::new(increment).unwrap()))
    })
}

/// A whole profile, every field drawn.
fn any_profile() -> impl Strategy<Value = ProfileData> {
    let place = (
        any::<u64>(),
        any_name(),
        prop::collection::vec(any_name(), 0..=4),
        prop::sample::select(CURRENCIES.to_vec()),
        prop::sample::select(ZONES.to_vec()),
        0_u8..24,
        0_u8..60,
        any_cash_rounding(),
    );
    let pages =
        prop::collection::vec((any_name(), prop::collection::vec(any::<Index>(), 1..=4)), 0..=3);
    let team = prop::collection::vec(any_name(), 1..=4);
    (place, any_rules(), any_shape(), pages, team).prop_map(
        |(
            (location, title, address, currency, zone, hour, minute, cash_rounding),
            rules,
            shape,
            pages,
            team,
        )| {
            let catalog = catalog(&shape, currency);
            let variants: Vec<Id<Variant>> = catalog
                .items
                .iter()
                .flat_map(|item| item.variants.iter().map(|variant| variant.id))
                .collect();
            ProfileData {
                location: id(location >> 16),
                name: title,
                address,
                currency,
                business_day: BusinessDay {
                    time_zone: zone.to_owned(),
                    cutoff_hour: hour,
                    cutoff_minute: minute,
                },
                cash_rounding,
                rules,
                catalog,
                menu: Menu {
                    pages: pages
                        .into_iter()
                        .map(|(name, buttons)| Page {
                            name,
                            buttons: buttons.iter().map(|b| *b.get(&variants)).collect(),
                        })
                        .collect(),
                },
                team: team
                    .into_iter()
                    .enumerate()
                    .map(|(at, name)| Member { id: id(0x500 + u64::try_from(at).unwrap()), name })
                    .collect(),
            }
        },
    )
}

/// What a node of a profile's encoding is, which says which of a map's keys may be absent and
/// whether an array is a set, whose order is fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Profile,
    Rules,
    Taxes,
    Tax,
    Categories,
    Catalog,
    Items,
    Item,
    Variants,
    Variant,
    Groups,
    Group,
    Modifiers,
    Modifier,
    Menu,
    Pages,
    Page,
    Team,
    Member,
    Other,
}

impl Kind {
    /// The kind of the value under key `key` of a map of this kind.
    fn child(self, key: u64) -> Kind {
        match (self, key) {
            (Kind::Profile, 8) => Kind::Rules,
            (Kind::Profile, 9) => Kind::Catalog,
            (Kind::Profile, 10) => Kind::Menu,
            (Kind::Profile, 11) => Kind::Team,
            (Kind::Rules, 3) => Kind::Taxes,
            (Kind::Tax, 4) => Kind::Categories,
            (Kind::Catalog, 1) => Kind::Items,
            (Kind::Catalog, 2) => Kind::Groups,
            (Kind::Item, 4) => Kind::Variants,
            (Kind::Group, 7) => Kind::Modifiers,
            (Kind::Menu, 1) => Kind::Pages,
            _ => Kind::Other,
        }
    }

    /// The kind of an element of an array of this kind.
    fn element(self) -> Kind {
        match self {
            Kind::Taxes => Kind::Tax,
            Kind::Items => Kind::Item,
            Kind::Variants => Kind::Variant,
            Kind::Groups => Kind::Group,
            Kind::Modifiers => Kind::Modifier,
            Kind::Pages => Kind::Page,
            Kind::Team => Kind::Member,
            _ => Kind::Other,
        }
    }

    /// The keys a map of this kind may leave out.
    fn optional(self) -> &'static [u64] {
        match self {
            Kind::Profile => &[7],
            Kind::Tax => &[5],
            Kind::Variant => &[2],
            Kind::Group => &[3, 5],
            _ => &[],
        }
    }

    /// Whether an array of this kind is a set, written in ascending order.
    fn is_set(self) -> bool {
        matches!(self, Kind::Categories | Kind::Groups)
    }
}

/// A step from a node to one under it.
#[derive(Clone, Debug)]
enum Step {
    Key(Value),
    At(usize),
}

/// Every node of `value`, a node of kind `kind` at `path`, with its path and kind.
fn nodes(value: &Value, kind: Kind, path: &mut Vec<Step>, out: &mut Vec<(Vec<Step>, Kind)>) {
    out.push((path.clone(), kind));
    match value {
        Value::Map(map) => {
            for (key, child) in map.iter() {
                path.push(Step::Key(key.clone()));
                nodes(child, key.as_u64().map_or(Kind::Other, |key| kind.child(key)), path, out);
                path.pop();
            }
        }
        Value::Array(elements) => {
            for (at, element) in elements.iter().enumerate() {
                path.push(Step::At(at));
                nodes(element, kind.element(), path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// `value`, with the node at `path` replaced by what `change` makes of it.
fn replace(value: &Value, path: &[Step], change: &dyn Fn(&Value) -> Value) -> Value {
    let Some((step, rest)) = path.split_first() else { return change(value) };
    match (value, step) {
        (Value::Map(map), Step::Key(key)) => {
            let entries = map.iter().map(|(k, v)| {
                let v = if k == key { replace(v, rest, change) } else { v.clone() };
                (k.clone(), v)
            });
            Value::Map(Map::from_entries(entries.collect::<Vec<_>>()).unwrap())
        }
        (Value::Array(elements), Step::At(at)) => Value::Array(
            elements
                .iter()
                .enumerate()
                .map(|(i, e)| if i == *at { replace(e, rest, change) } else { e.clone() })
                .collect(),
        ),
        _ => panic!("no node at {path:?}"),
    }
}

/// Values of every type a node may be changed to.
fn others() -> Vec<Value> {
    vec![
        Value::Unsigned(0),
        Value::Unsigned(1),
        Value::Unsigned(2),
        Value::Unsigned(5),
        Value::Unsigned(255),
        Value::Unsigned(256),
        Value::Unsigned(u64::MAX),
        Value::Negative(0),
        Value::Text(String::new()),
        Value::from("0.1"),
        Value::from("0.10"),
        Value::from("USD"),
        Value::from("UTC"),
        Value::Bytes(vec![0; 16]),
        Value::Bytes(id::<Variant>(0x4000).to_bytes().to_vec()),
        Value::Bool(false),
        Value::Bool(true),
        Value::Null,
        Value::Array(vec![]),
        Value::Map(Map::new()),
    ]
}

/// A change to one node of an encoding.
#[derive(Clone, Debug)]
enum Change {
    /// Replace it with another value.
    Replace(Index),
    /// Add a key that its record doesn't define.
    AddKey,
    /// Take away one of its keys.
    RemoveKey(Index),
    /// Swap two of its elements.
    Swap(Index, Index),
    /// Write one of its elements again, at its end.
    Repeat(Index),
    /// Take away one of its elements.
    Remove(Index),
}

fn any_change() -> impl Strategy<Value = Change> {
    prop_oneof![
        3 => any::<Index>().prop_map(Change::Replace),
        1 => Just(Change::AddKey),
        2 => any::<Index>().prop_map(Change::RemoveKey),
        2 => (any::<Index>(), any::<Index>()).prop_map(|(a, b)| Change::Swap(a, b)),
        1 => any::<Index>().prop_map(Change::Repeat),
        1 => any::<Index>().prop_map(Change::Remove),
    ]
}

/// What a change makes of a node of kind `kind`, and whether a profile must refuse it, or
/// `None` if it can't change this node.
fn apply(node: &Value, kind: Kind, change: &Change) -> Option<(Value, bool)> {
    match (change, node) {
        (Change::Replace(pick), _) => Some((pick.get(&others()).clone(), false)),
        (Change::AddKey, Value::Map(map)) => {
            let unknown = map.iter().filter_map(|(k, _)| k.as_u64()).max().unwrap_or(0) + 1;
            let mut entries = map.clone().into_entries();
            entries.push((Value::Unsigned(unknown), Value::Null));
            Some((Value::Map(Map::from_entries(entries).unwrap()), true))
        }
        (Change::RemoveKey(pick), Value::Map(map)) if !map.is_empty() => {
            let mut entries = map.clone().into_entries();
            let (key, _) = entries.remove(pick.index(entries.len()));
            let required = !key.as_u64().is_some_and(|key| kind.optional().contains(&key));
            Some((Value::Map(Map::from_entries(entries).unwrap()), required))
        }
        (Change::Swap(a, b), Value::Array(elements)) if elements.len() >= 2 => {
            let (a, b) = (a.index(elements.len()), b.index(elements.len()));
            let mut swapped = elements.clone();
            swapped.swap(a, b);
            // A set's elements are distinct, so a swap of two of them puts it out of order.
            Some((Value::Array(swapped), kind.is_set() && a != b))
        }
        (Change::Repeat(pick), Value::Array(elements)) if !elements.is_empty() => {
            let mut repeated = elements.clone();
            repeated.push(pick.get(elements).clone());
            Some((Value::Array(repeated), kind.is_set()))
        }
        (Change::Remove(pick), Value::Array(elements)) if !elements.is_empty() => {
            let mut fewer = elements.clone();
            fewer.remove(pick.index(elements.len()));
            Some((Value::Array(fewer), false))
        }
        _ => None,
    }
}

proptest! {
    /// Choices ring exactly when they break no rule, and as the model says; a refusal names a
    /// rule they break.
    #[test]
    fn choices_ring_as_the_model_says((shape, item, variant, choices) in any_shape().prop_flat_map(any_choices)) {
        let profile = build(&shape);
        let offered = &shape.items[item].2;
        let found = violations(&shape, offered, &choices);
        match profile.ring(variant_id(item, variant), &choices) {
            Ok(rung) => {
                prop_assert!(found.is_empty(), "rang despite {found:?}");
                prop_assert_eq!(rung.item.variant, variant_id(item, variant));
                prop_assert_eq!(rung.item.name.as_str(), variant_name(&shape, item, variant));
                prop_assert_eq!(rung.item.tax_category, category_id(shape.items[item].1));
                prop_assert_eq!(rung.item.unit_price, Money::from_minor(shape.items[item].0[variant], Currency::USD));
                prop_assert_eq!(rung.item.catalog_version, profile.catalog_version());
                prop_assert_eq!(rung.modifiers, model(&shape, offered, &choices));
            }
            Err(error) => prop_assert!(found.contains(&error), "{error:?} isn't among {found:?}"),
        }
    }

    /// A profile, every field of it drawn, round-trips its encoding, versions and all; one
    /// whose tax categories aren't in order is refused.
    #[test]
    fn a_random_profile_round_trips(data in any_profile()) {
        let ascending = data.rules.taxes.iter().all(|tax| {
            tax.categories.windows(2).all(|pair| pair[0].to_bytes() < pair[1].to_bytes())
        });
        if !ascending {
            prop_assert_eq!(
                Profile::new(data).map(|_| ()).unwrap_err(),
                keel_domain::profile::ProfileError::Invalid("a tax's categories not each once, in ascending order")
            );
            return Ok(());
        }
        let profile = Profile::new(data.clone()).unwrap();
        prop_assert_eq!(profile.data(), &data);
        let bytes = profile.encode();
        let decoded = Profile::decode(&bytes).unwrap();
        prop_assert_eq!(decoded.data(), profile.data());
        prop_assert_eq!(decoded.encode(), bytes);
        prop_assert_eq!(decoded.catalog_version(), profile.catalog_version());
        prop_assert_eq!(decoded.rules_version(), profile.rules_version());
    }

    /// A change to any node of a profile's encoding is refused, or decodes to a profile whose
    /// encoding is exactly the changed one: no profile has two encodings. An unknown key, a
    /// required key taken away and a set out of order or repeated are refused.
    #[test]
    fn every_change_to_an_encoding_is_refused_or_canonical(
        data in any_profile(),
        kind_at in any::<Index>(),
        at in any::<Index>(),
        change in any_change(),
    ) {
        let Ok(profile) = Profile::new(data) else { return Ok(()) };
        let bytes = profile.encode();
        let value = cbor::decode(&bytes).unwrap();
        let mut all = Vec::new();
        nodes(&value, Kind::Profile, &mut Vec::new(), &mut all);
        // A kind of node first, then a node of it: picked from all nodes at once, the few that
        // are sets, or records of a rare kind, would hardly ever change.
        let mut kinds: Vec<Kind> = all.iter().map(|(_, kind)| *kind).collect();
        kinds.sort_by_key(|kind| format!("{kind:?}"));
        kinds.dedup();
        // Leaves, where most values are, a quarter of the time.
        let records = kinds.len();
        kinds.extend(core::iter::repeat_n(Kind::Other, records / 3));
        let kind = *kind_at.get(&kinds);
        let of_kind: Vec<&(Vec<Step>, Kind)> = all.iter().filter(|(_, k)| *k == kind).collect();
        let (path, kind) = *at.get(&of_kind);
        let mut target = &value;
        for step in path {
            target = match (target, step) {
                (Value::Map(map), Step::Key(key)) => map.get(key).unwrap(),
                (Value::Array(elements), Step::At(at)) => &elements[*at],
                _ => panic!("no node at {path:?}"),
            };
        }
        if let Some((changed, refused)) = apply(target, *kind, &change) {
            let changed = replace(&value, path, &|_| changed.clone()).encode();
            if let Ok(profile) = Profile::decode(&changed) {
                prop_assert!(!refused, "{change:?} at {path:?} ({kind:?}) was accepted");
                prop_assert_eq!(profile.encode(), changed, "{:?} at {:?}", change, path);
            }
        }
    }
}
