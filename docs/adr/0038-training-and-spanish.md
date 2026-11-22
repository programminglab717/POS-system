# ADR-0038: Training mode in a store of its own, and each member's and station's language

- **Status:** Accepted
- **Date:** 2026-11-22

## Context

The MVP's tenth and last step ([roadmap §8](../roadmap.md#the-mvps-build-steps)) is training mode
and Spanish (B-10, B-12, X-01): staff learning on a real register with no payment, fiscal or
stock effect, and the register, kitchen and receipts in English or Spanish. What is built shapes
it:

- **Every device's log is real**: its events are signed, synced through the hub to the cloud,
  folded into every report, and exported (ADR-0019, ADR-0028, ADR-0037). An event marked
  "training" in the real log would be one every reader must remember to skip, forever.
- **A register is a runtime over one store**, opened by the shell with a path (ADR-0025), and
  starting sync is the shell's call.
- **Every print job is rendered in one place**, when the shell takes it (ADR-0024): a receipt, a
  kitchen ticket, a report, a drawer's kick.
- **Receipts print in English or Spanish already**, by the register's locale (ADR-0024); the
  kernel formats money and quantities for `en-US` and `es-US` (X-01's formats), but the kitchen
  ticket's words are English, and the Android register's words are all English.
- **The profile is versioned by format**, the earliest that holds it (ADR-0029, ADR-0036).

## Decision

1. **Training is a register over a store of its own**, which the shell opens apart from the
   real one, with an identity of its own, and deletes to start again. Its kernel knows it is
   training, and trains only over a store that holds nothing:
   - it refuses to start sync, so nothing it records reaches the hub, the cloud, a report or
     an export: no fiscal, payment or stock effect, by construction;
   - every document it prints carries a banner above and below, "TRAINING - NOT A RECEIPT" in
     its language ("DO NOT MAKE" on a kitchen ticket), on its own printer, and no drawer is
     kicked;
   - card payments go to the simulated terminal, the shell's: never a real one.
   Everything else is the real register: the same profile, the same intents, the same rules.

2. **Languages are Keel's locales**, `en-US` and `es-US`. A team member may have one, which the
   register's own words follow while they are signed in; a kitchen station may have one, which
   its tickets are printed in. With none, the register's locale. Receipts are in the register's
   locale, as before. Profile format 6 holds them: key 5 of a member, key 6 of a station, the
   locale's tag; an older kernel refuses such a profile whole rather than misread it.

3. **The kitchen ticket's words** (the order's mode, "Order", "ALLERGENS", "and N more") are
   in English and Spanish, as the receipt's are.

4. **The register's words** are in a table of English and Spanish in the shell: the screens ask
   it for each, by its English, in the signed-in member's language, and a test checks every
   one the screens ask for has its Spanish. What the kernel says in refusing an intent stays in
   English (X-01's message catalog comes with the eight languages of v1).

5. **The back office** sets each member's language with their role, and each station's with
   its printer.

6. **Built in slices**, each verified as it lands, then reviewed:
   1. *The kernel*: profile format 6, the member's language through the bindings, the kitchen
      ticket's words, training in the runtime and the bindings.
   2. *The back office*: the languages in the staff and settings pages.
   3. *The Android register*: its words in English and Spanish, and training mode.

7. **Out of scope**: guided training exercises; a receipt in the guest's language, chosen at
   checkout; languages beyond English and Spanish; the kernel's messages translated.

## As built

Built in three slices on 2026-11-22, each verified as it landed, then reviewed: the kernel and
back office, and the Android register, each by a reviewer who hadn't built them. A test reads
the app's screens and checks every word they show goes through the table and has its Spanish.
What the reviews found, and what was done:

- **A training register's events were the real device's**, signed with its key, in its log's
  sequence, so if they ever synced its log would fork; and nothing but memory said a store was
  training. The shell gives a training register an identity of its own, which no roster has,
  and the kernel trains only over a store that holds nothing.
- **Its kitchen tickets printed in the live kitchen**, marked "not a receipt": they print on
  the register's own printer, marked "do not make".
- **The lower banner printed after the cut**, on the next slip: both banners go above the feed
  and cut that end a document. A drawer's kick printed a slip of banners: it is done, unprinted.
- **Joining the network** started before the kernel refused a training register: the bindings
  refuse first.
- **The training register rang from the profile provisioning wrote**, without the team and
  prices published since: it opens with the profile the real register rings from now.
- **What a trainee saved changed the real register**: the printer and the kitchen screen's
  stations are kept apart in training. Starting twice opened two training registers, and one's
  loops ran on for good; training outlived its screen; and a failed start said nothing. Each is
  fixed: one at a time, ended with the screen, closed when it ends, and its failure said.
- **Many of the screens' words weren't asked for in the member's language**: every literal the
  screens show is, which the words test now checks; and the Spanish a cashier might misread is
  plainer ("práctica", not "entrenamiento"; a safe drop is a "depósito").

Gaps that remain:

- **Sentences with numbers in them** ("Pay $4.50", "Check 2 (paid)") and what the kernel says in
  refusing stay in English, as does a dynamic text such as an alert.
- **The kitchen display** shows English: no member signs in to it. Its printed tickets are in
  the station's language.
- **A training banner** is in the register's language, a kitchen ticket's too.
- **Format 6 has no pin against an independent encoder**, as the earlier formats have; its
  tests are its round trip, its format byte, and the profile properties, which now draw
  languages.
- **Training is ended by the trainee**: nothing ends it after a while.

## Consequences

**Positive**
- Nothing a trainee does can reach the real log: there is nothing to filter, ever.
- A Spanish-speaking cook reads Spanish tickets while the front counter rings in English.

**Negative**
- Training starts from an empty store: no open orders or history of the real day to practise on.
- A refusal's reason is in English on a Spanish register until the kernel's messages are
  translated.
- A new profile format, which every replica's kernel must know before a language is set.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Training events in the real log, marked | Every report, projection, export and future reader must skip them, and one that forgets counts them. |
| A training location in the cloud | Needs provisioning and a network; a register should train alone. |
| Android's string resources for the register's words | The words are the shared register model's too (its messages), which isn't Android's; one table serves both. |

## References
- [ADR-0019](./0019-replication-and-deterministic-simulation.md), [ADR-0024](./0024-receipts-and-printing.md),
  [ADR-0025](./0025-bindings.md), [ADR-0029](./0029-staff-roles-and-approvals.md),
  [ADR-0034](./0034-the-kitchen.md), [ADR-0037](./0037-the-back-office.md); the feature catalog's
  B-10, B-12 and X-01.
