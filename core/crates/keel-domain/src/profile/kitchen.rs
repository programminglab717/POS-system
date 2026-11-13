//! The kitchen of a location profile, format 3 (ADR-0034): its stations, the routes that send
//! lines to them, how long a ticket takes to be late, and the printer tickets fall back to; and
//! the allergens a catalog gives its items and modifiers.
//!
//! | Record | Keys |
//! |---|---|
//! | kitchen | 1 stations (at least one), 2 routes, 3 default station (optional), 4 `[seconds until a ticket is late-ish, seconds until it is late]`, 5 fallback printer (optional) |
//! | station | 1 identifier, 2 name, 3 expo, 4 screen, 5 printer (optional) |
//! | route | 1 station, 2 items (a set, optional), 3 modifiers (a set, optional), 4 order modes (a set, optional: absent for all) |
//! | printer | 1 host, 2 port |
//!
//! A route names at least one item or modifier. A set is in the ascending order of its members'
//! bytes or codes, and never empty: an empty one is absent.

use core::num::{NonZeroU16, NonZeroU32};
use std::collections::BTreeSet;

use keel_events::cbor::Value;
use keel_types::Id;

use super::ProfileError;
use super::catalog::{Catalog, Index};
use crate::codec::{CodeSet, Field, Fields, IdSet, Name, Record, code_enum};
use crate::order::{ChosenModifier, Mode};
use crate::refs::{Item, Modifier, Station as StationRef, Variant};

/// The most stations a kitchen has.
pub const MAX_STATIONS: usize = 64;
/// The most routes a kitchen has.
pub const MAX_ROUTES: usize = 10_000;
/// The most items, or modifiers, a route names.
pub const MAX_ROUTED: usize = 10_000;
/// The longest a printer's host is, in bytes.
pub const MAX_HOST: usize = 253;

code_enum! {
    /// An allergen a catalog's item or modifier contains: the fourteen the EU requires named,
    /// which include the nine the US does (ADR-0034).
    pub enum Allergen {
        /// Cereals containing gluten.
        Gluten = 0,
        /// Crustaceans.
        Crustaceans = 1,
        /// Eggs.
        Eggs = 2,
        /// Fish.
        Fish = 3,
        /// Peanuts.
        Peanuts = 4,
        /// Soy.
        Soy = 5,
        /// Milk.
        Milk = 6,
        /// Tree nuts.
        TreeNuts = 7,
        /// Celery.
        Celery = 8,
        /// Mustard.
        Mustard = 9,
        /// Sesame.
        Sesame = 10,
        /// Sulphites.
        Sulphites = 11,
        /// Lupin.
        Lupin = 12,
        /// Molluscs.
        Molluscs = 13,
    }
}

impl Allergen {
    /// Its name, as a kitchen ticket shows it.
    pub const fn name(self) -> &'static str {
        match self {
            Allergen::Gluten => "gluten",
            Allergen::Crustaceans => "crustaceans",
            Allergen::Eggs => "eggs",
            Allergen::Fish => "fish",
            Allergen::Peanuts => "peanuts",
            Allergen::Soy => "soy",
            Allergen::Milk => "milk",
            Allergen::TreeNuts => "tree nuts",
            Allergen::Celery => "celery",
            Allergen::Mustard => "mustard",
            Allergen::Sesame => "sesame",
            Allergen::Sulphites => "sulphites",
            Allergen::Lupin => "lupin",
            Allergen::Molluscs => "molluscs",
        }
    }
}

/// A location's kitchen: its stations, and how lines are routed to them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kitchen {
    /// Its stations, in the order a screen lists them: at least one.
    pub stations: Vec<Station>,
    /// The routes that send lines to stations.
    pub routes: Vec<Route>,
    /// The station a line goes to when no route names it, not an expo station; with none, such
    /// a line goes to no station but expo, as bottled water goes to no cook.
    pub default_station: Option<Id<StationRef>>,
    /// How long a ticket waits before it ages.
    pub aging: Aging,
    /// Where a ticket prints when no screen showed it and its station has no printer.
    pub fallback_printer: Option<Printer>,
}

/// A kitchen station, such as the grill, or expo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Station {
    /// Its identifier.
    pub id: Id<StationRef>,
    /// Its name, such as "Grill".
    pub name: Name,
    /// Whether it is an expo station: it gets every line of every order, to hand it off.
    pub expo: bool,
    /// Whether a screen shows its tickets; one that isn't shown prints them at once.
    pub screen: bool,
    /// Its printer, where its tickets print when no screen shows them.
    pub printer: Option<Printer>,
}

/// A route: the station that makes some items, or some modifiers, in some order modes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// The station.
    pub station: Id<StationRef>,
    /// The items it makes, whatever their variant.
    pub items: BTreeSet<Id<Item>>,
    /// The modifiers it makes, on their own, as components of a line.
    pub modifiers: BTreeSet<Id<Modifier>>,
    /// The order modes it applies to; empty for every mode.
    pub modes: BTreeSet<Mode>,
}

/// How long a ticket waits before it ages, from when it was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aging {
    /// Seconds until it is late-ish.
    pub late_ish: NonZeroU32,
    /// Seconds until it is late: more than `late_ish`.
    pub late: NonZeroU32,
}

/// A network printer: where it listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printer {
    /// Its host: a name or an address, of printable ASCII and no spaces.
    pub host: String,
    /// Its port.
    pub port: NonZeroU16,
}

/// Where a line goes: a station, and the line whole or one of its modifiers as a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routed {
    /// The station.
    pub station: Id<StationRef>,
    /// The modifier sent on its own, as a component of the line; `None` for the line whole.
    pub component: Option<Id<Modifier>>,
}

impl Kitchen {
    /// The station `station`, if the kitchen has it.
    pub fn station(&self, station: Id<StationRef>) -> Option<&Station> {
        self.stations.iter().find(|candidate| candidate.id == station)
    }

    /// Where a line of `item` with `modifiers`, in an order of `mode`, goes (ADR-0034, decision
    /// 2): to every station a route names for its item in that mode, or the default station, if
    /// there is one, when none does; each modifier, at any depth, that a route names for a station the line doesn't
    /// go to whole, to that station on its own; and to every expo station whole. In the order of
    /// the kitchen's stations, the line whole before its components, its components in the order
    /// of its modifiers.
    pub fn route(
        &self,
        mode: Mode,
        item: Option<Id<Item>>,
        modifiers: &[ChosenModifier],
    ) -> Vec<Routed> {
        let applies = |route: &&Route| route.modes.is_empty() || route.modes.contains(&mode);
        let mut whole: BTreeSet<Id<StationRef>> = self
            .routes
            .iter()
            .filter(applies)
            .filter(|route| item.is_some_and(|item| route.items.contains(&item)))
            .map(|route| route.station)
            .collect();
        if whole.is_empty()
            && let Some(default) = self.default_station
        {
            whole.insert(default);
        }
        whole.extend(self.stations.iter().filter(|station| station.expo).map(|station| station.id));
        let mut chosen = Vec::new();
        flatten(modifiers, &mut chosen);
        let mut routed = Vec::new();
        for station in &self.stations {
            if whole.contains(&station.id) {
                routed.push(Routed { station: station.id, component: None });
                continue;
            }
            let mut sent: BTreeSet<Id<Modifier>> = BTreeSet::new();
            for &modifier in &chosen {
                let named = self.routes.iter().filter(applies).any(|route| {
                    route.station == station.id && route.modifiers.contains(&modifier)
                });
                if named && sent.insert(modifier) {
                    routed.push(Routed { station: station.id, component: Some(modifier) });
                }
            }
        }
        routed
    }
}

/// The modifiers of `modifiers`, each before those under it.
fn flatten(modifiers: &[ChosenModifier], out: &mut Vec<Id<Modifier>>) {
    for modifier in modifiers {
        out.push(modifier.modifier);
        flatten(&modifier.modifiers, out);
    }
}

/// Checks `kitchen` against `catalog`, indexed as `index`.
pub(super) fn check(
    kitchen: &Kitchen,
    catalog: &Catalog,
    index: &Index,
) -> Result<(), ProfileError> {
    if kitchen.stations.is_empty() {
        return Err(ProfileError::Invalid("a kitchen with no stations"));
    }
    if kitchen.stations.len() > MAX_STATIONS {
        return Err(ProfileError::Invalid("more stations than a kitchen holds"));
    }
    if kitchen.routes.len() > MAX_ROUTES {
        return Err(ProfileError::Invalid("more routes than a kitchen holds"));
    }
    let mut ids = BTreeSet::new();
    for station in &kitchen.stations {
        if !ids.insert(station.id) {
            return Err(ProfileError::Duplicate("stations"));
        }
        if !station.screen && station.printer.is_none() {
            return Err(ProfileError::Invalid("a station no screen shows and that prints nowhere"));
        }
        if let Some(printer) = &station.printer {
            check_printer(printer)?;
        }
    }
    if let Some(printer) = &kitchen.fallback_printer {
        check_printer(printer)?;
    }
    if let Some(default) = kitchen.default_station {
        let station = kitchen.station(default).ok_or(ProfileError::Unknown("station"))?;
        if station.expo {
            return Err(ProfileError::Invalid("an expo station as the default"));
        }
    }
    if kitchen.aging.late_ish >= kitchen.aging.late {
        return Err(ProfileError::Invalid("a ticket late before it is late-ish"));
    }
    let items: BTreeSet<Id<Item>> = catalog.items.iter().map(|item| item.id).collect();
    for route in &kitchen.routes {
        let station = kitchen.station(route.station).ok_or(ProfileError::Unknown("station"))?;
        if station.expo {
            return Err(ProfileError::Invalid("a route to an expo station, which gets every line"));
        }
        if route.items.is_empty() && route.modifiers.is_empty() {
            return Err(ProfileError::Invalid("a route that names nothing"));
        }
        if route.items.len() > MAX_ROUTED || route.modifiers.len() > MAX_ROUTED {
            return Err(ProfileError::Invalid("more items or modifiers than a route names"));
        }
        if !route.items.iter().all(|item| items.contains(item)) {
            return Err(ProfileError::Unknown("item"));
        }
        if !route.modifiers.iter().all(|modifier| index.modifiers.contains_key(modifier)) {
            return Err(ProfileError::Unknown("modifier"));
        }
    }
    Ok(())
}

fn check_printer(printer: &Printer) -> Result<(), ProfileError> {
    let printable = printer.host.bytes().all(|byte| byte.is_ascii_graphic());
    if printer.host.is_empty() || printer.host.len() > MAX_HOST || !printable {
        return Err(ProfileError::Invalid("a printer's host that isn't a name or an address"));
    }
    Ok(())
}

/// The item `variant` belongs to, in `catalog`, indexed as `index`.
pub(super) fn item_of(catalog: &Catalog, index: &Index, variant: Id<Variant>) -> Option<Id<Item>> {
    let &(item, _, _) = index.variants.get(&variant)?;
    catalog.items.get(item).map(|item| item.id)
}

/// Kitchen keys.
mod key {
    pub(super) const STATIONS: u64 = 1;
    pub(super) const ROUTES: u64 = 2;
    pub(super) const DEFAULT: u64 = 3;
    pub(super) const AGING: u64 = 4;
    pub(super) const FALLBACK: u64 = 5;

    pub(super) const ID: u64 = 1;
    pub(super) const NAME: u64 = 2;
    pub(super) const EXPO: u64 = 3;
    pub(super) const SCREEN: u64 = 4;
    pub(super) const PRINTER: u64 = 5;

    pub(super) const STATION: u64 = 1;
    pub(super) const ITEMS: u64 = 2;
    pub(super) const MODIFIERS: u64 = 3;
    pub(super) const MODES: u64 = 4;

    pub(super) const HOST: u64 = 1;
    pub(super) const PORT: u64 = 2;
}

impl Field for Kitchen {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::STATIONS, &self.stations)
            .field(key::ROUTES, &self.routes)
            .optional(key::DEFAULT, self.default_station.as_ref())
            .field(key::AGING, &self.aging)
            .optional(key::FALLBACK, self.fallback_printer.as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<Kitchen> {
        let mut fields = Fields::read(value).ok()?;
        let kitchen = Kitchen {
            stations: fields.required(key::STATIONS, "stations").ok()?,
            routes: fields.required(key::ROUTES, "routes").ok()?,
            default_station: fields.optional(key::DEFAULT, "default station").ok()?,
            aging: fields.required(key::AGING, "aging").ok()?,
            fallback_printer: fields.optional(key::FALLBACK, "fallback printer").ok()?,
        };
        fields.finish().ok()?;
        Some(kitchen)
    }
}

impl Field for Station {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::ID, &self.id)
            .field(key::NAME, &self.name)
            .field(key::EXPO, &self.expo)
            .field(key::SCREEN, &self.screen)
            .optional(key::PRINTER, self.printer.as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<Station> {
        let mut fields = Fields::read(value).ok()?;
        let station = Station {
            id: fields.required(key::ID, "station").ok()?,
            name: fields.required(key::NAME, "station name").ok()?,
            expo: fields.required(key::EXPO, "expo").ok()?,
            screen: fields.required(key::SCREEN, "screen").ok()?,
            printer: fields.optional(key::PRINTER, "printer").ok()?,
        };
        fields.finish().ok()?;
        Some(station)
    }
}

impl Field for Route {
    fn to_value(&self) -> Value {
        let items = IdSet::new(self.items.iter().copied()).ok();
        let modifiers = IdSet::new(self.modifiers.iter().copied()).ok();
        let modes = (!self.modes.is_empty()).then(|| CodeSet(self.modes.clone()));
        Record::default()
            .field(key::STATION, &self.station)
            .optional(key::ITEMS, items.as_ref())
            .optional(key::MODIFIERS, modifiers.as_ref())
            .optional(key::MODES, modes.as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<Route> {
        let mut fields = Fields::read(value).ok()?;
        let items: Option<IdSet<Item>> = fields.optional(key::ITEMS, "items").ok()?;
        let modifiers: Option<IdSet<Modifier>> =
            fields.optional(key::MODIFIERS, "modifiers").ok()?;
        let modes: Option<CodeSet<Mode>> = fields.optional(key::MODES, "order modes").ok()?;
        let route = Route {
            station: fields.required(key::STATION, "station").ok()?,
            items: items.map(|set| set.iter().collect()).unwrap_or_default(),
            modifiers: modifiers.map(|set| set.iter().collect()).unwrap_or_default(),
            modes: modes.map(|set| set.0).unwrap_or_default(),
        };
        fields.finish().ok()?;
        Some(route)
    }
}

impl Field for Aging {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Unsigned(u64::from(self.late_ish.get())),
            Value::Unsigned(u64::from(self.late.get())),
        ])
    }

    fn from_value(value: &Value) -> Option<Aging> {
        let [late_ish, late] = value.as_array()? else { return None };
        let seconds = |value: &Value| NonZeroU32::new(u32::try_from(value.as_u64()?).ok()?);
        Some(Aging { late_ish: seconds(late_ish)?, late: seconds(late)? })
    }
}

impl Field for Printer {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::HOST, &HostField(self.host.clone()))
            .field(key::PORT, &self.port)
            .build()
    }

    fn from_value(value: &Value) -> Option<Printer> {
        let mut fields = Fields::read(value).ok()?;
        let host: HostField = fields.required(key::HOST, "host").ok()?;
        let printer = Printer { host: host.0, port: fields.required(key::PORT, "port").ok()? };
        fields.finish().ok()?;
        Some(printer)
    }
}

/// A printer's host, as text.
struct HostField(String);

impl Field for HostField {
    fn to_value(&self) -> Value {
        Value::from(self.0.as_str())
    }

    fn from_value(value: &Value) -> Option<HostField> {
        value.as_text().map(|text| HostField(text.to_owned()))
    }
}
