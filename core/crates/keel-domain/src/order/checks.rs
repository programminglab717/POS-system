//! Checks: how an order's lines are split among the parties paying for them (ADR-0015).
//!
//! Every order has a main check from its creation, identified by the order's own identifier.
//! More checks can be opened, and each live line is allocated to one or more checks in whole
//! shares: one share on one check for a line that check pays for alone, one share on each of
//! three checks for a bottle of wine split three ways. A check closes once its payments cover
//! it, with a snapshot of what it was charged ([`CheckClosed`]).

use core::num::{NonZeroU16, NonZeroU32};

use keel_events::cbor::Value;
use keel_types::Id;

use super::closing::CheckClosed;
use super::state::Line;
use crate::codec::{Field, Fields, IdSet, PayloadError, Record};

/// A check: what one party pays for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub(super) id: Id<Check>,
    pub(super) number: NonZeroU32,
    pub(super) closed: Option<CheckClosed>,
}

impl Check {
    /// The check's identifier. The main check's is the order's own.
    pub const fn id(&self) -> Id<Check> {
        self.id
    }

    /// The check's number within the order, in the order checks were opened: the main check is
    /// number 1.
    pub const fn number(&self) -> NonZeroU32 {
        self.number
    }

    /// What the check was charged, and the payments that settled it, if it is closed. A closed
    /// check's lines are frozen until the order is reopened.
    pub const fn closed(&self) -> Option<&CheckClosed> {
        self.closed.as_ref()
    }

    /// Whether the check is open: not closed, or reopened since.
    pub const fn is_open(&self) -> bool {
        self.closed.is_none()
    }
}

/// A line's shares on one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckShare {
    /// The check.
    pub check: Id<Check>,
    /// How many shares of the line it has.
    pub shares: NonZeroU16,
}

/// Shares of a line, allocated to a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Allocation {
    /// The line.
    pub line: Id<Line>,
    /// The check.
    pub check: Id<Check>,
    /// How many shares of the line the check has.
    pub shares: NonZeroU16,
}

/// New allocations for some lines: from then on, each line listed belongs to exactly the checks
/// listed for it, in proportion to their shares.
///
/// Allocations are kept in ascending order of line, then check, and each line's shares are in
/// lowest terms, so each split has exactly one encoding.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinesAllocated(Vec<Allocation>);

impl LinesAllocated {
    /// The allocations `allocations`, in any order. Each line's shares are reduced to lowest
    /// terms: two shares and four become one and two.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] if there are no allocations, or a line is allocated to the same
    /// check twice.
    pub fn new(
        allocations: impl IntoIterator<Item = Allocation>,
    ) -> Result<LinesAllocated, PayloadError> {
        let mut allocations: Vec<Allocation> = allocations.into_iter().collect();
        allocations.sort_by_key(order_key);
        let repeats = allocations.windows(2).any(|pair| match pair {
            [left, right] => order_key(left) == order_key(right),
            _ => false,
        });
        if allocations.is_empty() || repeats {
            return Err(PayloadError::Invalid("allocations"));
        }
        for group in allocations.chunk_by_mut(|left, right| left.line == right.line) {
            let divisor = group.iter().fold(0, |divisor, allocation| {
                greatest_common_divisor(divisor, allocation.shares.get())
            });
            for allocation in group.iter_mut() {
                let reduced = allocation.shares.get().checked_div(divisor);
                allocation.shares = reduced.and_then(NonZeroU16::new).unwrap_or(NonZeroU16::MIN);
            }
        }
        Ok(LinesAllocated(allocations))
    }

    /// `lines`, each moved whole to `check`.
    pub fn moving(lines: &IdSet<Line>, check: Id<Check>) -> LinesAllocated {
        LinesAllocated(
            lines.iter().map(|line| Allocation { line, check, shares: NonZeroU16::MIN }).collect(),
        )
    }

    /// `lines`, each split evenly among `checks`.
    pub fn splitting(lines: &IdSet<Line>, checks: &IdSet<Check>) -> LinesAllocated {
        LinesAllocated(
            lines
                .iter()
                .flat_map(|line| {
                    checks.iter().map(move |check| Allocation {
                        line,
                        check,
                        shares: NonZeroU16::MIN,
                    })
                })
                .collect(),
        )
    }

    /// Every allocation, in ascending order of line, then check.
    pub fn iter(&self) -> impl Iterator<Item = &Allocation> {
        self.0.iter()
    }

    /// Each line listed, in ascending order, with its new shares, in ascending order of check.
    pub fn lines(&self) -> impl Iterator<Item = (Id<Line>, Vec<CheckShare>)> + '_ {
        self.0.chunk_by(|left, right| left.line == right.line).filter_map(|group| {
            let line = group.first()?.line;
            let shares = group
                .iter()
                .map(|allocation| CheckShare { check: allocation.check, shares: allocation.shares })
                .collect();
            Some((line, shares))
        })
    }
}

/// The order allocations are kept in: by line, then by check.
fn order_key(allocation: &Allocation) -> ([u8; 16], [u8; 16]) {
    (allocation.line.to_bytes(), allocation.check.to_bytes())
}

/// The greatest common divisor, by Euclid's algorithm; `0` and `n` give `n`.
fn greatest_common_divisor(mut a: u16, mut b: u16) -> u16 {
    while let Some(remainder) = a.checked_rem(b) {
        a = b;
        b = remainder;
    }
    a
}

/// Whether allocations are in strictly ascending order of line, then check, with each line's
/// shares in lowest terms: the only encoding a split has.
fn is_canonical(allocations: &[Allocation]) -> bool {
    let ascending = allocations.windows(2).all(|pair| match pair {
        [left, right] => order_key(left) < order_key(right),
        _ => true,
    });
    let lowest_terms = allocations.chunk_by(|left, right| left.line == right.line).all(|group| {
        group.iter().fold(0, |divisor, allocation| {
            greatest_common_divisor(divisor, allocation.shares.get())
        }) == 1
    });
    !allocations.is_empty() && ascending && lowest_terms
}

/// `Allocation` keys.
mod key {
    pub(super) const LINE: u64 = 1;
    pub(super) const CHECK: u64 = 2;
    pub(super) const SHARES: u64 = 3;
}

impl Field for Allocation {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::LINE, &self.line)
            .field(key::CHECK, &self.check)
            .field(key::SHARES, &self.shares)
            .build()
    }

    fn from_value(value: &Value) -> Option<Allocation> {
        let mut fields = Fields::read(value).ok()?;
        let allocation = Allocation {
            line: fields.required(key::LINE, "line").ok()?,
            check: fields.required(key::CHECK, "check").ok()?,
            shares: fields.required(key::SHARES, "shares").ok()?,
        };
        fields.finish().ok()?;
        Some(allocation)
    }
}

impl Field for LinesAllocated {
    fn to_value(&self) -> Value {
        self.0.to_value()
    }

    fn from_value(value: &Value) -> Option<LinesAllocated> {
        let allocations: Vec<Allocation> = Field::from_value(value)?;
        is_canonical(&allocations).then_some(LinesAllocated(allocations))
    }
}
