//! Writes how Keel shows a spread of amounts and quantities in every locale it knows, one to a
//! line, for `tools/icu_crosscheck.js` to compare with ICU (see `data/cldr/README.md`):
//!
//! ```sh
//! cargo run -q -p keel-types --example locale_dump > /tmp/keel-locale.tsv
//! node core/crates/keel-types/tools/icu_crosscheck.js /tmp/keel-locale.tsv
//! ```
//!
//! Each line is tab-separated: `M`, the locale, the currency, its decimals, the amount in minor
//! units and how Keel shows it; or `Q`, the locale, `-`, `6`, the quantity in millionths and how
//! Keel shows it.

use std::error::Error;
use std::io::{self, BufWriter, Write};

use keel_types::{Currency, Locale, Money, Quantity, Unit};

/// Values to show: small ones, each power of ten with its neighbours, the ends of `i64`, and 200
/// more spread pseudo-randomly over every magnitude, each with either sign.
fn values() -> Result<Vec<i64>, Box<dyn Error>> {
    let mut values = vec![0, 1, -1, 5, -7, 9, 99, 101, 999, 1001, 123_456, -123_456];
    values.extend([i64::MAX, i64::MAX.saturating_sub(1), i64::MIN, i64::MIN.saturating_add(1)]);
    let mut power: i64 = 1;
    while let Some(next) = power.checked_mul(10) {
        power = next;
        for value in [power, power.saturating_sub(1), power.saturating_add(1)] {
            values.extend([value, value.saturating_neg()]);
        }
    }
    let mut state: u64 = 0x1234_5678_9abc_def0;
    for _ in 0..200 {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        // Up to 63 bits, as many as the top six bits of the state say, and a sign from the lowest.
        let drop = u32::try_from(state.checked_shr(58).unwrap_or(0))?;
        let magnitude =
            i64::try_from(state.checked_shr(1).unwrap_or(0).checked_shr(drop).unwrap_or(0))?;
        values.push(if state & 1 == 0 { magnitude } else { magnitude.saturating_neg() });
    }
    Ok(values)
}

fn main() -> Result<(), Box<dyn Error>> {
    let values = values()?;
    let mut out = BufWriter::new(io::stdout().lock());
    for locale in Locale::ALL {
        for currency in Currency::known() {
            let (code, decimals) = (currency.code(), currency.minor_units());
            for &minor in &values {
                let shown = locale.money(Money::from_minor(minor, currency));
                writeln!(out, "M\t{locale}\t{code}\t{decimals}\t{minor}\t{shown}")?;
            }
        }
        for &micros in &values {
            let shown = locale.quantity(Quantity::from_micros(micros, Unit::Each));
            writeln!(out, "Q\t{locale}\t-\t6\t{micros}\t{shown}")?;
        }
    }
    out.flush()?;
    Ok(())
}
