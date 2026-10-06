# CLDR number data snapshot

`en/` and `es-US/` hold CLDR's number data for the locales Keel formats amounts in
([ADR-0023](../../../../../docs/adr/0023-register-shell.md), decision 5): each locale's
`numbers.json` (its decimal, grouping and minus symbols, and its currency patterns) and
`currencies.json` (each currency's symbol in it), from the `cldr-numbers-full` package of
[`unicode-org/cldr-json`](https://github.com/unicode-org/cldr-json), release 48.2.3 (CLDR 48),
unchanged. CLDR's `en` is American English: `en-US` has no data of its own.

| File | SHA-256 |
|---|---|
| `en/numbers.json` | `07a58e473f6d28dcc5d5562539d3ab5c8fbe37e435414969e3a2672525a80d5f` |
| `en/currencies.json` | `a6c41b28f4035f65ae698b53a66866982b4ff49bc0de7e840f91d2c6e6bdc23b` |
| `es-US/numbers.json` | `44123c99c6ee9718de009971d8586018d887be5317239a1f7710ae4dc112cab1` |
| `es-US/currencies.json` | `f4626225d35b641d7313c052e35925181f58b03b723b7f668d79b54b339d4465` |

Snapshot taken 2026-10-02, from
`https://raw.githubusercontent.com/unicode-org/cldr-json/48.2.3/cldr-json/cldr-numbers-full/main/<locale>/<file>.json`.
CLDR's data is under the Unicode License v3.

## How it is used

`tools/gen_locale_table.py` generates `src/locale/table.rs` from these files: each locale's
decimal, grouping and minus symbols, and the currencies whose symbol in it isn't their ISO 4217
code, with whether a no-break space comes between the symbol and the digits. That is CLDR's
currency spacing after the symbol, as ICU applies it: a space when the symbol's last character is
neither a symbol nor a separator, a letter (`EUR`) or a period (`Cg.`) alike.

The generator stops with an error if anything else CLDR says about how amounts and quantities
look isn't what `Locale` implements: the currency patterns `¤#,##0.00` and `¤ #,##0.00` (a
no-break space), the currency spacing on both sides of the symbol, the decimal pattern
`#,##0.###`, grouping from four digits, the Latin digits, and no separators of amounts' own or
of any currency's own. A CLDR update can therefore change only what the table holds, which CI
shows: it regenerates the table and fails if it changes.

`Locale` differs from CLDR deliberately in showing every digit a value has: every minor unit of
an amount, from the currency's ISO 4217 exponent, where CLDR would use its own digit count for a
few currencies; and every decimal of a quantity, up to the millionths it is held in, where CLDR's
pattern shows three.

The test `tests/locale_table.rs` re-reads these files and fails if the table ever drifts from
them.

## Verification performed

Keel's formatting is compared with ICU's, through Node.js's `Intl.NumberFormat`, by
`tools/icu_crosscheck.js`, which reads what the `locale_dump` example writes:

```sh
cargo run -q -p keel-types --example locale_dump > /tmp/keel-locale.tsv
node core/crates/keel-types/tools/icu_crosscheck.js /tmp/keel-locale.tsv
```

The example shows 324 values in each of the 161 currencies Keel knows, in both locales, as
amounts in minor units, and as quantities in millionths: zero, small values, every power of ten
and its neighbours, the ends of the 64-bit range, and 200 values spread over every magnitude,
each with either sign. ICU is told to show what Keel always does: each currency's ISO 4217
decimals, and up to six decimals of a quantity. With Node.js 22.22, whose ICU 78.2 has CLDR
48.0, all 104,976 were identical (2026-10-06).

## Updating

1. Download the files of a newer release into `en/` and `es-US/`, and update the table above.
2. Run `python3 tools/gen_locale_table.py`.
3. Review the diff of `src/locale/table.rs`, run the tests, and compare with ICU again, with a
   Node.js whose ICU has the same CLDR release.
