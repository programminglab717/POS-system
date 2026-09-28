//! The golden baskets: over a hundred baskets and pricing rules, with the totals an independent
//! Python oracle computed for them with exact fractions (`golden/generate.py`), including checks
//! that hold shares of lines. The engine must match every amount exactly, on every platform.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: malformed fixtures should fail loudly"
)]

use core::num::NonZeroU32;

use keel_pricing::{
    Basket, Dining, Discount, Line, Modifier, Rules, Share, Step, Tax, TaxRounding, TaxScope,
    Totals, price,
};
use keel_types::{Currency, Decimal, Id, Money, Quantity, Rate, RoundingMode, Unit};
use serde_json::Value;

const GOLDEN: &str = include_str!("golden/baskets.json");

/// The identifier a fixture's small number stands for.
fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn number(value: &Value) -> u64 {
    value.as_u64().unwrap()
}

fn mode(value: &Value) -> RoundingMode {
    match value.as_str().unwrap() {
        "half_away_from_zero" => RoundingMode::HalfAwayFromZero,
        "half_even" => RoundingMode::HalfEven,
        "half_toward_zero" => RoundingMode::HalfTowardZero,
        "away_from_zero" => RoundingMode::AwayFromZero,
        "toward_zero" => RoundingMode::TowardZero,
        "ceiling" => RoundingMode::Ceiling,
        "floor" => RoundingMode::Floor,
        other => panic!("unknown rounding mode {other}"),
    }
}

fn dining(value: &Value) -> Dining {
    match value.as_str().unwrap() {
        "on_premises" => Dining::OnPremises,
        "to_go" => Dining::ToGo,
        other => panic!("unknown dining {other}"),
    }
}

fn rate(value: &Value) -> Rate {
    Rate::from_fraction(value.as_str().unwrap().parse::<Decimal>().unwrap())
}

fn money(value: &Value, currency: Currency) -> Money {
    Money::from_minor(value.as_i64().unwrap(), currency)
}

fn discount(value: &Value, currency: Currency) -> Discount {
    match value.get("percent") {
        Some(fraction) => Discount::Percent(rate(fraction)),
        None => Discount::Amount(money(&value["amount"], currency)),
    }
}

fn modifier(value: &Value, currency: Currency) -> Modifier {
    Modifier {
        unit_price: money(&value["unit_price"], currency),
        quantity: NonZeroU32::new(u32::try_from(number(&value["quantity"])).unwrap()).unwrap(),
        modifiers: value["modifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| modifier(m, currency))
            .collect(),
    }
}

fn line(value: &Value, currency: Currency) -> Line {
    let quantity = &value["quantity"];
    Line {
        unit_price: money(&value["unit_price"], currency),
        modifiers: value["modifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| modifier(m, currency))
            .collect(),
        quantity: Quantity::from_micros(
            quantity["micros"].as_i64().unwrap(),
            Unit::from_code(quantity["unit"].as_str().unwrap()).unwrap(),
        ),
        tax_category: id(number(&value["category"])),
        comped: value["comped"].as_bool().unwrap(),
        discounts: value["discounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| discount(d, currency))
            .collect(),
        share: (!value["share"].is_null()).then(|| share(&value["share"])),
    }
}

fn share(value: &Value) -> Share {
    Share {
        weights: value["weights"].as_array().unwrap().iter().map(number).collect(),
        index: usize::try_from(number(&value["index"])).unwrap(),
    }
}

fn basket(value: &Value) -> Basket {
    let currency = Currency::from_code(value["currency"].as_str().unwrap()).unwrap();
    Basket {
        currency,
        lines: value["lines"].as_array().unwrap().iter().map(|l| line(l, currency)).collect(),
        discounts: value["discounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| discount(d, currency))
            .collect(),
        dining: dining(&value["dining"]),
        exemptions: value["exemptions"].as_array().unwrap().iter().map(|n| id(number(n))).collect(),
    }
}

fn rules(value: &Value) -> Rules {
    let rounding = &value["tax_rounding"];
    Rules {
        extension: mode(&value["extension"]),
        discounts: mode(&value["discounts"]),
        taxes: value["taxes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tax| Tax {
                id: id(number(&tax["id"])),
                name: tax["name"].as_str().unwrap().to_owned(),
                rate: rate(&tax["rate"]),
                categories: tax["categories"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| id(number(n)))
                    .collect(),
                dining: (!tax["dining"].is_null()).then(|| dining(&tax["dining"])),
            })
            .collect(),
        tax_rounding: TaxRounding {
            scope: match rounding["scope"].as_str().unwrap() {
                "line" => TaxScope::Line,
                "document" => TaxScope::Document,
                other => panic!("unknown tax scope {other}"),
            },
            mode: mode(&rounding["mode"]),
        },
    }
}

/// Collects every difference between `expected` and the engine's `totals`, by path.
struct Differences(Vec<String>);

impl Differences {
    fn amount(&mut self, path: &str, expected: &Value, actual: Money) {
        if expected.as_i64() != Some(actual.minor()) {
            self.0.push(format!("{path}: expected {expected}, got {}", actual.minor()));
        }
    }

    fn amounts(&mut self, path: &str, expected: &Value, actual: &[Money]) {
        let expected = expected.as_array().unwrap();
        if expected.len() != actual.len() {
            self.0.push(format!(
                "{path}: expected {} amounts, got {}",
                expected.len(),
                actual.len()
            ));
            return;
        }
        for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
            self.amount(&format!("{path}[{index}]"), expected, *actual);
        }
    }

    fn totals(&mut self, expected: &Value, totals: &Totals) {
        for (field, actual) in [
            ("gross", totals.gross),
            ("discounted", totals.discounted),
            ("net", totals.net),
            ("tax", totals.tax),
            ("total", totals.total),
        ] {
            self.amount(field, &expected[field], actual);
        }
        self.amounts("discounts", &expected["discounts"], &totals.discounts);
        let taxes = expected["taxes"].as_array().unwrap();
        if taxes.len() != totals.taxes.len() {
            self.0.push(format!("taxes: expected {}, got {}", taxes.len(), totals.taxes.len()));
        }
        for (index, (expected, actual)) in taxes.iter().zip(&totals.taxes).enumerate() {
            if expected["exempt"].as_bool() != Some(actual.exempt) {
                self.0.push(format!("taxes[{index}].exempt: expected {}", expected["exempt"]));
            }
            self.amount(&format!("taxes[{index}].taxable"), &expected["taxable"], actual.taxable);
            self.amount(&format!("taxes[{index}].tax"), &expected["tax"], actual.tax);
        }
        let lines = expected["lines"].as_array().unwrap();
        if lines.len() != totals.lines.len() {
            self.0.push(format!("lines: expected {}, got {}", lines.len(), totals.lines.len()));
        }
        for (index, (expected, actual)) in lines.iter().zip(&totals.lines).enumerate() {
            let at = |field: &str| format!("lines[{index}].{field}");
            let whole = totals.trace.iter().find_map(|step| match step {
                Step::Extended { line, gross, .. } if *line == index => Some(*gross),
                _ => None,
            });
            match whole {
                Some(whole) => self.amount(&at("whole"), &expected["whole"], whole),
                None => self.0.push(format!("{}: no extension in the trace", at("whole"))),
            }
            for (field, amount) in [
                ("unit_price", actual.unit_price),
                ("gross", actual.gross),
                ("comp", actual.comp),
                ("net", actual.net),
                ("tax", actual.tax),
                ("total", actual.total),
            ] {
                self.amount(&at(field), &expected[field], amount);
            }
            self.amounts(&at("discounts"), &expected["discounts"], &actual.discounts);
            self.amounts(
                &at("order_discounts"),
                &expected["order_discounts"],
                &actual.order_discounts,
            );
            let taxes = expected["taxes"].as_array().unwrap();
            for (tax, (expected, actual)) in taxes.iter().zip(&actual.taxes).enumerate() {
                match actual {
                    None if expected.is_null() => {}
                    Some(actual) if !expected.is_null() => {
                        let at = at(&format!("taxes[{tax}]"));
                        self.amount(&format!("{at}.taxable"), &expected["taxable"], actual.taxable);
                        self.amount(&format!("{at}.tax"), &expected["tax"], actual.tax);
                    }
                    _ => self
                        .0
                        .push(format!("{}: expected {expected}", at(&format!("taxes[{tax}]")))),
                }
            }
            if taxes.len() != actual.taxes.len() {
                self.0.push(format!("{}: expected {} taxes", at("taxes"), taxes.len()));
            }
        }
    }
}

#[test]
fn golden_baskets_match_the_oracle_exactly() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(golden["format"], 2);
    let baskets = golden["baskets"].as_array().unwrap();
    assert!(baskets.len() >= 100, "the suite has {} baskets", baskets.len());
    let mut failures = Vec::new();
    for case in baskets {
        let title = format!("basket {} ({})", case["number"], case["title"].as_str().unwrap());
        match price(&basket(&case["basket"]), &rules(&case["rules"])) {
            Ok(totals) => {
                let mut differences = Differences(Vec::new());
                differences.totals(&case["expected"], &totals);
                failures.extend(
                    differences.0.into_iter().map(|difference| format!("{title}: {difference}")),
                );
            }
            Err(error) => failures.push(format!("{title}: refused: {error}")),
        }
    }
    assert!(failures.is_empty(), "{} differences:\n{}", failures.len(), failures.join("\n"));
}
