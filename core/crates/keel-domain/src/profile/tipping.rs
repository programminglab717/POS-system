//! Tipping, the location's setting (ADR-0035, decision 5): key 14 of a format-4 profile.
//!
//! | Record | Keys |
//! |---|---|
//! | tipping | 1 presets: one to [`MAX_PRESETS`] percentages, in basis points, strictly ascending, each from 1 to 10,000; 2 where the tip is asked for: 0 the register's screen, 1 the terminal |
//!
//! With no tipping, there are no tips: tipping is off unless the merchant turns it on (H-10).

use core::num::NonZeroU16;

use keel_events::cbor::Value;

use super::ProfileError;
use crate::codec::{Field, Fields, Record, code_enum};

/// The most tip presets a location offers.
pub const MAX_PRESETS: usize = 4;

/// The largest preset: 100%, in basis points.
pub const MAX_PRESET: u16 = 10_000;

code_enum! {
    /// Where a customer is asked for a tip.
    pub enum TipPrompt {
        /// On the register's screen, before the card is presented.
        Screen = 0,
        /// On the terminal, as the card is presented.
        Terminal = 1,
    }
}

/// How a location takes tips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tipping {
    /// The percentages offered, in basis points, ascending: 1,500 is 15%.
    pub presets: Vec<NonZeroU16>,
    /// Where the tip is asked for.
    pub prompt: TipPrompt,
}

/// Checks `tipping`: one to [`MAX_PRESETS`] presets, strictly ascending, none above 100%.
pub(super) fn check(tipping: &Tipping) -> Result<(), ProfileError> {
    if tipping.presets.is_empty() || tipping.presets.len() > MAX_PRESETS {
        return Err(ProfileError::Invalid("tipping with no presets, or too many"));
    }
    if tipping.presets.windows(2).any(|pair| pair.first() >= pair.get(1)) {
        return Err(ProfileError::Invalid("tip presets out of order, or repeated"));
    }
    if tipping.presets.iter().any(|preset| preset.get() > MAX_PRESET) {
        return Err(ProfileError::Invalid("a tip preset above 100%"));
    }
    Ok(())
}

mod key {
    pub(super) const PRESETS: u64 = 1;
    pub(super) const PROMPT: u64 = 2;
}

impl Field for Tipping {
    fn to_value(&self) -> Value {
        Record::default()
            .field(key::PRESETS, &self.presets)
            .field(key::PROMPT, &self.prompt)
            .build()
    }

    fn from_value(value: &Value) -> Option<Tipping> {
        let mut fields = Fields::read(value).ok()?;
        let tipping = Tipping {
            presets: fields.required(key::PRESETS, "presets").ok()?,
            prompt: fields.required(key::PROMPT, "prompt").ok()?,
        };
        fields.finish().ok()?;
        Some(tipping)
    }
}
