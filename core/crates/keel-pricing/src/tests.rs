//! Known-answer tests: worked examples of each pipeline step, with the arithmetic in the comments.

use core::num::NonZeroU32;

use keel_types::{Currency, Decimal, Id, Money, Quantity, Rate, RoundingMode, RoundingRule, Unit};

use super::*;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn usd(text: &str) -> Money {
    Money::parse(text, Currency::USD).unwrap()
}

fn each(count: i64) -> Quantity {
    Quantity::from_whole(count, Unit::Each).unwrap()
}

fn kg(text: &str) -> Quantity {
    Quantity::parse(text, Unit::Kilogram).unwrap()
}

fn percent(text: &str) -> Rate {
    Rate::from_percent(text.parse::<Decimal>().unwrap()).unwrap()
}

const FOOD: u64 = 1;
const GROCERY: u64 = 2;

fn line(price: &str, quantity: Quantity) -> Line {
    Line {
        unit_price: usd(price),
        modifiers: Vec::new(),
        quantity,
        tax_category: id(FOOD),
        comped: false,
        discounts: Vec::new(),
        share: None,
    }
}

fn modifier(price: &str, quantity: u32, modifiers: Vec<Modifier>) -> Modifier {
    Modifier { unit_price: usd(price), quantity: NonZeroU32::new(quantity).unwrap(), modifiers }
}

fn basket(lines: Vec<Line>) -> Basket {
    Basket {
        currency: Currency::USD,
        lines,
        discounts: Vec::new(),
        dining: Dining::OnPremises,
        exemptions: Vec::new(),
    }
}

fn tax(n: u64, rate: &str, categories: &[u64]) -> Tax {
    Tax {
        id: id(0x100_u64.checked_add(n).unwrap()),
        name: format!("tax {n}"),
        rate: percent(rate),
        categories: categories.iter().map(|&category| id(category)).collect(),
        dining: None,
    }
}

fn rules(taxes: Vec<Tax>, scope: TaxScope) -> Rules {
    Rules {
        taxes,
        tax_rounding: TaxRounding { scope, mode: RoundingMode::HalfAwayFromZero },
        ..Rules::untaxed()
    }
}

fn amounts(amounts: &[&str]) -> Vec<Money> {
    amounts.iter().map(|amount| usd(amount)).collect()
}

#[test]
fn a_cafe_order_is_taxed_once_and_the_tax_allocated() {
    // Two lattes at 4.50 with oat milk at 0.50: (4.50 + 0.50) × 2 = 10.00. A bagel: 3.25.
    let mut latte = line("4.50", each(2));
    latte.modifiers = vec![modifier("0.50", 1, Vec::new())];
    let totals = price(
        &basket(vec![latte, line("3.25", each(1))]),
        &rules(vec![tax(1, "8.875", &[FOOD])], TaxScope::Document),
    )
    .unwrap();
    assert_eq!(totals.lines[0].unit_price, usd("5.00"));
    assert_eq!(totals.gross, usd("13.25"));
    // 13.25 × 8.875% = 1.1759375, so 1.18. Shared 1000 : 325, the 118 cents give 89.06 and
    // 28.94: the floors add up to 117, and the last cent goes to the larger remainder, line 2.
    assert_eq!(totals.tax, usd("1.18"));
    let line_taxes: Vec<Money> = totals.lines.iter().map(|line| line.tax).collect();
    assert_eq!(line_taxes, amounts(&["0.89", "0.29"]));
    assert_eq!(totals.total, usd("14.43"));
    assert_eq!(totals.lines[0].total, usd("10.89"));
}

#[test]
fn tax_rounds_per_line_or_per_document_as_the_rules_say() {
    // Three items at 0.10 with a 5% tax. Per line, each is 0.005, which rounds to 0.01: 0.03 in
    // all. Per document, 0.30 × 5% = 0.015 rounds once, to 0.02.
    let items = basket(vec![line("0.10", each(1)); 3]);
    let taxes = vec![tax(1, "5", &[FOOD])];
    let per_line = price(&items, &rules(taxes.clone(), TaxScope::Line)).unwrap();
    assert_eq!(per_line.tax, usd("0.03"));
    let per_document = price(&items, &rules(taxes, TaxScope::Document)).unwrap();
    assert_eq!(per_document.tax, usd("0.02"));
    // The two cents go to the earlier lines: every remainder is the same.
    let line_taxes: Vec<Money> = per_document.lines.iter().map(|line| line.tax).collect();
    assert_eq!(line_taxes, amounts(&["0.01", "0.01", "0.00"]));
}

#[test]
fn weighed_items_round_once_with_the_extension_mode() {
    // 12.99 per kg × 0.453 kg = 5.88447, which rounds to 5.88.
    let totals = price(&basket(vec![line("12.99", kg("0.453"))]), &Rules::untaxed()).unwrap();
    assert_eq!(totals.gross, usd("5.88"));
    // 0.97 per kg × 0.5 kg = 0.485: a tie, which each mode breaks its own way.
    for (mode, expected) in [
        (RoundingMode::HalfAwayFromZero, "0.49"),
        (RoundingMode::HalfEven, "0.48"),
        (RoundingMode::HalfTowardZero, "0.48"),
        (RoundingMode::Ceiling, "0.49"),
        (RoundingMode::Floor, "0.48"),
    ] {
        let rules = Rules { extension: mode, ..Rules::untaxed() };
        let totals = price(&basket(vec![line("0.97", kg("0.5"))]), &rules).unwrap();
        assert_eq!(totals.gross, usd(expected), "{mode:?}");
    }
}

#[test]
fn nested_modifiers_count_per_unit_of_what_they_modify() {
    // A steak at 20.00 with a side salad at 3.00 that has two extra dressings at 0.50: the salad
    // counts 3.00 + 2 × 0.50 = 4.00, so each steak is 24.00, and two are 48.00. Two extra
    // sauces at 0.75 on each steak add 1.50 more per steak: 51.00.
    let mut steak = line("20.00", each(2));
    steak.modifiers = vec![
        modifier("3.00", 1, vec![modifier("0.50", 2, Vec::new())]),
        modifier("0.75", 2, Vec::new()),
    ];
    let totals = price(&basket(vec![steak]), &Rules::untaxed()).unwrap();
    assert_eq!(totals.lines[0].unit_price, usd("25.50"));
    assert_eq!(totals.gross, usd("51.00"));
}

/// The same line, shared among `weights.len()` baskets: the one holding each part.
fn shared(line: &Line, weights: &[u64]) -> Vec<Line> {
    (0..weights.len())
        .map(|index| Line {
            share: Some(Share { weights: weights.to_vec(), index }),
            ..line.clone()
        })
        .collect()
}

#[test]
fn a_shared_line_is_split_by_largest_remainder() {
    // 10.00 split three ways is 3.333... each: the floors make 9.99, and the spare cent goes to
    // the first part, since every remainder is the same.
    let parts: Vec<Totals> = shared(&line("10.00", each(1)), &[1, 1, 1])
        .into_iter()
        .map(|line| price(&basket(vec![line]), &Rules::untaxed()).unwrap())
        .collect();
    let gross: Vec<Money> = parts.iter().map(|totals| totals.gross).collect();
    assert_eq!(gross, amounts(&["3.34", "3.33", "3.33"]));
    assert_eq!(parts[0].trace[1].to_string(), "line 1: part 1 of USD 10.00 split 1:1:1 = USD 3.34");

    // 5.88 of cheese by weight, split one part to two: 1.96 and 3.92.
    let cheese = line("12.99", kg("0.453"));
    let parts: Vec<Money> = shared(&cheese, &[1, 2])
        .into_iter()
        .map(|line| price(&basket(vec![line]), &Rules::untaxed()).unwrap().gross)
        .collect();
    assert_eq!(parts, amounts(&["1.96", "3.92"]));

    // A comp takes each part whole.
    let mut platter = line("24.00", each(1));
    platter.comped = true;
    for held in shared(&platter, &[1, 1]) {
        let totals = price(&basket(vec![held]), &Rules::untaxed()).unwrap();
        assert_eq!((totals.lines[0].gross, totals.lines[0].comp), (usd("12.00"), usd("12.00")));
        assert_eq!(totals.total, usd("0.00"));
    }
}

#[test]
fn each_check_is_taxed_on_its_own_part() {
    // A 30.00 bottle of wine split three ways, with an 8.875% tax: each check's 10.00 is taxed
    // 0.8875, so 0.89, and the three checks pay 2.67 of tax. The whole bottle on one check would
    // pay 2.6625, so 2.66: each check is a separate sale (ADR-0015).
    let taxes = rules(vec![tax(1, "8.875", &[FOOD])], TaxScope::Document);
    for held in shared(&line("30.00", each(1)), &[1, 1, 1]) {
        let totals = price(&basket(vec![held]), &taxes).unwrap();
        assert_eq!(
            (totals.net, totals.tax, totals.total),
            (usd("10.00"), usd("0.89"), usd("10.89"))
        );
    }
    let whole = price(&basket(vec![line("30.00", each(1))]), &taxes).unwrap();
    assert_eq!(whole.tax, usd("2.66"));
}

#[test]
fn line_discounts_take_in_turn_from_what_is_left() {
    // 20% of 9.99 is 1.998, so 2.00; 5.00 off leaves 2.99; then 10.00 off can only take 2.99.
    let mut item = line("9.99", each(1));
    item.discounts = vec![
        Discount::Percent(percent("20")),
        Discount::Amount(usd("5.00")),
        Discount::Amount(usd("10.00")),
    ];
    let totals = price(&basket(vec![item]), &Rules::untaxed()).unwrap();
    assert_eq!(totals.lines[0].discounts, amounts(&["2.00", "5.00", "2.99"]));
    assert_eq!(totals.net, usd("0.00"));
    assert!(matches!(totals.trace.last(), Some(Step::LineDiscounted { limited: true, .. })));
}

#[test]
fn an_amount_discount_of_exactly_what_is_left_takes_it_all_without_limit() {
    let mut item = line("5.00", each(1));
    item.discounts = vec![Discount::Amount(usd("5.00"))];
    let mut order = basket(vec![item, line("3.00", each(1))]);
    order.discounts = vec![Discount::Amount(usd("3.00"))];
    let totals = price(&order, &Rules::untaxed()).unwrap();
    assert_eq!(totals.net, usd("0.00"));
    let limited: Vec<bool> = totals
        .trace
        .iter()
        .filter_map(|step| match step {
            Step::LineDiscounted { limited, .. } | Step::OrderDiscounted { limited, .. } => {
                Some(*limited)
            }
            _ => None,
        })
        .collect();
    assert_eq!(limited, [false, false]);
}

#[test]
fn order_discounts_are_allocated_by_what_is_left_of_each_line() {
    // Lines of 10.00, 5.00 and 5.00. 10% of 20.00 is 2.00: shares 1.00, 0.50 and 0.50. Then
    // 1.00 off what is left, 18.00, in the proportions 9 : 4.5 : 4.5: exactly 0.50, 0.25, 0.25.
    let mut order =
        basket(vec![line("10.00", each(1)), line("5.00", each(1)), line("5.00", each(1))]);
    order.discounts = vec![Discount::Percent(percent("10")), Discount::Amount(usd("1.00"))];
    let totals = price(&order, &Rules::untaxed()).unwrap();
    assert_eq!(totals.discounts, amounts(&["2.00", "1.00"]));
    assert_eq!(totals.lines[0].order_discounts, amounts(&["1.00", "0.50"]));
    assert_eq!(totals.lines[1].order_discounts, amounts(&["0.50", "0.25"]));
    assert_eq!(totals.net, usd("17.00"));

    // 1.00 off three lines of 1.00: a third each is 33⅓ cents. The floors give 99 cents, and the
    // last cent goes to the first line, since the remainders tie.
    let mut thirds = basket(vec![line("1.00", each(1)); 3]);
    thirds.discounts = vec![Discount::Amount(usd("1.00"))];
    let totals = price(&thirds, &Rules::untaxed()).unwrap();
    let shares: Vec<Money> = totals.lines.iter().map(|line| line.order_discounts[0]).collect();
    assert_eq!(shares, amounts(&["0.34", "0.33", "0.33"]));
}

#[test]
fn discounts_reduce_the_taxable_amount() {
    // 20.00 less 25% is 15.00, and 10% tax on it is 1.50.
    let mut order = basket(vec![line("20.00", each(1))]);
    order.discounts = vec![Discount::Percent(percent("25"))];
    let totals = price(&order, &rules(vec![tax(1, "10", &[FOOD])], TaxScope::Line)).unwrap();
    assert_eq!(totals.taxes[0].taxable, usd("15.00"));
    assert_eq!(totals.tax, usd("1.50"));
    assert_eq!(totals.total, usd("16.50"));
}

#[test]
fn comped_lines_cost_nothing_and_take_no_order_discount() {
    let mut dessert = line("8.00", each(1));
    dessert.comped = true;
    dessert.discounts = vec![Discount::Amount(usd("1.00"))];
    let mut order = basket(vec![line("12.00", each(1)), dessert]);
    order.discounts = vec![Discount::Amount(usd("2.00"))];
    let totals = price(&order, &rules(vec![tax(1, "10", &[FOOD])], TaxScope::Line)).unwrap();
    let comped = &totals.lines[1];
    assert_eq!((comped.comp, comped.net, comped.tax), (usd("8.00"), usd("0.00"), usd("0.00")));
    // The comp took everything, so its own discount takes nothing, and so does its share of the
    // order's discount.
    assert_eq!(comped.discounts, amounts(&["0.00"]));
    assert_eq!(comped.order_discounts, amounts(&["0.00"]));
    assert_eq!(totals.lines[0].net, usd("10.00"));
    assert_eq!(totals.discounted, usd("10.00"));
    assert_eq!(totals.total, usd("11.00"));
}

#[test]
fn taxes_follow_categories_dining_and_exemptions() {
    // A state tax on everything, a city tax on food only, and a tax on food eaten on the
    // premises only.
    let mut on_premises = tax(3, "1", &[FOOD]);
    on_premises.dining = Some(Dining::OnPremises);
    let rules = rules(
        vec![tax(1, "4", &[FOOD, GROCERY]), tax(2, "4.5", &[FOOD]), on_premises],
        TaxScope::Line,
    );
    let mut milk = line("5.00", each(1));
    milk.tax_category = id(GROCERY);
    let mut order = basket(vec![line("10.00", each(1)), milk]);

    let totals = price(&order, &rules).unwrap();
    // Food: 0.40 + 0.45 + 0.10; milk: 0.20 only.
    assert_eq!(totals.lines[0].tax, usd("0.95"));
    assert_eq!(totals.lines[1].tax, usd("0.20"));
    assert!(totals.lines[1].taxes[1].is_none());

    order.dining = Dining::ToGo;
    let to_go = price(&order, &rules).unwrap();
    assert_eq!(to_go.lines[0].tax, usd("0.85"));
    assert_eq!(to_go.taxes[2].taxable, usd("0.00"));

    // A customer exempt from the state tax still pays the city's.
    order.exemptions = vec![id(0x101)];
    let exempt = price(&order, &rules).unwrap();
    assert!(exempt.taxes[0].exempt);
    assert_eq!(exempt.tax, usd("0.45"));
    assert!(exempt.trace.contains(&Step::Exempted { tax: 0 }));
}

#[test]
fn an_empty_basket_costs_nothing_and_lists_every_tax() {
    // No lines, and two taxes, one of them exempt: the case on which a property test found a bug
    // in its own replay of the trace. Every total is zero, and each tax still has its entry.
    let mut empty = basket(Vec::new());
    empty.exemptions = vec![id(0x102)];
    let taxes = vec![tax(1, "0.01", &[]), tax(2, "21.34", &[])];
    let totals = price(&empty, &rules(taxes, TaxScope::Line)).unwrap();
    assert!(totals.lines.is_empty());
    assert_eq!(totals.total, usd("0.00"));
    assert_eq!(totals.taxes.len(), 2);
    assert!(!totals.taxes[0].exempt && totals.taxes[1].exempt);
    assert_eq!(totals.trace, [Step::Exempted { tax: 1 }]);
}

#[test]
fn a_tax_above_100_percent_is_allocated_like_any_other() {
    // Some excise taxes exceed the price. Three items at 0.01 with a 250% tax: 0.03 × 250% =
    // 0.075, which rounds to 0.08, more than the lines' nets add up to. Shared equally, each line
    // gets 0.0266...: the floors add up to 0.06, and the two cents left go to the earlier lines.
    let items = basket(vec![line("0.01", each(1)); 3]);
    let totals = price(&items, &rules(vec![tax(1, "250", &[FOOD])], TaxScope::Document)).unwrap();
    assert_eq!(totals.tax, usd("0.08"));
    let line_taxes: Vec<Money> = totals.lines.iter().map(|line| line.tax).collect();
    assert_eq!(line_taxes, amounts(&["0.03", "0.03", "0.02"]));
}

#[test]
fn invalid_baskets_and_rules_are_refused() {
    let untaxed = Rules::untaxed();
    let refused = |basket: &Basket, rules: &Rules| price(basket, rules).unwrap_err();

    let mut euros = basket(vec![line("1.00", each(1))]);
    euros.lines[0].unit_price = Money::parse("1.00", Currency::EUR).unwrap();
    assert!(matches!(refused(&euros, &untaxed), PricingError::CurrencyMismatch { .. }));

    let negative = basket(vec![line("-1.00", each(1))]);
    assert_eq!(refused(&negative, &untaxed), PricingError::NegativePrice { line: 0 });
    let mut negative_modifier = basket(vec![line("1.00", each(1))]);
    negative_modifier.lines[0].modifiers = vec![modifier("-0.01", 1, Vec::new())];
    assert_eq!(refused(&negative_modifier, &untaxed), PricingError::NegativePrice { line: 0 });

    let none = basket(vec![line("1.00", each(0))]);
    assert_eq!(refused(&none, &untaxed), PricingError::QuantityNotPositive { line: 0 });

    let mut too_much = basket(vec![line("1.00", each(1))]);
    too_much.lines[0].discounts = vec![Discount::Percent(percent("100.01"))];
    let at = DiscountRef::Line { line: 0, discount: 0 };
    assert_eq!(refused(&too_much, &untaxed), PricingError::InvalidDiscount(at));
    let mut negative_off = basket(vec![line("1.00", each(1))]);
    negative_off.discounts = vec![Discount::Amount(usd("-0.01"))];
    let at = DiscountRef::Order { discount: 0 };
    assert_eq!(refused(&negative_off, &untaxed), PricingError::InvalidDiscount(at));

    let items = basket(vec![line("1.00", each(1))]);
    let negative_rate = rules(vec![tax(1, "-1", &[FOOD])], TaxScope::Line);
    assert_eq!(refused(&items, &negative_rate), PricingError::NegativeTaxRate { tax: 0 });
    let repeated = rules(vec![tax(1, "1", &[FOOD]), tax(1, "2", &[FOOD])], TaxScope::Line);
    assert_eq!(refused(&items, &repeated), PricingError::DuplicateTax { tax: 1 });

    let mut deep = modifier("0.01", 1, Vec::new());
    for _ in 0..MAX_MODIFIER_DEPTH {
        deep = modifier("0.01", 1, vec![deep]);
    }
    let mut nested = basket(vec![line("1.00", each(1))]);
    nested.lines[0].modifiers = vec![deep];
    assert_eq!(refused(&nested, &untaxed), PricingError::ModifiersTooDeep { line: 0 });

    for (weights, index) in [(vec![], 0), (vec![1, 0], 0), (vec![1, 1], 2)] {
        let mut odd = basket(vec![line("1.00", each(1))]);
        odd.lines[0].share = Some(Share { weights, index });
        assert_eq!(refused(&odd, &untaxed), PricingError::InvalidShare { line: 0 });
    }

    let huge = basket(vec![line("90000000000000000.00", each(2))]);
    assert_eq!(refused(&huge, &untaxed), PricingError::Overflow);
}

#[test]
fn the_deepest_allowed_modifiers_are_priced() {
    let mut deep = modifier("0.01", 1, Vec::new());
    for _ in 1..MAX_MODIFIER_DEPTH {
        deep = modifier("0.01", 1, vec![deep]);
    }
    let mut nested = basket(vec![line("1.00", each(1))]);
    nested.lines[0].modifiers = vec![deep];
    assert_eq!(price(&nested, &Rules::untaxed()).unwrap().gross, usd("1.32"));
}

#[test]
fn the_trace_reads_as_the_calculation() {
    let mut latte = line("4.50", each(2));
    latte.modifiers = vec![modifier("0.50", 1, Vec::new())];
    latte.discounts = vec![Discount::Percent(percent("10"))];
    let mut order = basket(vec![latte, line("3.25", each(1))]);
    order.discounts = vec![Discount::Amount(usd("1.00"))];
    let totals = price(&order, &rules(vec![tax(1, "8.875", &[FOOD])], TaxScope::Document)).unwrap();
    // The order discount splits 100 cents 900 : 325, into 73.47 and 26.53: 73 and 27. That leaves
    // nets of 8.27 and 2.98, and 11.25 × 8.875% = 0.998..., so 1.00 of tax, split 827 : 298 into
    // 73.51 and 26.49: 74 and 26.
    let lines: Vec<String> = totals.trace.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "line 1: (USD 4.50 + modifiers USD 0.50) × 2 each = USD 10.00, rounded half away \
             from zero",
            "line 1, discount 1: 10% of USD 10.00 = USD 1.00, rounded half away from zero",
            "line 2: (USD 3.25 + modifiers USD 0.00) × 1 each = USD 3.25, rounded half away \
             from zero",
            "order discount 1: USD 1.00 off USD 12.25 = USD 1.00; shares USD 0.73 USD 0.27",
            "tax 1: 8.875% of USD 11.25 = USD 1.00, rounded half away from zero once; shares \
             USD 0.74 USD 0.26",
        ]
    );
}

#[test]
fn cash_rounds_to_the_jurisdictions_coin() {
    // Canada: to the nearest 5 cents. 1.01 and 1.02 round down, 1.03 and 1.04 up, 1.06 and 1.07
    // down, 1.08 and 1.09 up.
    let nickel = RoundingRule::new(RoundingMode::HalfAwayFromZero, NonZeroU32::new(5).unwrap());
    let cad = |text: &str| Money::parse(text, Currency::CAD).unwrap();
    for (amount, due, rounding) in [
        ("1.01", "1.00", "-0.01"),
        ("1.02", "1.00", "-0.02"),
        ("1.03", "1.05", "0.02"),
        ("1.04", "1.05", "0.01"),
        ("1.05", "1.05", "0.00"),
        ("1.06", "1.05", "-0.01"),
        ("1.07", "1.05", "-0.02"),
        ("1.08", "1.10", "0.02"),
        ("1.09", "1.10", "0.01"),
    ] {
        let cash = round_cash(cad(amount), nickel).unwrap();
        assert_eq!((cash.due, cash.rounding), (cad(due), cad(rounding)), "{amount}");
    }
}
