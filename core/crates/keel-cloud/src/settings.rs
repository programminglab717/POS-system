//! A location's settings as the back office reads and writes them (ADR-0037, decision 1): its
//! name and address, business day, cash rounding, taxes and how prices round, tipping, and the
//! kitchen. A write replaces those and keeps the catalog, menu, team and roles, checked whole by
//! the kernel. The location and its currency are read, never written: a new currency is a new
//! location.

use core::num::{NonZeroU16, NonZeroU32};
use std::collections::BTreeSet;

use keel_domain::codec::{Name, ProfileVersion};
use keel_domain::order::Mode;
use keel_domain::profile::{
    Aging, BusinessDay, Kitchen, Printer, Profile, Route, Station, TipPrompt, Tipping,
};
use keel_pricing::{Dining, Tax, TaxRounding, TaxScope};
use keel_types::{
    BusinessDateError, BusinessDayPolicy, Decimal, Id, Rate, RoundingMode, RoundingRule,
};
use serde::{Deserialize, Serialize};

use crate::catalog::hex;

/// A location's settings, as read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// The version of the profile they are part of.
    pub version: String,
    /// The location: never written.
    pub location: String,
    /// Its currency, ISO 4217: never written.
    pub currency: String,
    /// What can be written.
    #[serde(flatten)]
    pub settings: Settings,
}

/// What a location's settings are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    /// Its name, as receipts show it.
    pub name: String,
    /// Its address, a line each.
    pub address: Vec<String>,
    /// Its IANA time zone, such as `America/New_York`.
    pub time_zone: String,
    /// When its business days begin, local time, `HH:MM`.
    pub cutoff: String,
    /// How cash is rounded, if it is: to 5 cents, say. Absent for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cash_rounding: Option<CashRoundingJson>,
    /// How prices round.
    pub rounding: RoundingJson,
    /// Its taxes.
    pub taxes: Vec<TaxJson>,
    /// How it takes tips; absent for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tipping: Option<TippingJson>,
    /// Its kitchen; absent for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kitchen: Option<KitchenJson>,
}

/// How cash is rounded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CashRoundingJson {
    /// How: one of [`MODES`]' names.
    pub mode: String,
    /// To multiples of this many minor units.
    pub increment: u32,
}

/// How prices round.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoundingJson {
    /// A unit price times a fractional quantity.
    pub extension: String,
    /// A percentage discount.
    pub discounts: String,
    /// Tax: rounded per `line` or per `document`.
    pub tax_scope: String,
    /// How tax rounds.
    pub tax: String,
}

/// A tax.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaxJson {
    /// Its identifier.
    pub id: String,
    /// Its name, as receipts show it.
    pub name: String,
    /// Its rate, a percentage: `8.875`.
    pub rate: String,
    /// The tax categories it applies to.
    pub categories: Vec<String>,
    /// `on_premises` or `to_go` if it applies only so; absent for always.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dining: Option<String>,
}

/// How a location takes tips.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TippingJson {
    /// The percentages offered, in basis points, ascending: 1,500 is 15%.
    pub presets: Vec<u16>,
    /// `screen` or `terminal`.
    pub prompt: String,
}

/// A kitchen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KitchenJson {
    /// Its stations, in the order a screen lists them.
    pub stations: Vec<StationJson>,
    /// The routes that send lines to stations.
    pub routes: Vec<RouteJson>,
    /// Where a line no route names goes; absent for no station but expo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_station: Option<String>,
    /// Seconds until a ticket is late-ish.
    pub late_ish_seconds: u32,
    /// Seconds until it is late.
    pub late_seconds: u32,
    /// Where a ticket prints when no screen showed it and its station has no printer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_printer: Option<PrinterJson>,
}

/// A kitchen station.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StationJson {
    /// Its identifier.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Whether it gets every line, to hand it off.
    pub expo: bool,
    /// Whether a screen shows its tickets.
    pub screen: bool,
    /// Its printer, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printer: Option<PrinterJson>,
    /// The language its tickets print in, a locale's tag such as `es-US`; absent for the
    /// register's (ADR-0038).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// A route.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteJson {
    /// The station.
    pub station: String,
    /// The catalog's items it makes.
    pub items: Vec<String>,
    /// The modifiers it makes on their own.
    pub modifiers: Vec<String>,
    /// The order modes it applies to, by name; empty for every mode.
    pub modes: Vec<String>,
}

/// A network printer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterJson {
    /// Its host.
    pub host: String,
    /// Its port.
    pub port: u16,
}

/// Settings to write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsEdit {
    /// The version they were read at: if another has been published since, it is refused.
    #[serde(default)]
    pub base: Option<String>,
    /// The settings.
    #[serde(flatten)]
    pub settings: Settings,
}

/// Why settings can't be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsError {
    /// A field isn't what it must be.
    #[error("{0}")]
    Field(String),
    /// The profile they make breaks a rule.
    #[error("{0}")]
    Profile(String),
}

/// How each rounding mode is named.
pub const MODES: [(&str, RoundingMode); 7] = [
    ("half_away_from_zero", RoundingMode::HalfAwayFromZero),
    ("half_even", RoundingMode::HalfEven),
    ("half_toward_zero", RoundingMode::HalfTowardZero),
    ("away_from_zero", RoundingMode::AwayFromZero),
    ("toward_zero", RoundingMode::TowardZero),
    ("ceiling", RoundingMode::Ceiling),
    ("floor", RoundingMode::Floor),
];

/// How each order mode is named.
pub const ORDER_MODES: [(&str, Mode); 8] = [
    ("dine_in", Mode::DineIn),
    ("takeout", Mode::Takeout),
    ("pickup", Mode::Pickup),
    ("delivery", Mode::Delivery),
    ("ship", Mode::Ship),
    ("service", Mode::Service),
    ("drive_thru", Mode::DriveThru),
    ("curbside", Mode::Curbside),
];

fn mode_name(mode: RoundingMode) -> String {
    MODES.iter().find(|(_, known)| *known == mode).map_or("", |(name, _)| name).to_owned()
}

fn mode(name: &str) -> Result<RoundingMode, SettingsError> {
    MODES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, mode)| *mode)
        .ok_or_else(|| SettingsError::Field(format!("{name:?} isn't a way of rounding")))
}

fn order_mode(name: &str) -> Result<Mode, SettingsError> {
    ORDER_MODES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, mode)| *mode)
        .ok_or_else(|| SettingsError::Field(format!("{name:?} isn't an order mode")))
}

fn order_mode_name(mode: Mode) -> String {
    ORDER_MODES.iter().find(|(_, known)| *known == mode).map_or("", |(name, _)| name).to_owned()
}

fn id<T>(text: &str, what: &str) -> Result<Id<T>, SettingsError> {
    Id::parse(text)
        .map_err(|_| SettingsError::Field(format!("{what} {text:?} isn't an identifier")))
}

fn name(text: &str, what: &str) -> Result<Name, SettingsError> {
    Name::new(text).map_err(|_| SettingsError::Field(format!("{what} {text:?} isn't a name")))
}

/// The locale `tag` names: one Keel formats for, exactly as written.
fn language(tag: &str) -> Result<keel_types::Locale, SettingsError> {
    keel_types::Locale::ALL.into_iter().find(|locale| locale.tag() == tag).ok_or_else(|| {
        SettingsError::Field(format!("{tag:?} isn't a language Keel speaks: en-US or es-US"))
    })
}

fn printer_json(printer: &Printer) -> PrinterJson {
    PrinterJson { host: printer.host.clone(), port: printer.port.get() }
}

fn printer(json: &PrinterJson) -> Result<Printer, SettingsError> {
    let port = NonZeroU16::new(json.port)
        .ok_or_else(|| SettingsError::Field(format!("a printer at {:?} on port 0", json.host)))?;
    Ok(Printer { host: json.host.clone(), port })
}

/// `profile`'s settings, as read.
pub fn view(profile: &Profile, version: ProfileVersion) -> SettingsView {
    let data = profile.data();
    let rules = &data.rules;
    SettingsView {
        version: hex(version),
        location: data.location.to_string(),
        currency: data.currency.code().to_owned(),
        settings: Settings {
            name: data.name.as_str().to_owned(),
            address: data.address.iter().map(|line| line.as_str().to_owned()).collect(),
            time_zone: data.business_day.time_zone.clone(),
            cutoff: format!(
                "{:02}:{:02}",
                data.business_day.cutoff_hour, data.business_day.cutoff_minute
            ),
            cash_rounding: data.cash_rounding.map(|rule| CashRoundingJson {
                mode: mode_name(rule.mode()),
                increment: rule.increment().get(),
            }),
            rounding: RoundingJson {
                extension: mode_name(rules.extension),
                discounts: mode_name(rules.discounts),
                tax_scope: match rules.tax_rounding.scope {
                    TaxScope::Line => "line",
                    TaxScope::Document => "document",
                }
                .to_owned(),
                tax: mode_name(rules.tax_rounding.mode),
            },
            taxes: rules
                .taxes
                .iter()
                .map(|tax| TaxJson {
                    id: tax.id.to_string(),
                    name: tax.name.clone(),
                    rate: tax
                        .rate
                        .as_percent()
                        .map_or_else(String::new, |rate| rate.normalize().to_string()),
                    categories: tax.categories.iter().map(ToString::to_string).collect(),
                    dining: tax.dining.map(|dining| {
                        match dining {
                            Dining::OnPremises => "on_premises",
                            Dining::ToGo => "to_go",
                        }
                        .to_owned()
                    }),
                })
                .collect(),
            tipping: data.tipping.as_ref().map(|tipping| TippingJson {
                presets: tipping.presets.iter().map(|preset| preset.get()).collect(),
                prompt: match tipping.prompt {
                    TipPrompt::Screen => "screen",
                    TipPrompt::Terminal => "terminal",
                }
                .to_owned(),
            }),
            kitchen: data.kitchen.as_ref().map(|kitchen| KitchenJson {
                stations: kitchen
                    .stations
                    .iter()
                    .map(|station| StationJson {
                        id: station.id.to_string(),
                        name: station.name.as_str().to_owned(),
                        expo: station.expo,
                        screen: station.screen,
                        printer: station.printer.as_ref().map(printer_json),
                        language: station.language.map(|locale| locale.tag().to_owned()),
                    })
                    .collect(),
                routes: kitchen
                    .routes
                    .iter()
                    .map(|route| RouteJson {
                        station: route.station.to_string(),
                        items: route.items.iter().map(ToString::to_string).collect(),
                        modifiers: route.modifiers.iter().map(ToString::to_string).collect(),
                        modes: route.modes.iter().map(|mode| order_mode_name(*mode)).collect(),
                    })
                    .collect(),
                default_station: kitchen.default_station.map(|station| station.to_string()),
                late_ish_seconds: kitchen.aging.late_ish.get(),
                late_seconds: kitchen.aging.late.get(),
                fallback_printer: kitchen.fallback_printer.as_ref().map(printer_json),
            }),
        },
    }
}

/// The business day of `settings`: its time zone and its cutoff, `HH:MM`.
fn business_day(settings: &Settings) -> Result<BusinessDay, SettingsError> {
    let bad = || SettingsError::Field(format!("{:?} isn't a time, HH:MM", settings.cutoff));
    let (hour, minute) = settings.cutoff.split_once(':').ok_or_else(bad)?;
    let two = |text: &str| (text.len() == 2).then(|| text.parse::<u8>().ok()).flatten();
    let (cutoff_hour, cutoff_minute) = two(hour).zip(two(minute)).ok_or_else(bad)?;
    // Checked as the profile checks it, so the words say which is wrong: found in testing, both
    // read "a business day policy that doesn't exist".
    BusinessDayPolicy::new(&settings.time_zone, cutoff_hour, cutoff_minute).map_err(|error| {
        match error {
            BusinessDateError::InvalidCutoff { .. } => bad(),
            _ => SettingsError::Field(format!(
                "{:?} isn't a time zone, such as America/New_York",
                settings.time_zone
            )),
        }
    })?;
    Ok(BusinessDay { time_zone: settings.time_zone.clone(), cutoff_hour, cutoff_minute })
}

fn tax(json: &TaxJson) -> Result<Tax, SettingsError> {
    let percent: Decimal = json.rate.parse().map_err(|_| {
        SettingsError::Field(format!("{}'s rate {:?} isn't a number", json.name, json.rate))
    })?;
    let rate = Rate::from_percent(percent)
        .map_err(|error| SettingsError::Field(format!("{}'s rate: {error}", json.name)))?;
    let dining = match json.dining.as_deref() {
        None => None,
        Some("on_premises") => Some(Dining::OnPremises),
        Some("to_go") => Some(Dining::ToGo),
        Some(other) => {
            return Err(SettingsError::Field(format!("{other:?} isn't on_premises or to_go")));
        }
    };
    Ok(Tax {
        id: id(&json.id, "tax")?,
        name: json.name.clone(),
        rate,
        categories: json
            .categories
            .iter()
            .map(|category| id(category, "tax category"))
            .collect::<Result<_, _>>()?,
        dining,
    })
}

fn kitchen(json: &KitchenJson) -> Result<Kitchen, SettingsError> {
    let seconds = |n: u32, what: &str| {
        NonZeroU32::new(n).ok_or_else(|| SettingsError::Field(format!("{what} of no seconds")))
    };
    Ok(Kitchen {
        stations: json
            .stations
            .iter()
            .map(|station| {
                Ok(Station {
                    id: id(&station.id, "station")?,
                    name: name(&station.name, "station name")?,
                    expo: station.expo,
                    screen: station.screen,
                    printer: station.printer.as_ref().map(printer).transpose()?,
                    language: station.language.as_deref().map(language).transpose()?,
                })
            })
            .collect::<Result<_, SettingsError>>()?,
        routes: json
            .routes
            .iter()
            .map(|route| {
                Ok(Route {
                    station: id(&route.station, "station")?,
                    items: route
                        .items
                        .iter()
                        .map(|item| id(item, "item"))
                        .collect::<Result<BTreeSet<_>, _>>()?,
                    modifiers: route
                        .modifiers
                        .iter()
                        .map(|modifier| id(modifier, "modifier"))
                        .collect::<Result<BTreeSet<_>, _>>()?,
                    modes: route
                        .modes
                        .iter()
                        .map(|mode| order_mode(mode))
                        .collect::<Result<BTreeSet<_>, _>>()?,
                })
            })
            .collect::<Result<_, SettingsError>>()?,
        default_station: json
            .default_station
            .as_deref()
            .map(|station| id(station, "station"))
            .transpose()?,
        aging: Aging {
            late_ish: seconds(json.late_ish_seconds, "late-ish")?,
            late: seconds(json.late_seconds, "late")?,
        },
        fallback_printer: json.fallback_printer.as_ref().map(printer).transpose()?,
    })
}

/// `profile` with the settings of `edit`, checked whole: its catalog, menu, team and roles kept.
///
/// # Errors
/// [`SettingsError`] if a field isn't what it must be, or the profile they make breaks a rule,
/// as a route to an item the catalog doesn't have.
pub fn apply(profile: &Profile, edit: &SettingsEdit) -> Result<Profile, SettingsError> {
    let settings = &edit.settings;
    let mut data = profile.data().clone();
    data.name = name(&settings.name, "the location's name")?;
    data.address = settings
        .address
        .iter()
        .map(|line| name(line, "an address line"))
        .collect::<Result<_, _>>()?;
    data.business_day = business_day(settings)?;
    data.cash_rounding = settings
        .cash_rounding
        .as_ref()
        .map(|rounding| {
            let increment = NonZeroU32::new(rounding.increment).ok_or_else(|| {
                SettingsError::Field("cash rounded to multiples of nothing".to_owned())
            })?;
            Ok::<_, SettingsError>(RoundingRule::new(mode(&rounding.mode)?, increment))
        })
        .transpose()?;
    data.rules.extension = mode(&settings.rounding.extension)?;
    data.rules.discounts = mode(&settings.rounding.discounts)?;
    data.rules.tax_rounding = TaxRounding {
        scope: match settings.rounding.tax_scope.as_str() {
            "line" => TaxScope::Line,
            "document" => TaxScope::Document,
            other => return Err(SettingsError::Field(format!("{other:?} isn't line or document"))),
        },
        mode: mode(&settings.rounding.tax)?,
    };
    data.rules.taxes = settings.taxes.iter().map(tax).collect::<Result<_, _>>()?;
    data.tipping = settings
        .tipping
        .as_ref()
        .map(|tipping| {
            Ok::<_, SettingsError>(Tipping {
                presets: tipping
                    .presets
                    .iter()
                    .map(|preset| {
                        NonZeroU16::new(*preset)
                            .ok_or_else(|| SettingsError::Field("a tip preset of 0%".to_owned()))
                    })
                    .collect::<Result<_, _>>()?,
                prompt: match tipping.prompt.as_str() {
                    "screen" => TipPrompt::Screen,
                    "terminal" => TipPrompt::Terminal,
                    other => {
                        return Err(SettingsError::Field(format!(
                            "{other:?} isn't screen or terminal"
                        )));
                    }
                },
            })
        })
        .transpose()?;
    data.kitchen = settings.kitchen.as_ref().map(kitchen).transpose()?;
    Profile::new(data).map_err(|error| SettingsError::Profile(error.to_string()))
}

#[cfg(test)]
mod tests {
    use keel_domain::profile::demo;
    use keel_domain::publication::version;

    use super::*;

    fn edit(view: &SettingsView) -> SettingsEdit {
        SettingsEdit { base: Some(view.version.clone()), settings: view.settings.clone() }
    }

    #[test]
    fn settings_read_and_written_back_are_the_same_profile() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        assert_eq!(read.currency, "USD");
        assert!(read.settings.kitchen.is_some() && read.settings.tipping.is_some());
        assert!(!read.settings.taxes.is_empty());
        let written = apply(&profile, &edit(&read)).unwrap();
        assert_eq!(written.encode(), profile.encode(), "nothing changed");
        // Through JSON, as the back office sends them.
        let json = serde_json::to_string(&edit(&read)).unwrap();
        let parsed: SettingsEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(apply(&profile, &parsed).unwrap().encode(), profile.encode());
    }

    #[test]
    fn settings_written_change_only_themselves() {
        let profile = demo().unwrap();
        let mut changed = edit(&view(&profile, version(&profile)));
        changed.settings.name = "Corner Café".to_owned();
        changed.settings.cutoff = "05:30".to_owned();
        changed.settings.taxes[0].rate = "9.25".to_owned();
        changed.settings.tipping = None;
        let written = apply(&profile, &changed).unwrap();
        let data = written.data();
        assert_eq!(data.name.as_str(), "Corner Café");
        assert_eq!((data.business_day.cutoff_hour, data.business_day.cutoff_minute), (5, 30));
        assert_eq!(view(&written, version(&written)).settings.taxes[0].rate, "9.25");
        assert!(data.tipping.is_none());
        // Asked on the terminal, it reads back so: found by a planted bug.
        let mut terminal = changed.clone();
        terminal.settings.tipping =
            Some(TippingJson { presets: vec![1_500, 2_000], prompt: "terminal".to_owned() });
        let written = apply(&profile, &terminal).unwrap();
        assert_eq!(
            written.data().tipping.as_ref().map(|tipping| tipping.prompt),
            Some(TipPrompt::Terminal)
        );
        let read = view(&written, version(&written)).settings.tipping.unwrap();
        assert_eq!((read.presets, read.prompt.as_str()), (vec![1_500, 2_000], "terminal"));
        assert_eq!(data.catalog, profile.data().catalog);
        assert_eq!((&data.team, &data.roles), (&profile.data().team, &profile.data().roles));
    }

    #[test]
    fn settings_the_kernel_refuses_are_refused_with_its_words() {
        let profile = demo().unwrap();
        let read = view(&profile, version(&profile));
        let refused = |change: &dyn Fn(&mut Settings)| {
            let mut changed = edit(&read);
            change(&mut changed.settings);
            apply(&profile, &changed).unwrap_err().to_string()
        };
        assert!(refused(&|s| s.cutoff = "25:00".to_owned()).contains("HH:MM"), "a cutoff past 23");
        assert!(refused(&|s| s.cutoff = "05:60".to_owned()).contains("HH:MM"));
        assert!(refused(&|s| s.cutoff = "5:30".to_owned()).contains("HH:MM"));
        assert!(refused(&|s| s.time_zone = "Mars/Olympus".to_owned()).contains("time zone"));
        assert!(refused(&|s| s.taxes[0].rate = "-1".to_owned()).contains("negative tax rate"));
        assert!(refused(&|s| s.rounding.tax = "sideways".to_owned()).contains("rounding"));
        // An item the catalog doesn't have: a known one's identifier, its last digit changed.
        assert!(
            refused(&|s| {
                if let Some(route) =
                    s.kitchen.as_mut().and_then(|kitchen| kitchen.routes.first_mut())
                {
                    let mut unknown = route.items[0].clone();
                    let last = if unknown.ends_with('f') { 'e' } else { 'f' };
                    unknown.pop();
                    unknown.push(last);
                    route.items.push(unknown);
                }
            })
            .contains("item")
        );
        assert!(
            refused(&|s| s.tipping =
                Some(TippingJson { presets: vec![2_000, 1_500], prompt: "screen".to_owned() }))
            .contains("order")
        );
    }

    /// A station's language is written and read back, and one Keel doesn't speak, or in
    /// another case, is refused (ADR-0038).
    #[test]
    fn a_stations_language_is_written_and_read_back() {
        let profile = demo().unwrap();
        let mut changed = edit(&view(&profile, version(&profile)));
        changed.settings.kitchen.as_mut().unwrap().stations[0].language = Some("es-US".to_owned());
        let written = apply(&profile, &changed).unwrap();
        let station = &written.data().kitchen.as_ref().unwrap().stations[0];
        assert_eq!(station.language, Some(keel_types::Locale::ES_US));
        let read = view(&written, version(&written));
        assert_eq!(read.settings.kitchen.unwrap().stations[0].language.as_deref(), Some("es-US"));
        for bad in ["ES-US", "fr-FR"] {
            changed.settings.kitchen.as_mut().unwrap().stations[0].language = Some(bad.to_owned());
            assert!(
                apply(&profile, &changed).unwrap_err().to_string().contains("language"),
                "{bad}"
            );
        }
    }
}
