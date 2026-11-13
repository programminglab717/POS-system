//! The kitchen at the register and on its screens (ADR-0034): an order's pending lines fired,
//! each station sent a ticket of what it makes, in the fire's own write; a screen's tickets,
//! shown, bumped and recalled; the tickets no screen showed in 10 s, printed where they can be;
//! and a void slip where a ticket printed.

use core::num::NonZeroU8;
use core::time::Duration;
use std::collections::{BTreeMap, BTreeSet};

use keel_doc::{Align, Block, Doc, Style};
use keel_domain::codec::{IdSet, Name};
use keel_domain::kitchen::{
    KitchenCommand, KitchenItem, KitchenModifier, KitchenTicket, RerouteTo, TicketSent,
};
use keel_domain::order::{
    ChosenModifier, Line, LineStatus, Mode, Order, OrderCommand, OrderStatus, Prefix,
};
use keel_domain::profile::{Allergen, Printer, Profile, Routed, Station as StationConfig};
use keel_domain::refs::Station;
use keel_events::envelope::{Actor, Component, Device};
use keel_events::keys::Signer;
use keel_store::{KitchenSummary, Writing};
use keel_types::{Clock, Entropy, Id, LocalTime, Quantity, Timestamp};

use crate::error::RuntimeError;
use crate::print::{self, Job};
use crate::runtime::{Meta, Runtime, append, known};

/// How long a ticket for a screen waits to be shown before it prints elsewhere.
pub const FALLBACK_WAIT: Duration = Duration::from_secs(10);

/// How long a reroute is shown to staff.
pub const ALERT_WINDOW: Duration = Duration::from_secs(600);

/// The system's component a screen records as.
const SCREEN: &str = "kds";

/// The system's component a reroute is recorded as.
const ROUTER: &str = "kitchen";

/// A ticket to send in a fire's write: its identifier, what it holds, and, for a station no
/// screen shows, its print job, printer and document.
pub(crate) struct Planned {
    ticket: Id<KitchenTicket>,
    sent: TicketSent,
    print: Option<(Id<Job>, Printer, Doc)>,
}

/// A fire planned before its write: the lines it fires, and the tickets it sends.
pub(crate) struct Firing {
    order: Id<Order>,
    lines: IdSet<Line>,
    tickets: Vec<Planned>,
}

/// How old a ticket is, by the kitchen's thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Aging {
    /// Younger than late-ish.
    Fresh,
    /// Late-ish, not yet late.
    LateIsh,
    /// Late.
    Late,
}

/// An item of a ticket on a screen: what to make, whether its line was voided since, and, on an
/// expo station, whether every station making it has bumped it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenItem {
    /// What to make, as it was sent.
    pub item: KitchenItem,
    /// Whether its line, or its order, was voided since: the screen strikes it through.
    pub void: bool,
    /// On an expo station, whether the stations that make it have all bumped it; `None` on
    /// another.
    pub ready: Option<bool>,
}

/// A ticket on a screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenTicket {
    /// The ticket.
    pub ticket: Id<KitchenTicket>,
    /// Its station.
    pub station: Id<Station>,
    /// Its station's name.
    pub station_name: Name,
    /// Its order.
    pub order: Id<Order>,
    /// Its order's code, as its receipt prints it: the last six characters of its identifier.
    pub code: String,
    /// How the order reaches its customer.
    pub mode: Mode,
    /// When it was sent, in the location's time.
    pub sent_at: LocalTime,
    /// How long ago it was sent, in whole seconds.
    pub age: u64,
    /// How old that is, by the kitchen's thresholds.
    pub aging: Aging,
    /// What to make.
    pub items: Vec<ScreenItem>,
    /// Where it was rerouted, if no screen showed it in time.
    pub rerouted: Option<RerouteTo>,
}

/// A station's day so far: how many tickets it bumped, and their average time from sent to
/// bumped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StationTimes {
    /// The station.
    pub station: Id<Station>,
    /// Its name.
    pub name: Name,
    /// How many tickets it bumped today.
    pub bumped: u32,
    /// Their average time from sent to bumped, in whole seconds; `None` before the first.
    pub average: Option<u64>,
}

/// What a screen showing some stations shows (ADR-0034, decision 6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenScreen {
    /// Their tickets not yet bumped, the earliest sent first.
    pub tickets: Vec<ScreenTicket>,
    /// The ticket of theirs bumped last today, which Recall brings back.
    pub last_bumped: Option<Id<KitchenTicket>>,
    /// Each station's day so far.
    pub times: Vec<StationTimes>,
}

/// A ticket no screen showed in time, as staff are told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KitchenAlert {
    /// The ticket.
    pub ticket: Id<KitchenTicket>,
    /// Its station's name.
    pub station: Name,
    /// Its order's code.
    pub code: String,
    /// Where it was sent instead.
    pub to: RerouteTo,
    /// When, in the location's time.
    pub at: LocalTime,
}

/// The receipt code of `order`: the last six characters of its identifier, in capitals.
pub(crate) fn code(order: Id<Order>) -> String {
    let id = order.to_string();
    id.get(id.len().saturating_sub(6)..).unwrap_or_default().to_uppercase()
}

/// `modifiers`, each before those under it, `depth` deep and down.
fn flatten(
    modifiers: &[ChosenModifier],
    depth: u8,
    out: &mut Vec<KitchenModifier>,
) -> Result<(), RuntimeError> {
    for modifier in modifiers {
        let at = NonZeroU8::new(depth).ok_or(RuntimeError::Time)?;
        out.push(KitchenModifier {
            name: modifier.name.clone(),
            prefix: modifier.prefix,
            quantity: modifier.quantity,
            depth: at,
        });
        flatten(&modifier.modifiers, depth.checked_add(1).ok_or(RuntimeError::Time)?, out)?;
    }
    Ok(())
}

/// The allergens `modifiers` add, at any depth: none for one left out.
fn modifier_allergens(
    profile: &Profile,
    modifiers: &[ChosenModifier],
    out: &mut BTreeSet<Allergen>,
) {
    for modifier in modifiers {
        if modifier.prefix == Prefix::No {
            continue;
        }
        if let Some(found) = profile.modifier(modifier.modifier) {
            out.extend(found.allergens.iter().copied());
        }
        modifier_allergens(profile, &modifier.modifiers, out);
    }
}

/// The first modifier `modifier` among `modifiers`, at any depth, each before those under it.
fn find(
    modifiers: &[ChosenModifier],
    modifier: Id<keel_domain::refs::Modifier>,
) -> Option<&ChosenModifier> {
    for chosen in modifiers {
        if chosen.modifier == modifier {
            return Some(chosen);
        }
        if let Some(found) = find(&chosen.modifiers, modifier) {
            return Some(found);
        }
    }
    None
}

/// What `line` sends a station `routed` names: the line whole, or one of its modifiers as a
/// component, its quantity the line's times the modifier's.
fn item(
    profile: &Profile,
    line: &Line,
    routed: &Routed,
) -> Result<Option<KitchenItem>, RuntimeError> {
    let mut modifiers = Vec::new();
    let mut allergens = BTreeSet::new();
    let Some(component) = routed.component else {
        flatten(line.modifiers(), 1, &mut modifiers)?;
        if let Some((item, _)) = profile.variant(line.item().variant) {
            allergens.extend(item.allergens.iter().copied());
        }
        modifier_allergens(profile, line.modifiers(), &mut allergens);
        return Ok(Some(KitchenItem {
            line: line.id(),
            name: line.item().name.clone(),
            quantity: line.quantity(),
            modifiers,
            notes: line.notes().cloned(),
            allergens,
            component_of: None,
        }));
    };
    let Some(chosen) = find(line.modifiers(), component) else { return Ok(None) };
    flatten(&chosen.modifiers, 1, &mut modifiers)?;
    modifier_allergens(profile, core::slice::from_ref(chosen), &mut allergens);
    let quantity = line.quantity();
    let micros = quantity
        .micros()
        .checked_mul(i64::from(chosen.quantity.get()))
        .ok_or(RuntimeError::Time)?;
    Ok(Some(KitchenItem {
        line: line.id(),
        name: chosen.name.clone(),
        quantity: Quantity::from_micros(micros, quantity.unit()),
        modifiers,
        notes: line.notes().cloned(),
        allergens,
        component_of: Some(line.item().name.clone()),
    }))
}

/// How a ticket's quantity reads: "2 x" for a count, "0.250 kg" for a measure.
pub fn how_many(quantity: Quantity) -> String {
    let micros = quantity.micros();
    if quantity.unit() == keel_types::Unit::Each && micros.checked_rem(1_000_000) == Some(0) {
        return format!("{} x", micros.checked_div(1_000_000).unwrap_or_default());
    }
    let whole = micros.checked_div(1_000_000).unwrap_or_default();
    let thousandths =
        micros.checked_rem(1_000_000).and_then(|rest| rest.checked_div(1_000)).unwrap_or_default();
    format!("{whole}.{:03} {}", thousandths.unsigned_abs(), quantity.unit().code())
}

/// A modifier as a kitchen ticket prints it: "NO onion", "EXTRA shot", "2 x Vanilla".
fn modifier_words(modifier: &KitchenModifier) -> String {
    let prefix = match modifier.prefix {
        Prefix::Plain => "",
        Prefix::No => "NO ",
        Prefix::Extra => "EXTRA ",
        Prefix::Light => "LIGHT ",
        Prefix::Side => "SIDE ",
        Prefix::Sub => "SUB ",
    };
    let count = if modifier.quantity.get() > 1 {
        format!("{} x ", modifier.quantity)
    } else {
        String::new()
    };
    format!("{prefix}{count}{}", modifier.name)
}

/// The allergens of an item, as a ticket names them.
pub(crate) fn allergen_words(allergens: &BTreeSet<Allergen>) -> String {
    allergens.iter().map(|allergen| allergen.name()).collect::<Vec<_>>().join(", ")
}

/// A kitchen ticket's document: its station, its order's code and mode and when it was sent,
/// and each item, its modifiers, notes and allergens; marked rerouted, or void, as it is.
/// The most characters of a text a kitchen ticket prints: a note of 500 characters beyond
/// Latin-1, or a component named after a long line, mustn't pass a block's limit. Found in
/// review: one refused the payment, and jammed the print loop.
const MOST_CHARS: usize = 240;

/// The most blocks a kitchen ticket's items take: a long order's expo ticket says how many more
/// there are, rather than pass a document's limit.
const MOST_ITEM_BLOCKS: usize = 900;

/// `text`, clipped to [`MOST_CHARS`] characters, ending in "..." when it was longer.
fn clip(text: &str) -> String {
    if text.chars().count() <= MOST_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(MOST_CHARS.saturating_sub(3)).collect();
    format!("{kept}...")
}

pub(crate) fn ticket_doc(
    station: &StationConfig,
    sent: &TicketSent,
    at: LocalTime,
    mark: Option<&str>,
) -> Result<Doc, RuntimeError> {
    let large = Style { bold: true, wide: true, tall: true };
    let bold = Style { bold: true, wide: false, tall: false };
    let mut blocks = Vec::new();
    // Tall and bold, but not wide: a reroute's reason fits a line.
    if let Some(mark) = mark {
        blocks.push(Block::text(
            clip(mark),
            Align::Center,
            Style { bold: true, wide: false, tall: true },
        ));
    }
    blocks.push(Block::text(clip(station.name.as_str()), Align::Center, large));
    blocks.push(Block::row(format!("Order {}", code(sent.order)), at.to_string()));
    blocks.push(Block::text(mode_words(sent.mode), Align::Left, bold));
    blocks.push(Block::Rule);
    let before = blocks.len();
    for (at, item) in sent.items.iter().enumerate() {
        if blocks.len().saturating_sub(before) > MOST_ITEM_BLOCKS {
            let more = sent.items.len().saturating_sub(at);
            blocks.push(Block::text(
                format!("... and {more} more: see the screen"),
                Align::Left,
                bold,
            ));
            break;
        }
        let name = match &item.component_of {
            Some(line) => format!("{} {} (for {line})", how_many(item.quantity), item.name),
            None => format!("{} {}", how_many(item.quantity), item.name),
        };
        blocks.push(Block::Row {
            left: clip(&name),
            right: String::new(),
            indent: 0,
            style: Style { bold: true, wide: false, tall: true },
        });
        for modifier in &item.modifiers {
            let indent = modifier.depth.get().saturating_mul(2);
            let style = if modifier.prefix == Prefix::Plain { Style::default() } else { bold };
            blocks.push(Block::Row {
                left: clip(&modifier_words(modifier)),
                right: String::new(),
                indent,
                style,
            });
        }
        if let Some(notes) = &item.notes {
            blocks.push(Block::Row {
                left: clip(&format!("NOTE: {notes}")),
                right: String::new(),
                indent: 2,
                style: bold,
            });
        }
        if !item.allergens.is_empty() {
            let words = format!("ALLERGENS: {}", allergen_words(&item.allergens));
            blocks.push(Block::Row { left: words, right: String::new(), indent: 2, style: bold });
        }
    }
    blocks.push(Block::Rule);
    blocks.push(Block::Feed(2));
    blocks.push(Block::Cut);
    Ok(Doc::new(blocks)?)
}

/// How an order's mode reads on a ticket.
pub(crate) const fn mode_words(mode: Mode) -> &'static str {
    match mode {
        Mode::DineIn => "DINE IN",
        Mode::Takeout => "TO GO",
        Mode::Pickup => "PICKUP",
        Mode::Delivery => "DELIVERY",
        Mode::Ship => "SHIP",
        Mode::Service => "SERVICE",
        Mode::DriveThru => "DRIVE-THRU",
        Mode::Curbside => "CURBSIDE",
    }
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Plans firing `order`'s pending lines, or those with a part on `check`, at `now`: which
    /// lines, and the ticket each station gets, a ticket's identifier and, for a station no
    /// screen shows, its print job, drawn now. `None` in a location without a kitchen, or for
    /// an order with nothing pending.
    pub(crate) fn plan_firing(
        &mut self,
        order: &Order,
        check: Option<Id<keel_domain::order::Check>>,
        now: Timestamp,
    ) -> Result<Option<Firing>, RuntimeError> {
        let Some(kitchen) = self.profile.kitchen() else { return Ok(None) };
        // Paying a check fires what of it is pending, not another check's lines. Found in
        // testing: paying one of two checks sent the other's, which could no longer be taken off.
        let on_check: Option<BTreeSet<Id<Line>>> =
            check.map(|check| order.lines_on(check).map(Line::id).collect());
        let pending: Vec<&Line> = order
            .lines()
            .iter()
            .filter(|line| *line.status() == LineStatus::Pending)
            .filter(|line| on_check.as_ref().is_none_or(|on| on.contains(&line.id())))
            .collect();
        if pending.is_empty() {
            return Ok(None);
        }
        let mode = order.info().map_or(Mode::Takeout, |info| info.mode);
        let mut items: BTreeMap<Id<Station>, Vec<KitchenItem>> = BTreeMap::new();
        for line in &pending {
            for routed in self.profile.route(mode, line.item().variant, line.modifiers()) {
                if let Some(item) = item(&self.profile, line, &routed)? {
                    items.entry(routed.station).or_default().push(item);
                }
            }
        }
        let at = self.profile.business_day().local_time(now)?;
        let mut tickets = Vec::new();
        for station in &kitchen.stations {
            let Some(made) = items.remove(&station.id) else { continue };
            let sent = TicketSent { order: order.id(), station: station.id, mode, items: made };
            let ticket = self.ids.generate(now)?;
            // A station no screen shows prints its tickets at once.
            let print = match (&station.screen, &station.printer) {
                (false, Some(printer)) => {
                    let job = self.ids.generate(now)?;
                    Some((job, printer.clone(), ticket_doc(station, &sent, at, None)?))
                }
                _ => None,
            };
            tickets.push(Planned { ticket, sent, print });
        }
        let lines = IdSet::new(pending.iter().map(|line| line.id()))
            .map_err(keel_domain::order::CommandError::Invalid)?;
        Ok(Some(Firing { order: order.id(), lines, tickets }))
    }

    /// Sends `order`'s pending lines to the kitchen (ADR-0034, decision 3): fires them, and
    /// sends each station its ticket, in one write; a station no screen shows prints it. An
    /// order with nothing pending, or a location without a kitchen, sends nothing.
    ///
    /// # Errors
    /// [`RuntimeError`] if no one is signed in, or the order refuses: it isn't open, or another
    /// device owns it.
    pub fn send(&mut self, order: Id<Order>) -> Result<crate::Ticket, RuntimeError> {
        let meta = self.meta()?;
        let loaded = self.store.load(Order::new(order))?;
        known(&loaded)?;
        // Only the order's owner sends it, as only it pays it: two devices sending one order
        // would send its stations two tickets. Found in review.
        if let Some(owner) = loaded.ownership()
            && owner.device != self.device
        {
            return Err(keel_domain::order::CommandError::NotOwner(owner.device).into());
        }
        let firing = self.plan_firing(&loaded, None, meta.now)?;
        let payments = self.payments(order)?;
        let (location, device) = (self.location(), self.device);
        let (profile, locale) = (&self.profile, self.locale);
        self.store.write(|w| {
            if let Some(firing) = &firing {
                write_firing(w, firing, location, device, &meta)?;
            }
            crate::runtime::view(w, order, &payments, profile, locale)
        })
    }

    /// What a screen showing `stations` shows (ADR-0034, decision 6), and the tickets it now
    /// shows for the first time recorded as shown, so no register reroutes them: their tickets
    /// not yet bumped, the earliest sent first, each with its age and its items, those voided
    /// since struck through; the one bumped last today; and each station's day so far.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read or written.
    pub fn kitchen_screen(
        &mut self,
        stations: &[Id<Station>],
    ) -> Result<KitchenScreen, RuntimeError> {
        let Some(kitchen) = self.profile.kitchen().cloned() else {
            return Ok(KitchenScreen { tickets: Vec::new(), last_bumped: None, times: Vec::new() });
        };
        let now = self.clock.now();
        let wanted: BTreeSet<Id<Station>> = stations.iter().copied().collect();
        let open: Vec<KitchenSummary> = self
            .store
            .open_kitchen_tickets()?
            .into_iter()
            .filter(|summary| summary.station.is_some_and(|station| wanted.contains(&station)))
            .collect();
        self.record_shown(&open)?;
        let mut orders: BTreeMap<Id<Order>, Order> = BTreeMap::new();
        let mut tickets = Vec::new();
        for summary in &open {
            let ticket = self.store.load(KitchenTicket::new(summary.id))?;
            let Some((sent, _)) = ticket.sent() else { continue };
            if let std::collections::btree_map::Entry::Vacant(entry) = orders.entry(sent.order) {
                entry.insert(self.store.load(Order::new(sent.order))?);
            }
            let Some(order) = orders.get(&sent.order) else { continue };
            if let Some(shown) = self.screen_ticket(&kitchen, &ticket, order, now)? {
                tickets.push(shown);
            }
        }
        let (last_bumped, times) = self.day_so_far(&kitchen, &wanted, now)?;
        Ok(KitchenScreen { tickets, last_bumped, times })
    }

    /// Records as shown the tickets of `open` a screen shows for the first time, unless the log
    /// isn't settled, when the screen shows them all the same.
    fn record_shown(&mut self, open: &[KitchenSummary]) -> Result<(), RuntimeError> {
        let unshown: Vec<Id<KitchenTicket>> = open
            .iter()
            .filter(|summary| summary.shown_at.is_none() && summary.rerouted.is_none())
            .map(|summary| summary.id)
            .collect();
        if unshown.is_empty() {
            return Ok(());
        }
        let Ok(meta) = self.system_meta(SCREEN) else { return Ok(()) };
        let location = self.location();
        self.store.write(|w| -> Result<(), RuntimeError> {
            for ticket in &unshown {
                let loaded = w.load(KitchenTicket::new(*ticket))?;
                if let Ok(event) = loaded.decide(location, KitchenCommand::Show) {
                    append(w, ticket.cast(), &event, &meta)?;
                }
            }
            Ok(())
        })
    }

    /// `ticket` of `order` as a screen shows it at `now`: its items, those voided since struck
    /// through, and on expo whether each is made; its age, and how old that is.
    fn screen_ticket(
        &self,
        kitchen: &keel_domain::profile::Kitchen,
        ticket: &KitchenTicket,
        order: &Order,
        now: Timestamp,
    ) -> Result<Option<ScreenTicket>, RuntimeError> {
        let Some((sent, moment)) = ticket.sent() else { return Ok(None) };
        let Some(station) = kitchen.station(sent.station) else { return Ok(None) };
        let order_void = matches!(order.status(), OrderStatus::Voided(_));
        let ready = if station.expo { Some(self.made(sent.order, kitchen)?) } else { None };
        let items = sent
            .items
            .iter()
            .map(|item| {
                let line_void = order.line(item.line).is_none_or(|line| {
                    matches!(line.status(), LineStatus::Voided(_) | LineStatus::Removed)
                });
                ScreenItem {
                    item: item.clone(),
                    void: order_void || line_void,
                    ready: ready.as_ref().map(|made| made.get(&item.line).copied().unwrap_or(true)),
                }
            })
            .collect();
        let sent_at = moment.at().ok_or(RuntimeError::Time)?;
        let age = u64::try_from(now.as_micros().saturating_sub(sent_at.as_micros()) / 1_000_000)
            .unwrap_or(0);
        let aging = if age >= u64::from(kitchen.aging.late.get()) {
            Aging::Late
        } else if age >= u64::from(kitchen.aging.late_ish.get()) {
            Aging::LateIsh
        } else {
            Aging::Fresh
        };
        Ok(Some(ScreenTicket {
            ticket: ticket.id(),
            station: station.id,
            station_name: station.name.clone(),
            order: sent.order,
            code: code(sent.order),
            mode: sent.mode,
            sent_at: self.profile.business_day().local_time(sent_at)?,
            age,
            aging,
            items,
            rerouted: ticket.rerouted().map(|(to, _)| to),
        }))
    }

    /// The ticket of `wanted` stations bumped last today, and each one's day so far.
    fn day_so_far(
        &self,
        kitchen: &keel_domain::profile::Kitchen,
        wanted: &BTreeSet<Id<Station>>,
        now: Timestamp,
    ) -> Result<(Option<Id<KitchenTicket>>, Vec<StationTimes>), RuntimeError> {
        let today = self.profile.business_day().business_date_of(now)?;
        let day = self.store.day_kitchen_tickets(today)?;
        let last_bumped = day
            .iter()
            .filter(|summary| summary.station.is_some_and(|station| wanted.contains(&station)))
            .filter(|summary| summary.bumped)
            .max_by_key(|summary| (summary.bumped_at, summary.id.to_bytes()))
            .map(|summary| summary.id);
        let times = kitchen
            .stations
            .iter()
            .filter(|station| wanted.contains(&station.id))
            .map(|station| {
                let spans: Vec<i64> = day
                    .iter()
                    .filter(|summary| summary.station == Some(station.id) && summary.bumped)
                    .filter_map(|summary| {
                        let (sent, _) = summary.sent?;
                        Some(summary.bumped_at?.as_micros().saturating_sub(sent.as_micros()).max(0))
                    })
                    .collect();
                let count = i64::try_from(spans.len()).unwrap_or(i64::MAX);
                let total = spans.iter().fold(0_i64, |sum, span| sum.saturating_add(*span));
                StationTimes {
                    station: station.id,
                    name: station.name.clone(),
                    bumped: u32::try_from(spans.len()).unwrap_or(u32::MAX),
                    average: total
                        .checked_div(count)
                        .and_then(|micros| u64::try_from(micros / 1_000_000).ok()),
                }
            })
            .collect();
        Ok((last_bumped, times))
    }

    /// For each line of `order` an expo station hands off, whether every station making it has
    /// bumped its ticket.
    fn made(
        &self,
        order: Id<Order>,
        kitchen: &keel_domain::profile::Kitchen,
    ) -> Result<BTreeMap<Id<Line>, bool>, RuntimeError> {
        let mut made: BTreeMap<Id<Line>, bool> = BTreeMap::new();
        for summary in self.store.order_kitchen_tickets(order)? {
            let ticket = self.store.load(KitchenTicket::new(summary.id))?;
            let Some((sent, _)) = ticket.sent() else { continue };
            if kitchen.station(sent.station).is_none_or(|station| station.expo) {
                continue;
            }
            for item in &sent.items {
                let ready = made.entry(item.line).or_insert(true);
                *ready = *ready && ticket.is_bumped();
            }
        }
        Ok(made)
    }

    /// Bumps `ticket`: its station is done with it. A screen records it as the system's
    /// component `kds`: no one need be signed in.
    ///
    /// # Errors
    /// [`RuntimeError::Kitchen`] if it isn't sent, or is bumped already; [`RuntimeError`] if the
    /// log isn't settled or the store can't be written.
    pub fn bump(&mut self, ticket: Id<KitchenTicket>) -> Result<(), RuntimeError> {
        self.kitchen_command(ticket, KitchenCommand::Bump)
    }

    /// Brings `ticket` back on its station's screen.
    ///
    /// # Errors
    /// As [`Runtime::bump`]; [`RuntimeError::Kitchen`] if it isn't bumped.
    pub fn recall_ticket(&mut self, ticket: Id<KitchenTicket>) -> Result<(), RuntimeError> {
        self.kitchen_command(ticket, KitchenCommand::Recall)
    }

    fn kitchen_command(
        &mut self,
        ticket: Id<KitchenTicket>,
        command: KitchenCommand,
    ) -> Result<(), RuntimeError> {
        let meta = self.system_meta(SCREEN)?;
        let location = self.location();
        self.store.write(|w| -> Result<(), RuntimeError> {
            let event = w.load(KitchenTicket::new(ticket))?.decide(location, command)?;
            append(w, ticket.cast(), &event, &meta)?;
            Ok(())
        })
    }

    /// How many kitchen events the store holds: a screen reads its tickets again when it moves.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn kitchen_version(&self) -> Result<u64, RuntimeError> {
        Ok(self.store.kitchen_version()?)
    }

    /// The tickets rerouted in the last ten minutes, the latest first: what every device shows.
    ///
    /// # Errors
    /// [`RuntimeError`] if the store can't be read.
    pub fn kitchen_alerts(&self) -> Result<Vec<KitchenAlert>, RuntimeError> {
        let Some(kitchen) = self.profile.kitchen() else { return Ok(Vec::new()) };
        let now = self.clock.now();
        let window = i64::try_from(ALERT_WINDOW.as_micros()).map_err(|_| RuntimeError::Time)?;
        let since = Timestamp::from_micros(now.as_micros().saturating_sub(window))
            .map_err(|_| RuntimeError::Time)?;
        let mut alerts = Vec::new();
        for summary in self.store.rerouted_kitchen_tickets(since)? {
            let (Some((to, at)), Some(order), Some(station)) =
                (summary.rerouted, summary.order, summary.station)
            else {
                continue;
            };
            let Some(station) = kitchen.station(station) else { continue };
            alerts.push(KitchenAlert {
                ticket: summary.id,
                station: station.name.clone(),
                code: code(order),
                to,
                at: self.profile.business_day().local_time(at)?,
            });
        }
        Ok(alerts)
    }

    /// Reroutes the tickets this device sent for a screen that no screen showed in
    /// [`FALLBACK_WAIT`] (ADR-0034, decision 5): each prints on its station's printer, or the
    /// kitchen's fallback, marked rerouted, or goes nowhere, and the reroute is recorded. Run
    /// from the print loop; a log not yet settled waits.
    pub(crate) fn reroute_overdue(&mut self) -> Result<(), RuntimeError> {
        let Some(kitchen) = self.profile.kitchen().cloned() else { return Ok(()) };
        let now = self.clock.now();
        let wait = i64::try_from(FALLBACK_WAIT.as_micros()).map_err(|_| RuntimeError::Time)?;
        let before = Timestamp::from_micros(now.as_micros().saturating_sub(wait))
            .map_err(|_| RuntimeError::Time)?;
        let overdue: Vec<KitchenSummary> = self
            .store
            .unshown_kitchen_tickets(self.device, before)?
            .into_iter()
            .filter(|summary| {
                summary
                    .station
                    .and_then(|station| kitchen.station(station))
                    .is_some_and(|station| station.screen)
            })
            .collect();
        if overdue.is_empty() {
            return Ok(());
        }
        let Ok(meta) = self.system_meta(ROUTER) else { return Ok(()) };
        let mut planned = Vec::new();
        for summary in &overdue {
            let ticket = self.store.load(KitchenTicket::new(summary.id))?;
            let Some((sent, moment)) = ticket.sent() else { continue };
            let Some(station) = kitchen.station(sent.station) else { continue };
            // What was voided since isn't printed: the kitchen would make it. Found in review.
            let order = self.store.load(Order::new(sent.order))?;
            let voided = matches!(order.status(), OrderStatus::Voided(_));
            let live: Vec<KitchenItem> = sent
                .items
                .iter()
                .filter(|item| {
                    !voided
                        && order.line(item.line).is_some_and(|line| {
                            !matches!(line.status(), LineStatus::Voided(_) | LineStatus::Removed)
                        })
                })
                .cloned()
                .collect();
            let (to, printer) = match (&station.printer, &kitchen.fallback_printer) {
                _ if live.is_empty() => (RerouteTo::Nowhere, None),
                (Some(printer), _) => (RerouteTo::StationPrinter, Some(printer.clone())),
                (None, Some(printer)) => (RerouteTo::FallbackPrinter, Some(printer.clone())),
                (None, None) => (RerouteTo::Nowhere, None),
            };
            let at =
                self.profile.business_day().local_time(moment.at().ok_or(RuntimeError::Time)?)?;
            let mark = format!("REROUTED: {}'s screen didn't show it", station.name);
            let print = match printer {
                Some(printer) => {
                    let printing = TicketSent { items: live, ..sent.clone() };
                    Some((
                        self.ids.generate(now)?,
                        printer,
                        ticket_doc(station, &printing, at, Some(&mark))?,
                    ))
                }
                None => None,
            };
            planned.push((summary.id, to, print));
        }
        let location = self.location();
        self.store.write(|w| -> Result<(), RuntimeError> {
            for (ticket, to, print) in &planned {
                let Ok(event) = w
                    .load(KitchenTicket::new(*ticket))?
                    .decide(location, KitchenCommand::Reroute(*to))
                else {
                    continue;
                };
                let rerouted = append(w, ticket.cast(), &event, &meta)?;
                if let Some((job, printer, doc)) = print {
                    w.enqueue(
                        &print::kitchen_effect(*job, doc, printer, Some(rerouted))?,
                        meta.now,
                    )?;
                }
            }
            Ok(())
        })
    }

    /// The void slips to print for `lines` of `order`, voided now: one for each ticket holding
    /// some of them that printed, at its station's printer or where it was rerouted.
    pub(crate) fn void_slips(
        &mut self,
        order: Id<Order>,
        lines: &BTreeSet<Id<Line>>,
        now: Timestamp,
    ) -> Result<Vec<(Id<Job>, Printer, Doc)>, RuntimeError> {
        let Some(kitchen) = self.profile.kitchen().cloned() else { return Ok(Vec::new()) };
        let mut slips = Vec::new();
        for summary in self.store.order_kitchen_tickets(order)? {
            let ticket = self.store.load(KitchenTicket::new(summary.id))?;
            let Some((sent, _)) = ticket.sent() else { continue };
            let Some(station) = kitchen.station(sent.station) else { continue };
            let printer = match ticket.rerouted() {
                Some((RerouteTo::StationPrinter, _)) => station.printer.clone(),
                Some((RerouteTo::FallbackPrinter, _)) => kitchen.fallback_printer.clone(),
                _ if !station.screen => station.printer.clone(),
                _ => None,
            };
            let Some(printer) = printer else { continue };
            let voided: Vec<KitchenItem> =
                sent.items.iter().filter(|item| lines.contains(&item.line)).cloned().collect();
            if voided.is_empty() {
                continue;
            }
            let slip = TicketSent { items: voided, ..sent.clone() };
            let at = self.profile.business_day().local_time(now)?;
            slips.push((
                self.ids.generate(now)?,
                printer,
                ticket_doc(station, &slip, at, Some("VOID"))?,
            ));
        }
        Ok(slips)
    }

    /// What the events of the system's `component` record now: no team member, and the log
    /// settled, as any write needs.
    pub(crate) fn system_meta(&mut self, component: &str) -> Result<Meta, RuntimeError> {
        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now())) {
            return Err(RuntimeError::NotSettled);
        }
        let now = self.clock.now();
        let component = Component::new(component).map_err(|_| RuntimeError::Time)?;
        Ok(Meta {
            now,
            business_date: self.profile.business_day().business_date_of(now)?,
            actor: Actor::System(component),
            correlation: self.ids.generate(now)?,
            approval: None,
        })
    }
}

/// Writes `firing` in `w`: fires its lines on its order, and sends each ticket, enqueuing the
/// print jobs of those no screen shows.
pub(crate) fn write_firing<S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    firing: &Firing,
    location: Id<keel_events::envelope::Location>,
    device: Id<Device>,
    meta: &Meta,
) -> Result<(), RuntimeError> {
    // The lines fire whether or not any station makes them: water no cook makes is sent too.
    // Found in review: with no ticket, nothing fired.
    let loaded = w.load(Order::new(firing.order))?;
    let event = loaded.decide(location, device, OrderCommand::FireLines(firing.lines.clone()))?;
    append(w, firing.order.cast(), &event, meta)?;
    for planned in &firing.tickets {
        let ticket = w.load(KitchenTicket::new(planned.ticket))?;
        let event = ticket.decide(location, KitchenCommand::Send(planned.sent.clone()))?;
        let sent = append(w, planned.ticket.cast(), &event, meta)?;
        if let Some((job, printer, doc)) = &planned.print {
            w.enqueue(&print::kitchen_effect(*job, doc, printer, Some(sent))?, meta.now)?;
        }
    }
    Ok(())
}
