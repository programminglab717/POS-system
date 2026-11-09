# ADR-0024: Receipts and printing: KeelDoc v0, an ESC/POS renderer, a raw TCP driver, and print jobs in the outbox that are never sent twice by themselves

- **Status:** Proposed
- **Date:** 2026-11-09

## Context

Slice 2 of step 7 is receipts and printing ([ADR-0023](./0023-register-shell.md), decision 6):
"KeelDoc v0, an ESC/POS renderer, and raw TCP to port 9100, with unconfirmed jobs never printed
twice by themselves". ADR-0023 fixes its outline:

- **KeelDoc v0**, in a portable crate, `keel-doc`: lines of text with alignment, emphasis and
  double width or height; rows of a left and a right column, the left wrapping; separators;
  feeds; a cut; a drawer kick. A receipt is built from a closed check, its payments and the
  profile, and a reprint says "REPRINT".
- **An ESC/POS renderer** in the subset Epson, Star (in its ESC/POS emulation), Bixolon, Citizen
  and generic printers share: Windows-1252 (`ESC t 16`), the international set fixed to the US
  and Kanji mode cancelled; text outside the code page transliterated, then `?`; control
  characters stripped from every text, so that no name can kick a drawer; columns per line from
  a printer profile.
- **Effects:** paying cash enqueues the receipt and a drawer kick in the write that closes the
  check ([ADR-0017](./0017-projections-and-outbox.md)'s outbox).
- **The first driver**, raw TCP to port 9100, one connection per job: the printer's status
  (`DLE EOT`) before a job; its word that the job printed (`GS ( H`) or was processed (`GS r`)
  after it. A job retries by itself only if none of it was sent. Once any of it was, a job that
  fails, or is found running after a restart, is **unconfirmed**: never sent again by itself.
- **Tests:** bytes pinned per command, an ESC/POS interpreter that turns bytes back into lines,
  and a scripted fake printer on TCP.

What slice 1 built shapes the rest:

- The runtime ([ADR-0023](./0023-register-shell.md), decision 2) makes every intent one write,
  and answers with views whose amounts carry `Locale`'s text. A closed order's ticket is built
  from its check's snapshot. The runtime does no I/O but the store's: drivers sit beside it and
  take work from it, as `keel-sync`'s replicator does.
- The outbox ([ADR-0017](./0017-projections-and-outbox.md)) holds effects that commit with the
  events that cause them: pending, running, done or failed. An effect is started in a write
  before its executor acts on it; a running effect found after a restart is in doubt, and the
  store never restarts it by itself. An effect's payload is the executor's, up to 64 KiB.
- The location profile names the location and its address, but no printers.

Printers constrain the design:

- A printer can't always say what it printed. TCP's acknowledgement means bytes reached the
  printer's network interface, not paper; a printer that loses power mid-job, or jams, may
  have printed some of it, all of it, or none.
- Epson's `GS ( H` (function 48) asks for a process ID response, `37 22 d1 d2 d3 d4 00`, sent
  when the printing before it has completed; when several are pending, only the latest is sent.
  Not every model has it (Epson lists the TM-U220 and TM-U230 among those without). `GS r` is
  answered once the data before it is processed, which is weaker.
- `DLE EOT` is a real-time command: the printer acts on `10 04 n` wherever it finds it, even in
  the middle of other data, as it does `10 14` (a drawer pulse in real time). Text that reaches
  the printer must never contain those bytes.

## Decision

1. **Two crates and the runtime.**
   - `keel-doc`, portable (built for `wasm32`, like the kernel crates): the document model, its
     encoding, the layout of a document in a number of columns, and renderers to ESC/POS bytes
     and to plain text, with the decoding of ESC/POS status bytes. No I/O.
   - `keel-print`, a platform crate: the print driver, which runs one job against a printer over
     a `Transport` and says what happened, and the TCP transport for port 9100, on `std::net`.
     It knows nothing of the store.
   - `keel-runtime` builds receipts, enqueues jobs in its intents' writes, hands due jobs to a
     driver and records their outcomes. The shell (slice 3 onward) runs the driver on a thread
     of its own, so that a slow printer never holds up the runtime's thread.

2. **KeelDoc v0** is a list of blocks:
   - `Text(text, align, style)`: a paragraph, wrapped at spaces, a word longer than a line
     broken where it must be; aligned left, centred or right.
   - `Row(left, right, indent, style)`: the left text wrapped in the columns the right leaves
     it, indented by `indent` columns on every line, and the right text at the end of the first
     line; when the right text takes more than half the line, it goes on a line of its own after
     the left, aligned right.
   - `Rule`: a line of `-` across the paper. `Feed(n)`: `n` empty lines, 1 to 9. `Cut`.
     `DrawerKick`.
   - A style is bold, wide (double width, half the columns) and tall (double height).

   It is canonical CBOR, as the profile is: `{1: format, 2: [block, ...]}`, each block an array
   whose first element is its kind, decoded strictly, at most 1,000 blocks and 1,000 bytes of
   text in each. A document is laid out for a number of columns, from 24 to 64, into lines; both
   renderers print those lines, so what the plain text shows is what the paper shows.

3. **Text for the printer.** Every text is turned into Windows-1252 before it is laid out, a
   character to a byte, so that a column is a byte:
   - the characters the code page has map to their bytes;
   - spaces of other widths, no-break ones included, become a space; the minus sign and other
     dashes become `-`;
   - letters with marks the code page lacks become their base letters (`ł` to `l`, `ő` to `o`),
     from a table covering Latin Extended-A;
   - control and format characters are dropped: C0, DEL, C1, and the zero-width and
     direction-changing marks;
   - anything else becomes `?`.

   So no text yields a byte below `0x20` or `0x7F`, and the only control bytes on the wire are
   the renderer's own commands.

4. **The ESC/POS renderer.** A printer profile gives the columns per line (48 for 80 mm paper at
   203 dpi, 42 at 180 dpi, 32 for 58 mm), whether there is a cutter, and how the printer
   confirms a job (decision 6). A document renders as:
   - `ESC @` (initialize), `ESC t 16` (Windows-1252), `ESC R 0` (the US set), `FS .` (Kanji
     mode cancelled);
   - each line: `ESC a n` (alignment), `ESC E n` (bold), `GS ! n` (width and height), its bytes,
     `LF`; the style reset when it changes;
   - a feed as `LF`s; a cut as `GS V 66 0` (feed to the cutter and cut partly), or three feeds
     without a cutter; a drawer kick as `ESC p 0 25 250` (pin 2, 50 ms on, 500 ms off).

   Arguments never hold `0x10`, so no real-time command can appear but the driver's own.

5. **Print jobs are effects.** Kinds `print.receipt` and `print.drawer`, keyed by an identifier
   the runtime draws, their payload the document. Their states are the outbox's:
   - **queued**: pending;
   - **sending**: running, and handed to the driver by this runtime since it opened;
   - **unconfirmed**: running and not with the driver: one whose attempt ended with some of it
     perhaps sent, or found running when the store opened;
   - **printed**: done; **dismissed**: failed, ended by staff without a print they know of.

   Paying cash enqueues `print.drawer` when cash changed hands, then `print.receipt`, in the
   write that closes the order, caused by its closing event. An order that closes owing nothing
   prints its receipt and opens no drawer. A missing printer never blocks a sale: jobs wait.

6. **The driver.** One job, one connection:
   1. Ask the printer's status, `DLE EOT 1`, and if it is offline its cause, `DLE EOT 2`; then
      its paper, `DLE EOT 4`. A byte whose fixed bits (bits 1 and 4 set, 0 and 7 clear) are
      wrong isn't a status, and nothing is sent. A printer that is offline, open, out of paper
      or in error, or doesn't answer within its time, gets none of the job: **not sent**.
   2. Send the job. A write that fails before any byte of the job is accepted is **not sent**;
      one that fails after is **unconfirmed**.
   3. Wait for the printer's word, as its profile says: `GS ( H` with a process ID of four
      printable characters, answered when the job has printed; `GS r 1`, answered when it is
      processed; or nothing, for a printer that has neither, when a clean close of the
      connection is all there is. No word in time is **unconfirmed**.

   It answers printed (noting paper near its end), not sent, with the reason, or unconfirmed,
   with the reason.

7. **The runtime's part**, each step one write:
   - `next_print_job(now)`: the earliest due job, started, and rendered for the printer, unless
     one is with the driver already: one job at a time, in the order they were enqueued, so the
     drawer opens before its receipt prints.
   - `print_outcome(job, outcome, now)`: printed finishes it; not sent retries it, due after a
     backoff that doubles from 2 s to at most 60 s; unconfirmed leaves it running, no longer
     with the driver.
   - `resolve(job, printed | again | dismiss)`, for an unconfirmed job: staff say it printed,
     which finishes it; or ask for it again, which dismisses it and enqueues the same document
     marked "REPRINT"; or dismiss it.
   - `reprint(order)`: a closed order's receipt again, marked "REPRINT" above and below.
   - `print_jobs()`: the jobs queued, sending and unconfirmed, the view a shell shows.

   A runtime that crashes between starting a job and its first byte finds it unconfirmed: staff
   are asked about a job that never printed. That is the price of never printing twice.

8. **The receipt v0**, from the closed order's ticket and its payments, in the locale's
   language (`en` or `es`): the location's name, large and centred, and its address; the
   order's short number (the last six hex digits of its identifier), the local time of its close
   and the cashier; each line, with its quantity, its amount, and its modifiers indented under
   it with their prefixes and prices; the subtotal, each tax and the total; cash tendered, any
   cash rounding, and change; a thank-you; and a cut. Times are written `2026-10-06 08:15` in
   the location's zone until `Locale` knows dates. The receipt is built in the write that closes
   the order, so it shows the order as it closed.

9. **The printer's setup** is the device's, not the location's, until routes come with the
   cloud ([hardware.md §4.2](../architecture/hardware.md#42-discovery-and-configuration)): the
   shell gives the runtime a printer profile, and the driver an address. Without one, jobs queue.

10. **Verification:**
    - known answers: every command's bytes; each block laid out at its boundaries (a word as
      wide as the line, a right column too wide to share it, wide text at half the columns);
      transliteration and stripping of every class of character; status bytes of every kind;
      the demo café's receipt, pinned as text and as bytes; the document's encoding pinned
      against Python's `cbor2`;
    - property tests: documents round-trip their encoding; the ESC/POS bytes, read back by an
      interpreter written for the tests, give the lines a model of the layout gives, no line
      wider than its columns, and no real-time command and no control byte outside the
      renderer's commands, whatever the text; the runtime's jobs against a model of their states,
      through outcomes, restarts and staff's decisions, never sending a job again by itself once
      any of it may have been sent;
    - the driver against a scripted fake printer on TCP: printing, each status that refuses,
      garbled status, a reset mid-job, silence after the job, and a printer without `GS ( H`;
    - planted bugs, caught by the property tests where they can reach them.

## Consequences

**Positive**
- A receipt can't be lost to a crash, and never prints twice unless staff ask.
- Screens, receipts and their plain text show the same lines, laid out once.
- No text a merchant or a cashier types can reach the printer as a command.
- The driver is tested against a printer that misbehaves on cue, here and in CI, without one.

**Negative**
- Staff are asked about jobs the driver can't vouch for, a crash before the first byte
  included, and about every job on printers that confirm nothing but a clean close.
- One printer per device, its address set by the shell, until routes and fallbacks.
- Windows-1252 only: other scripts print as `?` until raster text.
- Times without the locale's format, and receipt labels in two languages.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Retry any job that failed | A printer that printed and then dropped the connection prints the receipt twice, and a kitchen ticket twice is a dish made twice. |
| A job's outcome as events | Replicated and kept forever for each receipt; the outbox already holds what the device needs. Events for drawer openings, which audits want, come with cash management. |
| Render at enqueue, store the bytes | Ties a job to one printer's columns and code page; a document renders for whichever printer takes it. |
| Build receipts at print time from the store | A reopened order would print as it is now, not as it closed. |
| Each platform's printing (Android's print framework, Star and Epson SDKs) | Different on every platform, and untestable on the host. Vendor SDKs come later as adapters for built-in printers. |
| Bytes on the wire as they come, without a status check | A printer out of paper takes the job and prints nothing. |

## References
- [ADR-0010](./0010-hardware-abstraction-and-document-model.md),
  [ADR-0017](./0017-projections-and-outbox.md), [ADR-0023](./0023-register-shell.md),
  [hardware.md §4, §5](../architecture/hardware.md#4-peripheral-service)
- Epson's ESC/POS reference: [`GS ( H` function 48](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/gs_lparen_ch_fn48.html),
  [`DLE EOT`](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/dle_eot.html),
  [commands in code order](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/commands.html).
