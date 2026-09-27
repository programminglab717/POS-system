# ISO 4217 currency data snapshot

`codes-all.csv` is a snapshot of ISO 4217 list one (current codes) plus historic entries, as
mirrored by the [`datasets/currency-codes`](https://github.com/datasets/currency-codes) project
(`data/codes-all.csv`). Columns: `Entity, Currency, AlphabeticCode, NumericCode, MinorUnit,
WithdrawalDate`.

| Property | Value |
|---|---|
| Snapshot taken | 2026-09-27 |
| SHA-256 | `c4b6829a966f0564e77dc6c2d100d268cce61b30f7637bf3d5ec626b0393407f` |
| Active codes | 178 |

## Verification performed

The official source (SIX Group, the ISO 4217 maintenance agency) was not reachable from the build
environment, so the snapshot was cross-checked against an independent source instead:

- The `iso_currency` crate v0.7.1 (`isodata.tsv`) lists the same 178 active codes, with
  **identical minor units and identical numeric codes** for every one of them.
- Recent ISO changes are present:
  - Bulgarian lev (BGN) withdrawn 2026-01, after Bulgaria adopted the euro;
  - Caribbean guilder (XCG) replaced the Netherlands Antillean guilder (ANG), 2025-03;
  - Zimbabwe Gold (ZWG) replaced ZWL, 2024-09;
  - the redenominated leone (SLE) replaced SLL, 2023-12.

## How it is used

`tools/gen_currency_table.py` generates `src/currency/table.rs` from this file. It includes every
active code that is a circulating currency, excluding the non-currency codes below. It also
includes currencies **withdrawn since 2020**, marked as withdrawn, so recent history can still be
imported: ANG, BGN, CUC, HRK, SLL, ZWL.

Excluded non-currency codes:
- funds: BOV, CHE, CHW, CLF, COU, MXV, USN, UYI, UYW;
- units of account: XAD, XDR, XSU, XUA;
- bond market units: XBA, XBB, XBC, XBD;
- precious metals: XAG, XAU, XPD, XPT;
- testing and no-currency codes: XTS, XXX.

The minor units of the included withdrawn currencies (all 2) come from the `iso_currency`
cross-check. ANG is the exception: the crate doesn't list it, so its value comes from its last ISO
listing, where it matched its successor XCG.

The test `tests/currency_table.rs` re-reads this CSV and fails if the generated table ever drifts
from it.

## Updating

1. Replace `codes-all.csv` with a newer snapshot and update the table above.
2. Run `python3 tools/gen_currency_table.py`.
3. Review the diff of `src/currency/table.rs` and run the tests.
