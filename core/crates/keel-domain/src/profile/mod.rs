//! The location profile v0 (ADR-0023, decision 3): everything a register needs to ring at a
//! location, in one document.
//!
//! - **The location**: its identifier, name and the address lines a receipt shows; its currency;
//!   its business-day policy; and its cash rounding rule, if it rounds cash payments.
//! - **Its pricing rules**: `keel-pricing`'s taxes and rounding modes.
//! - **Its catalog** ([`Catalog`]): items, their variants, and the modifier groups they offer,
//!   from which a cashier's choices ring as a line ([`Profile::ring`]).
//! - **Its menu**: pages of buttons, each a variant.
//! - **Its team**: the members who may sign in.
//!
//! A profile is canonical CBOR, like an event payload ([`crate::codec`]), and decoded as strictly:
//! every value has exactly one encoding. Its catalog's encoding hashes, with SHA-256, to the
//! [`CatalogVersion`] its lines record, and its pricing rules' to the [`RulesVersion`] its checks'
//! snapshots record. [`Profile::new`] checks a profile whole before anything uses it.
//!
//! Until the cloud publishes reference data, a profile is a file provisioned with the device.
//!
//! The profile, format 1:
//!
//! | Key | Field | Rules |
//! |---|---|---|
//! | 1 | format | 1 |
//! | 2 | location | an identifier |
//! | 3 | name | a name |
//! | 4 | address | up to four names, the lines a receipt shows |
//! | 5 | currency | a circulating currency, every price's |
//! | 6 | business day | `[IANA time zone, cutoff hour, cutoff minute]` |
//! | 7 | cash rounding | `[rounding mode, increment in minor units, more than 1]`; absent when cash isn't rounded |
//! | 8 | rules | the pricing rules, below |
//! | 9 | catalog | the catalog, below |
//! | 10 | menu | `{1: pages}`; a page is `{1: name, 2: [variant, ...]}`, with at least one button |
//! | 11 | team | members, each `{1: identifier, 2: name}`; at least one |
//!
//! The pricing rules: `{1: extension rounding, 2: discount rounding, 3: taxes, 4: [tax rounding
//! scope, mode]}`. A tax is `{1: identifier, 2: name, 3: rate, 4: tax categories, 5: dining}`:
//! its rate a decimal fraction in text, `0.08875` for 8.875%, with no trailing zeros; its tax
//! categories a set of identifiers, at most [`MAX_CATEGORIES`]; and dining (0 on the premises, 1
//! to go) absent when it applies to both. Rounding modes are coded 0 half away from zero, 1 half
//! even, 2 half toward zero, 3 away from zero, 4 toward zero, 5 ceiling and 6 floor; tax rounding
//! scopes 0 per line and 1 per document.
//!
//! The catalog: `{1: items, 2: modifier groups}`.
//!
//! | Record | Keys |
//! |---|---|
//! | item | 1 identifier, 2 name, 3 tax category, 4 variants (at least one), 5 modifier groups |
//! | variant | 1 identifier, 2 name (absent if it has none), 3 price |
//! | modifier group | 1 identifier, 2 name, 3 minimum (absent if 0), 4 maximum, 5 free applications (absent if 0), 6 whether modifiers repeat, 7 modifiers (at least one) |
//! | modifier | 1 identifier, 2 name, 3 prefix, 4 price, 5 modifier groups under it |

mod catalog;
mod demo;
mod rules;
#[cfg(test)]
mod tests;

use core::num::{NonZeroU8, NonZeroU32};
use std::collections::{BTreeMap, BTreeSet};

use keel_events::cbor::{self, CborError, Value};
use keel_events::envelope::{Location, TeamMember};
use keel_events::hash::sha256;
use keel_pricing::Rules;
use keel_types::{BusinessDayPolicy, Currency, Id, RoundingRule};

pub use catalog::{
    Catalog, CatalogGroup, CatalogItem, CatalogModifier, CatalogVariant, Choice, RingError, Rung,
};
pub use demo::demo;

use crate::codec::{CatalogVersion, Field, Fields, Name, PayloadError, Record, RulesVersion};
use crate::refs::{ModifierGroup, Variant};
use catalog::{Index, full_name};

/// The profile format this kernel reads and writes.
pub const FORMAT: u64 = 1;

/// The most items a catalog holds.
pub const MAX_ITEMS: usize = 200_000;
/// The most variants a catalog holds, of all its items.
pub const MAX_ALL_VARIANTS: usize = 1_000_000;
/// The most modifier groups a catalog holds.
pub const MAX_GROUPS: usize = 100_000;
/// The most modifiers a catalog holds, in all its groups.
pub const MAX_ALL_MODIFIERS: usize = 1_000_000;
/// The most variants an item has.
pub const MAX_VARIANTS: usize = 100;
/// The most modifiers in a group, and groups offered by an item or a modifier.
pub const MAX_PER_GROUP: usize = 255;
/// The deepest modifier groups nest: an item's groups are at depth 1.
pub const MAX_DEPTH: usize = 4;
/// The most address lines a receipt shows.
pub const MAX_ADDRESS: usize = 4;
/// The most menu pages.
pub const MAX_PAGES: usize = 100;
/// The most buttons on a menu page.
pub const MAX_BUTTONS: usize = 500;
/// The most team members.
pub const MAX_TEAM: usize = 10_000;
/// The most taxes.
pub const MAX_TAXES: usize = 64;
/// The most tax categories a tax applies to.
pub const MAX_CATEGORIES: usize = 1_000;

/// A location's business-day policy, as a profile writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusinessDay {
    /// The IANA time zone, such as `America/New_York`.
    pub time_zone: String,
    /// The local hour business days begin at, 0 to 23.
    pub cutoff_hour: u8,
    /// The minute past it, 0 to 59.
    pub cutoff_minute: u8,
}

/// The menu: pages of buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// Its pages, in order.
    pub pages: Vec<Page>,
}

/// A page of the menu, such as "Coffee".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Its name.
    pub name: Name,
    /// Its buttons, in order, each a variant to ring.
    pub buttons: Vec<Id<Variant>>,
}

/// A team member who may sign in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Their identifier, which events they cause record.
    pub id: Id<TeamMember>,
    /// The name the register shows for them.
    pub name: Name,
}

/// A location profile's contents, unchecked: [`Profile::new`] checks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileData {
    /// The location.
    pub location: Id<Location>,
    /// Its name, as a receipt shows it.
    pub name: Name,
    /// Its address, as a receipt shows it.
    pub address: Vec<Name>,
    /// Its currency, every price's.
    pub currency: Currency,
    /// When its business days begin.
    pub business_day: BusinessDay,
    /// How it rounds what is paid in cash, if it does.
    pub cash_rounding: Option<RoundingRule>,
    /// Its taxes and rounding modes.
    pub rules: Rules,
    /// What it sells.
    pub catalog: Catalog,
    /// How the register lays out what it sells.
    pub menu: Menu,
    /// Who may sign in.
    pub team: Vec<Member>,
}

/// A location profile, checked whole.
#[derive(Clone, Debug)]
pub struct Profile {
    data: ProfileData,
    business_day: BusinessDayPolicy,
    index: Index,
    catalog_version: CatalogVersion,
    rules_version: RulesVersion,
}

/// Why a profile can't be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    /// The bytes aren't canonical CBOR.
    #[error("the profile isn't canonical CBOR: {0}")]
    Cbor(#[from] CborError),
    /// A profile format this kernel doesn't read.
    #[error("profile format {0} isn't one this kernel reads")]
    Format(u64),
    /// A field is missing, unknown or malformed.
    #[error(transparent)]
    Payload(#[from] PayloadError),
    /// An identifier appears twice: two items, variants, modifier groups, modifiers, taxes or team
    /// members share it.
    #[error("two {0} share an identifier")]
    Duplicate(&'static str),
    /// A reference to something the profile doesn't have.
    #[error("a reference to a {0} the profile doesn't have")]
    Unknown(&'static str),
    /// A rule the profile breaks.
    #[error("{0}")]
    Invalid(&'static str),
}

impl Profile {
    /// `data`, checked: identifiers unique, every reference resolved, modifier groups nested at
    /// most [`MAX_DEPTH`] deep and never within themselves, their bounds coherent, every price in
    /// the location's currency and none negative, and every count within its limit.
    ///
    /// # Errors
    /// [`ProfileError`] for the first rule `data` breaks.
    pub fn new(data: ProfileData) -> Result<Profile, ProfileError> {
        let business_day = BusinessDayPolicy::new(
            &data.business_day.time_zone,
            data.business_day.cutoff_hour,
            data.business_day.cutoff_minute,
        )
        .map_err(|_| ProfileError::Invalid("a business day policy that doesn't exist"))?;
        // A time zone is found whatever the case of its name, and read back in the database's:
        // `america/new_york` would encode as `America/New_York`. Each of the database's names
        // for a zone, such as `US/Eastern` for New York's, is a name of its own.
        if business_day.time_zone() != data.business_day.time_zone {
            return Err(ProfileError::Invalid("a time zone not named as the database names it"));
        }
        // Absent, cash isn't rounded: rounding to the minor unit would be a second way to say so.
        if data.cash_rounding.is_some_and(|rule| rule.increment().get() == 1) {
            return Err(ProfileError::Invalid("cash rounded to the minor unit, which rounds none"));
        }
        if !data.currency.is_circulating() {
            return Err(ProfileError::Invalid("a currency that isn't circulating"));
        }
        if data.address.len() > MAX_ADDRESS {
            return Err(ProfileError::Invalid("more than four address lines"));
        }
        rules::check(&data.rules)?;
        let index = check_catalog(&data.catalog, data.currency)?;
        check_menu(&data.menu, &index)?;
        check_team(&data.team)?;
        let catalog_version =
            CatalogVersion::from_bytes(sha256(&catalog_value(&data.catalog).encode()));
        let rules_version =
            RulesVersion::from_bytes(sha256(&rules::to_value(&data.rules).encode()));
        Ok(Profile { data, business_day, index, catalog_version, rules_version })
    }

    /// The profile `bytes` encode, checked.
    ///
    /// # Errors
    /// [`ProfileError`] if `bytes` aren't a profile of format [`FORMAT`] in canonical CBOR, or
    /// the profile breaks a rule ([`Profile::new`]).
    pub fn decode(bytes: &[u8]) -> Result<Profile, ProfileError> {
        let value = cbor::decode(bytes)?;
        let mut fields = Fields::read(&value)?;
        let format: u64 = fields.required(key::FORMAT, "profile format")?;
        if format != FORMAT {
            return Err(ProfileError::Format(format));
        }
        let business_day: BusinessDay = fields.required(key::BUSINESS_DAY, "business day")?;
        let rules = fields.required::<RulesField>(key::RULES, "pricing rules")?.0;
        let catalog = fields.required::<CatalogField>(key::CATALOG, "catalog")?.0;
        let data = ProfileData {
            location: fields.required(key::LOCATION, "location")?,
            name: fields.required(key::NAME, "location name")?,
            address: fields.required(key::ADDRESS, "address")?,
            currency: fields.required(key::CURRENCY, "currency")?,
            business_day,
            cash_rounding: fields
                .optional::<RoundingRuleField>(key::CASH_ROUNDING, "cash rounding")?
                .map(|rule| rule.0),
            rules,
            catalog,
            menu: fields.required(key::MENU, "menu")?,
            team: fields.required(key::TEAM, "team")?,
        };
        fields.finish()?;
        Profile::new(data)
    }

    /// The profile's canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let data = &self.data;
        Record::default()
            .field(key::FORMAT, &FORMAT)
            .field(key::LOCATION, &data.location)
            .field(key::NAME, &data.name)
            .field(key::ADDRESS, &data.address)
            .field(key::CURRENCY, &data.currency)
            .field(key::BUSINESS_DAY, &data.business_day)
            .optional(key::CASH_ROUNDING, data.cash_rounding.map(RoundingRuleField).as_ref())
            .field(key::RULES, &Encoded(rules::to_value(&data.rules)))
            .field(key::CATALOG, &Encoded(catalog_value(&data.catalog)))
            .field(key::MENU, &data.menu)
            .field(key::TEAM, &data.team)
            .build()
            .encode()
    }

    /// What the profile holds.
    pub const fn data(&self) -> &ProfileData {
        &self.data
    }

    /// When the location's business days begin.
    pub const fn business_day(&self) -> &BusinessDayPolicy {
        &self.business_day
    }

    /// The catalog's version: the SHA-256 hash of its encoding.
    pub const fn catalog_version(&self) -> CatalogVersion {
        self.catalog_version
    }

    /// The pricing rules' version: the SHA-256 hash of their encoding.
    pub const fn rules_version(&self) -> RulesVersion {
        self.rules_version
    }

    /// What `variant`, with the modifiers in `choices`, rings as (ADR-0023, decision 4).
    ///
    /// # Errors
    /// [`RingError`] if the catalog has no such variant, or the choices break its modifier
    /// groups' rules.
    pub fn ring(&self, variant: Id<Variant>, choices: &[Choice]) -> Result<Rung, RingError> {
        self.data.catalog.ring(&self.index, self.catalog_version, variant, choices)
    }

    /// The item `variant` belongs to, and the variant.
    pub fn variant(&self, variant: Id<Variant>) -> Option<(&CatalogItem, &CatalogVariant)> {
        let &(item, position, _) = self.index.variants.get(&variant)?;
        let item = self.data.catalog.items.get(item)?;
        Some((item, item.variants.get(position)?))
    }

    /// The name a line shows for `variant`, such as "Latte (Large)".
    pub fn variant_name(&self, variant: Id<Variant>) -> Option<&Name> {
        self.index.variants.get(&variant).map(|(_, _, name)| name)
    }

    /// The modifier group `group`.
    pub fn group(&self, group: Id<ModifierGroup>) -> Option<&CatalogGroup> {
        self.data.catalog.groups.get(*self.index.groups.get(&group)?)
    }

    /// The team member `member`, if the team has them.
    pub fn member(&self, member: Id<TeamMember>) -> Option<&Member> {
        self.data.team.iter().find(|candidate| candidate.id == member)
    }
}

/// Checks the catalog, and indexes it for ringing.
fn check_catalog(catalog: &Catalog, currency: Currency) -> Result<Index, ProfileError> {
    if catalog.items.len() > MAX_ITEMS {
        return Err(ProfileError::Invalid("more items than a catalog holds"));
    }
    if catalog.groups.len() > MAX_GROUPS {
        return Err(ProfileError::Invalid("more modifier groups than a catalog holds"));
    }
    let priced = |price: keel_types::Money| {
        if price.currency() != currency {
            Err(ProfileError::Invalid("a price in another currency than the location's"))
        } else if price.is_negative() {
            Err(ProfileError::Invalid("a negative price"))
        } else {
            Ok(())
        }
    };
    let variants = catalog.items.iter().map(|item| item.variants.len());
    if variants.fold(0_usize, usize::saturating_add) > MAX_ALL_VARIANTS {
        return Err(ProfileError::Invalid("more variants than a catalog holds"));
    }
    let modifiers = catalog.groups.iter().map(|group| group.modifiers.len());
    if modifiers.fold(0_usize, usize::saturating_add) > MAX_ALL_MODIFIERS {
        return Err(ProfileError::Invalid("more modifiers than a catalog holds"));
    }
    let mut index = Index::default();
    for (position, group) in catalog.groups.iter().enumerate() {
        if index.groups.insert(group.id, position).is_some() {
            return Err(ProfileError::Duplicate("modifier groups"));
        }
        if group.modifiers.is_empty() {
            return Err(ProfileError::Invalid("a modifier group with no modifiers"));
        }
        if group.modifiers.len() > MAX_PER_GROUP {
            return Err(ProfileError::Invalid("more modifiers in a group than it holds"));
        }
        if group.min > group.max.get() {
            return Err(ProfileError::Invalid(
                "a modifier group whose minimum is over its maximum",
            ));
        }
        if group.free > group.max.get() {
            return Err(ProfileError::Invalid("a modifier group with more free than its maximum"));
        }
        if !group.repeat && usize::from(group.min) > group.modifiers.len() {
            return Err(ProfileError::Invalid("a modifier group whose minimum can't be met"));
        }
        for (at, modifier) in group.modifiers.iter().enumerate() {
            if index.modifiers.insert(modifier.id, (position, at)).is_some() {
                return Err(ProfileError::Duplicate("modifiers"));
            }
            priced(modifier.price)?;
            offered(&modifier.groups)?;
        }
    }
    for (position, item) in catalog.items.iter().enumerate() {
        if item.variants.is_empty() {
            return Err(ProfileError::Invalid("an item with no variants"));
        }
        if item.variants.len() > MAX_VARIANTS {
            return Err(ProfileError::Invalid("more variants of an item than it holds"));
        }
        offered(&item.groups)?;
        let mut names = BTreeSet::new();
        for (at, variant) in item.variants.iter().enumerate() {
            priced(variant.price)?;
            if item.variants.len() > 1
                && !variant.name.as_ref().is_some_and(|name| names.insert(name.clone()))
            {
                return Err(ProfileError::Invalid(
                    "an item with several variants, one of them unnamed or named twice",
                ));
            }
            let name = full_name(&item.name, variant.name.as_ref())
                .ok_or(ProfileError::Invalid("a variant whose full name is too long"))?;
            if index.variants.insert(variant.id, (position, at, name)).is_some() {
                return Err(ProfileError::Duplicate("variants"));
            }
        }
    }
    let mut items = BTreeSet::new();
    for item in &catalog.items {
        if !items.insert(item.id) {
            return Err(ProfileError::Duplicate("items"));
        }
    }
    check_nesting(catalog, &index)?;
    Ok(index)
}

/// Checks a list of groups offered together: within its limit, each once.
fn offered(groups: &[Id<ModifierGroup>]) -> Result<(), ProfileError> {
    if groups.len() > MAX_PER_GROUP {
        return Err(ProfileError::Invalid("more modifier groups offered than allowed"));
    }
    let distinct: BTreeSet<_> = groups.iter().collect();
    if distinct.len() == groups.len() {
        Ok(())
    } else {
        Err(ProfileError::Invalid("a modifier group offered twice in one place"))
    }
}

/// Checks that every group offered exists, and that groups nest at most [`MAX_DEPTH`] deep and
/// never within themselves.
fn check_nesting(catalog: &Catalog, index: &Index) -> Result<(), ProfileError> {
    // Each group's depth below it: 1 for a group whose modifiers offer none, and more for each
    // level under it, worked out from the groups with none upwards. A group in a cycle never
    // gets one.
    let mut height: BTreeMap<Id<ModifierGroup>, usize> = BTreeMap::new();
    let under = |group: &CatalogGroup| -> Vec<Id<ModifierGroup>> {
        group.modifiers.iter().flat_map(|modifier| modifier.groups.iter().copied()).collect()
    };
    for group in &catalog.groups {
        for id in under(group) {
            if !index.groups.contains_key(&id) {
                return Err(ProfileError::Unknown("modifier group"));
            }
        }
    }
    for item in &catalog.items {
        if item.groups.iter().any(|id| !index.groups.contains_key(id)) {
            return Err(ProfileError::Unknown("modifier group"));
        }
    }
    loop {
        let mut progressed = false;
        for group in &catalog.groups {
            if height.contains_key(&group.id) {
                continue;
            }
            let below: Option<Vec<usize>> =
                under(group).iter().map(|id| height.get(id).copied()).collect();
            if let Some(below) = below {
                let depth = below.into_iter().max().unwrap_or(0).saturating_add(1);
                if depth > MAX_DEPTH {
                    return Err(ProfileError::Invalid("modifier groups nested too deep"));
                }
                height.insert(group.id, depth);
                progressed = true;
            }
        }
        if height.len() == catalog.groups.len() {
            return Ok(());
        }
        if !progressed {
            return Err(ProfileError::Invalid("a modifier group nested within itself"));
        }
    }
}

/// Checks the menu's pages.
fn check_menu(menu: &Menu, index: &Index) -> Result<(), ProfileError> {
    if menu.pages.len() > MAX_PAGES {
        return Err(ProfileError::Invalid("more menu pages than allowed"));
    }
    for page in &menu.pages {
        if page.buttons.is_empty() {
            return Err(ProfileError::Invalid("a menu page with no buttons"));
        }
        if page.buttons.len() > MAX_BUTTONS {
            return Err(ProfileError::Invalid("more buttons on a page than allowed"));
        }
        if page.buttons.iter().any(|variant| !index.variants.contains_key(variant)) {
            return Err(ProfileError::Unknown("variant"));
        }
    }
    Ok(())
}

/// Checks the team.
fn check_team(team: &[Member]) -> Result<(), ProfileError> {
    if team.is_empty() {
        return Err(ProfileError::Invalid("no team members"));
    }
    if team.len() > MAX_TEAM {
        return Err(ProfileError::Invalid("more team members than allowed"));
    }
    let ids: BTreeSet<_> = team.iter().map(|member| member.id).collect();
    if ids.len() == team.len() { Ok(()) } else { Err(ProfileError::Duplicate("team members")) }
}

/// Profile keys.
mod key {
    pub(super) const FORMAT: u64 = 1;
    pub(super) const LOCATION: u64 = 2;
    pub(super) const NAME: u64 = 3;
    pub(super) const ADDRESS: u64 = 4;
    pub(super) const CURRENCY: u64 = 5;
    pub(super) const BUSINESS_DAY: u64 = 6;
    pub(super) const CASH_ROUNDING: u64 = 7;
    pub(super) const RULES: u64 = 8;
    pub(super) const CATALOG: u64 = 9;
    pub(super) const MENU: u64 = 10;
    pub(super) const TEAM: u64 = 11;
}

impl Field for u64 {
    fn to_value(&self) -> Value {
        Value::Unsigned(*self)
    }

    fn from_value(value: &Value) -> Option<u64> {
        value.as_u64()
    }
}

impl Field for BusinessDay {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::from(self.time_zone.as_str()),
            Value::Unsigned(u64::from(self.cutoff_hour)),
            Value::Unsigned(u64::from(self.cutoff_minute)),
        ])
    }

    fn from_value(value: &Value) -> Option<BusinessDay> {
        let [time_zone, hour, minute] = value.as_array()? else { return None };
        Some(BusinessDay {
            time_zone: time_zone.as_text()?.to_owned(),
            cutoff_hour: u8::try_from(hour.as_u64()?).ok()?,
            cutoff_minute: u8::try_from(minute.as_u64()?).ok()?,
        })
    }
}

/// A rounding rule, as a profile writes it.
struct RoundingRuleField(RoundingRule);

impl Field for RoundingRuleField {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            rules::mode_value(self.0.mode()),
            Value::Unsigned(u64::from(self.0.increment().get())),
        ])
    }

    fn from_value(value: &Value) -> Option<RoundingRuleField> {
        let [mode, increment] = value.as_array()? else { return None };
        let increment = NonZeroU32::new(u32::try_from(increment.as_u64()?).ok()?)?;
        Some(RoundingRuleField(RoundingRule::new(rules::mode_from(mode)?, increment)))
    }
}

/// A part of the profile, already encoded.
struct Encoded(Value);

impl Field for Encoded {
    fn to_value(&self) -> Value {
        self.0.clone()
    }

    fn from_value(value: &Value) -> Option<Encoded> {
        Some(Encoded(value.clone()))
    }
}

/// The pricing rules, as a profile writes them.
struct RulesField(Rules);

impl Field for RulesField {
    fn to_value(&self) -> Value {
        rules::to_value(&self.0)
    }

    fn from_value(value: &Value) -> Option<RulesField> {
        rules::from_value(value).map(RulesField)
    }
}

/// The catalog, as a profile writes it.
struct CatalogField(Catalog);

impl Field for CatalogField {
    fn to_value(&self) -> Value {
        catalog_value(&self.0)
    }

    fn from_value(value: &Value) -> Option<CatalogField> {
        let mut fields = Fields::read(value).ok()?;
        let catalog = Catalog {
            items: fields.required(catalog_key::ITEMS, "items").ok()?,
            groups: fields.required(catalog_key::GROUPS, "modifier groups").ok()?,
        };
        fields.finish().ok()?;
        Some(CatalogField(catalog))
    }
}

/// The catalog's encoding, which its version hashes.
fn catalog_value(catalog: &Catalog) -> Value {
    Record::default()
        .field(catalog_key::ITEMS, &catalog.items)
        .field(catalog_key::GROUPS, &catalog.groups)
        .build()
}

/// Catalog keys: the catalog's, and its records'.
mod catalog_key {
    pub(super) const ITEMS: u64 = 1;
    pub(super) const GROUPS: u64 = 2;

    pub(super) const ID: u64 = 1;
    pub(super) const NAME: u64 = 2;

    pub(super) const ITEM_TAX_CATEGORY: u64 = 3;
    pub(super) const ITEM_VARIANTS: u64 = 4;
    pub(super) const ITEM_GROUPS: u64 = 5;

    pub(super) const VARIANT_PRICE: u64 = 3;

    pub(super) const GROUP_MIN: u64 = 3;
    pub(super) const GROUP_MAX: u64 = 4;
    pub(super) const GROUP_FREE: u64 = 5;
    pub(super) const GROUP_REPEAT: u64 = 6;
    pub(super) const GROUP_MODIFIERS: u64 = 7;

    pub(super) const MODIFIER_PREFIX: u64 = 3;
    pub(super) const MODIFIER_PRICE: u64 = 4;
    pub(super) const MODIFIER_GROUPS: u64 = 5;
}

impl Field for CatalogItem {
    fn to_value(&self) -> Value {
        Record::default()
            .field(catalog_key::ID, &self.id)
            .field(catalog_key::NAME, &self.name)
            .field(catalog_key::ITEM_TAX_CATEGORY, &self.tax_category)
            .field(catalog_key::ITEM_VARIANTS, &self.variants)
            .field(catalog_key::ITEM_GROUPS, &self.groups)
            .build()
    }

    fn from_value(value: &Value) -> Option<CatalogItem> {
        let mut fields = Fields::read(value).ok()?;
        let item = CatalogItem {
            id: fields.required(catalog_key::ID, "item").ok()?,
            name: fields.required(catalog_key::NAME, "item name").ok()?,
            tax_category: fields.required(catalog_key::ITEM_TAX_CATEGORY, "tax category").ok()?,
            variants: fields.required(catalog_key::ITEM_VARIANTS, "variants").ok()?,
            groups: fields.required(catalog_key::ITEM_GROUPS, "modifier groups").ok()?,
        };
        fields.finish().ok()?;
        Some(item)
    }
}

impl Field for CatalogVariant {
    fn to_value(&self) -> Value {
        Record::default()
            .field(catalog_key::ID, &self.id)
            .optional(catalog_key::NAME, self.name.as_ref())
            .field(catalog_key::VARIANT_PRICE, &self.price)
            .build()
    }

    fn from_value(value: &Value) -> Option<CatalogVariant> {
        let mut fields = Fields::read(value).ok()?;
        let variant = CatalogVariant {
            id: fields.required(catalog_key::ID, "variant").ok()?,
            name: fields.optional(catalog_key::NAME, "variant name").ok()?,
            price: fields.required(catalog_key::VARIANT_PRICE, "price").ok()?,
        };
        fields.finish().ok()?;
        Some(variant)
    }
}

impl Field for CatalogGroup {
    fn to_value(&self) -> Value {
        Record::default()
            .field(catalog_key::ID, &self.id)
            .field(catalog_key::NAME, &self.name)
            .optional(catalog_key::GROUP_MIN, NonZeroU8::new(self.min).as_ref())
            .field(catalog_key::GROUP_MAX, &self.max)
            .optional(catalog_key::GROUP_FREE, NonZeroU8::new(self.free).as_ref())
            .field(catalog_key::GROUP_REPEAT, &self.repeat)
            .field(catalog_key::GROUP_MODIFIERS, &self.modifiers)
            .build()
    }

    fn from_value(value: &Value) -> Option<CatalogGroup> {
        let mut fields = Fields::read(value).ok()?;
        let count = |count: Option<NonZeroU8>| count.map_or(0, NonZeroU8::get);
        let group = CatalogGroup {
            id: fields.required(catalog_key::ID, "modifier group").ok()?,
            name: fields.required(catalog_key::NAME, "modifier group name").ok()?,
            min: count(fields.optional(catalog_key::GROUP_MIN, "minimum").ok()?),
            max: fields.required(catalog_key::GROUP_MAX, "maximum").ok()?,
            free: count(fields.optional(catalog_key::GROUP_FREE, "free applications").ok()?),
            repeat: fields.required(catalog_key::GROUP_REPEAT, "repeat").ok()?,
            modifiers: fields.required(catalog_key::GROUP_MODIFIERS, "modifiers").ok()?,
        };
        fields.finish().ok()?;
        Some(group)
    }
}

impl Field for CatalogModifier {
    fn to_value(&self) -> Value {
        Record::default()
            .field(catalog_key::ID, &self.id)
            .field(catalog_key::NAME, &self.name)
            .field(catalog_key::MODIFIER_PREFIX, &self.prefix)
            .field(catalog_key::MODIFIER_PRICE, &self.price)
            .field(catalog_key::MODIFIER_GROUPS, &self.groups)
            .build()
    }

    fn from_value(value: &Value) -> Option<CatalogModifier> {
        let mut fields = Fields::read(value).ok()?;
        let modifier = CatalogModifier {
            id: fields.required(catalog_key::ID, "modifier").ok()?,
            name: fields.required(catalog_key::NAME, "modifier name").ok()?,
            prefix: fields.required(catalog_key::MODIFIER_PREFIX, "prefix").ok()?,
            price: fields.required(catalog_key::MODIFIER_PRICE, "price").ok()?,
            groups: fields.required(catalog_key::MODIFIER_GROUPS, "modifier groups").ok()?,
        };
        fields.finish().ok()?;
        Some(modifier)
    }
}

impl Field for Menu {
    fn to_value(&self) -> Value {
        Record::default().field(1, &self.pages).build()
    }

    fn from_value(value: &Value) -> Option<Menu> {
        let mut fields = Fields::read(value).ok()?;
        let menu = Menu { pages: fields.required(1, "pages").ok()? };
        fields.finish().ok()?;
        Some(menu)
    }
}

impl Field for Page {
    fn to_value(&self) -> Value {
        Record::default().field(1, &self.name).field(2, &self.buttons).build()
    }

    fn from_value(value: &Value) -> Option<Page> {
        let mut fields = Fields::read(value).ok()?;
        let page = Page {
            name: fields.required(1, "page name").ok()?,
            buttons: fields.required(2, "buttons").ok()?,
        };
        fields.finish().ok()?;
        Some(page)
    }
}

impl Field for Member {
    fn to_value(&self) -> Value {
        Record::default().field(1, &self.id).field(2, &self.name).build()
    }

    fn from_value(value: &Value) -> Option<Member> {
        let mut fields = Fields::read(value).ok()?;
        let member = Member {
            id: fields.required(1, "team member").ok()?,
            name: fields.required(2, "team member name").ok()?,
        };
        fields.finish().ok()?;
        Some(member)
    }
}
