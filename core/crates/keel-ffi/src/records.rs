//! What crosses the boundary (ADR-0025, decision 3): records and enumerations, never the
//! kernel's types. Identifiers are their canonical text; amounts their minor units, currency and
//! text.

use keel_domain::order;
use keel_runtime as runtime;

/// An amount, and how the locale shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Amount {
    /// The amount, in the currency's minor units.
    pub minor: i64,
    /// The currency's ISO 4217 code.
    pub currency: String,
    /// How the locale shows it, such as `$4.50`.
    pub text: String,
}

impl From<&runtime::Amount> for Amount {
    fn from(amount: &runtime::Amount) -> Amount {
        Amount {
            minor: amount.money.minor(),
            currency: amount.money.currency().code().to_owned(),
            text: amount.text.clone(),
        }
    }
}

/// Where an order is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
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

/// How a modifier changes an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Prefix {
    /// As it is, such as "oat milk".
    Plain,
    /// Left out.
    No,
    /// More of it.
    Extra,
    /// Less of it.
    Light,
    /// On the side.
    Side,
    /// In place of a default part.
    Sub,
}

impl From<order::Prefix> for Prefix {
    fn from(prefix: order::Prefix) -> Prefix {
        match prefix {
            order::Prefix::Plain => Prefix::Plain,
            order::Prefix::No => Prefix::No,
            order::Prefix::Extra => Prefix::Extra,
            order::Prefix::Light => Prefix::Light,
            order::Prefix::Side => Prefix::Side,
            order::Prefix::Sub => Prefix::Sub,
        }
    }
}

/// How an order is fulfilled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Mode {
    /// Eaten or used on the premises.
    DineIn,
    /// Taken away at once.
    Takeout,
    /// Collected later.
    Pickup,
    /// Delivered locally.
    Delivery,
    /// Shipped.
    Ship,
    /// A service performed for the customer.
    Service,
}

impl From<Mode> for order::Mode {
    fn from(mode: Mode) -> order::Mode {
        match mode {
            Mode::DineIn => order::Mode::DineIn,
            Mode::Takeout => order::Mode::Takeout,
            Mode::Pickup => order::Mode::Pickup,
            Mode::Delivery => order::Mode::Delivery,
            Mode::Ship => order::Mode::Ship,
            Mode::Service => order::Mode::Service,
        }
    }
}

/// An order as the register shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Ticket {
    /// The order.
    pub order: String,
    /// Where it is in its life.
    pub state: TicketState,
    /// Its live lines, in the order they were rung.
    pub lines: Vec<TicketLine>,
    /// The discounts on the whole order, in the order they were applied.
    pub discounts: Vec<TicketDiscount>,
    /// The lines' amounts before tax, less every discount, added up.
    pub subtotal: Amount,
    /// Each tax with something to tax, in the location's order of taxes.
    pub taxes: Vec<TicketTax>,
    /// What the order costs.
    pub total: Amount,
    /// What payments have covered.
    pub paid: Amount,
    /// What it still owes.
    pub due: Amount,
    /// What it still owes in cash, rounded as the location rounds cash.
    pub cash_due: Amount,
}

/// A line of a ticket.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketLine {
    /// The line.
    pub line: String,
    /// What it sells, such as "Latte (Large)".
    pub name: String,
    /// How many, in millionths.
    pub quantity_millionths: i64,
    /// How many, as the locale shows it.
    pub quantity_text: String,
    /// Its modifiers, each followed by those chosen under it.
    pub modifiers: Vec<TicketModifier>,
    /// Its price with its modifiers, for its quantity, before tax.
    pub amount: Amount,
    /// Its discounts, in the order they were applied.
    pub discounts: Vec<TicketDiscount>,
}

/// A discount on a ticket: on a line, or on the whole order (ADR-0030).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketDiscount {
    /// The discount: what `remove_discount` takes.
    pub discount: String,
    /// Why it was given: the merchant's reason code.
    pub reason: String,
    /// The percentage it takes, in basis points; `None` for an amount off.
    pub percent_basis_points: Option<u32>,
    /// What it took off, before tax.
    pub amount: Amount,
}

impl From<&runtime::TicketDiscount> for TicketDiscount {
    fn from(discount: &runtime::TicketDiscount) -> TicketDiscount {
        TicketDiscount {
            discount: discount.discount.to_string(),
            reason: discount.reason.as_str().to_owned(),
            percent_basis_points: discount.percent,
            amount: (&discount.amount).into(),
        }
    }
}

/// A modifier on a ticket's line.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketModifier {
    /// How deep it is: 1 for a modifier of the line, 2 for one chosen under it, and so on.
    pub depth: u8,
    /// The modifier.
    pub modifier: String,
    /// How it changes the item.
    pub prefix: Prefix,
    /// Its name.
    pub name: String,
    /// How many times it is applied.
    pub quantity: u8,
    /// The price of one application: zero when it is free.
    pub price: Amount,
}

/// A tax on a ticket.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketTax {
    /// The tax.
    pub tax: String,
    /// Its name.
    pub name: String,
    /// The tax.
    pub amount: Amount,
}

impl From<&runtime::Ticket> for Ticket {
    fn from(ticket: &runtime::Ticket) -> Ticket {
        Ticket {
            order: ticket.order.to_string(),
            state: match ticket.state {
                runtime::TicketState::Open => TicketState::Open,
                runtime::TicketState::Closed => TicketState::Closed,
                runtime::TicketState::Voided => TicketState::Voided,
                runtime::TicketState::Abandoned => TicketState::Abandoned,
            },
            lines: ticket
                .lines
                .iter()
                .map(|line| TicketLine {
                    line: line.line.to_string(),
                    name: line.name.to_string(),
                    quantity_millionths: line.quantity.micros(),
                    quantity_text: line.quantity_text.clone(),
                    modifiers: line
                        .modifiers
                        .iter()
                        .map(|modifier| TicketModifier {
                            depth: modifier.depth,
                            modifier: modifier.modifier.to_string(),
                            prefix: modifier.prefix.into(),
                            name: modifier.name.to_string(),
                            quantity: modifier.quantity,
                            price: (&modifier.price).into(),
                        })
                        .collect(),
                    amount: (&line.amount).into(),
                    discounts: line.discounts.iter().map(TicketDiscount::from).collect(),
                })
                .collect(),
            discounts: ticket.discounts.iter().map(TicketDiscount::from).collect(),
            subtotal: (&ticket.subtotal).into(),
            taxes: ticket
                .taxes
                .iter()
                .map(|tax| TicketTax {
                    tax: tax.tax.to_string(),
                    name: tax.name.clone(),
                    amount: (&tax.amount).into(),
                })
                .collect(),
            total: (&ticket.total).into(),
            paid: (&ticket.paid).into(),
            due: (&ticket.due).into(),
            cash_due: (&ticket.cash_due).into(),
        }
    }
}

/// What paying cash did.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CashPaid {
    /// The change to hand back.
    pub change: Amount,
    /// The order's ticket afterwards.
    pub ticket: Ticket,
}

/// The menu: its pages of buttons.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Menu {
    /// Its pages, in order.
    pub pages: Vec<MenuPage>,
}

/// A page of the menu.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MenuPage {
    /// Its name.
    pub name: String,
    /// Its buttons, in order.
    pub buttons: Vec<MenuButton>,
}

/// A button on the menu: a variant to ring.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MenuButton {
    /// The variant.
    pub variant: String,
    /// Its name, such as "Latte (Large)".
    pub name: String,
    /// Its price.
    pub price: Amount,
    /// Whether it has modifiers to choose.
    pub choices: bool,
    /// Whether some of them must be chosen.
    pub required: bool,
}

impl From<&runtime::MenuView> for Menu {
    fn from(menu: &runtime::MenuView) -> Menu {
        Menu {
            pages: menu
                .pages
                .iter()
                .map(|page| MenuPage {
                    name: page.name.to_string(),
                    buttons: page
                        .buttons
                        .iter()
                        .map(|button| MenuButton {
                            variant: button.variant.to_string(),
                            name: button.name.to_string(),
                            price: (&button.price).into(),
                            choices: button.choices,
                            required: button.required,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// An item's variant, and the modifier groups offered for it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Item {
    /// The variant.
    pub variant: String,
    /// Its name.
    pub name: String,
    /// Its price.
    pub price: Amount,
    /// Every group offered for it or under its modifiers, each followed by those offered under
    /// its modifiers.
    pub groups: Vec<Group>,
}

/// A modifier group, as an item shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Group {
    /// The group.
    pub group: String,
    /// The modifier it is offered under, or none for one the item offers.
    pub under: Option<String>,
    /// How deep it is: 1 for a group the item offers, 2 for one under its modifiers, and so on.
    pub depth: u8,
    /// Its name.
    pub name: String,
    /// The fewest applications of its modifiers a line may have.
    pub min: u8,
    /// The most.
    pub max: u8,
    /// How many are free: the lowest-priced that cost something.
    pub free: u8,
    /// Whether a modifier may be applied more than once.
    pub repeat: bool,
    /// Its modifiers, in order.
    pub modifiers: Vec<ModifierOption>,
}

/// A modifier a group offers.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ModifierOption {
    /// The modifier.
    pub modifier: String,
    /// Its name.
    pub name: String,
    /// How it changes the item.
    pub prefix: Prefix,
    /// The price of one application.
    pub price: Amount,
}

impl From<&runtime::ItemView> for Item {
    fn from(item: &runtime::ItemView) -> Item {
        let mut groups = Vec::new();
        flatten(&item.groups, None, 1, &mut groups);
        Item {
            variant: item.variant.to_string(),
            name: item.name.to_string(),
            price: (&item.price).into(),
            groups,
        }
    }
}

/// `groups`, each followed by the groups under its modifiers, `depth` deep.
fn flatten(groups: &[runtime::GroupView], under: Option<&str>, depth: u8, out: &mut Vec<Group>) {
    for group in groups {
        out.push(Group {
            group: group.group.to_string(),
            under: under.map(str::to_owned),
            depth,
            name: group.name.to_string(),
            min: group.min,
            max: group.max,
            free: group.free,
            repeat: group.repeat,
            modifiers: group
                .modifiers
                .iter()
                .map(|modifier| ModifierOption {
                    modifier: modifier.modifier.to_string(),
                    name: modifier.name.to_string(),
                    prefix: modifier.prefix.into(),
                    price: (&modifier.price).into(),
                })
                .collect(),
        });
        for modifier in &group.modifiers {
            let id = modifier.modifier.to_string();
            flatten(&modifier.groups, Some(&id), depth.saturating_add(1), out);
        }
    }
}

/// A cashier's choice of a modifier, flattened: the choices under a modifier name the index of
/// its choice, earlier in the list, as their parent.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Choice {
    /// The modifier.
    pub modifier: String,
    /// How many times it is applied: 1 or more.
    pub quantity: u8,
    /// The index of the choice it is chosen under, or none for a choice of the line.
    pub parent: Option<u32>,
}

/// How a printer says a job is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Confirm {
    /// `GS ( H`: once the job has printed.
    Printed,
    /// `GS r 1`: once it is processed.
    Processed,
    /// Nothing but its own close of the connection.
    Nothing,
}

impl From<Confirm> for keel_doc::Confirm {
    fn from(confirm: Confirm) -> keel_doc::Confirm {
        match confirm {
            Confirm::Printed => keel_doc::Confirm::Printed,
            Confirm::Processed => keel_doc::Confirm::Processed,
            Confirm::Nothing => keel_doc::Confirm::Nothing,
        }
    }
}

impl From<keel_doc::Confirm> for Confirm {
    fn from(confirm: keel_doc::Confirm) -> Confirm {
        match confirm {
            keel_doc::Confirm::Printed => Confirm::Printed,
            keel_doc::Confirm::Processed => Confirm::Processed,
            keel_doc::Confirm::Nothing => Confirm::Nothing,
        }
    }
}

/// The printer jobs go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Printer {
    /// The columns of a line: 48 for 80 mm paper at 203 dpi, 42 at 180 dpi, 32 for 58 mm.
    pub columns: u8,
    /// Whether it cuts the paper.
    pub cutter: bool,
    /// How it says a job is done.
    pub confirm: Confirm,
}

/// What a job prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JobKind {
    /// A receipt.
    Receipt,
    /// The cash drawer's pulse.
    Drawer,
}

impl From<runtime::JobKind> for JobKind {
    fn from(kind: runtime::JobKind) -> JobKind {
        match kind {
            runtime::JobKind::Receipt => JobKind::Receipt,
            runtime::JobKind::Drawer => JobKind::Drawer,
        }
    }
}

/// A job for the driver.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PrintJob {
    /// The job.
    pub job: String,
    /// What it prints.
    pub kind: JobKind,
    /// The bytes, rendered for the printer.
    pub bytes: Vec<u8>,
    /// How the printer says it is done.
    pub confirm: Confirm,
    /// The process ID the driver asks the printer to answer with: four bytes.
    pub process_id: Vec<u8>,
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JobState {
    /// Waiting for the printer, or to be tried again.
    Queued,
    /// With the driver.
    Sending,
    /// Some of it may have been sent: staff decide.
    Unconfirmed,
}

/// A job, as the register shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Job {
    /// The job.
    pub job: String,
    /// What it prints.
    pub kind: JobKind,
    /// Where it is.
    pub state: JobState,
    /// How many times it was handed to the driver.
    pub attempts: u32,
}

impl From<&runtime::JobView> for Job {
    fn from(view: &runtime::JobView) -> Job {
        Job {
            job: view.job.to_string(),
            kind: view.kind.into(),
            state: match view.state {
                runtime::JobState::Queued => JobState::Queued,
                runtime::JobState::Sending => JobState::Sending,
                runtime::JobState::Unconfirmed => JobState::Unconfirmed,
            },
            attempts: view.attempts,
        }
    }
}

/// What staff decide about an unconfirmed job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Resolution {
    /// It printed.
    Printed,
    /// Print it again, marked as a reprint.
    Again,
    /// Leave it.
    Dismiss,
}

impl From<Resolution> for runtime::Resolution {
    fn from(resolution: Resolution) -> runtime::Resolution {
        match resolution {
            Resolution::Printed => runtime::Resolution::Printed,
            Resolution::Again => runtime::Resolution::Again,
            Resolution::Dismiss => runtime::Resolution::Dismiss,
        }
    }
}

/// Why none of a job was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NotSent {
    /// The printer couldn't be reached.
    Unreachable,
    /// It didn't answer.
    NoAnswer,
    /// It answered with something that isn't a status.
    NotAStatus,
    /// Its cover is open.
    CoverOpen,
    /// It is out of paper.
    PaperOut,
    /// It has an error.
    Error,
    /// It is offline for another reason.
    Offline,
    /// The connection failed before any of the job was accepted.
    SendFailed,
}

/// Why a job isn't known to have printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Unconfirmed {
    /// The connection failed after some of the job was accepted.
    SendFailed,
    /// The printer closed the connection before its word.
    Closed,
    /// No word came in time.
    NoWord,
}

/// What happened to a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Outcome {
    /// It printed.
    Printed {
        /// The paper is near its end.
        paper_low: bool,
    },
    /// None of it was sent: it is tried again.
    NotSent {
        /// Why.
        reason: NotSent,
    },
    /// Some of it may have been sent: it is never sent again by itself.
    Unconfirmed {
        /// Why.
        reason: Unconfirmed,
    },
}

impl From<keel_print::Outcome> for Outcome {
    fn from(outcome: keel_print::Outcome) -> Outcome {
        match outcome {
            keel_print::Outcome::Printed { paper_low } => Outcome::Printed { paper_low },
            keel_print::Outcome::NotSent(reason) => Outcome::NotSent {
                reason: match reason {
                    keel_print::NotSent::Unreachable => NotSent::Unreachable,
                    keel_print::NotSent::NoAnswer => NotSent::NoAnswer,
                    keel_print::NotSent::NotAStatus(_) => NotSent::NotAStatus,
                    keel_print::NotSent::CoverOpen => NotSent::CoverOpen,
                    keel_print::NotSent::PaperOut => NotSent::PaperOut,
                    keel_print::NotSent::Error => NotSent::Error,
                    keel_print::NotSent::SendFailed => NotSent::SendFailed,
                    _ => NotSent::Offline,
                },
            },
            keel_print::Outcome::Unconfirmed(reason) => Outcome::Unconfirmed {
                reason: match reason {
                    keel_print::Unconfirmed::SendFailed => Unconfirmed::SendFailed,
                    keel_print::Unconfirmed::Closed => Unconfirmed::Closed,
                    // A reason added later can only be one where no word came.
                    _ => Unconfirmed::NoWord,
                },
            },
        }
    }
}

impl From<Outcome> for keel_print::Outcome {
    fn from(outcome: Outcome) -> keel_print::Outcome {
        match outcome {
            Outcome::Printed { paper_low } => keel_print::Outcome::Printed { paper_low },
            Outcome::NotSent { reason } => keel_print::Outcome::NotSent(match reason {
                NotSent::Unreachable => keel_print::NotSent::Unreachable,
                NotSent::NoAnswer => keel_print::NotSent::NoAnswer,
                NotSent::NotAStatus => keel_print::NotSent::NotAStatus(0),
                NotSent::CoverOpen => keel_print::NotSent::CoverOpen,
                NotSent::PaperOut => keel_print::NotSent::PaperOut,
                NotSent::Error => keel_print::NotSent::Error,
                NotSent::Offline => keel_print::NotSent::Offline,
                NotSent::SendFailed => keel_print::NotSent::SendFailed,
            }),
            Outcome::Unconfirmed { reason } => keel_print::Outcome::Unconfirmed(match reason {
                Unconfirmed::SendFailed => keel_print::Unconfirmed::SendFailed,
                Unconfirmed::Closed => keel_print::Unconfirmed::Closed,
                Unconfirmed::NoWord => keel_print::Unconfirmed::NoWord,
            }),
        }
    }
}

/// How long the driver waits, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Waits {
    /// For each answer to a status question.
    pub status_ms: u64,
    /// For the printer's word that a job printed, and for each write of it to make progress.
    pub word_ms: u64,
}

/// What a register opens with.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RegisterConfig {
    /// The store's path.
    pub path: String,
    /// The store's key: 32 bytes, which the shell keeps wrapped by the platform's keystore.
    pub store_key: Vec<u8>,
    /// The device's identifier.
    pub device: String,
    /// The device's signing key, a software Ed25519 key's 32 secret bytes.
    pub signing_key: Vec<u8>,
    /// The location profile's encoding.
    pub profile: Vec<u8>,
    /// The locale amounts are shown in, such as `en-US`.
    pub locale: String,
}

/// A new device's identity, which a shell keeps, its keys wrapped by the platform's keystore,
/// before it opens its first register.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NewDevice {
    /// The device's identifier.
    pub device: String,
    /// Its signing key: a software Ed25519 key's 32 secret bytes.
    pub signing_key: Vec<u8>,
    /// Its store's key: 32 random bytes.
    pub store_key: Vec<u8>,
}

/// A team member who may sign in.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Member {
    /// Their identifier, for [`crate::Register::sign_in`].
    pub id: String,
    /// The name the register shows for them.
    pub name: String,
    /// Their role's name, such as "Manager"; `None` in a profile without roles.
    pub role: Option<String>,
    /// Whether they sign in with a PIN: in a profile without roles, no one does.
    pub has_pin: bool,
}

/// What a device joins its location's network with: its kit's roster and TLS credentials.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SyncKit {
    /// The roster's encoding.
    pub roster: Vec<u8>,
    /// The location's CA's certificate, DER: the only one trusted.
    pub ca: Vec<u8>,
    /// The device's certificate, DER.
    pub certificate: Vec<u8>,
    /// The device's TLS key, PKCS #8 DER.
    pub tls_key: Vec<u8>,
}

/// How replication stands.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a fact of its own about replication, as the runtime reports them"
)]
pub struct SyncState {
    /// The register listens for its location's devices, as the hub's address.
    pub listening: bool,
    /// The device's own log is settled: until it is, intents are refused, `NotSettled`.
    pub settled: bool,
    /// A hub is reachable: when it isn't, the device is an island.
    pub hub_reachable: bool,
    /// This device holds the hub's role.
    pub is_hub: bool,
    /// This device serves as the hub now.
    pub serving: bool,
    /// How far into this device's own log the location's cloud is known to hold (ADR-0028): 0
    /// if it holds none of it, or the location has no cloud.
    pub backed_up: u64,
    /// How far this device's own log goes: what is past `backed_up` isn't backed up yet.
    pub head: u64,
    /// The version of the profile the register rings from, 64 hexadecimal digits: the one
    /// provisioned until the cloud publishes another. When it changes, the menu is read again.
    pub profile_version: String,
}
