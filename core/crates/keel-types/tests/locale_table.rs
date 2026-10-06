//! Verifies the generated locale table against the committed CLDR snapshot (`data/cldr/`): each
//! locale's symbols, and the symbol every currency Keel knows shows in it. If the table and the
//! data ever drift, this fails.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a malformed snapshot should fail loudly"
)]

use keel_types::{Currency, Locale, Money};
use serde_json::Value;

/// The CLDR locale whose data a Keel locale uses: CLDR's `en` is American English.
fn cldr_locale(locale: Locale) -> &'static str {
    match locale.tag() {
        "en-US" => "en",
        "es-US" => "es-US",
        other => panic!("no CLDR locale for {other}"),
    }
}

fn snapshot(locale: &str, file: &str) -> Value {
    let path = format!("{}/data/cldr/{locale}/{file}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("CLDR snapshot is readable");
    let document: Value = serde_json::from_str(&text).expect("CLDR snapshot is JSON");
    document["main"][locale]["numbers"].clone()
}

/// Whether `c` is a currency sign, Unicode's general category Sc (Unicode 16).
fn is_currency_sign(c: char) -> bool {
    matches!(
        c,
        '$' | '\u{a2}'..='\u{a5}'
            | '\u{58f}'
            | '\u{60b}'
            | '\u{7fe}'..='\u{7ff}'
            | '\u{9f2}'..='\u{9f3}'
            | '\u{9fb}'
            | '\u{af1}'
            | '\u{bf9}'
            | '\u{e3f}'
            | '\u{17db}'
            | '\u{20a0}'..='\u{20c0}'
            | '\u{a838}'
            | '\u{fdfc}'
            | '\u{fe69}'
            | '\u{ff04}'
            | '\u{ffe0}'..='\u{ffe1}'
            | '\u{ffe5}'..='\u{ffe6}'
            | '\u{11fdd}'..='\u{11fe0}'
            | '\u{1e2ff}'
            | '\u{1ecb0}'
    )
}

/// Whether CLDR keeps `symbol` apart from the digits with a no-break space: its last character
/// is neither a symbol nor a separator (`[[:^S:]&[:^Z:]]`). Every currency symbol in the snapshot
/// ends in a letter, a currency sign or a full stop; anything else needs this test extended.
fn spaced(symbol: &str) -> bool {
    let last = symbol.chars().next_back().expect("a symbol isn't empty");
    assert!(
        last.is_alphabetic() || last == '.' || is_currency_sign(last),
        "{symbol:?} ends in a character this test doesn't classify",
    );
    !is_currency_sign(last)
}

#[test]
fn every_currency_shows_its_cldr_symbol_in_every_locale() {
    for locale in Locale::ALL {
        let cldr = cldr_locale(locale);
        let currencies = &snapshot(cldr, "currencies")["currencies"];
        for currency in Currency::known() {
            // CLDR's symbol, or the ISO 4217 code for a currency CLDR doesn't list.
            let symbol = currencies[currency.code()]["symbol"].as_str().unwrap_or(currency.code());
            let space = if spaced(symbol) { "\u{a0}" } else { "" };
            let zero = if currency.minor_units() == 0 {
                "0".to_owned()
            } else {
                format!("0.{}", "0".repeat(usize::from(currency.minor_units())))
            };
            let shown = locale.money(Money::from_minor(0, currency));
            assert_eq!(shown, format!("{symbol}{space}{zero}"), "{locale} {}", currency.code());
        }
    }
}

#[test]
fn every_locale_uses_its_cldr_separators() {
    for locale in Locale::ALL {
        let numbers = snapshot(cldr_locale(locale), "numbers");
        let symbols = &numbers["symbols-numberSystem-latn"];
        let (decimal, group, minus) = (
            symbols["decimal"].as_str().expect("a decimal symbol"),
            symbols["group"].as_str().expect("a group symbol"),
            symbols["minusSign"].as_str().expect("a minus sign"),
        );
        let shown = locale.money(Money::from_minor(-123_456_789, Currency::USD));
        assert_eq!(shown, format!("{minus}${}{group}234{group}567{decimal}89", 1), "{locale}");
        assert_eq!(numbers["minimumGroupingDigits"], "1", "{locale}");
    }
}
