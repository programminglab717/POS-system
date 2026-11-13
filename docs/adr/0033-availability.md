# ADR-0033: Availability: an 86 as an event per item, the latest by HLC winning, in-store within 10 s

- **Status:** Proposed
- **Date:** 2026-11-13

## Context

The MVP's fifth step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is availability: a global
86 in-store, within 10 s (G-06, in-store scope). When the kitchen runs out of something, every
register in the store stops selling it, at once, and starts again when it's back.

The domain model makes availability a first-class, event-sourced state per item and location
(§5.3), changed by `ItemAvailabilityChanged` (§17). Offline and sync calls a device's 86 a local
override of reference data: an event, folded last-writer-wins by HLC (§6.5), reaching every
in-store device within a second and every channel within 10 s. Limited-quantity countdowns are
hub-coordinated (class C, §18); auto-86 at zero stock needs stock (step 8).

The kernel has what this needs: replication through the hub carries every event to every device
of the location (ADR-0019, ADR-0026); the roles already list `item.availability` (ADR-0029); a menu and a
modifier sheet are read from the location's profile (ADR-0023).

## Decision

1. **Availability is an aggregate per item**, stream kind `availability`, its identifier the
   variant's or the modifier's. Any device writes it. One event, v1:

   | Schema | Keys |
   |---|---|
   | `availability.changed` | 1 kind (0 variant, 1 modifier), 2 available (a bool), 3 until (optional: a time, when back on sale), 4 reason code (optional), 5 note (optional) |

   An 86 (available false) with `until` is sold out until then; with none, until it is put back.
   An un-86 (available true) has no `until`.

2. **The latest change wins**, by the events' canonical order, which follows their HLCs: two
   devices that 86 and un-86 an item at once agree on the order, and on which stands. The fold
   reports a change at another location (not applied) and one naming the other kind of item
   (applied: the kind follows the latest). Commands refuse a change that changes nothing, an
   `until` that isn't after the change, and an un-86 with an `until`.

3. **An 86 lasts the business day** by default: the register fills `until` with the start of the
   next business day, by the profile's cutoff. A cashier can 86 until it is put back instead.
   Whether an item is sold out is a question asked of a time: an 86 whose `until` has passed is
   no longer one, with no event.

4. **Gated as the roles say**: an 86 and an un-86 are `item.availability` (ADR-0029).

5. **The store projects availability**: an `availability` table, one row per item changed (its
   kind, whether available, until when, its last HLC and business date), so the register asks
   whether an item is sold out without reading its events; and a version, which moves with every
   change, so a shell knows to read the menu again. It is a projection, rebuilt from the events:
   no new store schema.

6. **Ringing** refuses an item, or a modifier chosen, that is sold out at the time it is rung; a
   line rung before stays. The menu and the modifier sheet show what is sold out, and until when.

7. **In-store within 10 s**: the 86 reaches every device of the location through the hub, as
   every event does; a register reads the menu again when the availability version moves, which
   its shell asks for every 2 s. A test with a hub and a register measures it in the runtime's
   time.

8. **The shells**: the bindings take an 86 and an un-86, the menu and sheet's availability, and
   the version; the Android register 86s an item from the menu, puts it back, and shows what is
   sold out.

9. **Built in slices**, each verified as it lands, then reviewed:
   1. *The domain and the store*: the schema, the fold, the commands, the projection.
   2. *The register*: the runtime's 86, un-86, the menu's and sheet's availability, the refusal
      when ringing, the version, the bindings; the propagation test.
   3. *The shell*: the Android register's 86.

10. **Out of scope**: limited-quantity countdowns (hub-coordinated); auto-86 at zero stock
    (step 8); availability per channel, and its push to online channels, marketplaces and agent
    catalogs (G-06's full scope); hiding an item; the back office's view of what is sold out.

## Consequences

**Positive**
- An 86 needs no hub and no network: a register that can't reach the hub 86s for itself, and the
  store agrees once it hears.
- Items' availability never conflicts: concurrent changes resolve the same way on every device.
- Ringing a sold-out item is refused at the register, offline.

**Negative**
- One new schema, which every replica's kernel must know before any device writes it.
- An 86 made on an island register reaches the others only when it hears the hub again.
- A shell that polls every 2 s takes up to that, past the hub's delivery, to show an 86.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Availability in the location's profile, published by the cloud | An 86 is made at the register, mid-service, often offline: publishing waits for the cloud, and a profile is reference data, not a running state. |
| One stream for the whole location's availability | Every change would fold the location's whole history; a stream per item stays as short as its 86s. |
| The hub as the only writer, serializing 86s | An 86 must work offline, and needs no coordination: the latest by HLC is the merchant's intent either way. |

## References
- [ADR-0019](./0019-replication-and-deterministic-simulation.md),
  [ADR-0023](./0023-register-shell.md), [ADR-0026](./0026-local-hub.md),
  [ADR-0029](./0029-staff-roles-and-approvals.md); domain model §5.3, §17, §18; offline and sync
  §6.5; [feature catalog G-06](../product/feature-catalog.md)
