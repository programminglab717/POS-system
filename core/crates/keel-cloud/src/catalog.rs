//! A location's catalog as the back office reads and writes it (ADR-0028, decision 4): JSON, its
//! amounts in minor units of the location's currency and its identifiers as UUIDs. A catalog
//! written replaces the profile's catalog and menu, keeping everything else, and is checked
//! whole by the kernel, as [`Profile::new`] checks every profile.

use core::num::NonZeroU8;
use std::collections::BTreeMap;

use keel_domain::codec::{Name, ProfileVersion};
use keel_domain::order::Prefix;
use keel_domain::profile::{
    Catalog, CatalogGroup, CatalogItem, CatalogModifier, CatalogVariant, Menu, Page, Profile,
};
use keel_types::{Id, Money};
use serde::{Deserialize, Serialize};

/// A version as the API writes it: 64 hexadecimal digits.
pub fn hex(version: ProfileVersion) -> String {
    version.to_hex()
}

/// A location's catalog, as read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    /// The version of the profile it is part of.
    pub version: String,
    /// The location's currency, ISO 4217.
    pub currency: String,
    /// How many digits of its minor unit there are: 2 for US dollars.
    pub minor_units: u8,
    /// The tax categories an item may be in, each with the names of the taxes it is in.
    pub tax_categories: Vec<TaxCategoryView>,
    /// The catalog's items.
    pub items: Vec<ItemJson>,
    /// Its modifier groups.
    pub groups: Vec<GroupJson>,
    /// The menu's pages.
    pub pages: Vec<PageJson>,
}

/// A tax category an item may be in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxCategoryView {
    /// Its identifier.
    pub id: String,
    /// The taxes it is in.
    pub taxes: Vec<String>,
}

/// A catalog to write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEdit {
    /// The version it was read at: if another has been published since, it is refused.
    #[serde(default)]
    pub base: Option<String>,
    /// The items.
    pub items: Vec<ItemJson>,
    /// The modifier groups.
    pub groups: Vec<GroupJson>,
    /// The menu's pages.
    pub pages: Vec<PageJson>,
}

/// An item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemJson {
    /// Its identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its tax category.
    pub tax_category: String,
    /// Its variants: at least one.
    pub variants: Vec<VariantJson>,
    /// The modifier groups it offers.
    #[serde(default)]
    pub groups: Vec<String>,
}

/// A variant of an item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantJson {
    /// Its identifier.
    pub id: String,
    /// Its name, if the item has more than one.
    #[serde(default)]
    pub name: Option<String>,
    /// Its price, in minor units.
    pub price: i64,
}

/// A modifier group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupJson {
    /// Its identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The fewest choices a cashier must make.
    pub min: u8,
    /// The most.
    pub max: u8,
    /// How many are free.
    #[serde(default)]
    pub free: u8,
    /// Whether one modifier may be chosen more than once.
    #[serde(default)]
    pub repeat: bool,
    /// Its modifiers.
    pub modifiers: Vec<ModifierJson>,
}

/// A modifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierJson {
    /// Its identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// How it changes the item: `plain`, `no`, `extra`, `light`, `side` or `sub`.
    #[serde(default = "plain")]
    pub prefix: String,
    /// Its price, in minor units.
    #[serde(default)]
    pub price: i64,
    /// The groups it offers in turn.
    #[serde(default)]
    pub groups: Vec<String>,
}

fn plain() -> String {
    "plain".into()
}

/// A page of the menu.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageJson {
    /// Its name.
    pub name: String,
    /// The variants its buttons ring, in order.
    pub buttons: Vec<String>,
}

const PREFIXES: [(Prefix, &str); 6] = [
    (Prefix::Plain, "plain"),
    (Prefix::No, "no"),
    (Prefix::Extra, "extra"),
    (Prefix::Light, "light"),
    (Prefix::Side, "side"),
    (Prefix::Sub, "sub"),
];

fn prefix_text(prefix: Prefix) -> String {
    PREFIXES.iter().find(|(known, _)| *known == prefix).map_or("plain", |(_, text)| text).to_owned()
}

/// `profile`'s catalog and menu, as read.
pub fn view(profile: &Profile, version: ProfileVersion) -> CatalogView {
    let data = profile.data();
    let mut categories: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &data.catalog.items {
        categories.entry(item.tax_category.to_string()).or_default();
    }
    for tax in &data.rules.taxes {
        for category in &tax.categories {
            categories.entry(category.to_string()).or_default().push(tax.name.clone());
        }
    }
    CatalogView {
        version: hex(version),
        currency: data.currency.code().to_owned(),
        minor_units: data.currency.minor_units(),
        tax_categories: categories
            .into_iter()
            .map(|(id, taxes)| TaxCategoryView { id, taxes })
            .collect(),
        items: data
            .catalog
            .items
            .iter()
            .map(|item| ItemJson {
                id: item.id.to_string(),
                name: item.name.to_string(),
                tax_category: item.tax_category.to_string(),
                variants: item
                    .variants
                    .iter()
                    .map(|variant| VariantJson {
                        id: variant.id.to_string(),
                        name: variant.name.as_ref().map(ToString::to_string),
                        price: variant.price.minor(),
                    })
                    .collect(),
                groups: item.groups.iter().map(ToString::to_string).collect(),
            })
            .collect(),
        groups: data
            .catalog
            .groups
            .iter()
            .map(|group| GroupJson {
                id: group.id.to_string(),
                name: group.name.to_string(),
                min: group.min,
                max: group.max.get(),
                free: group.free,
                repeat: group.repeat,
                modifiers: group
                    .modifiers
                    .iter()
                    .map(|modifier| ModifierJson {
                        id: modifier.id.to_string(),
                        name: modifier.name.to_string(),
                        prefix: prefix_text(modifier.prefix),
                        price: modifier.price.minor(),
                        groups: modifier.groups.iter().map(ToString::to_string).collect(),
                    })
                    .collect(),
            })
            .collect(),
        pages: data
            .menu
            .pages
            .iter()
            .map(|page| PageJson {
                name: page.name.to_string(),
                buttons: page.buttons.iter().map(ToString::to_string).collect(),
            })
            .collect(),
    }
}

/// Why a catalog can't be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    /// A field isn't what it must be: an identifier that isn't a UUID, a name that isn't one, a
    /// prefix that isn't known, or a group's most that is 0.
    #[error("{0}")]
    Field(String),
    /// The profile it makes breaks a rule.
    #[error("{0}")]
    Profile(String),
}

fn id<T>(text: &str, what: &str) -> Result<Id<T>, EditError> {
    Id::parse(text).map_err(|_| EditError::Field(format!("{what} {text:?} isn't an identifier")))
}

fn name(text: &str, what: &str) -> Result<Name, EditError> {
    Name::new(text).map_err(|_| EditError::Field(format!("{what} {text:?} isn't a name")))
}

fn ids<T>(texts: &[String], what: &str) -> Result<Vec<Id<T>>, EditError> {
    texts.iter().map(|text| id(text, what)).collect()
}

/// `profile` with the catalog and menu of `edit`, checked whole.
///
/// # Errors
/// [`EditError`] if a field isn't what it must be, or the profile it makes breaks a rule.
pub fn apply(profile: &Profile, edit: &CatalogEdit) -> Result<Profile, EditError> {
    let mut data = profile.data().clone();
    let currency = data.currency;
    let money = |minor: i64| Money::from_minor(minor, currency);
    let items = edit
        .items
        .iter()
        .map(|item| {
            Ok(CatalogItem {
                id: id(&item.id, "item")?,
                name: name(&item.name, "item name")?,
                tax_category: id(&item.tax_category, "tax category")?,
                variants: item
                    .variants
                    .iter()
                    .map(|variant| {
                        Ok(CatalogVariant {
                            id: id(&variant.id, "variant")?,
                            name: variant
                                .name
                                .as_deref()
                                .map(|text| name(text, "variant name"))
                                .transpose()?,
                            price: money(variant.price),
                        })
                    })
                    .collect::<Result<_, EditError>>()?,
                groups: ids(&item.groups, "modifier group")?,
            })
        })
        .collect::<Result<_, EditError>>()?;
    let groups = edit
        .groups
        .iter()
        .map(|group| {
            Ok(CatalogGroup {
                id: id(&group.id, "modifier group")?,
                name: name(&group.name, "group name")?,
                min: group.min,
                max: NonZeroU8::new(group.max).ok_or_else(|| {
                    EditError::Field(format!("group {:?} allows no choice", group.name))
                })?,
                free: group.free,
                repeat: group.repeat,
                modifiers: group
                    .modifiers
                    .iter()
                    .map(|modifier| {
                        let prefix = PREFIXES
                            .iter()
                            .find(|(_, text)| *text == modifier.prefix)
                            .map(|(prefix, _)| *prefix)
                            .ok_or_else(|| {
                                EditError::Field(format!(
                                    "prefix {:?} isn't one of plain, no, extra, light, side, sub",
                                    modifier.prefix
                                ))
                            })?;
                        Ok(CatalogModifier {
                            id: id(&modifier.id, "modifier")?,
                            name: name(&modifier.name, "modifier name")?,
                            prefix,
                            price: money(modifier.price),
                            groups: ids(&modifier.groups, "modifier group")?,
                        })
                    })
                    .collect::<Result<_, EditError>>()?,
            })
        })
        .collect::<Result<_, EditError>>()?;
    // The catalog keeps its groups in the order of their identifiers, wherever the editor put
    // them.
    let mut groups: Vec<CatalogGroup> = groups;
    groups.sort_by_key(|group| group.id.to_bytes());
    let pages = edit
        .pages
        .iter()
        .map(|page| {
            Ok(Page {
                name: name(&page.name, "page name")?,
                buttons: ids(&page.buttons, "button")?,
            })
        })
        .collect::<Result<_, EditError>>()?;
    data.catalog = Catalog { items, groups };
    data.menu = Menu { pages };
    Profile::new(data).map_err(|error| EditError::Profile(error.to_string()))
}

#[cfg(test)]
mod tests {
    use keel_domain::profile::demo;
    use keel_domain::publication::version;

    use super::*;

    fn edit(view: &CatalogView) -> CatalogEdit {
        CatalogEdit {
            base: Some(view.version.clone()),
            items: view.items.clone(),
            groups: view.groups.clone(),
            pages: view.pages.clone(),
        }
    }

    #[test]
    fn a_catalog_read_and_written_back_is_the_same_profile() {
        let profile = demo().unwrap();
        let view = view(&profile, version(&profile));
        assert_eq!(view.version, version(&profile).to_hex());
        let written = apply(&profile, &edit(&view)).unwrap();
        assert_eq!(written.encode(), profile.encode());
        // Every prefix the demo's modifiers have reads and writes as itself.
        let prefixes: Vec<&str> = view
            .groups
            .iter()
            .flat_map(|group| &group.modifiers)
            .map(|modifier| modifier.prefix.as_str())
            .collect();
        assert!(prefixes.iter().all(|prefix| PREFIXES.iter().any(|(_, text)| text == prefix)));
    }

    #[test]
    fn groups_in_any_order_are_kept_in_the_catalog_s() {
        let profile = demo().unwrap();
        let mut edit = edit(&view(&profile, version(&profile)));
        edit.groups.reverse();
        assert!(edit.groups.len() > 1);
        assert_eq!(apply(&profile, &edit).unwrap().encode(), profile.encode());
    }

    #[test]
    fn what_isnt_a_catalog_is_refused_and_says_why() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let refused = |change: &dyn Fn(&mut CatalogEdit)| {
            let mut edit = edit(&read);
            change(&mut edit);
            apply(&profile, &edit).map(|_| ()).unwrap_err()
        };
        let unknown = refused(&|edit| edit.groups[0].modifiers[0].prefix = "double".into());
        assert!(matches!(&unknown, EditError::Field(message) if message.contains("double")));
        let none = refused(&|edit| edit.groups[0].max = 0);
        assert!(matches!(&none, EditError::Field(message) if message.contains("allows no choice")));
        let id = refused(&|edit| edit.items[0].id = "0192f0c1-0000-4000-8000-000000000001".into());
        assert!(
            matches!(&id, EditError::Field(message) if message.contains("isn't an identifier"))
        );
        let name = refused(&|edit| edit.items[0].name = String::new());
        assert!(matches!(&name, EditError::Field(message) if message.contains("isn't a name")));
        // The kernel's rules: a button for a variant the catalog doesn't have.
        let button = refused(&|edit| {
            edit.pages[0].buttons.push("0192f0c1-0000-7000-8000-00000000dead".into());
        });
        assert!(matches!(button, EditError::Profile(_)));
    }

    #[test]
    fn a_price_changed_is_a_new_version() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let mut changed = edit(&read);
        changed.items[0].variants[0].price = changed.items[0].variants[0].price.saturating_add(10);
        let written = apply(&profile, &changed).unwrap();
        assert_ne!(version(&written), version(&profile));
        let again = view(&written, version(&written));
        assert_eq!(again.items[0].variants[0].price, changed.items[0].variants[0].price);
        // Everything else the profile holds is as it was.
        assert_eq!(written.data().team, profile.data().team);
        assert_eq!(written.data().rules, profile.data().rules);
        assert_eq!(written.data().name, profile.data().name);
    }
}
