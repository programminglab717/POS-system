# ADR-0037: The back office: a location's settings, a live dashboard, and a full export with escrow

- **Status:** Proposed
- **Date:** 2026-11-16

## Context

The MVP's ninth step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is the back office:
staff and roles, settings, the dashboard and export (O-01, V-07, and the export V-08 names). What
is built shapes it:

- The **cloud cell** keeps each location's durable copy: every device's log, its projections and
  a runtime over them, so the cell answers what any register would (ADR-0028). Its API is JSON
  over HTTPS, behind one operator's token until the control plane's identity.
- The **back office** is a React app on that API: a catalog editor, reports of sales by day, item
  and tender, the devices and their replication, and since step 1 the staff and their roles
  (ADR-0029, decision 6). Staff and roles are built.
- A **profile edit** is read with its version, changed, checked whole by the kernel, and published
  as a new version, refused if its base is no longer current (ADR-0028, decision 3). The catalog
  and the staff are edited so; the rest of the profile, the location's settings, is set only at
  provisioning.
- The **day report** is the kernel's: every device's orders, tenders, refunds, tips, drawers,
  waste and counts for a business day, from the projections (ADR-0031 to ADR-0036).
- Every event has a **schema and version**, a pinned example payload, and integer keys (ADR-0013):
  compact, and meaningless to a reader without the schema.

## Decision

1. **Settings are the third part of the profile the back office edits**, beside the catalog and
   the staff, the same way: `GET /api/locations/{id}/settings`, `PUT` with its base version, and
   `POST .../check`. They are the location's name and address; its business day (time zone and
   cutoff); cash rounding; taxes and how they round; tipping; and the kitchen (stations, routes,
   aging and the fallback printer). A write replaces those and keeps the catalog, menu, team and
   roles. The location and its currency are shown but never written: a new currency is a new
   location, as totals across days would mix. Checked whole by the kernel, so a route to an item
   the catalog doesn't have, or a tax rate out of range, is refused with the kernel's words.

2. **The dashboard (O-01)** is one query of a business day, `GET .../dashboard?date=`, the
   current day by default, from the durable copy:
   - the day's report, as the registers' X report computes it, and the same weekday a week before
     to compare;
   - sales by hour of the location's local time, and the top ten items by amount;
   - open orders, and how long the oldest has been open;
   - the kitchen: tickets waiting, and the median and longest time to bump of those bumped;
   - items 86'd, and items whose stock on hand is below zero;
   - each device, and how far the durable copy is behind it.
   The back office's dashboard page asks again every 10 s while it is shown, and reads at phone
   width. Labor waits for the time clock (M-03, v1).

3. **The export (V-08)** is one file of a location, `GET .../export`, JSON Lines, streamed so it
   never sits whole in memory:
   - a manifest: the format (`keel.export` version 1), the location, when it was made, how many
     events, and each schema it holds with its fields;
   - the current profile, as the back office reads it: PIN hashes left out;
   - every event, by device and sequence: its envelope's fields by name, its payload by field
     name, and its signed bytes, base64, so it can be verified and imported as it was written.
   Field names come from a **field registry in the kernel**: each schema version's top-level keys,
   named and described. A test checks every pinned example payload's keys are in its schema's
   registry, and a generated reference, `docs/architecture/export.md`, is checked to be current.

4. **Verified both ways.** A *round trip*: the export imported into a fresh store gives the same
   events, the same projections and the same day reports. And a *reader independent of Keel*, a
   Python script in `tools/export/`, adds up each business day's sales from the export alone, and
   matches the kernel's report: the format is open in fact, not only on paper.

5. **Escrow (V-07)**: the cell writes each location's export to an escrow directory, if its
   configuration names one, once a day after the location's business-day cutoff, keeping the last
   seven. The escrow agent the contract names takes them from there. The export is the escrow's
   format, so what a merchant holds can be read without Keel.

6. **Built in slices**, each verified as it lands, then reviewed:
   1. *Settings*: their JSON, the API and its checks, and the back office's settings page.
   2. *The dashboard*: its query in the kernel and the cell, and the back office's page.
   3. *The export and escrow*: the field registry and its reference, the export, the import and
      its round trip, the independent reader, and escrow.

7. **Out of scope**: labor (M-03); the report builder and saved views (O-04); several locations
   rolled up; the control plane's identity, users and their roles in the back office (ADR-0028,
   decision 7); recording stock from the back office, which waits for receiving with purchase
   orders and costs (K-04, K-06); tables for a warehouse (O-08); importing from other systems
   (V-02).

## Consequences

**Positive**
- A merchant sets up and changes their location without a support call, checked by the same
  kernel that will run it.
- The dashboard's numbers are the registers' own, so the back office and the X report never
  disagree.
- Leaving Keel takes one file, which a script of a hundred lines reads.

**Negative**
- The field registry is one more thing every new schema version adds; its test makes forgetting
  fail.
- A dashboard asked every 10 s by many viewers runs the day's queries each time: fine for a
  location, to cache if it isn't.
- An export holds every event the location wrote: large for a busy year, and only as private as
  where it is kept.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Export tables (orders, lines, payments) as CSV | Derived: they lose what the events say and can't be imported back. They can come from the export. |
| Payloads with their integer keys, and a schema document beside them | Every reader would have to join the two; named fields read alone. |
| A zip of files | A new dependency for no gain over one stream a line at a time. |
| The dashboard pushed over a socket | Polling every 10 s is real time enough for a manager, and needs nothing new. |

## References
- [ADR-0013](./0013-event-payloads-and-schema-evolution.md), [ADR-0028](./0028-cloud-cell-v0.md),
  [ADR-0029](./0029-staff-roles-and-approvals.md), [ADR-0031](./0031-cash-drawers-and-day-reports.md),
  [ADR-0034](./0034-the-kitchen.md), [ADR-0036](./0036-inventory.md); the feature catalog's O-01,
  V-07 and V-08.
