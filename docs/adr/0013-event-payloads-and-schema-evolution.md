# ADR-0013: Event payloads: integer-keyed canonical maps, strict versions, and total folds

- **Status:** Proposed
- **Date:** 2026-09-28

## Context

[ADR-0012](./0012-event-wire-format.md) fixed the event envelope. Its payload is a byte string
holding one canonical CBOR item, and each schema decides what that item is. The first domain
events, for orders, need four decisions that are as hard to change as the envelope, because
payloads are signed and kept for years:

- **the payload format**: how fields and values are written;
- **schema evolution**: how payloads change while a fleet runs mixed versions during rollouts
  ([offline-and-sync.md §11](../architecture/offline-and-sync.md#11-mixed-versions-and-upgrades));
- **undecodable events**: what a kernel does with a payload it can't decode;
- **events that don't fit**: what the fold does with an event that doesn't fit the aggregate's
  state, because its device checked it against a different, concurrent view.

The constraints:

- Every payload must have exactly one encoding, like the envelope, so that decoding and
  re-encoding an event gives back the bytes that were signed.
- Replicas must agree: the same events, folded in the same order, give the same state everywhere
  (offline-and-sync §4).
- A trusted device's log must never stall. Its events are chained, so if replicas refused one
  event, every later event from that device would be stuck behind it.
- No event is rejected or discarded at merge time
  ([ADR-0002](./0002-event-sourced-signed-event-log.md)). Events record things that already
  happened: a card was charged, a ticket printed, food fired.
- Size matters: a busy restaurant records about 30,000 events a day, and a stadium a million
  ([offline-and-sync.md §3.4](../architecture/offline-and-sync.md#34-volume-sanity-check)). So
  does what a person needs to read one.

## Decision

1. **A payload is a canonical CBOR map with small unsigned integer keys**, documented per schema.
   Values are encoded the same way in every schema:

   | Value | Encoding |
   |---|---|
   | Identifier | 16 bytes: a UUIDv7 |
   | Money | `[minor units, currency code]`, such as `[450, "USD"]` |
   | Quantity | `[millionths, unit code]`, such as `[1500000, "kg"]` |
   | Catalog version | 32 bytes |
   | Enumeration | an unsigned integer code, fixed for each enumeration |
   | Name | text: 1 to 200 characters, with no control characters |
   | Note | text: 1 to 500 characters, with no control characters except line feeds |
   | Reason code | text: a lowercase identifier of at most 32 bytes |
   | Count | an unsigned integer from 1: up to 255 for courses and modifier quantities, 65,535 for seats and guests |
   | Set of identifiers | an array of identifiers in strictly ascending byte order, not empty |

   - An absent optional field is omitted, never written as `null`. In an event that changes
     fields, an omitted field is unchanged and `null` clears it.
   - **Decoding is strict.** Unknown keys, missing fields, wrong types, values out of range and
     invalid combinations are rejected. Each event therefore has exactly one payload encoding.
   - Each schema also has rules across fields. For orders: every price in a payload is in one
     currency; quantities are positive and prices are zero or more; a change changes at least one
     field.
   - Schema names are `stream.event`, such as `order.line_added`, and versions count from 1.
   - Text can't contain control characters, which could otherwise drive receipt printers and
     kitchen displays.
2. **The schema registry** is the list of schemas each aggregate decodes (`DomainEvent::SCHEMAS`
   in `keel-domain`). Each listed schema has a pinned example payload, which tests check byte for
   byte and which Python's `cbor2` decoded independently. A release can't remove a schema or change
   what one decodes; a change is a new version.
3. **Every payload change is a new version, and writers wait until every kernel knows it.**
   - Strict decoding means an older kernel rejects even a new optional field, so there is no such
     thing as a compatible change within a version. This replaces the domain model's earlier
     "backward-compatible within a major version".
   - A kernel decodes every version it knows, and upcasts older versions to the current form as it
     decodes them.
   - A writer emits a version only when every kernel at its location knows it. Sync negotiates this
     (`keel-sync`), so in normal operation a newer version never meets an older kernel. Rollouts
     ship decoders first, and enable the new writer later.
4. **Undecodable events are kept, and the fold skips them.** An event whose signature and log
   position are valid ([ADR-0012](./0012-event-wire-format.md)) stays in the log, and is stored and
   relayed like any other, even when its payload can't be decoded. The fold skips it and the
   aggregate records why:
   - an unknown schema, from a newer kernel: the aggregate says it **needs update**;
   - a malformed payload, which only a faulty writer produces;
   - an event of another stream.
5. **Folds are total. Most events that don't fit still apply; a few don't apply at all.** Either
   way the event stays in the log and the aggregate records a **conflict** for a person to
   review.
   - Most events that don't fit apply with their natural effect, and are flagged: a line added to
     a closed order, a fired line removed, a line changed after it was fired.
   - Events that would break the aggregate's invariants don't apply. For orders: a line priced in
     another currency, a quantity in another unit than the line's, an event from before the
     order's creation, or one recorded at another location than the order's. Every price in an
     order can therefore always be added up.
   - Conflicts are derived from the events by the fold, so every replica computes the same ones,
     and no event records them. The domain model's `ConflictFlagged` event is dropped. A person's
     resolution of a conflict will be an event.
   - The order's rules are in [domain-model.md §6.5](../architecture/domain-model.md#65-as-built-order-events-v1).
6. **Commands are checked, and every event they produce must decode.** A command is checked against
   the device's current view, and becomes exactly one event. Before the kernel records the event,
   it encodes the payload and decodes it again under its own schema. A kernel therefore never
   writes a payload that it, or a peer at the same version, would reject. Commands that wouldn't
   change anything, such as a change to the value a field already has, are refused, so the log
   records only real changes.

## Consequences

**Positive**
- Every payload has one encoding. Integer keys keep payloads small: a line with one modifier is
  182 bytes, and a removal 19.
- Any CBOR library decodes payloads, and the key tables in the code say what each key means.
- A device's log never stalls on a payload. A kernel that meets a newer schema shows "needs
  update", and replicas never fold the same event differently.
- An order's totals are always computable, because nothing in another currency or unit gets in.

**Negative**
- Integer keys need the schema's key table to read. An export tool that labels fields will be
  needed for auditors and support.
- Every change, even a new optional field, needs a new version, an upcaster and a pinned example.
- A location adopts a new version only when its slowest kernel knows it.
- Events that don't apply need a person to resolve them. They should be rare: they need devices
  acting concurrently on stale views, or a faulty writer.
- Returns and negative lines don't fit version 1's positive quantities and non-negative prices.
  They will get their own events.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Field names as map keys | The pinned payloads grow 1.6 times overall (a line with one modifier from 182 to 320 bytes), and the names become part of the signed format anyway |
| Tolerant decoding that ignores unknown fields | Kernels at different versions would fold the same event differently, and a payload would have many encodings |
| Refusing undecodable events at the log | The device's chain would stall, and events recording real-world facts would be lost |
| Applying every event whatever its content | An order could mix currencies or units, and its total couldn't be computed |
| Recording conflicts as events | Conflicts follow from the events; an event recording them would duplicate the fold and could disagree with it |
| Protocol Buffers or Avro for payloads | Protocol Buffers doesn't guarantee one encoding, and Avro can't be decoded without the writer's schema |

## References
- [ADR-0002](./0002-event-sourced-signed-event-log.md), [ADR-0006](./0006-own-sync-protocol.md),
  [ADR-0012](./0012-event-wire-format.md)
- [domain-model.md §6](../architecture/domain-model.md#6-orders--the-universal-transaction) and
  [§17](../architecture/domain-model.md#17-events),
  [offline-and-sync.md §4, §5.2 and §11](../architecture/offline-and-sync.md)
- Implementation: [`core/crates/keel-domain`](../../core/crates/keel-domain/); the order key
  tables are in `src/order/events.rs`.
