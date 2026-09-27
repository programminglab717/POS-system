//! Verifies the generated currency table against the committed ISO 4217 snapshot
//! (`data/iso4217/codes-all.csv`). If the table and the data ever drift, this fails.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code: a malformed snapshot should fail loudly"
)]

use std::collections::{BTreeMap, BTreeSet};

use keel_types::{Currency, CurrencyStatus};

/// Active ISO 4217 codes that are not spendable currencies (see `data/iso4217/README.md`).
const EXCLUDED: [&str; 23] = [
    "BOV", "CHE", "CHW", "CLF", "COU", "MXV", "USN", "UYI", "UYW", // funds
    "XAD", "XDR", "XSU", "XUA", // units of account
    "XBA", "XBB", "XBC", "XBD", // bond market units
    "XAG", "XAU", "XPD", "XPT", // precious metals
    "XTS", "XXX", // testing, no currency
];

/// Currencies withdrawn since 2020 that remain importable: (code, year, month, minor units).
const WITHDRAWN: [(&str, u16, u8, u8); 6] = [
    ("ANG", 2025, 3, 2),
    ("BGN", 2026, 1, 2),
    ("CUC", 2021, 6, 2),
    ("HRK", 2023, 1, 2),
    ("SLL", 2023, 12, 2),
    ("ZWL", 2024, 9, 2),
];

fn snapshot_rows() -> Vec<csv::StringRecord> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/iso4217/codes-all.csv");
    let mut reader = csv::Reader::from_path(path).expect("ISO 4217 snapshot is readable");
    reader.records().map(|record| record.expect("valid CSV record")).collect()
}

#[test]
fn circulating_currencies_match_the_iso_snapshot_exactly() {
    let mut expected: BTreeMap<String, (u16, u8, String)> = BTreeMap::new();
    for row in snapshot_rows() {
        let code = row[2].trim();
        let withdrawn = !row[5].trim().is_empty();
        if code.is_empty() || withdrawn || EXCLUDED.contains(&code) {
            continue;
        }
        let numeric: u16 = row[3].trim().parse().expect("numeric code");
        let minor_units: u8 = row[4].trim().parse().expect("minor units");
        expected.insert(code.to_owned(), (numeric, minor_units, row[1].trim().to_owned()));
    }

    let actual: BTreeMap<String, (u16, u8, String)> = Currency::known()
        .filter(|currency| currency.is_circulating())
        .map(|c| (c.code().to_owned(), (c.numeric(), c.minor_units(), c.name().to_owned())))
        .collect();

    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 155);
}

#[test]
fn recently_withdrawn_currencies_are_present_and_flagged() {
    for (code, year, month, minor_units) in WITHDRAWN {
        let currency = Currency::from_code(code).expect(code);
        assert_eq!(currency.status(), CurrencyStatus::Withdrawn { year, month }, "{code}");
        assert_eq!(currency.minor_units(), minor_units, "{code}");
        assert!(!currency.is_circulating(), "{code}");
    }
    let withdrawn_in_table: BTreeSet<&str> = Currency::known()
        .filter(|currency| !currency.is_circulating())
        .map(Currency::code)
        .collect();
    let expected: BTreeSet<&str> = WITHDRAWN.iter().map(|entry| entry.0).collect();
    assert_eq!(withdrawn_in_table, expected);
}

#[test]
fn excluded_codes_are_not_currencies() {
    for code in EXCLUDED {
        assert!(Currency::from_code(code).is_err(), "{code} must not be a currency");
    }
}

#[test]
fn table_is_sorted_unique_and_well_formed() {
    let codes: Vec<&str> = Currency::known().map(Currency::code).collect();
    assert!(codes.windows(2).all(|pair| pair[0] < pair[1]), "sorted and unique");

    for currency in Currency::known() {
        let code = currency.code();
        assert_eq!(code.len(), 3, "{code}");
        assert!(code.bytes().all(|byte| byte.is_ascii_uppercase()), "{code}");
        assert!(matches!(currency.minor_units(), 0 | 2 | 3), "{code}");
        assert!((1..=999).contains(&currency.numeric()), "{code}");
        assert!(!currency.name().is_empty(), "{code}");
        assert_eq!(Currency::from_code(code).unwrap(), currency);
    }
}

#[test]
fn numeric_codes_are_unique_among_circulating_currencies() {
    let mut seen = BTreeMap::new();
    for currency in Currency::known().filter(|currency| currency.is_circulating()) {
        if let Some(previous) = seen.insert(currency.numeric(), currency.code()) {
            panic!(
                "numeric {} used by both {previous} and {}",
                currency.numeric(),
                currency.code()
            );
        }
        assert_eq!(Currency::from_numeric(currency.numeric()), Some(currency));
    }
}
