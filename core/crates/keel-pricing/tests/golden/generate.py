#!/usr/bin/env python3
"""The golden baskets: baskets and pricing rules, with the totals ADR-0014 gives them, and
ADR-0015's shared lines.

The totals are computed here, with exact fractions, from the ADR's description of the pipeline.
Nothing is shared with the Rust engine, so the two can only agree if both follow the ADR.

    python3 core/crates/keel-pricing/tests/golden/generate.py

rewrites `baskets.json` next to this script. CI runs it and fails if the file changes, so the
committed expectations always come from this oracle; `tests/golden_baskets.rs` prices every basket
and must match every amount exactly.

The baskets are deterministic: a small SplitMix64 generator, not Python's `random`, whose methods
may change between Python versions. The tax rules are illustrative, not any real jurisdiction's.
"""

import json
import pathlib
from fractions import Fraction

HERE = pathlib.Path(__file__).resolve().parent
MASK = (1 << 64) - 1

# ---------------------------------------------------------------------------------------------
# The oracle.


def round_to_integer(value, mode):
    """Rounds a Fraction to an integer with one of the seven rounding modes."""
    floor = value.numerator // value.denominator
    if floor == value:
        return floor
    ceiling = floor + 1
    toward_zero, away_from_zero = (floor, ceiling) if value > 0 else (ceiling, floor)
    directed = {
        "away_from_zero": away_from_zero,
        "toward_zero": toward_zero,
        "ceiling": ceiling,
        "floor": floor,
    }
    if mode in directed:
        return directed[mode]
    distance = value - floor
    if distance < Fraction(1, 2):
        return floor
    if distance > Fraction(1, 2):
        return ceiling
    ties = {
        "half_away_from_zero": away_from_zero,
        "half_toward_zero": toward_zero,
        "half_even": floor if floor % 2 == 0 else ceiling,
    }
    return ties[mode]


def largest_remainder(amount, weights):
    """Splits a whole number of minor units in proportion to `weights`: floors first, then one
    unit each to the largest remainders, the earliest first among equals."""
    total = sum(weights)
    if amount == 0 or total == 0:
        return [0] * len(weights)
    shares = [Fraction(amount * weight, total) for weight in weights]
    parts = [share.numerator // share.denominator for share in shares]
    left = amount - sum(parts)
    by_remainder = sorted(range(len(weights)), key=lambda i: (-(shares[i] - parts[i]), i))
    for index in by_remainder[:left]:
        parts[index] += 1
    return parts


def modifiers_price(modifiers):
    return sum(m["quantity"] * (m["unit_price"] + modifiers_price(m["modifiers"])) for m in modifiers)


def take(rest, discount, mode):
    if "percent" in discount:
        return round_to_integer(rest * Fraction(discount["percent"]), mode)
    return min(discount["amount"], rest)


def price(basket, rules):
    """What ADR-0014 says the basket costs, in minor units."""
    lines = []
    for line in basket["lines"]:
        unit_price = line["unit_price"] + modifiers_price(line["modifiers"])
        whole = round_to_integer(
            Fraction(unit_price * line["quantity"]["micros"], 1_000_000), rules["extension"]
        )
        share = line["share"]
        gross = whole if share is None else largest_remainder(whole, share["weights"])[share["index"]]
        comp = gross if line["comped"] else 0
        rest = gross - comp
        discounts = []
        for discount in line["discounts"]:
            taken = take(rest, discount, rules["discounts"])
            rest -= taken
            discounts.append(taken)
        lines.append({
            "unit_price": unit_price,
            "whole": whole,
            "gross": gross,
            "comp": comp,
            "discounts": discounts,
            "order_discounts": [],
            "net": rest,
            "taxes": [],
        })

    order_discounts = []
    for discount in basket["discounts"]:
        rests = [line["net"] for line in lines]
        taken = take(sum(rests), discount, rules["discounts"])
        for line, share in zip(lines, largest_remainder(taken, rests)):
            line["net"] -= share
            line["order_discounts"].append(share)
        order_discounts.append(taken)

    taxes = []
    scope, mode = rules["tax_rounding"]["scope"], rules["tax_rounding"]["mode"]
    for tax in rules["taxes"]:
        if tax["id"] in basket["exemptions"]:
            for line in lines:
                line["taxes"].append(None)
            taxes.append({"exempt": True, "taxable": 0, "tax": 0})
            continue
        covered = [
            line["category"] in tax["categories"]
            and (tax["dining"] is None or tax["dining"] == basket["dining"])
            for line in basket["lines"]
        ]
        bases = [line["net"] if cover else 0 for line, cover in zip(lines, covered)]
        rate = Fraction(tax["rate"])
        if scope == "line":
            amounts = [round_to_integer(base * rate, mode) for base in bases]
        else:
            amounts = largest_remainder(round_to_integer(sum(bases) * rate, mode), bases)
        for line, cover, base, amount in zip(lines, covered, bases, amounts):
            line["taxes"].append({"taxable": base, "tax": amount} if cover else None)
        taxed = [amount for amount, cover in zip(amounts, covered) if cover]
        taxes.append({"exempt": False, "taxable": sum(bases), "tax": sum(taxed)})

    for line in lines:
        line["tax"] = sum(part["tax"] for part in line["taxes"] if part is not None)
        line["total"] = line["net"] + line["tax"]
    gross = sum(line["gross"] for line in lines)
    net = sum(line["net"] for line in lines)
    tax = sum(total["tax"] for total in taxes)
    return {
        "lines": lines,
        "discounts": order_discounts,
        "taxes": taxes,
        "gross": gross,
        "discounted": gross - net,
        "net": net,
        "tax": tax,
        "total": net + tax,
    }


# ---------------------------------------------------------------------------------------------
# The baskets.


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n

    def between(self, low, high):
        return low + self.below(high - low + 1)

    def choice(self, items):
        return items[self.below(len(items))]

    def chance(self, percent):
        return self.below(100) < percent


# Tax categories.
PREPARED, GROCERY, GENERAL, ALCOHOL = 1, 2, 3, 4

EACH = "each"


def quantity(micros, unit=EACH):
    return {"micros": micros, "unit": unit}


def count(n):
    return quantity(n * 1_000_000)


def modifier(unit_price, qty=1, modifiers=()):
    return {"unit_price": unit_price, "quantity": qty, "modifiers": list(modifiers)}


def line(unit_price, qty, category, modifiers=(), comped=False, discounts=(), share=None):
    return {
        "unit_price": unit_price,
        "quantity": qty,
        "category": category,
        "comped": comped,
        "modifiers": list(modifiers),
        "discounts": list(discounts),
        "share": share,
    }


def part(index, *weights):
    """The share of a line split in proportion to `weights` that one basket holds."""
    return {"weights": list(weights), "index": index}


def split(shared_line, weights):
    """The line as each basket sharing it holds it: one line per part."""
    return [dict(shared_line, share=part(index, *weights)) for index in range(len(weights))]


def decimal_string(value):
    """A Fraction with a finite decimal expansion, written exactly: 17/200 → '0.085'."""
    places = 0
    while (10 ** places) % value.denominator:
        places += 1
    digits = str(abs(value.numerator) * 10 ** places // value.denominator).rjust(places + 1, "0")
    sign = "-" if value < 0 else ""
    return sign + (digits[:-places] + "." + digits[-places:] if places else digits)


def pct(text):
    """A percentage as the exact decimal fraction: '8.875' → '0.08875'."""
    return decimal_string(Fraction(text) / 100)


def percent(text):
    return {"percent": pct(text)}


def off(amount):
    return {"amount": amount}


def tax(n, name, rate, categories, dining=None):
    return {"id": n, "name": name, "rate": pct(rate), "categories": list(categories), "dining": dining}


def rules(taxes, scope="document", mode="half_away_from_zero", extension="half_away_from_zero",
          discounts="half_away_from_zero"):
    return {
        "extension": extension,
        "discounts": discounts,
        "tax_rounding": {"scope": scope, "mode": mode},
        "taxes": taxes,
    }


def basket(lines, currency="USD", dining="on_premises", discounts=(), exemptions=()):
    return {
        "currency": currency,
        "dining": dining,
        "exemptions": list(exemptions),
        "lines": list(lines),
        "discounts": list(discounts),
    }


# Illustrative tax rules.
ONE_RATE = [tax(1, "Combined sales tax", "8.875", [PREPARED, GENERAL, ALCOHOL])]
STATE_AND_CITY = [
    tax(1, "State sales tax", "6.25", [PREPARED, GENERAL, ALCOHOL]),
    tax(2, "City sales tax", "2.5", [PREPARED, GENERAL, ALCOHOL]),
    tax(3, "Prepared food eaten on the premises", "0.5", [PREPARED], dining="on_premises"),
]
GROCERY_STATE = [
    tax(1, "State sales tax", "7", [PREPARED, GENERAL, ALCOHOL]),
    tax(2, "State tax on groceries", "1", [GROCERY]),
    tax(3, "County sales tax", "1.75", [PREPARED, GENERAL, ALCOHOL, GROCERY]),
]
ALCOHOL_SURTAX = [
    tax(1, "Sales tax", "8", [PREPARED, GENERAL, ALCOHOL]),
    tax(2, "Liquor tax", "10", [ALCOHOL]),
]

CAFE = [
    ("latte", 450, [modifier(50), modifier(75), modifier(60, 2)]),
    ("cappuccino", 425, [modifier(50), modifier(100)]),
    ("drip coffee", 275, [modifier(35)]),
    ("espresso", 300, [modifier(100)]),
    ("croissant", 375, [modifier(125)]),
    ("bagel", 325, [modifier(150), modifier(90)]),
    ("muffin", 350, []),
    ("cookie", 250, []),
]
KITCHEN = [
    ("burger", 1450, PREPARED, [modifier(150), modifier(250), modifier(0)]),
    ("salad", 1195, PREPARED, [modifier(300, 1, [modifier(50, 2)])]),
    ("steak", 2895, PREPARED, [modifier(300, 1, [modifier(50)]), modifier(75, 2)]),
    ("fries", 495, PREPARED, [modifier(75)]),
    ("soda", 299, PREPARED, []),
    ("beer", 750, ALCOHOL, []),
    ("wine", 1200, ALCOHOL, []),
    ("dessert", 895, PREPARED, [modifier(150)]),
]
SHELF = [
    ("apples", 399, GROCERY, "kg"),
    ("bananas", 69, GROCERY, "lb"),
    ("cheese", 1299, GROCERY, "kg"),
    ("deli ham", 1499, GROCERY, "kg"),
    ("milk", 349, GROCERY, EACH),
    ("bread", 299, GROCERY, EACH),
    ("soap", 499, GENERAL, EACH),
    ("hot rotisserie chicken", 899, PREPARED, EACH),
]
MODES = ["half_away_from_zero", "half_even", "half_toward_zero", "away_from_zero",
         "toward_zero", "ceiling", "floor"]


def cafe(rng):
    lines, notes = [], []
    for _ in range(rng.between(1, 4)):
        name, unit_price, extras = rng.choice(CAFE)
        mods = [m for m in extras if rng.chance(40)]
        discounts = [percent("50")] if rng.chance(8) else []
        lines.append(line(unit_price, count(rng.between(1, 3)), PREPARED, mods, discounts=discounts))
        notes.append(name)
    order = [off(100)] if rng.chance(25) else []
    dining = rng.choice(["on_premises", "to_go"])
    return basket(lines, dining=dining, discounts=order), rules(ONE_RATE), "café: " + ", ".join(notes)


def restaurant(rng):
    lines, notes = [], []
    for _ in range(rng.between(2, 6)):
        name, unit_price, category, extras = rng.choice(KITCHEN)
        mods = [m for m in extras if rng.chance(35)]
        comped = rng.chance(8)
        discounts = [rng.choice([percent("10"), percent("15"), off(200)])] if rng.chance(12) else []
        lines.append(line(unit_price, count(rng.between(1, 2)), category, mods, comped, discounts))
        notes.append(name + (" (comped)" if comped else ""))
    order = [rng.choice([percent("20"), off(500), percent("12.5")])] if rng.chance(35) else []
    setup = rng.choice([("state and city", STATE_AND_CITY), ("alcohol surtax", ALCOHOL_SURTAX)])
    scope = rng.choice(["line", "document"])
    mode = rng.choice(["half_away_from_zero", "half_away_from_zero", "half_even"])
    dining = rng.choice(["on_premises", "to_go"])
    title = f"restaurant, {setup[0]}, tax per {scope}: " + ", ".join(notes)
    return basket(lines, dining=dining, discounts=order), rules(setup[1], scope, mode), title


def grocery(rng):
    lines, notes = [], []
    for _ in range(rng.between(1, 6)):
        name, unit_price, category, unit = rng.choice(SHELF)
        if unit == EACH:
            qty = count(rng.between(1, 4))
        else:
            qty = quantity(rng.between(50, 2500) * 1000, unit)
        lines.append(line(unit_price, qty, category))
        notes.append(name)
    order = [off(rng.choice([100, 250, 500]))] if rng.chance(20) else []
    extension = rng.choice(["half_away_from_zero", "half_away_from_zero", "half_even", "floor"])
    title = f"grocery, weights rounded {extension}: " + ", ".join(notes)
    chosen = rules(GROCERY_STATE, "line", "half_away_from_zero", extension)
    return basket(lines, dining="to_go", discounts=order), chosen, title


def exempt(rng):
    shopped, chosen, title = restaurant(rng)
    exempted = rng.choice([1, 2])
    shopped["exemptions"] = [exempted]
    return shopped, chosen, f"exempt from tax {exempted}; " + title


def currencies(rng):
    if rng.chance(50):
        lines = [line(rng.between(100, 3000), count(rng.between(1, 3)), rng.choice([PREPARED, GENERAL]))
                 for _ in range(rng.between(1, 4))]
        taxes = [tax(1, "Consumption tax", "10", [GENERAL]), tax(2, "Reduced rate on food", "8", [PREPARED])]
        order = [percent("5")] if rng.chance(40) else []
        return basket(lines, "JPY", discounts=order), rules(taxes, "document", "floor"), "yen, no minor unit"
    lines = [line(rng.between(100, 9999), count(rng.between(1, 2)), GENERAL) for _ in range(rng.between(1, 4))]
    order = [off(rng.between(1, 500))] if rng.chance(40) else []
    title = "dinar, three decimal places"
    return basket(lines, "KWD", discounts=order), rules([tax(1, "VAT", "5", [GENERAL])], "line"), title


def edges():
    """Hand-picked baskets at the edges: nothing to price, everything taken, ties."""
    cases = []
    cases.append((basket([]), rules(ONE_RATE), "an empty basket"))
    cases.append((basket([line(500, count(1), PREPARED, comped=True), line(300, count(2), PREPARED, comped=True)]),
                  rules(ONE_RATE), "every line comped"))
    cases.append((basket([line(999, count(1), GENERAL, discounts=[percent("100")])]),
                  rules(ONE_RATE), "a 100% line discount"))
    cases.append((basket([line(0, count(3), GENERAL), line(100, count(1), GENERAL)]),
                  rules(ONE_RATE), "a free item"))
    cases.append((basket([line(1000, count(1), GENERAL)]),
                  rules([tax(1, "Zero-rated", "0", [GENERAL])]), "a zero tax rate"))
    cases.append((basket([line(100, count(1), GENERAL)] * 3, discounts=[off(100)]),
                  rules(ONE_RATE), "three identical lines share a dollar off: the spare cent goes first"))
    cases.append((basket([line(10, count(1), GENERAL)] * 3),
                  rules([tax(1, "Five percent", "5", [GENERAL])], "line"), "half-cent taxes rounded per line"))
    cases.append((basket([line(10, count(1), GENERAL)] * 3),
                  rules([tax(1, "Five percent", "5", [GENERAL])], "document"), "half-cent taxes rounded once"))
    cases.append((basket([line(1500, count(1), GENERAL)], discounts=[off(5000)]),
                  rules(ONE_RATE), "an order discount larger than the order"))
    cases.append((basket([line(2500000000, count(4), GENERAL)]),
                  rules(ONE_RATE), "a large order: 4 × USD 25,000,000.00"))
    for mode in MODES:
        cases.append((basket([line(97, quantity(500_000, "kg"), GROCERY)]),
                      rules([], extension=mode), f"0.97 per kg × 0.5 kg, a tie, rounded {mode}"))
        cases.append((basket([line(30, count(1), GENERAL)]),
                      rules([tax(1, "Five percent", "5", [GENERAL])], "line", mode),
                      f"5% of 0.30, a half cent, rounded {mode}"))
        cases.append((basket([line(999, count(1), GENERAL, discounts=[percent("50")])]),
                      rules([], discounts=mode), f"half of 9.99, a tie, rounded {mode}"))
    return cases


def shared_lines():
    """Lines split among checks (ADR-0015): each case is one check's basket."""
    cases = []
    wine = line(3000, count(1), ALCOHOL)
    for index, held in enumerate(split(wine, [1, 1, 1]), start=1):
        cases.append((basket([held]), rules(ALCOHOL_SURTAX, "document"),
                      f"a 30.00 bottle of wine split three ways: part {index}, taxed on its own"))
    for index, held in enumerate(split(line(1000, count(1), GENERAL), [1, 1, 1]), start=1):
        cases.append((basket([held]), rules(ONE_RATE, "line"),
                      f"10.00 split three ways: part {index}; the spare cent goes to the first"))
    for index, held in enumerate(split(line(1299, quantity(453_000, "kg"), GROCERY), [1, 2]), start=1):
        cases.append((basket([held]), rules(GROCERY_STATE, "line"),
                      f"5.88 of cheese by weight split 1:2: part {index}"))
    for index, held in enumerate(split(line(100, count(1), GENERAL), [1, 2, 3]), start=1):
        cases.append((basket([held]), rules([]), f"1.00 split 1:2:3: part {index}"))
    for index, held in enumerate(split(line(1, count(1), GENERAL), [1, 1]), start=1):
        cases.append((basket([held]), rules(ONE_RATE), f"one cent split two ways: part {index}"))
    for index, held in enumerate(split(line(2400, count(1), PREPARED, comped=True), [1, 1]), start=1):
        cases.append((basket([held, line(895, count(1), PREPARED)]), rules(STATE_AND_CITY),
                      f"a comped platter shared by two checks, with a dessert: part {index}"))
    return cases


def splits(rng):
    """A table's check: some lines its own, some shared with other checks."""
    lines, notes = [], []
    for _ in range(rng.between(1, 5)):
        name, unit_price, category, extras = rng.choice(KITCHEN)
        mods = [m for m in extras if rng.chance(30)]
        share = None
        if rng.chance(60):
            weights = [rng.between(1, 3) for _ in range(rng.between(2, 4))]
            share = part(rng.below(len(weights)), *weights)
            name += " (part {} of {})".format(share["index"] + 1, ":".join(map(str, weights)))
        comped = rng.chance(8)
        lines.append(line(unit_price, count(rng.between(1, 3)), category, mods, comped, share=share))
        notes.append(name)
    setup = rng.choice([("state and city", STATE_AND_CITY), ("alcohol surtax", ALCOHOL_SURTAX)])
    scope = rng.choice(["line", "document"])
    title = f"a check of a split table, {setup[0]}, tax per {scope}: " + ", ".join(notes)
    return basket(lines), rules(setup[1], scope), title


def worked_examples():
    """The examples the unit tests and the ADR explain step by step."""
    latte = line(450, count(2), PREPARED, [modifier(50)])
    steak = line(2000, count(2), PREPARED, [modifier(300, 1, [modifier(50, 2)]), modifier(75, 2)])
    return [
        (basket([latte, line(325, count(1), PREPARED)]), rules(ONE_RATE),
         "two lattes with oat milk and a bagel: 13.25, tax 1.18 shared 0.89 and 0.29"),
        (basket([line(1299, quantity(453_000, "kg"), GROCERY)]), rules([]), "12.99 per kg × 0.453 kg = 5.88"),
        (basket([steak]), rules([]), "two steaks with a side salad and two sauces: 51.00"),
        (basket([line(999, count(1), GENERAL, discounts=[percent("20"), off(500), off(1000)])]),
         rules([]), "20%, then 5.00, then 10.00 off 9.99: the last is limited to 2.99"),
        (basket([line(1000, count(1), GENERAL), line(500, count(1), GENERAL), line(500, count(1), GENERAL)],
                discounts=[percent("10"), off(100)]), rules([]),
         "10% then 1.00 off lines of 10.00, 5.00 and 5.00"),
        (basket([line(2000, count(1), PREPARED)], discounts=[percent("25")]),
         rules([tax(1, "Ten percent", "10", [PREPARED])], "line"), "discounts reduce the taxable amount"),
    ]


def layout(value, indent=0, width=100):
    """JSON indented two spaces a level, with anything that fits on a line kept on one line."""
    flat = json.dumps(value, ensure_ascii=False)
    if indent + len(flat) <= width or not isinstance(value, (dict, list)) or not value:
        return flat
    inner = " " * (indent + 2)
    if isinstance(value, dict):
        items = [f"{inner}{json.dumps(key)}: {layout(item, indent + 2, width)}"
                 for key, item in value.items()]
        return "{\n" + ",\n".join(items) + "\n" + " " * indent + "}"
    items = [inner + layout(item, indent + 2, width) for item in value]
    return "[\n" + ",\n".join(items) + "\n" + " " * indent + "]"


def main():
    rng = SplitMix64(20260928)
    cases = worked_examples() + edges()
    families = [(cafe, 20), (restaurant, 20), (grocery, 16), (exempt, 8), (currencies, 8)]
    for family, n in families:
        cases += [family(rng) for _ in range(n)]
    cases += shared_lines()
    families = [(splits, 16)]
    for family, n in families:
        cases += [family(rng) for _ in range(n)]
    out = []
    for number, (shopped, chosen, title) in enumerate(cases, start=1):
        out.append({
            "number": number,
            "title": title,
            "rules": chosen,
            "basket": shopped,
            "expected": price(shopped, chosen),
        })
    document = {
        "format": 2,
        "about": "Golden baskets for keel-pricing, generated by generate.py: do not edit.",
        "baskets": out,
    }
    (HERE / "baskets.json").write_text(layout(document) + "\n")
    print(f"wrote {len(out)} golden baskets")


if __name__ == "__main__":
    main()
