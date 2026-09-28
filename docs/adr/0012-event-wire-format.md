# ADR-0012: Event wire format: canonical CBOR, COSE_Sign1, and revocation by log position

- **Status:** Accepted (2026-09-28)
- **Date:** 2026-09-27

## Context

ADR-0002 made every commercial fact an immutable event in a signed, hash-chained per-device log.
Signatures and hashes cover bytes, so the exact bytes of an event must be fixed before the first
event is stored. A later change to the encoding would leave stored events unverifiable, and fiscal
regimes require records to stay verifiable for six to ten years.

The format must:
- give every event exactly **one encoding**, so its hash and signature are the same on every
  device, hub and cloud node, and in simulation;
- be **strict**, so no byte can change without the change being detected;
- be verifiable with **standard tools**, so auditors, fiscal authorities and merchants' own
  systems can check events without Keel's code;
- work with the keys that secure hardware holds (Secure Enclave, StrongBox, TPM), which support
  ECDSA on P-256, with a software fallback for devices without it;
- let a stolen or compromised device be **revoked** without trusting its clock.

## Decision

1. **Encoding: a strict subset of CBOR's core deterministic encoding** (RFC 8949 §4.2.1):
   - Only unsigned and negative integers, byte and text strings, arrays, maps, `false`, `true`
     and `null`. No floating point, tags, `undefined` or other simple values.
   - Shortest-form heads and definite lengths only.
   - Map keys sorted by their encoded bytes, with no duplicates.
   - Text is valid UTF-8, and values nest at most 32 levels deep.
   - Decoders reject everything else, so each value has exactly one accepted encoding.
2. **The event body is envelope format 1**: a map with small integer keys.

   | Key | Field | Encoding |
   |---|---|---|
   | 0 | format | 1 |
   | 1 | event id | 16 bytes: a UUIDv7 |
   | 2 | location | 16 bytes: a UUIDv7 |
   | 3 | stream kind | text: a lowercase identifier of up to 32 characters, such as `order` |
   | 4 | stream id | 16 bytes: a UUIDv7 |
   | 5 | schema name | text: dotted identifiers, up to 64 characters, such as `order.line_added` |
   | 6 | schema version | unsigned integer, 1 to 2³² − 1 |
   | 7 | origin device | 16 bytes: a UUIDv7 |
   | 8 | origin sequence | unsigned integer, from 1 |
   | 9 | hybrid logical clock | unsigned integer: 48 bits of milliseconds, 16-bit counter |
   | 10 | business date | text, `YYYY-MM-DD` |
   | 11 | actor | `[kind, id]`: team member 0, customer 1, integration 2, extension 3; or `[4, name]` for a Keel component |
   | 12 | approval (optional) | 16 bytes: the approving event's id |
   | 13 | causation (optional) | 16 bytes: the command or event that caused this one |
   | 14 | correlation (optional) | 16 bytes |
   | 15 | payload | byte string holding one canonical CBOR item |
   | 16 | previous hash | 32 bytes |

   - Optional fields are omitted when absent, never written as `null`.
   - Unknown keys, missing fields, wrong types and invalid values are all rejected.
   - Format 1 is closed. A new field needs format 2.
   - The payload is a byte string, so an envelope can be checked without knowing the payload's
     schema, and the payload's bytes are signed exactly as written.
   - **Location** is a new envelope field, not in the domain model's first draft. A device is
     enrolled at one location, and replicas reject its events for any other, so no device can
     inject events into another location's history. The field also makes partitioning by
     location cheap.
3. **Signatures are COSE_Sign1 messages** (RFC 9052), in one exact form:
   - untagged;
   - protected header exactly `{1: alg, 4: kid}`;
   - an empty unprotected header;
   - the encoded body as the attached payload;
   - the signature over `["Signature1", protected, h'', payload]`.

   Two algorithms are allowed:
   - **ES256** (−7, ECDSA on P-256 with SHA-256), for keys in secure hardware. Signatures must
     be low-S, so each has one accepted form.
   - **EdDSA** (−8, Ed25519), as the software fallback. Verification is strict: a canonical S and
     no small-order points. Weak public keys are rejected.

   The **key id** (`kid`) is the SHA-256 of the key's canonical COSE_Key (key type, curve and
   coordinates only). This follows the construction of RFC 9679 (COSE key thumbprints);
   conformance with that RFC hasn't been checked, because its text wasn't reachable when this was
   written.

   A signer verifies each signature it makes before returning it, so a faulty key store or a
   fault attack can't produce an event that doesn't verify.
4. **An event's hash is the SHA-256 of its encoded body**, not of the signed message. An event's
   identity is its content. ECDSA signatures from secure hardware are randomized, so hashing the
   signature would give one event many identities.
5. **The log rules** are checked by every replica:
   - Sequence numbers start at 1 and have no gaps.
   - Each event records the hash of the event before it; the first records 32 zero bytes.
   - The HLC increases strictly along a device's log, so a device can't date an event before its
     own previous event.

   A device appends in two steps: it signs the next event, stores it durably, and only then makes
   it the head of its log. An event is never sent before it is stored.
6. **Revocation is by position in the device's log, not by time.** A revocation says "events after
   sequence number *n* are not trusted, and the event at *n* has hash *h*".
   - A stolen device can set its clock back, so a cut-off time could be evaded.
   - The device can't change events that replicas already hold, and the hash pins the event at
     the cut. A history forged with the stolen key before the cut therefore conflicts with the
     genuine one, and is reported as soon as the two meet.
   - A device revoked more than once keeps its earliest cut, so every replica reaches the same
     result whatever order it learns of revocations in. Two revocations that disagree about the
     event at the same position are reported.
7. **Each device has one key for life.** The registry maps a device to one key and one location.
   A device that needs a new key (after a repair, or a reset of its secure hardware) or moves to
   another location is enrolled again as a new device, with a new log, and its old identity is
   revoked after its last event. Key rotation therefore never has to be reconciled with a log's
   history.
8. A **known-answer test** pins the format: the bytes and hash of a fixed event and its signed
   message. Python's `cbor2`, `hashlib` and `pycose` independently decoded, hashed and verified
   these bytes.

## Consequences

**Positive**
- Any CBOR and COSE library can decode and verify Keel events. `pycose` (backed by OpenSSL)
  verifies Keel's messages, and Keel verifies `pycose`'s.
- One encoding per event gives stable hashes everywhere. Strict decoding and single-form
  signatures leave no room for malleability.
- Hardware-backed P-256 keys sign events directly.
- Revocation can't be evaded by changing a clock.

**Negative**
- Strictness means a buggy producer's events are rejected rather than repaired. They are
  quarantined for review.
- Any new envelope field needs a new format number. Decoders will accept every format in use, and
  writers write the newest.
- A revocation also cuts off genuine events that the device recorded after its last sync. Those
  events are quarantined for review; the design for re-admitting them is future work.
- Replicas must receive each device's events in order. The version-vector sync protocol
  (ADR-0006) already delivers them that way.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| JSON with JCS canonicalization (RFC 8785) | Numbers are IEEE doubles; larger; canonicalization is subtle (number formatting, escapes) |
| Protocol Buffers | No canonical encoding guaranteed across implementations; unknown fields are kept silently |
| JWS / JOSE | JSON-based, with the same canonicalization problem for the payload, plus base64 overhead |
| A custom binary signature container | Auditors and authorities couldn't verify events with standard tools |
| Hashing the signed message | Randomized hardware signatures would give one event many hashes |
| Revocation by timestamp | Evaded by setting the device's clock back |

## References
- RFC 8949 (CBOR), RFC 9052 and RFC 9053 (COSE), RFC 9679 (COSE key thumbprints), RFC 8032
  (Ed25519), RFC 6979 (deterministic ECDSA)
- [ADR-0002](./0002-event-sourced-signed-event-log.md), [ADR-0006](./0006-own-sync-protocol.md)
- [domain-model.md §17](../architecture/domain-model.md#17-events),
  [offline-and-sync.md §3](../architecture/offline-and-sync.md#3-replication-protocol),
  [security.md](../architecture/security.md)
- Implementation: [`core/crates/keel-events`](../../core/crates/keel-events/)
