#!/usr/bin/env python3
"""Two location profiles, built independently of Keel's codec with Python's cbor2: each encoding's
SHA-256 and length, and its catalog's and pricing rules' versions, which keel-domain's known
answers pin.

- The demo café's (`profile::demo`), in New York: in format 2, with roles and its members' PINs
  (ADR-0029), hashed with `argon2-cffi`, the Argon2 reference implementation's bindings.
- A bakery's in Toronto (`golden_bakery` in `src/profile/tests.rs`), which writes every optional
  field the demo leaves out: cash rounding, a tax by dining, a group's minimum and free
  applications together, every prefix, and modifiers chosen under a modifier in a group that
  repeats.

Run from anywhere:  python3 core/crates/keel-domain/tests/golden/profile.py
(with `cbor2` and `argon2-cffi` installed, as in a virtual environment).
"""

import hashlib
import uuid

import cbor2


def ident(n):
    return uuid.UUID(f"0192f0c1-0000-7000-8000-{n:012x}").bytes


# Prefixes, by their codes in keel-domain's `Prefix`.
PLAIN, NO, EXTRA, LIGHT, SIDE, SUB = 0, 1, 2, 3, 4, 5

# Rounding modes, by their codes in the profile: 0 half away from zero, 1 half even, 2 half
# toward zero, 3 away from zero, 4 toward zero, 5 ceiling, 6 floor. Tax rounding scopes: 0 per
# line, 1 per document. Dining: 0 on the premises, 1 to go.
HALF_AWAY, HALF_EVEN, FLOOR = 0, 1, 6
PER_LINE, PER_DOCUMENT = 0, 1
ON_PREMISES, TO_GO = 0, 1


def builders(currency):
    def money(minor):
        return [minor, currency]

    def modifier(n, name, prefix, minor, groups=()):
        return {1: ident(n), 2: name, 3: prefix, 4: money(minor), 5: [ident(g) for g in groups]}

    def group(n, name, bounds, repeat, modifiers):
        low, high, free = bounds
        record = {1: ident(n), 2: name, 4: high, 6: repeat, 7: modifiers}
        if low:
            record[3] = low
        if free:
            record[5] = free
        return record

    def item(n, name, category, variants, groups):
        def variant(v, title, minor):
            record = {1: ident(v), 3: money(minor)}
            if title is not None:
                record[2] = title
            return record

        return {
            1: ident(n),
            2: name,
            3: ident(category),
            4: [variant(*v) for v in variants],
            5: [ident(g) for g in groups],
        }

    return modifier, group, item


def tax(n, name, rate, categories, dining=None):
    # A set of identifiers is written in the ascending order of their bytes.
    record = {1: ident(n), 2: name, 3: rate, 4: sorted(ident(c) for c in categories)}
    if dining is not None:
        record[5] = dining
    return record


def page(name, buttons):
    return {1: name, 2: [ident(b) for b in buttons]}


def team(*members):
    return [{1: ident(n), 2: name} for n, name in members]


def pin(digits, salt):
    """A PIN's hash, as ADR-0029 has it: Argon2id 0x13, 19 MiB, 2 passes, 1 lane, 32 bytes."""
    from argon2.low_level import Type, hash_secret_raw

    hashed = hash_secret_raw(digits.encode(), salt, time_cost=2, memory_cost=19456,
                             parallelism=1, hash_len=32, type=Type.ID, version=19)
    return {1: salt, 2: hashed}


# The actions of ADR-0029, by code: order.void, line.remove, discount.apply, price.override,
# refund.issue, drawer.no_sale, drawer.variance, drawer.paid_out, receipt.reprint,
# item.availability.
ACTIONS = range(10)


def demo():
    modifier, group, item = builders("USD")
    groups = [
        group(0x101, "Milk", (0, 1, 0), False, [
            modifier(0x201, "Whole milk", PLAIN, 0),
            modifier(0x202, "Oat milk", PLAIN, 75),
            modifier(0x203, "Almond milk", PLAIN, 75),
            modifier(0x204, "Skim milk", PLAIN, 0),
        ]),
        group(0x102, "Extra shots", (0, 4, 0), True, [modifier(0x205, "Shot", EXTRA, 95)]),
        group(0x103, "Syrup", (0, 3, 1), True, [
            modifier(0x206, "Vanilla", PLAIN, 60),
            modifier(0x207, "Caramel", PLAIN, 60),
            modifier(0x208, "Hazelnut", PLAIN, 60),
            modifier(0x209, "Sugar-free vanilla", PLAIN, 60),
        ]),
        group(0x104, "Spread", (1, 1, 0), False, [
            modifier(0x20A, "Cream cheese", PLAIN, 125),
            modifier(0x20B, "Butter", PLAIN, 50),
            modifier(0x20C, "Spread", NO, 0),
        ]),
        group(0x105, "Egg", (1, 1, 0), False, [
            modifier(0x20D, "Scrambled", PLAIN, 0),
            modifier(0x20E, "Fried", PLAIN, 0),
        ]),
        group(0x106, "Side", (0, 1, 0), False, [
            modifier(0x20F, "Fruit cup", PLAIN, 250),
            modifier(0x210, "Hash brown", PLAIN, 200, [0x107]),
        ]),
        group(0x107, "Sauce", (0, 2, 1), False, [
            modifier(0x211, "Ketchup", PLAIN, 25),
            modifier(0x212, "Hot sauce", PLAIN, 25),
        ]),
        group(0x108, "Toasting", (0, 1, 0), False, [modifier(0x213, "Toasted", PLAIN, 0)]),
    ]
    items = [
        item(0x301, "Espresso", 0x10, [(0x401, None, 325)], [0x102, 0x103]),
        item(0x302, "Latte", 0x10, [(0x402, "Small", 450), (0x403, "Large", 525)],
             [0x101, 0x102, 0x103]),
        item(0x303, "Drip coffee", 0x10,
             [(0x404, "Small", 250), (0x405, "Medium", 295), (0x406, "Large", 345)], [0x101]),
        item(0x304, "Bagel", 0x10, [(0x407, None, 275)], [0x104, 0x108]),
        item(0x305, "Breakfast sandwich", 0x10, [(0x408, None, 695)], [0x105, 0x106]),
        item(0x306, "Bottled water", 0x10, [(0x409, None, 200)], []),
        item(0x307, "Coffee beans, 12 oz", 0x11, [(0x40A, None, 1400)], []),
    ]
    catalog = {1: items, 2: groups}
    rules = {
        1: HALF_AWAY,
        2: HALF_AWAY,
        3: [tax(0x20, "NYC sales tax", "0.08875", [0x10])],
        4: [PER_DOCUMENT, HALF_AWAY],
    }
    profile = {
        1: 1,
        2: ident(0xC0FE),
        3: "Keel Café",
        4: ["12 Harbor Street", "Brooklyn, NY 11201"],
        5: "USD",
        6: ["America/New_York", 4, 0],
        8: rules,
        9: catalog,
        10: {1: [
            page("Coffee", [0x401, 0x402, 0x403, 0x404, 0x405, 0x406]),
            page("Food", [0x407, 0x408, 0x409]),
            page("Retail", [0x40A]),
        ]},
        11: [
            {1: ident(0x501), 2: "Alex", 3: ident(0x601), 4: pin("1234", b"keel-demo-alex-1")},
            {1: ident(0x502), 2: "Sam", 3: ident(0x602), 4: pin("2468", b"keel-demo-sam--2")},
        ],
        12: [
            {1: ident(0x601), 2: "Manager", 3: [{1: code} for code in ACTIONS]},
            {1: ident(0x602), 2: "Cashier", 3: [
                {1: 1},
                {1: 2, 2: 500, 3: 1000},
                {1: 6, 2: 500},
                {1: 8},
                {1: 9},
            ]},
        ],
    }
    profile[1] = 2
    return profile, catalog, rules


def bakery():
    modifier, group, item = builders("CAD")
    groups = [
        group(0x111, "Warming", (0, 1, 0), False, [modifier(0x221, "Warmed", PLAIN, 0)]),
        group(0x112, "Filling", (1, 2, 1), False, [
            modifier(0x222, "Almond paste", PLAIN, 150),
            modifier(0x223, "Chocolate", PLAIN, 100),
            modifier(0x224, "Filling", NO, 0),
        ]),
        group(0x113, "Toppings", (0, 5, 2), True, [
            modifier(0x225, "Icing sugar", EXTRA, 0, [0x114]),
            modifier(0x226, "Crème fraîche", SIDE, 175),
            modifier(0x227, "Berries", SUB, 225),
        ]),
        group(0x114, "Amount", (0, 1, 0), False, [
            modifier(0x228, "Dusting", LIGHT, 0),
            modifier(0x229, "Snowfall", EXTRA, 25),
        ]),
    ]
    items = [
        item(0x311, "Croissant", 0x14, [(0x411, None, 395)], [0x111, 0x112]),
        item(0x312, "Café au lait", 0x13, [(0x412, "Bol", 550), (0x413, "Tasse", 425)], []),
        item(0x313, "Tarte aux fruits", 0x14, [(0x414, "Slice", 650), (0x415, "Whole", 4800)],
             [0x113]),
        item(0x314, "Quiche", 0x12, [(0x416, None, 1195)], [0x111]),
    ]
    catalog = {1: items, 2: groups}
    rules = {
        1: HALF_EVEN,
        2: FLOOR,
        3: [
            tax(0x21, "HST", "0.13", [0x13, 0x12]),
            tax(0x22, "HST, baked goods eaten here", "0.13", [0x14], ON_PREMISES),
            tax(0x23, "Container levy", "0.005", [0x12, 0x13, 0x14], TO_GO),
        ],
        4: [PER_LINE, HALF_EVEN],
    }
    profile = {
        1: 1,
        2: ident(0xB4CE),
        3: "Boulangerie Keel",
        4: ["88 Queen Street West", "Toronto, ON M5H 2N2", "Canada"],
        5: "CAD",
        6: ["America/Toronto", 3, 30],
        7: [HALF_AWAY, 5],
        8: rules,
        9: catalog,
        10: {1: [
            page("Viennoiserie", [0x411]),
            page("Café", [0x412, 0x413]),
            page("Pâtisserie", [0x414, 0x415, 0x416]),
        ]},
        11: team((0x511, "Camille"), (0x512, "Jo"), (0x513, "Noor")),
    }
    return profile, catalog, rules


def encode(value):
    return cbor2.dumps(value, canonical=True)


for title, build in [("demo", demo), ("bakery", bakery)]:
    profile, catalog, rules = build()
    print(title)
    print("  profile SHA-256 ", hashlib.sha256(encode(profile)).hexdigest())
    print("  profile length  ", len(encode(profile)))
    print("  catalog version ", hashlib.sha256(encode(catalog)).hexdigest())
    print("  rules version   ", hashlib.sha256(encode(rules)).hexdigest())
