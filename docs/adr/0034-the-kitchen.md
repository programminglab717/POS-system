# ADR-0034: The kitchen: tickets per station, routed by the profile, shown on a KDS and printed when it isn't

- **Status:** Accepted
- **Date:** 2026-11-13

## Context

The MVP's sixth step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is the kitchen: routing to
stations (D-01), an expo view (D-02), bumping by touch and by bump bar, and recalling the last
bumped (D-03), modifiers and allergens a cook can't misread (D-06), ticket aging and the time each
station takes (D-07), and printers as the fallback when a screen doesn't show a ticket (D-11).

What is built shapes it:

- The order already **fires** lines: `order.lines_fired` sends pending lines to be prepared; a
  fired line can't be changed or taken off, only voided, and a void wins over a concurrent fire
  (ADR-0015; offline and sync §5.2). No intent fires yet: nothing reaches a kitchen.
- The domain model keeps fulfilment apart from ordering (§9): **kitchen tickets** are aggregates
  made when lines fire; routing rules decide which stations get what, and how an item splits into
  components; every station action is an event on the ticket, never a change to the order.
- Every device of a location holds every event of it, through the hub, within a second (ADR-0019,
  ADR-0026). A KDS is such a device: the same app, showing stations rather than a menu.
- Print jobs are effects in the store's outbox, queued, retried while nothing was sent, and
  unconfirmed once something may have been (ADR-0024). Each register prints to the printer set
  on it.
- Hardware routes are logical, with ordered fallbacks: `Grill → Grill's screen, else Grill's
  printer, else the expo printer`; staff are told of a reroute; kitchen reprints are marked
  (hardware §4.2, §5).

## Decision

1. **Stations and routes are in the location's profile**, a new format, 3, which adds key 13,
   the kitchen. Its roles, key 12, are optional in format 3, as if format 1 when absent. A
   profile with a kitchen is written as format 3; one without, byte for byte as before.

   | Record | Keys |
   |---|---|
   | kitchen | 1 stations (at least one), 2 routes, 3 default station (optional), 4 `[seconds until a ticket is late-ish, seconds until it is late]`, 5 fallback printer (optional) |
   | station | 1 identifier, 2 name, 3 expo (a bool), 4 screen (a bool: a KDS shows it), 5 printer (optional) |
   | route | 1 station, 2 items (a set), 3 modifiers (a set), 4 order modes (a set; absent for all) |
   | printer | 1 host, 2 port |

   Format 3 also lets the catalog give **allergens**: key 6 of an item and of a modifier, a set
   of codes from a fixed list (0 gluten, 1 crustaceans, 2 eggs, 3 fish, 4 peanuts, 5 soy, 6 milk,
   7 tree nuts, 8 celery, 9 mustard, 10 sesame, 11 sulphites, 12 lupin, 13 molluscs), absent when
   none, so a catalog without them hashes as before.

2. **Routing is a function of the profile**: a line goes to every station a route names for its
   item, in its order's mode, or to the default station when none does, if the kitchen has one:
   bottled water goes to no cook; a route that matches
   only one of its modifiers, at any depth, sends that modifier on its own, as a component, to
   its station ("Hash brown, for Breakfast sandwich"). Every expo station gets every line. So
   every device routes a fire alike, and a test asks the profile, not the kitchen.

3. **Firing at the register**: *Send* fires an order's pending lines; paying a check in cash
   fires what of it is still pending, in the payment's own write, so a counter order reaches the
   kitchen when it is paid, and another check's lines wait. A location without a kitchen fires
   nothing. A fired line shows as sent: it is voided, for a reason, as `line.remove`
   (ADR-0029), not taken off or changed; dropping an order with fired lines voids it, as
   `order.void`, its reason `dropped`.

4. **A kitchen ticket is an aggregate per station and fire**, stream kind `kitchen_ticket`, its
   identifier drawn by the device that fires, and written in the fire's own write. Its events,
   each v1:

   | Schema | Keys |
   |---|---|
   | `kitchen.ticket_sent` | 1 order, 2 station, 3 order mode, 4 items: each `{1 line, 2 name, 3 quantity, 4 modifiers, 5 notes (optional), 6 allergens (optional), 7 component of (optional: the line's name, for a component)}`, a modifier `{1 name, 2 prefix, 3 quantity, 4 depth}` |
   | `kitchen.ticket_shown` | no keys: a screen showed it |
   | `kitchen.ticket_bumped` | no keys: the station is done with it |
   | `kitchen.ticket_recalled` | no keys: it is back on the screen |
   | `kitchen.ticket_rerouted` | 1 to: 0 the station's printer, 1 the fallback printer, 2 nowhere |

   A ticket holds what to make as it was sent: a screen needs nothing else. What became of its
   lines since, a void above all, is read from the order. The latest bump or recall by canonical
   order stands; a ticket's first showing and its first reroute stand. Any device writes a
   ticket: the screen as the system's component `kds`, the register as its member, a reroute as
   the component `kitchen`, so a KDS needs no one signed in.

5. **Fallbacks print**: each ticket for a station with a printer and no screen prints at once.
   The device that sent a ticket for a screen looks at it again from its print loop: once 10 s
   have passed with no screen showing it, it prints on the station's printer, or on the kitchen's
   fallback printer, marked rerouted, and the device records the reroute. A ticket nowhere to
   print is rerouted nowhere. Every device shows the reroutes of the last ten minutes. A
   kitchen print job names the printer it goes to, and is handed over whether or not the
   register has a printer of its own; a receipt names none, and goes to the register's own. A
   line voided after it was printed prints a void slip where its ticket printed.

6. **The KDS** is the app on a device of the location, set to show some of its stations: their
   tickets not yet bumped, oldest first, each with its order's code, its mode, its age, and its
   lines, a voided one struck through as VOID; *No* and *Extra* modifiers, notes and allergens
   each in their own colour, legible without colour. A ticket ages through three colours at the
   kitchen's thresholds. A tap, or a bump bar's key, bumps the ticket selected; *Recall* brings
   back the last one bumped. An expo station shows, for each line, whether every station making
   it has bumped it. The screen shows each station's average time from sent to bumped today.

7. **The store projects kitchen tickets**: a `kitchen_tickets` table, one row per ticket (its
   order, station, when sent, shown and bumped, whether bumped, how it was rerouted, its business
   date), found by station and state, and by order. A projection: no new store schema.

8. **Built in slices**, each verified as it lands, then reviewed:
   1. *The domain and the store*: profile format 3, routing, the ticket aggregate and its five
      schemas, the projection.
   2. *The register and the kitchen*: firing, the tickets, voids and their slips, the KDS's
      queries and intents, the fallback jobs and the kitchen ticket's document, the bindings.
   3. *The shell*: the Android register's Send and voids, and the app as a KDS.

9. **Out of scope**: course holds and cook-time sequencing (D-04); all-day counts (D-05); one
   queue for other channels (D-08), pacing and promise times (D-09); dayparts and revenue
   centres in routing; parking and rushing a ticket; a KDS's own printer; store-wide order
   numbers (the order's receipt code names it); the buzzer; a fallback from one screen to
   another; reports of kitchen times beyond the screen's averages.

## Consequences

**Positive**
- A kitchen works with no network beyond the store: tickets reach every screen through the hub,
  and print where a screen doesn't show them, from the register that sent them.
- Kitchen actions never conflict with selling: they are on the ticket, not the order.
- A void reaches the kitchen however it was sent: the screen strikes it through, a printer gets
  a slip.

**Negative**
- Five new schemas and a profile format, which every replica's kernel must know before any
  device writes them, and before the cloud publishes a kitchen.
- A ticket's fallback runs on the device that sent it: one switched off in those 10 s leaves a
  ticket no screen showed unprinted, though every device shows it unshown.
- A register that can't hear the hub prints every ticket it sends, 10 s on, whether or not a
  screen it can't hear showed it.

## As built

Built in three slices on 2026-11-13, each verified as it landed, then reviewed: the kernel and the
Android shell each by a reviewer who hadn't built them. A ticket the register pushes is on a
screen through the hub with no time passing, and the screen's showing stops its reroute. What
the review found, and what was done:

- **A long note refused a payment, and jammed the print loop**: 500 characters beyond Latin-1
  passed a document's limit, and a station no screen shows, or a reroute, built one. A ticket's
  texts are clipped, its items capped, and a reroute that fails holds up no other job.
- **A reroute printed lines voided since**, with no VOID: it prints only what is still to make,
  and goes nowhere when nothing is.
- **Lines no station makes fired nothing**: a send with no tickets left them pending. They fire.
- **Any device could send an order**, and two sent its stations two tickets: only its owner
  does.
- **Kitchen jobs waited for the register's printer's word**, which a kitchen printer of
  another model may not give: they are confirmed by a clean close.
- **The cloud's editor refused deleting a routed item**: routes forget what an edit deletes.
- **On the register and the screen**: a press while a bump was read bumped again, refused; a
  second Recall recalled the ticket before; a publication that signed the cashier out lost the
  kitchen, and none read it again; a held key bumped ticket after ticket; reads piled up on the
  worker, and a failing one froze the screen silently; a sent line's notes could be edited,
  which the kernel refuses; alerts showed on the register alone; the instrumented test counted
  cards a grid hadn't drawn. Each is fixed, most with a model test; the fake till now fires on
  payment, voids a dropped order's lines, and refuses notes on a sent line, as the kernel does.

Gaps that remain:

- **A ticket's fallback runs on the device that sent it**: one switched off within 10 s leaves a
  ticket no screen showed unprinted, though every device shows it unshown.
- **Clock skew**: a ticket's age and its 10 s are by the sender's HLC, which a faster peer can
  push ahead: a reroute can come up to the HLC's drift late, and a screen show an age of 0.
- **Components**: a modifier chosen under two parents goes once; a nested one's quantity leaves
  out its parent's; a weighed line's takes the line's unit; placement (half and half) isn't on
  the ticket.
- **Kitchen printers are taken to be as wide as the register's**, or 80 mm, and no Android test
  sends a kitchen job to a printer's address.
- **Open tickets aren't limited to a day**: one never bumped stays on the screen.
- **A reroute that fails** holds up no other job, but no test makes one fail now that a ticket's
  texts are clipped: the guard is untested.
- **The Android register's instrumented test** waits for an emulator, as steps 3 to 5's do.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| The KDS reads the order and routes it itself, with no ticket | Bumps and recalls would be events on the order, which only its owner writes; and every screen would route afresh, from whatever profile it holds. |
| One ticket per order, its items' stations inside | A station's bump would be one item's state among many on a shared stream; a ticket per station is bumped whole, as a station works. |
| The hub prints the fallbacks | The hub isn't always there; the register that fired is, and it already prints. |
| Stations and screens in the roster | Which device shows which station changes on the floor, in seconds: it is the device's setting, as its printer is. What a station is, and its printer, is the location's. |

## References
- [ADR-0015](./0015-checks-and-payments.md), [ADR-0019](./0019-replication-and-deterministic-simulation.md),
  [ADR-0023](./0023-register-shell.md), [ADR-0024](./0024-receipts-and-printing.md),
  [ADR-0026](./0026-local-hub.md), [ADR-0029](./0029-staff-roles-and-approvals.md); domain model
  §9; offline and sync §5.2; hardware §4.2, §5; [feature catalog D-01…D-03, D-06, D-07,
  D-11](../product/feature-catalog.md)
