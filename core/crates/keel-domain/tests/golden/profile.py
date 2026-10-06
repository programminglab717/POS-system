#!/usr/bin/env python3
"""The demo profile, built independently of Keel's codec with Python's cbor2: its encoding's
SHA-256, and its catalog's and pricing rules' versions, which keel-domain's known answers pin.

Run from anywhere:  python3 core/crates/keel-domain/tests/golden/profile.py
"""

import hashlib
import uuid

import cbor2


def ident(n):
    return uuid.UUID(f"0192f0c1-0000-7000-8000-{n:012x}").bytes


def usd(cents):
    return [cents, "USD"]


PLAIN, NO, EXTRA = 0, 1, 2


def modifier(n, name, prefix, cents, groups=()):
    return {1: ident(n), 2: name, 3: prefix, 4: usd(cents), 5: [ident(g) for g in groups]}


def group(n, name, bounds, repeat, modifiers):
    low, high, free = bounds
    record = {1: ident(n), 2: name, 4: high, 6: repeat, 7: modifiers}
    if low:
        record[3] = low
    if free:
        record[5] = free
    return record


def item(n, name, category, variants, groups):
    def variant(v, title, cents):
        record = {1: ident(v), 3: usd(cents)}
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

# Rounding modes by position in RoundingMode::ALL: HalfAwayFromZero is 0. Scope 1 is per document.
rules = {
    1: 0,
    2: 0,
    3: [{1: ident(0x20), 2: "NYC sales tax", 3: "0.08875", 4: [ident(0x10)]}],
    4: [1, 0],
}


def page(name, buttons):
    return {1: name, 2: [ident(b) for b in buttons]}


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
    11: [{1: ident(0x501), 2: "Alex"}, {1: ident(0x502), 2: "Sam"}],
}


def encode(value):
    return cbor2.dumps(value, canonical=True)


print("profile SHA-256 ", hashlib.sha256(encode(profile)).hexdigest())
print("profile length  ", len(encode(profile)))
print("catalog version ", hashlib.sha256(encode(catalog)).hexdigest())
print("rules version   ", hashlib.sha256(encode(rules)).hexdigest())
