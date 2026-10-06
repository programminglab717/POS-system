//! Views: what a shell shows, whole, each amount with the text a person reads (ADR-0023,
//! decisions 2 and 5).

use keel_domain::codec::Name;
use keel_domain::order::{ChosenModifier, Line, Order, OrderStatus, Prefix};
use keel_domain::profile::{CatalogGroup, Profile};
use keel_domain::refs::{Modifier, ModifierGroup, Variant};
use keel_pricing::Tax;
use keel_types::{Id, Locale, Money, Quantity};

/// An amount, and how the locale shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amount {
    /// The amount.
    pub money: Money,
    /// How the locale shows it, such as `$4.50`.
    pub text: String,
}

impl Amount {
    pub(crate) fn new(money: Money, locale: Locale) -> Amount {
        Amount { money, text: locale.money(money) }
    }
}

/// Where an order is in its life, as its ticket shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketState {
    /// Open for changes and payment.
    Open,
    /// Paid and closed.
    Closed,
    /// Voided as a whole.
    Voided,
    /// Dropped before anything in it was sent to be prepared.
    Abandoned,
}

impl TicketState {
    pub(crate) const fn of(status: &OrderStatus) -> TicketState {
        match status {
            OrderStatus::Active => TicketState::Open,
            OrderStatus::Closed => TicketState::Closed,
            OrderStatus::Voided(_) => TicketState::Voided,
            OrderStatus::Abandoned => TicketState::Abandoned,
        }
    }
}

/// An order as the register shows it: its lines, what it costs, and what it still owes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    /// The order.
    pub order: Id<Order>,
    /// Where it is in its life.
    pub state: TicketState,
    /// Its live lines, in the order they were added.
    pub lines: Vec<TicketLine>,
    /// The lines' amounts before tax, added up.
    pub subtotal: Amount,
    /// Each tax with something to tax, in the location's order of taxes.
    pub taxes: Vec<TicketTax>,
    /// What the order costs: its subtotal and its taxes.
    pub total: Amount,
    /// What payments have covered.
    pub paid: Amount,
    /// What it still owes.
    pub due: Amount,
    /// What it still owes in cash, rounded as the location rounds cash.
    pub cash_due: Amount,
}

/// A line of a ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketLine {
    /// The line.
    pub line: Id<Line>,
    /// What it sells, such as "Latte (Large)".
    pub name: Name,
    /// How many.
    pub quantity: Quantity,
    /// How many, as the locale shows it.
    pub quantity_text: String,
    /// Its modifiers, each followed by those chosen under it.
    pub modifiers: Vec<TicketModifier>,
    /// Its price with its modifiers, for its quantity, before tax.
    pub amount: Amount,
}

/// A modifier on a ticket's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketModifier {
    /// How deep it is: 1 for a modifier of the line, 2 for one chosen under it, and so on.
    pub depth: u8,
    /// The modifier.
    pub modifier: Id<Modifier>,
    /// How it changes the item.
    pub prefix: Prefix,
    /// Its name.
    pub name: Name,
    /// How many times it is applied.
    pub quantity: u8,
    /// The price of one application: zero when it is free.
    pub price: Amount,
}

/// A tax on a ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketTax {
    /// The tax.
    pub tax: Id<Tax>,
    /// Its name, such as "NYC sales tax".
    pub name: String,
    /// The tax.
    pub amount: Amount,
}

/// The modifiers of a line, each followed by those chosen under it, `depth` deep.
pub(crate) fn ticket_modifiers(
    modifiers: &[ChosenModifier],
    depth: u8,
    locale: Locale,
    out: &mut Vec<TicketModifier>,
) {
    for chosen in modifiers {
        out.push(TicketModifier {
            depth,
            modifier: chosen.modifier,
            prefix: chosen.prefix,
            name: chosen.name.clone(),
            quantity: chosen.quantity.get(),
            price: Amount::new(chosen.unit_price, locale),
        });
        ticket_modifiers(&chosen.modifiers, depth.saturating_add(1), locale, out);
    }
}

/// The menu: its pages of buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    /// Its pages, in order.
    pub pages: Vec<MenuPage>,
}

/// A page of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuPage {
    /// Its name.
    pub name: Name,
    /// Its buttons, in order.
    pub buttons: Vec<MenuButton>,
}

/// A button on the menu: a variant to ring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuButton {
    /// The variant.
    pub variant: Id<Variant>,
    /// Its name, such as "Latte (Large)".
    pub name: Name,
    /// Its price, without modifiers.
    pub price: Amount,
    /// Whether the item offers modifiers.
    pub choices: bool,
    /// Whether some of them must be chosen: the register asks before it rings.
    pub required: bool,
}

/// An item's variant, with the modifier groups offered for it: what the register asks for
/// before it rings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemView {
    /// The variant.
    pub variant: Id<Variant>,
    /// Its name.
    pub name: Name,
    /// Its price, without modifiers.
    pub price: Amount,
    /// The groups offered for it, in order.
    pub groups: Vec<GroupView>,
}

/// A modifier group offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupView {
    /// The group.
    pub group: Id<ModifierGroup>,
    /// Its name.
    pub name: Name,
    /// The fewest applications it takes.
    pub min: u8,
    /// The most.
    pub max: u8,
    /// How many are free.
    pub free: u8,
    /// Whether a modifier may be applied more than once.
    pub repeat: bool,
    /// Its modifiers, in order.
    pub modifiers: Vec<ModifierView>,
}

/// A modifier offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModifierView {
    /// The modifier.
    pub modifier: Id<Modifier>,
    /// Its name.
    pub name: Name,
    /// How it changes the item.
    pub prefix: Prefix,
    /// The price of one application.
    pub price: Amount,
    /// The groups offered under it.
    pub groups: Vec<GroupView>,
}

/// The views of the groups `groups` of `profile`.
pub(crate) fn group_views(
    profile: &Profile,
    groups: &[Id<ModifierGroup>],
    locale: Locale,
) -> Vec<GroupView> {
    groups
        .iter()
        .filter_map(|&id| profile.group(id))
        .map(|group: &CatalogGroup| GroupView {
            group: group.id,
            name: group.name.clone(),
            min: group.min,
            max: group.max.get(),
            free: group.free,
            repeat: group.repeat,
            modifiers: group
                .modifiers
                .iter()
                .map(|modifier| ModifierView {
                    modifier: modifier.id,
                    name: modifier.name.clone(),
                    prefix: modifier.prefix,
                    price: Amount::new(modifier.price, locale),
                    groups: group_views(profile, &modifier.groups, locale),
                })
                .collect(),
        })
        .collect()
}

/// What paying cash did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashPaid {
    /// The change to hand back.
    pub change: Amount,
    /// The order's ticket afterwards.
    pub ticket: Ticket,
}
