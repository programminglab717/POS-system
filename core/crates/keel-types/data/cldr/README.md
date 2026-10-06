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
symbols, and the currencies whose symbol in it isn't their ISO 4217 code, with whether a no-break
space comes between the symbol and the digits (CLDR's `alphaNextToNumber` pattern, for a symbol
whose last character is neither a symbol nor a separator). It stops with an error if a locale's
patterns aren't the ones `Locale` implements: `¤#,##0.00`, `¤ #,##0.00` with a no-break space,
grouping from four digits, and the Latin digits. CI regenerates the table and fails if it
changes.

`Locale` shows every minor unit an amount has, from the currency's ISO 4217 exponent, where CLDR
would use its own digit count for the currency: an amount reads exactly as it is held.

The test `tests/locale_table.rs` re-reads these files and fails if the table ever drifts from
them.

## Verification performed

Keel's formatting was compared with ICU 78.2, whose data is CLDR 48.0, through Node.js
22's `Intl.NumberFormat`, with each currency's ISO 4217 decimals: 0, 5, 999, 123,456, −7,
−123,456 and 100,000,000 minor units of each of the 161 currencies Keel knows, in both locales.
All 2,254 were identical.

## Updating

1. Download the files of a newer release into `en/` and `es-US/`, and update the table above.
2. Run `python3 tools/gen_locale_table.py`.
3. Review the diff of `src/locale/table.rs`, run the tests, and compare with ICU again.
