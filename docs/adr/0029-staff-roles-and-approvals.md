# ADR-0029: Staff, roles and approvals: PINs and roles in the profile, permissions with limits, and approvals as events

- **Status:** Accepted (2026-11-11) after review, with the details settled in building and
  reviewing it, under "As built"
- **Date:** 2026-11-11

## Context

The MVP's first build step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is M-01, roles and
ABAC permissions, which every later step leans on: discounts, price overrides, refunds and cash
variances are each allowed up to a limit, and beyond it a manager approves
([domain model §12](../architecture/domain-model.md#12-workforce),
[security §2.2 and §6](../architecture/security.md)).

What is built shapes it:

- A location's team is part of its profile (ADR-0023): members with an identifier and a name,
  who sign in with no PIN. The profile is canonical CBOR, checked whole, and published by the
  cloud (ADR-0028), so the back office can author staff and roles as it authors the catalog.
- Every event records its actor, the member signed in. The envelope has an `approval` field,
  the event that approved the action (ADR-0012), unused so far.
- Intents are one write each (ADR-0023), refused whole when a rule says no.

## Decision

1. **Profile format 2 adds roles, and each member's role and PIN.** Key 12, roles: each
   `{1: identifier, 2: name, 3: permissions}`, at least one, their identifiers distinct. A
   member is `{1: identifier, 2: name, 3: role, 4: PIN}`, its role one of the profile's. A
   format-1 profile still decodes, and means what it meant: no roles, every member may do
   everything and signs in with no PIN. A profile with roles is written in format 2, one
   without in format 1, byte for byte as before.

2. **A PIN is 4 to 8 digits, kept as its Argon2id hash**: version 0x13, 19 MiB of memory, 2
   passes, 1 lane, 32 bytes, over the PIN's digits with a 16-byte salt drawn for it, written
   `{1: salt, 2: hash}`. Whoever sets a PIN (the cell, the provisioning tool) draws the salt; the
   kernel hashes deterministically, with RustCrypto's `argon2`, and compares in constant time.
   Other parameters would be another format.

3. **A permission is an action with its limits**: `{1: action, 2: most amount, 3: most percent}`,
   the amount in minor units of the location's currency and the percentage in basis points, each
   absent when unlimited. The actions of version 1:

   | Code | Action | Limits |
   |---|---|---|
   | 0 | `order.void`: abandon an order with lines on it | |
   | 1 | `line.remove` | |
   | 2 | `discount.apply` | amount, percent |
   | 3 | `price.override` | amount |
   | 4 | `refund.issue` | amount |
   | 5 | `drawer.no_sale` | |
   | 6 | `drawer.variance` | amount |
   | 7 | `drawer.paid_out` | amount |
   | 8 | `receipt.reprint` | |
   | 9 | `item.availability` | |

   A member may act if their role lists the action and the request is within its limits.
   Otherwise the action needs an approval by another member, whose role lists it within its
   limits: whoever asks never approves their own request. The kernel decides, on the device,
   offline as online.

4. **An approval is an event**, `approval.granted` v1, `{1: approver, 2: action, 3: amount,
   4: percent, 5: subject}`, in a stream of its own of kind `approval`, its actor the member who
   asked. It is written in the same write as the events of the action it approves, which name it
   in their envelope's `approval`.

5. **The runtime** (`keel-runtime`):
   - `sign_in(member, pin)` checks the PIN; `approve(member, pin)` checks an approver's, and
     holds their approval for the next intent that needs one, for 60 s.
   - Five wrong PINs in a row on a device lock PIN entry for 30 s, then twice as long after each
     further five, up to 15 minutes, by the runtime's clock.
   - An intent that needs an approval it doesn't have is refused, `ApprovalRequired`, naming the
     action, so the shell can ask a manager and try again.
   - What needs a permission now: abandoning an order with lines, removing a line, and
     reprinting a receipt. Each later step gates its own actions.

6. **The shells**: the bindings take a PIN to sign in and to approve, and report
   `ApprovalRequired`; the Android register signs in with a PIN pad and asks for a manager's PIN
   when an intent needs one; the back office edits staff, roles and PINs, published as profiles.

7. **Built in slices**, each verified as it lands, then reviewed:
   1. *The profile and the policy* (`keel-domain`): format 2, PINs, permissions, the policy's
      decisions, and `approval.granted`.
   2. *The runtime and the bindings*: PIN sign-in and lockout, approvals, the gated intents.
   3. *The shells*: the Android register's PIN pad and approval sheet, and the back office's
      staff and roles.

8. **Out of scope**: badges and biometrics; a pepper held in secure hardware (security §2.1);
   approvals pushed to a manager's phone; per-location and revenue-center grants; time-of-day
   and own-records-only constraints; sign-in and sign-out as audit events; and a lockout that
   survives a restart of the app.

## As built

The details settled in building it:

- **A PIN's lockout counts every wrong PIN on the device**, whoever's it was meant to be, through
  sign-in and approval alike: every five lock PIN entry, 30 s doubling to 15 minutes. Only 15
  minutes with no wrong PIN start the count again.
- **An approval is for the request asked.** An intent refused for want of one remembers what was
  asked: who, the action with its amount or percentage, and what it was on. `approve` checks the
  approver's PIN and that they may approve that request, and holds the approval for that request
  alone, for 60 s; `approvers()` names who may. A write that fails leaves it waiting for the same
  request; a write that succeeds spends it.
- **What is gated**: an order that has ever had a line, dropped (`order.void`), even once its
  lines are off; a line taken off, or fewer of it (`line.remove`); and a receipt printed again
  (`receipt.reprint`), asked only once there is a receipt to print.
- **A member whose role or PIN changes** in a profile the device takes is signed out, as one
  the new team doesn't have is.
- **The shells**: the bindings' `approvers()`; the Android register's approval sheet lists only
  who may approve, and tries the refused intent again, never the approval, so no PIN is kept; its
  PIN pad's digits stay out of the state Android saves. The back office shows a member with no
  role as such, gives the first role to everyone, and holds back publishing while a limit isn't
  an amount; a new PIN's digits are left out of the cell's `Debug`.

Found in review, each fixed with a test or a planted bug: any right PIN started the count of
wrong ones again, so a cashier could guess at a manager's PIN between their own sign-ins; an
approval covered whatever gated intent came next, so one given for a reprint that failed covered
another order's void; an order emptied line by line, which a cashier may do, was dropped with no
approval; fewer of a line needed none; a member's sign-in outlived a change to their PIN; the
Android register replayed an approver's PIN when an intent was refused again, kept PIN digits in
saved state, and listed every member as an approver; and the back office's role and limit fields
could show what wasn't so. Left as gaps: `resolve(Again)` prints an unconfirmed receipt again
without the gate, as staff decide what an unconfirmed job did; a print job doesn't name the
approval that allowed it; hashing a PIN holds the register's lock for as long as Argon2 takes,
some 100 to 400 ms on a slow device, and a device without the memory aborts; a kernel older than
this one refuses a format-2 profile, and keeps the format-1 one it has, so a fleet updates its
apps before it publishes roles; and the cell hashes every new PIN of a request on the location's
thread.

## Consequences

**Positive**
- Every later step gates its actions with one call, and the gate works offline.
- An approval is in the log beside what it approved, signed by the device: who asked, who
  approved, and for how much.
- Staff and roles are published as the catalog is, with no second path to devices.

**Negative**
- Every device holds every member's PIN hash: a 4-digit PIN falls to an offline search of its
  hash, Argon2id or not. The pepper (out of scope) is what would stop that.
- A lockout held in memory resets when the app restarts.
- A change of staff is a whole profile published, until deltas.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Staff and roles as their own events, apart from the profile | A second path for reference data; the profile is published, checked and taken atomically already. |
| bcrypt or PBKDF2 for PINs | Not memory-hard, as security §2.2 asks. |
| An approval as a field of the approved event | One approval may cover several events of one write; an event of its own is audited and counted alone. |
| Approval parameters on every intent | Each intent's signature would grow; holding the approval for the next intent keeps the intents as they are. |

## References
- [ADR-0012](./0012-event-wire-format.md), [ADR-0023](./0023-register-shell.md),
  [ADR-0028](./0028-cloud-cell-v0.md); [domain model §12](../architecture/domain-model.md#12-workforce);
  [security §2.2, §6](../architecture/security.md); [feature catalog M-01](../product/feature-catalog.md)
