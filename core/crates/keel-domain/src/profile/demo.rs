//! The demo profile: a café's, for tests, the simulator and the register shell before a location
//! has its own.

use core::num::NonZeroU8;

use keel_pricing::{Rules, Tax, TaxRounding, TaxScope};
use keel_types::{Currency, Id, Money, Rate, RoundingMode};

use super::{
    BusinessDay, Catalog, CatalogGroup, CatalogItem, CatalogModifier, CatalogVariant, Member, Menu,
    Page, Profile, ProfileData, ProfileError,
};
use crate::codec::Name;
use crate::order::Prefix;

/// An identifier of the demo's: a UUIDv7 ending in `n`.
fn id<T>(n: u64) -> Result<Id<T>, ProfileError> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}"))
        .map_err(|_| ProfileError::Invalid("a demo identifier"))
}

fn name(text: &str) -> Result<Name, ProfileError> {
    Ok(Name::new(text)?)
}

fn usd(cents: i64) -> Money {
    Money::from_minor(cents, Currency::USD)
}

fn count(n: u8) -> Result<NonZeroU8, ProfileError> {
    NonZeroU8::new(n).ok_or(ProfileError::Invalid("a demo count of zero"))
}

/// A modifier group of the demo's.
fn group(
    n: u64,
    title: &str,
    (min, max, free): (u8, u8, u8),
    repeat: bool,
    modifiers: Vec<CatalogModifier>,
) -> Result<CatalogGroup, ProfileError> {
    Ok(CatalogGroup {
        id: id(n)?,
        name: name(title)?,
        min,
        max: count(max)?,
        free,
        repeat,
        modifiers,
    })
}

/// A modifier of the demo's, with the groups offered under it.
fn modifier(
    n: u64,
    title: &str,
    prefix: Prefix,
    cents: i64,
    groups: &[u64],
) -> Result<CatalogModifier, ProfileError> {
    Ok(CatalogModifier {
        id: id(n)?,
        name: name(title)?,
        prefix,
        price: usd(cents),
        groups: groups.iter().map(|&group| id(group)).collect::<Result<_, _>>()?,
    })
}

/// An item of the demo's, with its variants and the groups offered for it.
fn item(
    n: u64,
    title: &str,
    category: u64,
    variants: &[(u64, Option<&str>, i64)],
    groups: &[u64],
) -> Result<CatalogItem, ProfileError> {
    Ok(CatalogItem {
        id: id(n)?,
        name: name(title)?,
        tax_category: id(category)?,
        variants: variants
            .iter()
            .map(|&(variant, title, cents)| {
                Ok(CatalogVariant {
                    id: id(variant)?,
                    name: title.map(name).transpose()?,
                    price: usd(cents),
                })
            })
            .collect::<Result<_, ProfileError>>()?,
        groups: groups.iter().map(|&group| id(group)).collect::<Result<_, _>>()?,
    })
}

/// The demo profile: Keel Café in Brooklyn, New York, which sells coffee and breakfast, taxed at
/// New York City's 8.875%, and coffee beans, which aren't taxed.
///
/// | Identifier ends in | What |
/// |---|---|
/// | `c0fe` | the location |
/// | `10`, `11` | the tax categories: food and drink, and groceries |
/// | `20` | the sales tax |
/// | `101` to `108` | the modifier groups: milk, extra shots, syrup, spread, egg, side, sauce, toasting |
/// | `201` to `213` | their modifiers |
/// | `301` to `307` | the items: espresso, latte, drip coffee, bagel, breakfast sandwich, bottled water, coffee beans |
/// | `401` to `40a` | their variants |
/// | `501`, `502` | the team: Alex and Sam |
///
/// # Errors
/// None in practice: the demo is checked like any profile.
pub fn demo() -> Result<Profile, ProfileError> {
    let page = |title: &str, buttons: &[u64]| -> Result<Page, ProfileError> {
        Ok(Page {
            name: name(title)?,
            buttons: buttons.iter().map(|&variant| id(variant)).collect::<Result<_, _>>()?,
        })
    };
    let rate = "0.08875".parse().map_err(|_| ProfileError::Invalid("the demo's tax rate"))?;
    Profile::new(ProfileData {
        location: id(0xc0fe)?,
        name: name("Keel Café")?,
        address: vec![name("12 Harbor Street")?, name("Brooklyn, NY 11201")?],
        currency: Currency::USD,
        business_day: BusinessDay {
            time_zone: "America/New_York".to_owned(),
            cutoff_hour: 4,
            cutoff_minute: 0,
        },
        cash_rounding: None,
        rules: Rules {
            taxes: vec![Tax {
                id: id(0x20)?,
                name: "NYC sales tax".to_owned(),
                rate: Rate::from_fraction(rate),
                categories: vec![id(0x10)?],
                dining: None,
            }],
            tax_rounding: TaxRounding {
                scope: TaxScope::Document,
                mode: RoundingMode::HalfAwayFromZero,
            },
            ..Rules::untaxed()
        },
        catalog: Catalog { items: items()?, groups: groups()? },
        menu: Menu {
            pages: vec![
                page("Coffee", &[0x401, 0x402, 0x403, 0x404, 0x405, 0x406])?,
                page("Food", &[0x407, 0x408, 0x409])?,
                page("Retail", &[0x40a])?,
            ],
        },
        team: vec![
            Member { id: id(0x501)?, name: name("Alex")? },
            Member { id: id(0x502)?, name: name("Sam")? },
        ],
    })
}

/// The demo's modifier groups.
fn groups() -> Result<Vec<CatalogGroup>, ProfileError> {
    use Prefix::{Extra, No, Plain};
    Ok(vec![
        group(
            0x101,
            "Milk",
            (0, 1, 0),
            false,
            vec![
                modifier(0x201, "Whole milk", Plain, 0, &[])?,
                modifier(0x202, "Oat milk", Plain, 75, &[])?,
                modifier(0x203, "Almond milk", Plain, 75, &[])?,
                modifier(0x204, "Skim milk", Plain, 0, &[])?,
            ],
        )?,
        group(
            0x102,
            "Extra shots",
            (0, 4, 0),
            true,
            vec![modifier(0x205, "Shot", Extra, 95, &[])?],
        )?,
        group(
            0x103,
            "Syrup",
            (0, 3, 1),
            true,
            vec![
                modifier(0x206, "Vanilla", Plain, 60, &[])?,
                modifier(0x207, "Caramel", Plain, 60, &[])?,
                modifier(0x208, "Hazelnut", Plain, 60, &[])?,
                modifier(0x209, "Sugar-free vanilla", Plain, 60, &[])?,
            ],
        )?,
        group(
            0x104,
            "Spread",
            (1, 1, 0),
            false,
            vec![
                modifier(0x20a, "Cream cheese", Plain, 125, &[])?,
                modifier(0x20b, "Butter", Plain, 50, &[])?,
                modifier(0x20c, "Spread", No, 0, &[])?,
            ],
        )?,
        group(
            0x105,
            "Egg",
            (1, 1, 0),
            false,
            vec![
                modifier(0x20d, "Scrambled", Plain, 0, &[])?,
                modifier(0x20e, "Fried", Plain, 0, &[])?,
            ],
        )?,
        group(
            0x106,
            "Side",
            (0, 1, 0),
            false,
            vec![
                modifier(0x20f, "Fruit cup", Plain, 250, &[])?,
                modifier(0x210, "Hash brown", Plain, 200, &[0x107])?,
            ],
        )?,
        group(
            0x107,
            "Sauce",
            (0, 2, 1),
            false,
            vec![
                modifier(0x211, "Ketchup", Plain, 25, &[])?,
                modifier(0x212, "Hot sauce", Plain, 25, &[])?,
            ],
        )?,
        group(
            0x108,
            "Toasting",
            (0, 1, 0),
            false,
            vec![modifier(0x213, "Toasted", Plain, 0, &[])?],
        )?,
    ])
}

/// The demo's items.
fn items() -> Result<Vec<CatalogItem>, ProfileError> {
    Ok(vec![
        item(0x301, "Espresso", 0x10, &[(0x401, None, 325)], &[0x102, 0x103])?,
        item(
            0x302,
            "Latte",
            0x10,
            &[(0x402, Some("Small"), 450), (0x403, Some("Large"), 525)],
            &[0x101, 0x102, 0x103],
        )?,
        item(
            0x303,
            "Drip coffee",
            0x10,
            &[
                (0x404, Some("Small"), 250),
                (0x405, Some("Medium"), 295),
                (0x406, Some("Large"), 345),
            ],
            &[0x101],
        )?,
        item(0x304, "Bagel", 0x10, &[(0x407, None, 275)], &[0x104, 0x108])?,
        item(0x305, "Breakfast sandwich", 0x10, &[(0x408, None, 695)], &[0x105, 0x106])?,
        item(0x306, "Bottled water", 0x10, &[(0x409, None, 200)], &[])?,
        item(0x307, "Coffee beans, 12 oz", 0x11, &[(0x40a, None, 1400)], &[])?,
    ])
}
