//! The values that order events carry.

use core::num::NonZeroU8;

use keel_events::cbor::Value;
use keel_types::{Currency, Id, Money};

use crate::codec::{CatalogVersion, Field, Fields, Name, Note, ReasonCode, Record, code_enum};
use crate::refs::{Modifier, TaxCategory, Variant};

code_enum! {
    /// Where an order was placed.
    pub enum Channel {
        /// At a register or handheld.
        Pos = 0,
        /// At a self-service kiosk.
        Kiosk = 1,
        /// On the merchant's website or app.
        Online = 2,
        /// By scanning a QR code, typically at the table.
        Qr = 3,
        /// By phone.
        Phone = 4,
        /// Through a delivery partner.
        DeliveryPartner = 5,
        /// Through the API, by an integration.
        Api = 6,
        /// On a marketplace.
        Marketplace = 7,
    }
}

code_enum! {
    /// How an order reaches the customer.
    pub enum Mode {
        /// Eaten or used on the premises.
        DineIn = 0,
        /// Taken away at once.
        Takeout = 1,
        /// Collected later.
        Pickup = 2,
        /// Delivered locally.
        Delivery = 3,
        /// Shipped.
        Ship = 4,
        /// A service performed for the customer, such as a haircut.
        Service = 5,
        /// Served at a drive-through window.
        DriveThru = 6,
        /// Brought out to the customer's car.
        Curbside = 7,
    }
}

code_enum! {
    /// How a modifier changes an item, for the kitchen and for recipes: "no onion" removes
    /// onion from the recipe, "extra cheese" adds a portion.
    pub enum Prefix {
        /// The modifier as it is, such as "oat milk".
        Plain = 0,
        /// Leave it out.
        No = 1,
        /// More of it.
        Extra = 2,
        /// Less of it.
        Light = 3,
        /// Served on the side.
        Side = 4,
        /// Substituted for a default component.
        Sub = 5,
    }
}

code_enum! {
    /// Which part of an item a modifier covers, for pizza-style items.
    pub enum Placement {
        /// The whole item.
        Whole = 0,
        /// The left half.
        LeftHalf = 1,
        /// The right half.
        RightHalf = 2,
        /// The first quarter.
        FirstQuarter = 3,
        /// The second quarter.
        SecondQuarter = 4,
        /// The third quarter.
        ThirdQuarter = 5,
        /// The fourth quarter.
        FourthQuarter = 6,
    }
}

/// What a line sells, as the catalog described it when the line was rung up. Later catalog
/// changes never change it; the catalog version it came from has everything else, such as names
/// in other languages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSnapshot {
    /// The variant sold.
    pub variant: Id<Variant>,
    /// The published catalog the line was priced against.
    pub catalog_version: CatalogVersion,
    /// The item's name.
    pub name: Name,
    /// The tax category, which the location's tax rules map to rates.
    pub tax_category: Id<TaxCategory>,
    /// The price of one unit of the variant: zero or more.
    pub unit_price: Money,
}

/// A modifier chosen for a line (or for another modifier), with the modifiers chosen for it in
/// turn, such as a steak's temperature or a side's dressing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChosenModifier {
    /// The catalog modifier.
    pub modifier: Id<Modifier>,
    /// Its name.
    pub name: Name,
    /// How it changes the item.
    pub prefix: Prefix,
    /// How many times it is applied, such as 2 for "2 × extra cheese".
    pub quantity: NonZeroU8,
    /// Which part of the item it covers.
    pub placement: Placement,
    /// The price of one application, as resolved when chosen (after any "first three free"
    /// rules): zero or more.
    pub unit_price: Money,
    /// The modifiers chosen for this one.
    pub modifiers: Vec<ChosenModifier>,
}

impl ChosenModifier {
    /// Whether every price in this modifier and those under it is in `currency`.
    pub fn is_priced_in(&self, currency: Currency) -> bool {
        self.unit_price.currency() == currency
            && self.modifiers.iter().all(|modifier| modifier.is_priced_in(currency))
    }
}

/// The currency of the first price in `modifiers`, if they have any.
pub(crate) fn modifier_currency(modifiers: &[ChosenModifier]) -> Option<Currency> {
    modifiers.first().map(|modifier| modifier.unit_price.currency())
}

/// `ChosenModifier` keys.
mod key {
    pub(super) const MODIFIER: u64 = 1;
    pub(super) const NAME: u64 = 2;
    pub(super) const PREFIX: u64 = 3;
    pub(super) const QUANTITY: u64 = 4;
    pub(super) const PLACEMENT: u64 = 5;
    pub(super) const UNIT_PRICE: u64 = 6;
    pub(super) const MODIFIERS: u64 = 7;
}

impl Field for ChosenModifier {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::MODIFIER, &self.modifier)
            .field(key::NAME, &self.name)
            .field(key::PREFIX, &self.prefix)
            .field(key::QUANTITY, &self.quantity)
            .field(key::PLACEMENT, &self.placement)
            .field(key::UNIT_PRICE, &self.unit_price)
            .field(key::MODIFIERS, &self.modifiers)
            .build()
    }

    fn from_value(value: &Value) -> Option<ChosenModifier> {
        let mut fields = Fields::read(value).ok()?;
        let modifier = ChosenModifier {
            modifier: fields.required(key::MODIFIER, "modifier").ok()?,
            name: fields.required(key::NAME, "modifier name").ok()?,
            prefix: fields.required(key::PREFIX, "prefix").ok()?,
            quantity: fields.required(key::QUANTITY, "modifier quantity").ok()?,
            placement: fields.required(key::PLACEMENT, "placement").ok()?,
            unit_price: fields.required(key::UNIT_PRICE, "modifier price").ok()?,
            modifiers: fields.required(key::MODIFIERS, "modifiers").ok()?,
        };
        fields.finish().ok()?;
        let prices_valid = !modifier.unit_price.is_negative()
            && modifier.is_priced_in(modifier.unit_price.currency());
        prices_valid.then_some(modifier)
    }
}

/// Why a line or an order was voided, or a line comped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    /// The merchant's reason code, which reports group by.
    pub code: ReasonCode,
    /// Anything more the person wanted to say.
    pub note: Option<Note>,
}
