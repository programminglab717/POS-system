//! Stock across the boundary (ADR-0036): an item's levels and what moved it, and the day's waste
//! and counts. Quantities are in millionths of their unit, with the locale's text.

use keel_domain::stock::Movement;
use keel_runtime as runtime;

/// How a movement moves an item's stock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StockMovement {
    /// Received, as from a delivery.
    Received,
    /// Adjusted up or down, as a correction: the quantity is signed.
    Adjusted,
    /// Wasted.
    Wasted,
    /// Set aside, as for a customer.
    Reserved,
    /// No longer set aside.
    Released,
}

impl From<StockMovement> for Movement {
    fn from(movement: StockMovement) -> Movement {
        match movement {
            StockMovement::Received => Movement::Received,
            StockMovement::Adjusted => Movement::Adjusted,
            StockMovement::Wasted => Movement::Wasted,
            StockMovement::Reserved => Movement::Reserved,
            StockMovement::Released => Movement::Released,
        }
    }
}

impl From<Movement> for StockMovement {
    fn from(movement: Movement) -> StockMovement {
        match movement {
            Movement::Received => StockMovement::Received,
            Movement::Adjusted => StockMovement::Adjusted,
            Movement::Wasted => StockMovement::Wasted,
            Movement::Reserved => StockMovement::Reserved,
            Movement::Released => StockMovement::Released,
        }
    }
}

/// A quantity: millionths of its unit, the unit's code, and how the locale shows it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StockQuantity {
    /// Millionths of the unit.
    pub micros: i64,
    /// The unit's code, such as `each` or `kg`.
    pub unit: String,
    /// How the locale shows it.
    pub text: String,
}

/// An item whose stock is tracked, and its levels.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StockItem {
    /// The variant's identifier.
    pub variant: String,
    /// Its name.
    pub name: String,
    /// On hand: below zero when more sold than was there.
    pub on_hand: StockQuantity,
    /// Set aside.
    pub reserved: StockQuantity,
    /// On hand less set aside.
    pub available: StockQuantity,
    /// When it was last counted, microseconds since the Unix epoch, if it was.
    pub counted_at: Option<i64>,
    /// What a person should look at: records of its stock that didn't count, as one in another
    /// unit.
    pub conflicts: u64,
}

fn quantity(quantity: keel_types::Quantity, text: String) -> StockQuantity {
    StockQuantity { micros: quantity.micros(), unit: quantity.unit().code().to_owned(), text }
}

impl From<&runtime::StockView> for StockItem {
    fn from(view: &runtime::StockView) -> StockItem {
        StockItem {
            variant: view.variant.to_string(),
            name: view.name.clone(),
            on_hand: quantity(view.on_hand, view.on_hand_text.clone()),
            reserved: quantity(view.reserved, view.reserved_text.clone()),
            available: quantity(view.available, view.available_text.clone()),
            counted_at: view.counted_at.map(keel_types::Timestamp::as_micros),
            conflicts: view.conflicts,
        }
    }
}

/// What moved an item's stock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StockChange {
    /// A movement.
    Moved {
        /// Which.
        movement: StockMovement,
    },
    /// A count: what was on hand then.
    Counted,
    /// Sold.
    Sold,
    /// Returned.
    Returned,
}

/// One thing that moved an item's stock since its latest count.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StockLine {
    /// When, microseconds since the Unix epoch: a count's as-of time.
    pub at: i64,
    /// What.
    pub change: StockChange,
    /// How much: a sale's negative.
    pub quantity: StockQuantity,
}

impl From<&runtime::StockLine> for StockLine {
    fn from(line: &runtime::StockLine) -> StockLine {
        StockLine {
            at: line.at.as_micros(),
            change: match line.change {
                runtime::StockChange::Moved(movement) => {
                    StockChange::Moved { movement: movement.into() }
                }
                runtime::StockChange::Counted => StockChange::Counted,
                runtime::StockChange::Sold => StockChange::Sold,
                runtime::StockChange::Returned => StockChange::Returned,
            },
            quantity: quantity(line.quantity, line.quantity_text.clone()),
        }
    }
}

/// What was wasted of an item on a day.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct WastedLine {
    /// The item's name.
    pub name: String,
    /// How much, as the locale shows it.
    pub quantity: String,
}

/// What a count found, against what was expected at its time.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CountLine {
    /// The item's name.
    pub name: String,
    /// What was counted.
    pub counted: String,
    /// What was expected.
    pub expected: String,
    /// Counted less expected.
    pub variance: String,
}
