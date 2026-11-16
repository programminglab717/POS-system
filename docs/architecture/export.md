# The export format

A location's export ([ADR-0037](../adr/0037-the-back-office.md), decision 3) is everything the
location's devices wrote, in one file a merchant can read without Keel: V-08's "leave anytime,
take everything", and V-07's escrow. The back office downloads it from
`GET /api/locations/{id}/export`, and the cell writes one to its escrow directory each day.

## The file

JSON Lines: UTF-8, one JSON object per line, each with a `type`.

1. **The manifest**, first:

   ```json
   {"type": "manifest", "format": "keel.export", "version": 1, "location": "…",
    "exportedAt": 1794560400000000, "events": 1234,
    "schemas": [{"name": "payment.captured", "version": 2,
                 "fields": [{"key": 1, "name": "amount", "doc": "…"}, …]}, …]}
   ```

   `exportedAt` is microseconds since the Unix epoch. `schemas` lists every schema version the
   events hold, each with its top-level fields: the same names the events' payloads use.

2. **The profile**, second: the location's settings, catalog and staff as the back office reads
   them (its settings, catalog and staff queries), with no PIN or anything of one.

   ```json
   {"type": "profile", "version": "…", "settings": {…}, "catalog": {…}, "staff": {…}}
   ```

3. **Every event**, after, by device and then by its sequence on the device:

   ```json
   {"type": "event", "id": "…", "stream": {"kind": "payment", "id": "…"},
    "device": "…", "seq": 7, "hlc": {"wallMs": 1794560400000, "logical": 0},
    "businessDate": "2026-11-13", "actor": {"member": "…"}, "approval": null,
    "schema": {"name": "payment.captured", "version": 1},
    "payload": {"amount": [450, "USD"], "tendered": [500, "USD"]},
    "signed": "…"}
   ```

   - `actor` is `{"member": id}`, `{"customer": id}`, `{"integration": id}`,
     `{"extension": id}`, or `{"system": "…"}` naming the part of Keel that wrote it.
   - `approval` is the identifier of the `approval.granted` event that approved it, or `null`.
   - `signed` is the event exactly as its device signed it, base64: it can be verified with the
     device's key and imported as it was written.

The events of one stream fold in the order every replica folds them: by `hlc` (`wallMs`, then
`logical`), then `device`, then `seq`.

## Values

A payload's top-level keys are named by its schema's fields. Its values are as the event holds
them:

| In the event | In the export |
|---|---|
| An integer | A JSON number. Integers can be beyond 2^53: read them as 64-bit. |
| Text, a boolean, nothing | A string, a boolean, `null`. |
| 16 bytes, an identifier | Its text, lowercase and hyphenated, as identifiers are written everywhere in the export; 16 bytes that aren't an identifier, as hex. |
| Other bytes | Lowercase hex. |
| An amount of money | `[minor units, "ISO 4217 code"]`: `[450, "USD"]` is $4.50. |
| A quantity | `[millionths, "unit code"]`: `[1500000, "kg"]` is 1.5 kg. |
| A time | Microseconds since the Unix epoch. |
| A list | A JSON array. |
| A nested record | A JSON object keyed by its integer keys, as text: `{"1": …, "2": …}`. |

A field that is absent from a payload is absent from the export: optional fields are left out,
never written as `null`.

## Reading one

`tools/export/read.py` reads an export with nothing of Keel's: it adds up what each business
day took, by tender, from the payments' events. `keel-cloud`'s export test checks that it agrees
with the kernel's day report, and that an export imported into a fresh store gives the same
events, projections and reports.

## Fields by schema

The reference below is generated from the kernel's field registry, and a test checks it is
current.
