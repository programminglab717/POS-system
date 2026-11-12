//! The receipt v0 (ADR-0024, decision 8), built from a closed order's ticket and its payments,
//! in the locale's language.

use keel_doc::{Align, Block, Doc, Style};
use keel_domain::codec::Name;
use keel_domain::order::Prefix;
use keel_domain::payment::Payment;
use keel_domain::profile::Profile;
use keel_types::{LocalTime, Locale, Money};

use crate::error::RuntimeError;
use crate::views::{Ticket, TicketDiscount};

/// The most bytes of blocks a receipt's lines take, well within an effect's payload: an order
/// with more lines than fit ends its list with how many more there are.
const MAX_LINES_BYTES: usize = 40_000;

/// The most blocks a receipt's lines take: a document holds 1,000, and the rest of a receipt, its
/// taxes included, and a reprint's two banners need the others.
const MAX_LINES_BLOCKS: usize = 800;

/// A receipt's words, in one language.
struct Words {
    order: &'static str,
    check: &'static str,
    cashier: &'static str,
    subtotal: &'static str,
    discount: &'static str,
    decimal: char,
    total: &'static str,
    cash: &'static str,
    rounding: &'static str,
    change: &'static str,
    thanks: &'static str,
    reprint: &'static str,
    more: &'static str,
    see_register: &'static str,
    prefixes: [&'static str; 6],
}

const ENGLISH: Words = Words {
    order: "Order",
    check: "Check",
    cashier: "Cashier",
    subtotal: "Subtotal",
    discount: "Discount",
    decimal: '.',
    total: "TOTAL",
    cash: "Cash",
    rounding: "Rounding",
    change: "Change",
    thanks: "Thank you!",
    reprint: "REPRINT",
    more: "more lines",
    see_register: "The register shows this order in full.",
    prefixes: ["", "No", "Extra", "Light", "Side", "Sub"],
};

const SPANISH: Words = Words {
    order: "Orden",
    check: "Cuenta",
    cashier: "Cajero",
    subtotal: "Subtotal",
    discount: "Descuento",
    decimal: ',',
    total: "TOTAL",
    cash: "Efectivo",
    rounding: "Redondeo",
    change: "Cambio",
    thanks: "¡Gracias!",
    reprint: "REIMPRESIÓN",
    more: "líneas más",
    see_register: "La caja muestra esta orden completa.",
    prefixes: ["", "Sin", "Extra", "Poco", "Aparte", "Cambio a"],
};

fn words(locale: Locale) -> &'static Words {
    if locale.tag().starts_with("es") { &SPANISH } else { &ENGLISH }
}

/// The banner a reprint carries above and below, in `locale`'s language.
pub(crate) fn reprint_banner(locale: Locale) -> Block {
    Block::text(words(locale).reprint, Align::Center, Style::LARGE)
}

/// Whether `block` is a reprint's banner, in any language a receipt is printed in.
pub(crate) fn is_banner(block: &Block) -> bool {
    [ENGLISH.reprint, SPANISH.reprint]
        .iter()
        .any(|reprint| *block == Block::text(*reprint, Align::Center, Style::LARGE))
}

/// What a receipt needs besides the ticket.
pub(crate) struct Closing<'a> {
    /// The order's payments.
    pub payments: &'a [Payment],
    /// Who closed it.
    pub cashier: Option<&'a Name>,
    /// When, in the location's time.
    pub at: LocalTime,
    /// Whether this is a reprint.
    pub reprint: bool,
    /// The check's number, for one check of a split order (ADR-0030).
    pub check: Option<u32>,
}

/// The receipt of the closed order `ticket` shows.
pub(crate) fn receipt(
    ticket: &Ticket,
    closing: &Closing<'_>,
    profile: &Profile,
    locale: Locale,
) -> Result<Doc, RuntimeError> {
    let words = words(locale);
    let money = |amount: Money| locale.money(amount);
    let mut blocks = Vec::new();
    if closing.reprint {
        blocks.push(reprint_banner(locale));
    }
    let data = profile.data();
    blocks.push(Block::text(data.name.as_str(), Align::Center, Style::LARGE));
    for line in &data.address {
        blocks.push(Block::text(line.as_str(), Align::Center, Style::default()));
    }
    blocks.push(Block::Rule);
    let id = ticket.order.to_string();
    let short = id.get(id.len().saturating_sub(6)..).unwrap_or_default().to_uppercase();
    blocks.push(Block::row(format!("{} {short}", words.order), closing.at.to_string()));
    if let Some(number) = closing.check {
        blocks.push(Block::row(format!("{} {number}", words.check), ""));
    }
    if let Some(cashier) = closing.cashier {
        blocks.push(Block::row(format!("{}: {cashier}", words.cashier), ""));
    }
    blocks.push(Block::Rule);
    blocks.extend(lines(ticket, words, locale)?);
    blocks.push(Block::Rule);
    for discount in &ticket.discounts {
        blocks.push(discount_row(discount, words, locale, 0)?);
    }
    blocks.push(Block::row(words.subtotal, ticket.subtotal.text.clone()));
    for tax in &ticket.taxes {
        blocks.push(Block::row(tax.name.clone(), tax.amount.text.clone()));
    }
    blocks.push(Block::Row {
        left: words.total.to_owned(),
        right: ticket.total.text.clone(),
        indent: 0,
        style: Style { bold: true, wide: false, tall: true },
    });
    for payment in closing.payments {
        let Some(captured) = payment.captured() else { continue };
        let Some(cash) = captured.cash else { continue };
        blocks.push(Block::row(words.cash, money(cash.tendered)));
        if let Some(rounding) = cash.rounding {
            blocks.push(Block::row(words.rounding, money(rounding)));
        }
        if let Some(change) = captured.change() {
            blocks.push(Block::row(words.change, money(change)));
        }
    }
    blocks.push(Block::Rule);
    blocks.push(Block::text(words.thanks, Align::Center, Style::default()));
    if closing.reprint {
        blocks.push(reprint_banner(locale));
    }
    blocks.push(Block::Feed(2));
    blocks.push(Block::Cut);
    Ok(Doc::new(blocks)?)
}

/// A ticket's lines, each with its modifiers indented under it, as many as fit in
/// [`MAX_LINES_BYTES`], and how many more there are when not all do.
fn lines(ticket: &Ticket, words: &Words, locale: Locale) -> Result<Vec<Block>, RuntimeError> {
    let money = |amount: Money| locale.money(amount);
    let mut lines = Vec::new();
    let mut size = 0_usize;
    let mut shown = 0_usize;
    for line in &ticket.lines {
        let mut these = Vec::new();
        let quantity = if line.quantity_text == "1" {
            String::new()
        } else {
            format!("{} × ", line.quantity_text)
        };
        these.push(Block::row(format!("{quantity}{}", line.name), line.amount.text.clone()));
        for modifier in &line.modifiers {
            let prefix = words
                .prefixes
                .get(usize::from(prefix_code(modifier.prefix)))
                .copied()
                .unwrap_or("");
            let times = if modifier.quantity > 1 {
                format!("{} × ", modifier.quantity)
            } else {
                String::new()
            };
            let name = if prefix.is_empty() {
                modifier.name.to_string()
            } else {
                format!("{prefix} {}", modifier.name)
            };
            let price = modifier.price.money.checked_mul(i64::from(modifier.quantity))?;
            let right = if price.is_zero() { String::new() } else { money(price) };
            these.push(Block::Row {
                left: format!("{times}{name}"),
                right,
                indent: modifier.depth.saturating_mul(2).min(keel_doc::doc::MAX_INDENT),
                style: Style::default(),
            });
        }
        // A line's notes, for the customer to check what was asked for, on one line of their own.
        if let Some(notes) = &line.notes {
            these.push(Block::Row {
                left: format!("\u{201c}{}\u{201d}", notes.as_str().replace('\n', " ")),
                right: String::new(),
                indent: 2,
                style: Style::default(),
            });
        }
        for discount in &line.discounts {
            these.push(discount_row(discount, words, locale, 2)?);
        }
        let bytes: usize = these.iter().map(block_size).sum();
        if size.saturating_add(bytes) > MAX_LINES_BYTES
            || lines.len().saturating_add(these.len()) > MAX_LINES_BLOCKS
        {
            break;
        }
        size = size.saturating_add(bytes);
        shown = shown.saturating_add(1);
        lines.extend(these);
    }
    let hidden = ticket.lines.len().saturating_sub(shown);
    if hidden > 0 {
        lines.push(Block::text(
            format!("… {hidden} {}", words.more),
            Align::Left,
            Style::default(),
        ));
    }
    Ok(lines)
}

/// A discount's row, `indent` in: "Discount 10%" and what it took, as less (ADR-0030).
fn discount_row(
    discount: &TicketDiscount,
    words: &Words,
    locale: Locale,
    indent: u8,
) -> Result<Block, RuntimeError> {
    let left = match discount.percent {
        Some(bp) => format!("{} {}", words.discount, percent(bp, words.decimal)),
        None => words.discount.to_owned(),
    };
    let right = locale.money(discount.amount.money.checked_neg()?);
    Ok(Block::Row { left, right, indent, style: Style::default() })
}

/// `bp` basis points as a percentage, with `decimal` before any hundredths: "10%", "12.5%".
fn percent(bp: u32, decimal: char) -> String {
    let (whole, part) = (bp / 100, bp % 100);
    if part == 0 {
        return format!("{whole}%");
    }
    let hundredths = format!("{part:02}");
    format!("{whole}{decimal}{}%", hundredths.trim_end_matches('0'))
}

/// A receipt for when the whole one can't be made, because its parts go past what a document
/// or an effect holds: the location, the order, what it cost and the change, and a line saying
/// the register shows the rest.
pub(crate) fn short(
    ticket: &Ticket,
    closing: &Closing<'_>,
    profile: &Profile,
    locale: Locale,
) -> Result<Doc, RuntimeError> {
    let words = words(locale);
    let id = ticket.order.to_string();
    let short = id.get(id.len().saturating_sub(6)..).unwrap_or_default().to_uppercase();
    let mut blocks = vec![
        Block::text(profile.data().name.as_str(), Align::Center, Style::LARGE),
        Block::Rule,
        Block::row(format!("{} {short}", words.order), closing.at.to_string()),
        Block::Rule,
        Block::row(words.total, ticket.total.text.clone()),
    ];
    for payment in closing.payments {
        let Some(captured) = payment.captured() else { continue };
        if let Some(change) = captured.change() {
            blocks.push(Block::row(words.change, locale.money(change)));
        }
    }
    blocks.extend([
        Block::text(words.see_register, Align::Left, Style::default()),
        Block::Feed(2),
        Block::Cut,
    ]);
    Ok(Doc::new(blocks)?)
}

fn prefix_code(prefix: Prefix) -> u8 {
    match prefix {
        Prefix::Plain => 0,
        Prefix::No => 1,
        Prefix::Extra => 2,
        Prefix::Light => 3,
        Prefix::Side => 4,
        Prefix::Sub => 5,
    }
}

/// About how many bytes a block takes in a document's encoding.
fn block_size(block: &Block) -> usize {
    match block {
        Block::Text { text, .. } => text.len().saturating_add(8),
        Block::Row { left, right, .. } => left.len().saturating_add(right.len()).saturating_add(10),
        _ => 4,
    }
}
