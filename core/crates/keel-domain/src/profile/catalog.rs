//! The catalog v0 (ADR-0023, decision 4): what a location sells, and how a cashier's choices
//! ring as a line.

use core::num::NonZeroU8;
use std::collections::BTreeMap;

use keel_types::{Id, Money};

use crate::codec::{CatalogVersion, Name};
use crate::order::{ChosenModifier, ItemSnapshot, Placement, Prefix};
use crate::refs::{Item, Modifier, ModifierGroup, TaxCategory, Variant};

/// What a location sells: its items, and the modifier groups that they, and the modifiers in
/// them, offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    /// The items, in the order the catalog lists them.
    pub items: Vec<CatalogItem>,
    /// Every modifier group, offered by items or by modifiers.
    pub groups: Vec<CatalogGroup>,
}

/// What a merchant thinks of as one thing they sell, such as a latte, whatever its sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogItem {
    /// Its identifier.
    pub id: Id<Item>,
    /// Its name.
    pub name: Name,
    /// The tax category the location's taxes apply to it by.
    pub tax_category: Id<TaxCategory>,
    /// The units it is sold in, such as its sizes: at least one.
    pub variants: Vec<CatalogVariant>,
    /// The modifier groups offered for it, in the order they are shown.
    pub groups: Vec<Id<ModifierGroup>>,
}

/// A unit an item is sold in, such as a large latte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogVariant {
    /// Its identifier.
    pub id: Id<Variant>,
    /// Its name, such as "Large", which an item with several variants gives each.
    pub name: Option<Name>,
    /// Its price: zero or more.
    pub price: Money,
}

/// A group of modifiers offered together, such as an item's milks, with bounds on how many may
/// be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogGroup {
    /// Its identifier.
    pub id: Id<ModifierGroup>,
    /// Its name, such as "Milk".
    pub name: Name,
    /// The fewest applications of its modifiers a line may have: 1 or more makes a choice
    /// required.
    pub min: u8,
    /// The most applications of its modifiers a line may have.
    pub max: NonZeroU8,
    /// How many applications are free, such as "first two syrups free": the lowest-priced.
    pub free: u8,
    /// Whether a modifier may be applied more than once, such as two extra shots.
    pub repeat: bool,
    /// Its modifiers, in the order they are shown: at least one.
    pub modifiers: Vec<CatalogModifier>,
}

/// A modifier, such as oat milk, and the groups chosen under it, such as a side's dressing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogModifier {
    /// Its identifier.
    pub id: Id<Modifier>,
    /// Its name.
    pub name: Name,
    /// How it changes the item.
    pub prefix: Prefix,
    /// The price of one application: zero or more.
    pub price: Money,
    /// The modifier groups offered under it, in the order they are shown.
    pub groups: Vec<Id<ModifierGroup>>,
}

/// A cashier's choice of a modifier for a line, or for another modifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The modifier.
    pub modifier: Id<Modifier>,
    /// How many times it is applied.
    pub quantity: NonZeroU8,
    /// The choices made under it.
    pub choices: Vec<Choice>,
}

impl Choice {
    /// One application of `modifier`, with nothing chosen under it.
    pub fn of(modifier: Id<Modifier>) -> Choice {
        Choice { modifier, quantity: NonZeroU8::MIN, choices: Vec::new() }
    }
}

/// What a line rings as: its item's snapshot, and its modifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rung {
    /// What it sells, at what price.
    pub item: ItemSnapshot,
    /// The modifiers chosen, in the catalog's order: by group as offered, then by modifier in
    /// the group. A modifier whose applications are part charged and part free appears twice,
    /// charged first.
    pub modifiers: Vec<ChosenModifier>,
}

/// Why choices don't ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RingError {
    /// The catalog has no such variant.
    #[error("no variant {0}")]
    UnknownVariant(Id<Variant>),
    /// The modifier isn't in a group offered where it was chosen.
    #[error("modifier {0} isn't offered there")]
    NotOffered(Id<Modifier>),
    /// The modifier was chosen twice at one level; a quantity applies it more than once.
    #[error("modifier {0} is chosen twice")]
    ChosenTwice(Id<Modifier>),
    /// The modifier was chosen more than once in a group that applies each modifier once.
    #[error("modifier {0} can be applied once")]
    NotRepeatable(Id<Modifier>),
    /// A group has fewer applications than its minimum.
    #[error("modifier group {group} needs at least {min}")]
    TooFew {
        /// The group.
        group: Id<ModifierGroup>,
        /// Its minimum.
        min: u8,
    },
    /// A group has more applications than its maximum.
    #[error("modifier group {group} allows at most {max}")]
    TooMany {
        /// The group.
        group: Id<ModifierGroup>,
        /// Its maximum.
        max: NonZeroU8,
    },
}

/// Where each of a checked catalog's entries is, for ringing.
#[derive(Clone, Debug, Default)]
pub(crate) struct Index {
    /// Each variant's item, its position in the item, and the name a line shows for it.
    pub(crate) variants: BTreeMap<Id<Variant>, (usize, usize, Name)>,
    /// Each group's position.
    pub(crate) groups: BTreeMap<Id<ModifierGroup>, usize>,
    /// Each modifier's group and position in it.
    pub(crate) modifiers: BTreeMap<Id<Modifier>, (usize, usize)>,
}

/// A choice, with where it stands among the groups offered at its level.
struct Placed<'a> {
    /// Its group's position among the groups offered.
    offered_at: usize,
    /// Its modifier's position in the group.
    position: usize,
    group: &'a CatalogGroup,
    modifier: &'a CatalogModifier,
    choice: &'a Choice,
}

impl Catalog {
    /// What `variant`, with `choices`, rings as, priced against the catalog `version`.
    ///
    /// `index` must be this catalog's, as the profile's checks built it.
    pub(crate) fn ring(
        &self,
        index: &Index,
        version: CatalogVersion,
        variant: Id<Variant>,
        choices: &[Choice],
    ) -> Result<Rung, RingError> {
        let (item, entry, name) = index
            .variants
            .get(&variant)
            .and_then(|(item, position, name)| {
                let item = self.items.get(*item)?;
                Some((item, item.variants.get(*position)?, name))
            })
            .ok_or(RingError::UnknownVariant(variant))?;
        let modifiers = self.ring_level(index, &item.groups, choices)?;
        Ok(Rung {
            item: ItemSnapshot {
                variant,
                catalog_version: version,
                name: name.clone(),
                tax_category: item.tax_category,
                unit_price: entry.price,
            },
            modifiers,
        })
    }

    /// The modifiers `choices` ring as, where `offered` are the groups offered: checks each
    /// group's bounds, rings what was chosen under each modifier, and shares out each group's
    /// free applications.
    fn ring_level(
        &self,
        index: &Index,
        offered: &[Id<ModifierGroup>],
        choices: &[Choice],
    ) -> Result<Vec<ChosenModifier>, RingError> {
        let mut placed: Vec<Placed<'_>> = Vec::with_capacity(choices.len());
        for choice in choices {
            let not_offered = RingError::NotOffered(choice.modifier);
            let &(group_index, position) =
                index.modifiers.get(&choice.modifier).ok_or(not_offered)?;
            let group = self.groups.get(group_index).ok_or(not_offered)?;
            let offered_at = offered.iter().position(|id| *id == group.id).ok_or(not_offered)?;
            if placed.iter().any(|earlier| earlier.choice.modifier == choice.modifier) {
                return Err(RingError::ChosenTwice(choice.modifier));
            }
            if choice.quantity.get() > 1 && !group.repeat {
                return Err(RingError::NotRepeatable(choice.modifier));
            }
            let modifier = group.modifiers.get(position).ok_or(not_offered)?;
            placed.push(Placed { offered_at, position, group, modifier, choice });
        }
        // Each offered group's bounds. A checked profile's index has every group offered.
        let groups = offered
            .iter()
            .filter_map(|id| index.groups.get(id).and_then(|&position| self.groups.get(position)));
        for (offered_at, group) in groups.enumerate() {
            let applications: u32 = placed
                .iter()
                .filter(|placed| placed.offered_at == offered_at)
                .map(|placed| u32::from(placed.choice.quantity.get()))
                .sum();
            if applications < u32::from(group.min) {
                return Err(RingError::TooFew { group: group.id, min: group.min });
            }
            if applications > u32::from(group.max.get()) {
                return Err(RingError::TooMany { group: group.id, max: group.max });
            }
        }
        // The catalog's order: by group as offered, then by modifier within the group.
        placed.sort_by_key(|placed| (placed.offered_at, placed.position));
        let mut rung = Vec::with_capacity(placed.len());
        let mut rest = placed.as_slice();
        while let Some(first) = rest.first() {
            let count = rest.iter().take_while(|p| p.offered_at == first.offered_at).count();
            let (in_group, after) = rest.split_at(count);
            rest = after;
            let free = free_applications(in_group, first.group.free);
            for (placed, free) in in_group.iter().zip(free) {
                let under =
                    self.ring_level(index, &placed.modifier.groups, &placed.choice.choices)?;
                let modifier = placed.modifier;
                let charged = placed.choice.quantity.get().saturating_sub(free);
                let zero = Money::from_minor(0, modifier.price.currency());
                for (quantity, price) in [(charged, modifier.price), (free, zero)] {
                    if let Some(quantity) = NonZeroU8::new(quantity) {
                        rung.push(ChosenModifier {
                            modifier: modifier.id,
                            name: modifier.name.clone(),
                            prefix: modifier.prefix,
                            quantity,
                            placement: Placement::Whole,
                            unit_price: price,
                            modifiers: under.clone(),
                        });
                    }
                }
            }
        }
        Ok(rung)
    }
}

/// How many of each choice's applications in one group are free, given the group's `free`
/// count: they go to the lowest-priced applications, ties to the modifier listed first.
fn free_applications(in_group: &[Placed<'_>], free: u8) -> Vec<u8> {
    // Each application: its price, its modifier's position in the group, and its choice.
    let mut applications: Vec<(i64, usize, usize)> = in_group
        .iter()
        .enumerate()
        .flat_map(|(choice, placed)| {
            let application = (placed.modifier.price.minor(), placed.position, choice);
            core::iter::repeat_n(application, usize::from(placed.choice.quantity.get()))
        })
        .collect();
    // A group's prices share the location's currency, which the profile's checks ensure.
    applications.sort_unstable();
    let mut counts = vec![0_u8; in_group.len()];
    for &(_, _, choice) in applications.iter().take(usize::from(free)) {
        if let Some(count) = counts.get_mut(choice) {
            *count = count.saturating_add(1);
        }
    }
    counts
}

/// The name a line shows for an item's variant: the item's, with the variant's after it, such as
/// "Latte (Large)", or `None` if that is too long for a name.
pub(crate) fn full_name(item: &Name, variant: Option<&Name>) -> Option<Name> {
    match variant {
        None => Some(item.clone()),
        Some(variant) => Name::new(&format!("{item} ({variant})")).ok(),
    }
}
