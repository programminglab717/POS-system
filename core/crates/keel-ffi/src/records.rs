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
    /// Served at a drive-through window.
    DriveThru,
    /// Brought out to the customer's car.
    Curbside,
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
            Mode::DriveThru => order::Mode::DriveThru,
            Mode::Curbside => order::Mode::Curbside,
        }
    }
}

impl From<order::Mode> for Mode {
    fn from(mode: order::Mode) -> Mode {
        match mode {
            order::Mode::DineIn => Mode::DineIn,
            order::Mode::Takeout => Mode::Takeout,
            order::Mode::Pickup => Mode::Pickup,
            order::Mode::Delivery => Mode::Delivery,
            order::Mode::Ship => Mode::Ship,
            order::Mode::Service => Mode::Service,
            order::Mode::DriveThru => Mode::DriveThru,
            order::Mode::Curbside => Mode::Curbside,
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
    /// Its checks, each with its own lines and amounts, when it is split among more than one;
    /// empty otherwise.
    pub checks: Vec<TicketCheck>,
    /// The device that owns it, which alone changes it: compare `Register::device_id`.
    pub owner: Option<String>,
    /// The devices that asked the hub for it, whose requests wait for an answer.
    pub asking: Vec<String>,
}

/// One check of a split order (ADR-0030): the check's lines, each with its part, and its
/// amounts.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketCheck {
    /// The check: what `pay_check_cash` and `move_line` take.
    pub check: String,
    /// Its number in the order, from 1.
    pub number: u32,
    /// Open, or closed once paid.
    pub state: TicketState,
    /// Its lines, each with the check's part of it.
    pub lines: Vec<TicketLine>,
    /// What the whole order's discounts took of it.
    pub discounts: Vec<TicketDiscount>,
    /// Its lines' amounts before tax, less every discount.
    pub subtotal: Amount,
    /// Each tax with something to tax on it.
    pub taxes: Vec<TicketTax>,
    /// What it costs.
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
    /// The price staff set for it, if they did.
    pub repriced: Option<TicketRepriced>,
    /// Its notes, for the kitchen and the receipt.
    pub notes: Option<String>,
    /// Whether it was sent to the kitchen: it is voided, not taken off or changed (ADR-0034).
    pub sent: bool,
}

/// A line's price as staff set it, and as it was rung (ADR-0030).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TicketRepriced {
    /// The unit price it is sold at now.
    pub unit_price: Amount,
    /// The unit price it was rung at.
    pub rung: Amount,
    /// Why: the merchant's reason code.
    pub reason: String,
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

impl From<runtime::TicketState> for TicketState {
    fn from(state: runtime::TicketState) -> TicketState {
        match state {
            runtime::TicketState::Open => TicketState::Open,
            runtime::TicketState::Closed => TicketState::Closed,
            runtime::TicketState::Voided => TicketState::Voided,
            runtime::TicketState::Abandoned => TicketState::Abandoned,
        }
    }
}

impl From<&runtime::TicketLine> for TicketLine {
    fn from(line: &runtime::TicketLine) -> TicketLine {
        TicketLine {
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
            repriced: line.repriced.as_ref().map(|repriced| TicketRepriced {
                unit_price: (&repriced.unit_price).into(),
                rung: (&repriced.rung).into(),
                reason: repriced.reason.as_str().to_owned(),
            }),
            notes: line.notes.as_ref().map(|notes| notes.as_str().to_owned()),
            sent: line.sent,
        }
    }
}

impl From<&runtime::TicketTax> for TicketTax {
    fn from(tax: &runtime::TicketTax) -> TicketTax {
        TicketTax { tax: tax.tax.to_string(), name: tax.name.clone(), amount: (&tax.amount).into() }
    }
}

impl From<&runtime::TicketCheck> for TicketCheck {
    fn from(check: &runtime::TicketCheck) -> TicketCheck {
        let ticket = &check.ticket;
        TicketCheck {
            check: check.check.to_string(),
            number: check.number,
            state: ticket.state.into(),
            lines: ticket.lines.iter().map(TicketLine::from).collect(),
            discounts: ticket.discounts.iter().map(TicketDiscount::from).collect(),
            subtotal: (&ticket.subtotal).into(),
            taxes: ticket.taxes.iter().map(TicketTax::from).collect(),
            total: (&ticket.total).into(),
            paid: (&ticket.paid).into(),
            due: (&ticket.due).into(),
            cash_due: (&ticket.cash_due).into(),
        }
    }
}

impl From<&runtime::Ticket> for Ticket {
    fn from(ticket: &runtime::Ticket) -> Ticket {
        Ticket {
            order: ticket.order.to_string(),
            state: ticket.state.into(),
            lines: ticket.lines.iter().map(TicketLine::from).collect(),
            discounts: ticket.discounts.iter().map(TicketDiscount::from).collect(),
            subtotal: (&ticket.subtotal).into(),
            taxes: ticket.taxes.iter().map(TicketTax::from).collect(),
            total: (&ticket.total).into(),
            paid: (&ticket.paid).into(),
            due: (&ticket.due).into(),
            cash_due: (&ticket.cash_due).into(),
            checks: ticket.checks.iter().map(TicketCheck::from).collect(),
            owner: ticket.owner.map(|device| device.to_string()),
            asking: ticket.asking.iter().map(ToString::to_string).collect(),
        }
    }
}

/// Which day report (ADR-0031): the day so far, or its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReportKind {
    /// The day so far, at any time.
    X,
    /// The day's end, once the register's drawers of the day are closed.
    Z,
}

/// A business day's report, from what the register holds.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DayReport {
    /// X or Z.
    pub kind: ReportKind,
    /// The business day, `YYYY-MM-DD`.
    pub date: String,
    /// The orders closed.
    pub orders: u64,
    /// What their lines came to before discounts and comps.
    pub gross: Amount,
    /// What discounts took.
    pub discounts: Amount,
    /// What comps took.
    pub comps: Amount,
    /// What the lines were sold for, before tax.
    pub net: Amount,
    /// The tax charged.
    pub tax: Amount,
    /// What the orders came to.
    pub total: Amount,
    /// What each tender captured: `cash` or `card`, how many payments, how much.
    pub tenders: Vec<TenderLine>,
    /// The orders voided, or dropped once they had lines.
    pub voided: u64,
    /// Each drawer session of the day, of every register.
    pub drawers: Vec<DrawerView>,
    /// Cash captured with no drawer open.
    pub cash_without_drawer: Amount,
    /// How many cash payments were captured with no drawer open.
    pub payments_without_drawer: u64,
    /// What each tender refunded on the day (ADR-0032): `cash` or `card`, how many refunds, how
    /// much was handed back.
    pub refunds: Vec<TenderLine>,
    /// Cash handed back with no drawer open.
    pub cash_refunded_without_drawer: Amount,
    /// How many cash refunds were handed back with no drawer open.
    pub refunds_without_drawer: u64,
}

/// What one tender captured on a day.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TenderLine {
    /// `cash` or `card`.
    pub tender: String,
    /// How many payments.
    pub payments: u64,
    /// What they captured, tips included.
    pub amount: Amount,
}

impl From<&runtime::TenderLine> for TenderLine {
    fn from(line: &runtime::TenderLine) -> TenderLine {
        TenderLine {
            tender: match line.tender {
                keel_domain::payment::Tender::Cash => "cash",
                keel_domain::payment::Tender::Card => "card",
            }
            .to_owned(),
            payments: line.payments,
            amount: (&line.amount).into(),
        }
    }
}

impl From<ReportKind> for runtime::ReportKind {
    fn from(kind: ReportKind) -> runtime::ReportKind {
        match kind {
            ReportKind::X => runtime::ReportKind::X,
            ReportKind::Z => runtime::ReportKind::Z,
        }
    }
}

impl From<&runtime::DayReport> for DayReport {
    fn from(report: &runtime::DayReport) -> DayReport {
        DayReport {
            kind: match report.kind {
                runtime::ReportKind::X => ReportKind::X,
                runtime::ReportKind::Z => ReportKind::Z,
            },
            date: report.date.to_string(),
            orders: report.orders,
            gross: (&report.gross).into(),
            discounts: (&report.discounts).into(),
            comps: (&report.comps).into(),
            net: (&report.net).into(),
            tax: (&report.tax).into(),
            total: (&report.total).into(),
            tenders: report.tenders.iter().map(TenderLine::from).collect(),
            voided: report.voided,
            drawers: report.drawers.iter().map(DrawerView::from).collect(),
            cash_without_drawer: (&report.cash_without_drawer).into(),
            payments_without_drawer: report.payments_without_drawer,
            refunds: report.refunds.iter().map(TenderLine::from).collect(),
            cash_refunded_without_drawer: (&report.cash_refunded_without_drawer).into(),
            refunds_without_drawer: report.refunds_without_drawer,
        }
    }
}

/// How cash moves in or out of a drawer other than by a sale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CashMovement {
    /// Cash put in.
    PaidIn,
    /// Cash taken out for an expense.
    PaidOut,
    /// Cash taken to the safe.
    Drop,
}

/// The register's drawer session (ADR-0031): its float and how many no-sales; once closed, what
/// moved through it, what it should have held, what was counted, and the variance.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DrawerView {
    /// The session.
    pub drawer: String,
    /// Whether it is open.
    pub open: bool,
    /// The cash put in to begin.
    pub float: Amount,
    /// Once closed: what its cash sales left in it. Blind while open, as the rest.
    pub taken: Option<Amount>,
    /// Once closed: cash paid in.
    pub paid_in: Option<Amount>,
    /// Once closed: cash paid out.
    pub paid_out: Option<Amount>,
    /// Once closed: cash dropped to the safe.
    pub dropped: Option<Amount>,
    /// Once closed: cash handed back for refunds (ADR-0032).
    pub refunded: Option<Amount>,
    /// How many times it was opened with no sale.
    pub no_sales: u64,
    /// Once closed: what it should have held.
    pub expected: Option<Amount>,
    /// Once closed: what was counted.
    pub counted: Option<Amount>,
    /// Once closed: the count less what was expected, over when more than zero.
    pub variance: Option<Amount>,
}

impl From<&runtime::DrawerView> for DrawerView {
    fn from(view: &runtime::DrawerView) -> DrawerView {
        DrawerView {
            drawer: view.drawer.to_string(),
            open: view.open,
            float: (&view.float).into(),
            taken: view.taken.as_ref().map(Amount::from),
            paid_in: view.paid_in.as_ref().map(Amount::from),
            paid_out: view.paid_out.as_ref().map(Amount::from),
            dropped: view.dropped.as_ref().map(Amount::from),
            refunded: view.refunded.as_ref().map(Amount::from),
            no_sales: view.no_sales,
            expected: view.expected.as_ref().map(Amount::from),
            counted: view.counted.as_ref().map(Amount::from),
            variance: view.variance.as_ref().map(Amount::from),
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
    /// Whether it is sold out now (ADR-0033): the register doesn't ring it.
    pub sold_out: bool,
    /// When it is back on sale, in the location's time, as `2026-10-03 04:00`: none while it is
    /// on sale, or sold out until it is put back.
    pub back_at: Option<String>,
}

/// Whether an item sold out is, and when it is back, as the bindings show it.
fn sold_out(until: Option<runtime::SoldOutUntil>) -> (bool, Option<String>) {
    match until {
        None => (false, None),
        Some(runtime::SoldOutUntil(back)) => (true, back.map(|back| back.to_string())),
    }
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
                        .map(|button| {
                            let (sold_out, back_at) = sold_out(button.sold_out);
                            MenuButton {
                                variant: button.variant.to_string(),
                                name: button.name.to_string(),
                                price: (&button.price).into(),
                                choices: button.choices,
                                required: button.required,
                                sold_out,
                                back_at,
                            }
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
    /// Whether it is sold out now (ADR-0033): the register doesn't ring a line that chooses it.
    pub sold_out: bool,
    /// When it is back on sale, as [`MenuButton::back_at`] is.
    pub back_at: Option<String>,
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
                .map(|modifier| {
                    let (sold_out, back_at) = sold_out(modifier.sold_out);
                    ModifierOption {
                        modifier: modifier.modifier.to_string(),
                        name: modifier.name.to_string(),
                        prefix: modifier.prefix.into(),
                        price: (&modifier.price).into(),
                        sold_out,
                        back_at,
                    }
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
    /// A kitchen ticket, or a void slip, at the kitchen printer the job names (ADR-0034).
    Kitchen,
}

impl From<runtime::JobKind> for JobKind {
    fn from(kind: runtime::JobKind) -> JobKind {
        match kind {
            runtime::JobKind::Receipt => JobKind::Receipt,
            runtime::JobKind::Drawer => JobKind::Drawer,
            runtime::JobKind::Kitchen => JobKind::Kitchen,
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
    /// For a kitchen job, the printer it goes to: its host and port; none for the register's
    /// own printer.
    pub host: Option<String>,
    /// The kitchen printer's port.
    pub port: Option<u16>,
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

/// A line of a closed order, and what is left of it to return (ADR-0032).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReturnableLine {
    /// The line's identifier.
    pub line: String,
    /// What it sold.
    pub name: String,
    /// How much is left to return, as the locale shows it.
    pub left_text: String,
    /// How many are left, for a line sold by the each; `None` for one sold by measure, which is
    /// returned whole.
    pub left_count: Option<u32>,
    /// Whether anything is left of it to return.
    pub any_left: bool,
    /// What its order's checks charged it, its net and tax.
    pub charged: Amount,
}

/// A closed order's lines, and what was refunded of it (ADR-0032).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Returnable {
    /// The order's identifier.
    pub order: String,
    /// Its lines.
    pub lines: Vec<ReturnableLine>,
    /// What its returns refunded so far.
    pub refunded: Amount,
    /// Whether it was paid in cash alone, as a refund in cash needs.
    pub cash_only: bool,
}

/// How much of a line to return: `count` of one sold by the each, or, with `count` absent, all
/// that is left of it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReturnLine {
    /// The line's identifier.
    pub line: String,
    /// How many, for a line sold by the each; absent for all that is left.
    pub count: Option<u32>,
}

/// A line returned, and what it refunded.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RefundedLine {
    /// What it sold.
    pub name: String,
    /// How much was returned, as the locale shows it.
    pub quantity_text: String,
    /// What it refunded: its net and tax.
    pub amount: Amount,
}

/// What a return did (ADR-0032).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Refunded {
    /// The refund's identifier.
    pub refund: String,
    /// Each line returned.
    pub lines: Vec<RefundedLine>,
    /// What the return refunds.
    pub amount: Amount,
    /// What was handed back in cash, rounded as the location rounds cash.
    pub handed: Amount,
}

impl From<&runtime::Returnable> for Returnable {
    fn from(returnable: &runtime::Returnable) -> Returnable {
        Returnable {
            order: returnable.order.to_string(),
            lines: returnable
                .lines
                .iter()
                .map(|line| ReturnableLine {
                    line: line.line.to_string(),
                    name: line.name.to_string(),
                    left_text: line.left_text.clone(),
                    left_count: line.left_count,
                    any_left: line.left.is_positive(),
                    charged: (&line.charged).into(),
                })
                .collect(),
            refunded: (&returnable.refunded).into(),
            cash_only: returnable.cash_only,
        }
    }
}

impl From<&runtime::Refunded> for Refunded {
    fn from(refunded: &runtime::Refunded) -> Refunded {
        Refunded {
            refund: refunded.refund.to_string(),
            lines: refunded
                .lines
                .iter()
                .map(|line| RefundedLine {
                    name: line.name.to_string(),
                    quantity_text: line.quantity_text.clone(),
                    amount: (&line.amount).into(),
                })
                .collect(),
            amount: (&refunded.amount).into(),
            handed: (&refunded.handed).into(),
        }
    }
}

/// What kind of item an 86 names (ADR-0033).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ItemKind {
    /// A variant: a button on the menu.
    Variant,
    /// A modifier: an option in an item's groups.
    Modifier,
}

/// How long an 86 lasts (ADR-0033).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Until {
    /// The rest of the business day: back on sale when the next begins.
    EndOfDay,
    /// Until it is put back.
    PutBack,
    /// Until a time, in microseconds since the Unix epoch.
    Time {
        /// The time.
        micros: i64,
    },
}

/// An item sold out now (ADR-0033).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SoldOut {
    /// The variant or the modifier.
    pub item: String,
    /// Which it is.
    pub kind: ItemKind,
    /// Its name.
    pub name: String,
    /// When it is back on sale, as [`MenuButton::back_at`] is.
    pub back_at: Option<String>,
}

impl From<&runtime::SoldOut> for SoldOut {
    fn from(sold_out: &runtime::SoldOut) -> SoldOut {
        let (item, kind) = match sold_out.item {
            runtime::ItemRef::Variant(variant) => (variant.to_string(), ItemKind::Variant),
            runtime::ItemRef::Modifier(modifier) => (modifier.to_string(), ItemKind::Modifier),
        };
        SoldOut {
            item,
            kind,
            name: sold_out.name.to_string(),
            back_at: sold_out.until.map(|until| until.to_string()),
        }
    }
}

/// A kitchen station, as a screen's settings list them (ADR-0034).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KitchenStation {
    /// The station.
    pub station: String,
    /// Its name, such as "Grill".
    pub name: String,
    /// Whether it is an expo station: it gets every line of every order.
    pub expo: bool,
    /// Whether a screen shows it.
    pub screen: bool,
}

/// How old a ticket is, by the kitchen's thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Aging {
    /// Younger than late-ish.
    Fresh,
    /// Late-ish.
    LateIsh,
    /// Late.
    Late,
}

impl From<runtime::Aging> for Aging {
    fn from(aging: runtime::Aging) -> Aging {
        match aging {
            runtime::Aging::Fresh => Aging::Fresh,
            runtime::Aging::LateIsh => Aging::LateIsh,
            runtime::Aging::Late => Aging::Late,
        }
    }
}

/// Where a ticket no screen showed was sent instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Rerouted {
    /// Its station's printer.
    StationPrinter,
    /// The kitchen's fallback printer.
    FallbackPrinter,
    /// Nowhere: there was no printer.
    Nowhere,
}

impl From<keel_domain::kitchen::RerouteTo> for Rerouted {
    fn from(to: keel_domain::kitchen::RerouteTo) -> Rerouted {
        match to {
            keel_domain::kitchen::RerouteTo::StationPrinter => Rerouted::StationPrinter,
            keel_domain::kitchen::RerouteTo::FallbackPrinter => Rerouted::FallbackPrinter,
            keel_domain::kitchen::RerouteTo::Nowhere => Rerouted::Nowhere,
        }
    }
}

/// A modifier of an item on a kitchen screen.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScreenModifier {
    /// Its name.
    pub name: String,
    /// How it changes the item: "No" and "Extra" are shown apart.
    pub prefix: Prefix,
    /// How many times it is applied.
    pub quantity: u8,
    /// How deep it is: 1 for one chosen on the item.
    pub depth: u8,
}

/// An item of a ticket on a kitchen screen.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScreenItem {
    /// The order's line it makes.
    pub line: String,
    /// What to make.
    pub name: String,
    /// How many, as "2 x", or a measure, as "0.250 kg".
    pub quantity: String,
    /// Its modifiers, each before those under it.
    pub modifiers: Vec<ScreenModifier>,
    /// The line's notes.
    pub notes: Option<String>,
    /// The allergens it contains, by name, such as "milk".
    pub allergens: Vec<String>,
    /// For a component, the line's name.
    pub component_of: Option<String>,
    /// Whether its line, or its order, was voided since.
    pub void: bool,
    /// On an expo station, whether every station making it has bumped it.
    pub ready: Option<bool>,
}

/// A ticket on a kitchen screen.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScreenTicket {
    /// The ticket.
    pub ticket: String,
    /// Its station.
    pub station: String,
    /// Its station's name.
    pub station_name: String,
    /// Its order.
    pub order: String,
    /// Its order's code, as its receipt prints it.
    pub code: String,
    /// How the order reaches its customer.
    pub mode: Mode,
    /// When it was sent, in the location's time, as `2026-10-02 09:00`.
    pub sent_at: String,
    /// How long ago, in whole seconds.
    pub age_seconds: u64,
    /// How old that is.
    pub aging: Aging,
    /// What to make.
    pub items: Vec<ScreenItem>,
    /// Where it was rerouted, if no screen showed it in time.
    pub rerouted: Option<Rerouted>,
}

/// A station's day so far.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StationTimes {
    /// The station.
    pub station: String,
    /// Its name.
    pub name: String,
    /// How many tickets it bumped today.
    pub bumped: u32,
    /// Their average time from sent to bumped, in whole seconds.
    pub average_seconds: Option<u64>,
}

/// What a kitchen screen shows (ADR-0034).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KitchenScreen {
    /// Its stations' tickets not yet bumped, the earliest sent first.
    pub tickets: Vec<ScreenTicket>,
    /// The ticket bumped last today, which Recall brings back.
    pub last_bumped: Option<String>,
    /// Each station's day so far.
    pub times: Vec<StationTimes>,
}

impl From<&runtime::KitchenScreen> for KitchenScreen {
    fn from(screen: &runtime::KitchenScreen) -> KitchenScreen {
        KitchenScreen {
            tickets: screen.tickets.iter().map(ScreenTicket::from).collect(),
            last_bumped: screen.last_bumped.map(|ticket| ticket.to_string()),
            times: screen
                .times
                .iter()
                .map(|times| StationTimes {
                    station: times.station.to_string(),
                    name: times.name.to_string(),
                    bumped: times.bumped,
                    average_seconds: times.average,
                })
                .collect(),
        }
    }
}

impl From<&runtime::ScreenTicket> for ScreenTicket {
    fn from(ticket: &runtime::ScreenTicket) -> ScreenTicket {
        ScreenTicket {
            ticket: ticket.ticket.to_string(),
            station: ticket.station.to_string(),
            station_name: ticket.station_name.to_string(),
            order: ticket.order.to_string(),
            code: ticket.code.clone(),
            mode: ticket.mode.into(),
            sent_at: ticket.sent_at.to_string(),
            age_seconds: ticket.age,
            aging: ticket.aging.into(),
            items: ticket
                .items
                .iter()
                .map(|shown| ScreenItem {
                    line: shown.item.line.to_string(),
                    name: shown.item.name.to_string(),
                    quantity: runtime::how_many(shown.item.quantity),
                    modifiers: shown
                        .item
                        .modifiers
                        .iter()
                        .map(|modifier| ScreenModifier {
                            name: modifier.name.to_string(),
                            prefix: modifier.prefix.into(),
                            quantity: modifier.quantity.get(),
                            depth: modifier.depth.get(),
                        })
                        .collect(),
                    notes: shown.item.notes.as_ref().map(|notes| notes.as_str().to_owned()),
                    allergens: shown
                        .item
                        .allergens
                        .iter()
                        .map(|allergen| allergen.name().to_owned())
                        .collect(),
                    component_of: shown
                        .item
                        .component_of
                        .as_ref()
                        .map(|name| name.as_str().to_owned()),
                    void: shown.void,
                    ready: shown.ready,
                })
                .collect(),
            rerouted: ticket.rerouted.map(Rerouted::from),
        }
    }
}

/// A ticket no screen showed in time, as staff are told.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KitchenAlert {
    /// The ticket.
    pub ticket: String,
    /// Its station's name.
    pub station: String,
    /// Its order's code.
    pub code: String,
    /// Where it was sent instead.
    pub to: Rerouted,
    /// When, in the location's time.
    pub at: String,
}

impl From<&runtime::KitchenAlert> for KitchenAlert {
    fn from(alert: &runtime::KitchenAlert) -> KitchenAlert {
        KitchenAlert {
            ticket: alert.ticket.to_string(),
            station: alert.station.to_string(),
            code: alert.code.clone(),
            to: alert.to.into(),
            at: alert.at.to_string(),
        }
    }
}
