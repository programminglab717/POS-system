//! Markers for the identifiers of entities that orders refer to but that other parts of Keel
//! manage: the catalog, the floor plan and the location's layout.
//!
//! Like every entity, these are identified by UUIDv7s ([`keel_types::Id`]); the markers only keep
//! one kind of identifier from being passed where another is expected.

/// Marks identifiers of catalog items: what a merchant thinks of as one thing they sell, such
/// as "Latte", whatever its sizes.
#[derive(Debug)]
pub enum Item {}

/// Marks identifiers of catalog variants: the sellable, stockable units (SKUs).
#[derive(Debug)]
pub enum Variant {}

/// Marks identifiers of catalog modifier groups, such as "Milk" or "Extra shots".
#[derive(Debug)]
pub enum ModifierGroup {}

/// Marks identifiers of catalog modifiers, such as "extra shot".
#[derive(Debug)]
pub enum Modifier {}

/// Marks identifiers of tax categories, which tax rule sets map to rates.
#[derive(Debug)]
pub enum TaxCategory {}

/// Marks identifiers of revenue centers: areas of a location with their own settings and
/// reporting, such as the bar or the patio.
#[derive(Debug)]
pub enum RevenueCenter {}

/// Marks identifiers of tables on a floor plan.
#[derive(Debug)]
pub enum Table {}
