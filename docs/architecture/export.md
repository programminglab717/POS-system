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
                 "fields": [{"key": 1, "name": "amount"}, …]}, …]}
   ```

   `exportedAt` is microseconds since the Unix epoch. `schemas` lists every schema version the
   events hold, each with its top-level fields: the same names the events' payloads use.

2. **The profile**, second: the location's settings, catalog and staff as the back office reads
   them (its settings, catalog and staff queries), with no PIN or anything of one.

   ```json
   {"type": "profile", "version": "…", "settings": {…}, "catalog": {…}, "staff": {…}}
   ```

3. **Every event**, after, by device and then by its sequence on the device, but the
   profile's publications (`profile.chunk`, `profile.published`): their chunks hold the team's
   PIN hashes and salts, in their payload and in the bytes their device signed alike, so they
   can't be redacted and are left out. The profile line is the profile, without them. Only the
   cloud publishes, and its log holds nothing else, so leaving them out leaves no log with a gap.

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
| 16 bytes, an identifier | Its text, lowercase and hyphenated, as identifiers are written everywhere in the export; 16 bytes that aren't an identifier, as hex. Every field of 16 bytes an exported schema has is an identifier. |
| Other bytes | Lowercase hex. |
| An amount of money | `[minor units, "ISO 4217 code"]`: `[450, "USD"]` is $4.50. |
| A quantity | `[millionths, "unit code"]`: `[1500000, "kg"]` is 1.5 kg. |
| A time | Microseconds since the Unix epoch. |
| A list | A JSON array. |
| A nested record | A JSON object keyed by its integer keys, as text: `{"1": …, "2": …}`. |

A field that is absent from a payload is absent from the export: optional fields are left out,
never written as `null`.

## Importing one

An import reads the manifest first, then each event's `signed` bytes, a line at a time, each
verified as sync verifies an event from another device and stored if it is the next of its
device's log. A file with no manifest, or with fewer or more events than its manifest says, as
one cut short, is refused, after storing what it held.

## Reading one

`tools/export/read.py` reads an export with nothing of Keel's: it adds up what each business
day took, by tender, from the payments' events. `keel-cloud`'s export test checks that it agrees
with the kernel's day report, and that an export imported into a fresh store gives the same
events, projections and reports.

## Fields by schema

The reference below is generated from the kernel's field registry, and a test checks it is
current.

| Schema | Version | Fields: key and name |
|---|---|---|
| `approval.granted` | 1 | 1 `approver`, 2 `action`, 3 `amount`, 4 `percent`, 5 `subject` |
| `approval.granted` | 2 | 1 `approver`, 2 `action`, 3 `amount`, 4 `percent`, 5 `subject` |
| `approval.granted` | 3 | 1 `approver`, 2 `action`, 3 `amount`, 4 `percent`, 5 `subject` |
| `availability.changed` | 1 | 1 `kind`, 2 `available`, 3 `until`, 4 `reason`, 5 `note` |
| `drawer.cash_moved` | 1 | 1 `kind`, 2 `amount`, 3 `reason`, 4 `note` |
| `drawer.cash_refunded` | 1 | 1 `refund`, 2 `amount` |
| `drawer.cash_taken` | 1 | 1 `payment`, 2 `amount` |
| `drawer.closed` | 1 | 1 `counted`, 2 `expected` |
| `drawer.no_sale` | 1 | 1 `reason`, 2 `note` |
| `drawer.opened` | 1 | 1 `float` |
| `hub.claimed` | 1 | 1 `epoch`, 2 `priority`, 3 `previous`, 4 `cuts` |
| `kitchen.ticket_bumped` | 1 | none |
| `kitchen.ticket_recalled` | 1 | none |
| `kitchen.ticket_rerouted` | 1 | 1 `to` |
| `kitchen.ticket_sent` | 1 | 1 `order`, 2 `station`, 3 `orderMode`, 4 `items` |
| `kitchen.ticket_shown` | 1 | none |
| `order.abandoned` | 1 | none |
| `order.attributes_changed` | 1 | 1 `mode`, 2 `revenueCenter`, 3 `table`, 4 `guestCount`, 5 `customer`, 6 `owner` |
| `order.check_closed` | 1 | 1 `check`, 2 `rulesVersion`, 3 `lines`, 4 `taxes`, 5 `total`, 6 `payments` |
| `order.check_opened` | 1 | 1 `check` |
| `order.closed` | 1 | none |
| `order.created` | 1 | 1 `channel`, 2 `mode`, 3 `currency`, 4 `revenueCenter`, 5 `table`, 6 `guestCount`, 7 `customer`, 8 `owner` |
| `order.discount_applied` | 1 | 1 `discount`, 2 `line`, 3 `percent`, 4 `amount`, 5 `reason`, 6 `note` |
| `order.discount_removed` | 1 | 1 `discount` |
| `order.line_added` | 1 | 1 `line`, 2 `variant`, 3 `catalogVersion`, 4 `name`, 5 `taxCategory`, 6 `unitPrice`, 7 `quantity`, 8 `modifiers`, 9 `seat`, 10 `course`, 11 `notes` |
| `order.line_changed` | 1 | 1 `line`, 2 `quantity`, 3 `modifiers`, 4 `seat`, 5 `course`, 6 `notes` |
| `order.line_comped` | 1 | 1 `line`, 2 `reason`, 3 `note` |
| `order.line_removed` | 1 | 1 `line` |
| `order.line_repriced` | 1 | 1 `line`, 2 `unitPrice`, 3 `reason`, 4 `note` |
| `order.line_voided` | 1 | 1 `line`, 2 `reason`, 3 `note` |
| `order.lines_allocated` | 1 | 1 `allocations` |
| `order.lines_fired` | 1 | 1 `lines` |
| `order.lines_returned` | 1 | 1 `refund`, 2 `lines`, 3 `reason`, 4 `note` |
| `order.ownership_granted` | 1 | 1 `request`, 2 `device`, 3 `lease`, 4 `epoch` |
| `order.ownership_overridden` | 1 | 1 `lease`, 2 `reason`, 3 `note` |
| `order.ownership_refused` | 1 | 1 `request`, 2 `refusal` |
| `order.ownership_requested` | 1 | 1 `lease` |
| `order.reopened` | 1 | 1 `reason`, 2 `note` |
| `order.voided` | 1 | 1 `reason`, 2 `note` |
| `payment.authorized` | 1 | 1 `amount`, 2 `reference` |
| `payment.captured` | 1 | 1 `amount`, 2 `tip`, 3 `reference`, 4 `tendered`, 5 `rounding` |
| `payment.captured` | 2 | 1 `amount`, 2 `tip`, 3 `reference`, 4 `tendered`, 5 `rounding`, 6 `card` |
| `payment.failed` | 1 | 1 `reason`, 2 `note`, 3 `reference` |
| `payment.initiated` | 1 | 1 `order`, 2 `check`, 3 `tender`, 4 `amount` |
| `payment.voided` | 1 | 1 `reason`, 2 `note`, 3 `reference` |
| `profile.chunk` | 1 | 1 `version`, 2 `index`, 3 `bytes` |
| `profile.published` | 1 | 1 `version`, 2 `size`, 3 `chunks` |
| `refund.completed` | 1 | 1 `amount`, 2 `rounding` |
| `refund.failed` | 1 | 1 `reason`, 2 `note`, 3 `reference` |
| `refund.initiated` | 1 | 1 `order`, 2 `tender`, 3 `amount` |
| `refund.initiated` | 2 | 1 `order`, 2 `tender`, 3 `amount`, 4 `payment` |
| `sequence.assigned` | 1 | 1 `epoch`, 2 `first`, 3 `runs` |
| `stock.counted` | 1 | 1 `quantity`, 2 `asOf`, 3 `note` |
| `stock.moved` | 1 | 1 `movement`, 2 `quantity`, 3 `reason`, 4 `note` |
