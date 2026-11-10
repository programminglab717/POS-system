# ADR-0028: The cloud cell v0: the cloud as each location's durable replica, catalogs published as chunked events, and a back office over the cell's API

- **Status:** Proposed
- **Date:** 2026-11-10

## Context

Step 8 of Phase 0 is the cloud cell v0: "sync ingest, catalog publishing, a minimal back office
(catalog editor, reports)" ([roadmap §8](../roadmap.md#8-first-engineering-milestones-the-next-build-steps)).
ADR-0007 fixes the cell's shape: a Rust modular monolith (Axum, Tokio, sqlx) linking the kernel
crates, PostgreSQL, NATS JetStream and object storage, serving a subset of tenants. ADR-0004 makes
the back office a React single-page app on the public API.

What is built shapes the step:

- `keel-sync` already gives the cloud its role (ADR-0020, decision 6). A replica may name a peer
  its **durable** peer; the hub and its standby name the cloud. That peer's `have` is the
  durable-ack watermark, which `durable` frames relay to every device, so a device knows what is
  backed up. The simulator runs a cloud node that way. Nothing outside it does yet: no roster
  names a cloud, and `Runtime::start_sync` names no durable peer.
- A location's network is a roster (ADR-0026): its devices and their keys, its hub's one address,
  and certificates from the location's CA. A device dials the roster's hub; only the device that
  listens there may host (ADR-0027). The hub is behind the store's router, so the cloud can't
  dial it: the hub must dial the cloud.
- A location's catalog is its profile (ADR-0023): canonical CBOR, checked whole, provisioned as a
  file, a restart to change. Class E reference data (offline-and-sync §6.5) is to be authored in
  the back office, published as immutable, content-addressed versions, and switched to atomically,
  within 60 s of publishing for an online device.
- An event is at most 256 KiB (`keel-events`), a frame 1 MiB. The demo café's profile is 2.3 KB;
  one with 100,000 items is 8.1 MB (ADR-0027's harness).

## Decision

1. **The cloud is a device of each location's roster.** It has an identifier, a signing key and a
   certificate from the location's CA, as any device does, and never hosts the hub. Roster v1
   (format 2) names it beside the hub's address: the cloud's device, its host and port, and the
   name its certificate carries. A format-1 roster still decodes, and names no cloud.
   - A device that may host the hub dials the cloud too, and names it its durable peer, as the
     simulator's hub and standby do. The others learn the watermark from the hub's `durable`
     frames.
   - `keel-net` dials a named peer: the roster's hub (as now), or its cloud, whose certificate must
     name the cloud's device and carry the cloud's name.
   - `keel-provision` provisions the cloud's kit with the location's.
   - The roster is still trusted as provisioned. Until the control plane's Device CA (decision 7),
     the cloud's kit, like every device's, comes from the provisioning tool.

2. **The durable replica is the location's own store.** The cell holds a `keel-store` for each
   location, opened as the cloud's device, with no one signed in, as the hub daemon opens its
   store. It replicates with the location's hub over the location's network, and so verifies and
   folds every event as each device does. The cell's totals are the kernel's, by construction
   (ADR-0007, "kernel reuse"). It listens for each location on a port of its own, behind the
   location's CA.
   - **Amends ADR-0007 for v0:** PostgreSQL comes with the cell's first cross-location work: the
     tenant directory, roll-ups, and the hot event window once a store's history is pruned.
     Until then, a location's store is its durable copy and its reports' source. NATS and object
     storage come with integrations and archives.

3. **A catalog is published as events in the cloud's own log.** The cell authors a location's
   profile; publishing appends it to the cloud's log, where replication carries it to every
   device, signed by the cloud's key, which the roster names:
   - `profile.chunk` v1, `{1: version, 2: index, 3: bytes}`: the profile's encoding, cut into
     pieces of at most 192 KiB, each an event;
   - `profile.published` v1, `{1: version, 2: size, 3: chunks}`: written after its chunks, it
     names a version by the SHA-256 of the profile's encoding, its size and how many chunks it
     has.
   - Each publication is a stream of its own, of kind `profile`, its identifier drawn as any
     stream's.
   - A device takes a publication only from the roster's cloud, only once it holds every chunk,
     and only if the chunks join to the size and hash it names and decode to a profile of its
     location that passes every check. It then switches to it atomically, between two intents:
     no intent ever sees half a profile. A later publication by HLC replaces an earlier one; a
     device restarted opens with the latest it took. An order begun under one version is priced
     under the one current when each line is rung, as every line records its catalog version
     already (ADR-0014).
   - The provisioned profile is the first version, until the cloud publishes another.

4. **The back office's API**, an Axum service in the cell's process (the `keel-cell` binary,
   library `keel-cloud`), JSON over HTTPS:
   - the locations the cell serves, and each one's replication: who is connected, how far the
     durable copy holds each device's log;
   - the catalog: read the current profile as JSON, write a new one, checked whole by the kernel,
     and publish it (decision 3);
   - reports from the durable copy's projections: sales by business day, by item, and payments by
     tender, each total computed by the kernel.
   - Until the control plane's identity (OIDC, decision 7), the API takes one operator's bearer
     token from the cell's configuration, compared in constant time.

5. **The back office**, `apps/web/backoffice`: a React single-page app in TypeScript, built with Vite, on the
   cell's API: a location's catalog editor (items, variants and prices, modifier groups, menu
   pages) and its reports. It is served by the cell itself.

6. **Built in slices**, each verified as it lands, then reviewed:
   1. *Sync ingest*: roster v1, `keel-net`'s named peers, the cloud's kit, the durable peer named,
      and `keel-cell`'s replication for each location. A sale rung on a register reaches the hub
      and the cloud, and the watermark comes back to the register.
   2. *Catalog publishing*: the two schemas, the runtime taking publications, and the cell
      publishing. A catalog published reaches a register within the 60 s budget, and the next
      sale is priced by it.
   3. *The back office's API*.
   4. *The back office*, with end-to-end tests in a browser.

7. **Out of scope for v0**, left for the control plane: identity and SSO; the tenant directory and
   cell router; the Device CA, enrollment and signed rosters; scheduled and delta publications;
   local overrides (86s); cross-location roll-ups; the warm standby; and the multi-tenant isolation
   PostgreSQL's row-level security will give.

## As built

The details settled in building it:

- **The hub's one network dials the cloud.** `Net::hub` listens for the location's devices and
  dials the roster's cloud beside, on the one network, so the hub's loop is unchanged; the cell
  listens for each location with `Net::cell`, which admits the devices that may host the hub
  alone. `Net::cloud` dials the cloud alone. The Android hub dials it with no change of its own.
- **The cloud's device is named `Cloud`** in the roster, and its kit is provisioned apart from
  the location's devices' (`--cloud`, `--cloud-name`; `Provisioned::cloud`).
- **`SyncStatus` gains `backed_up` and `head`**, and the bindings' `SyncState` them and
  `profile_version`, so a register can show what is backed up and read its menu again.
- **A publication is a whole profile**, and its version the SHA-256 of the profile's encoding,
  `ProfileVersion`. A publication holds 1 byte to 64 MiB, in exactly as many chunks as its size
  takes, each full but the last. A runtime looks for publications only in the part of the
  cloud's log it hasn't looked at, when replication starts and after each frame; with one
  publisher, the cloud's log order is its HLC order. A cashier the new team doesn't have is
  signed out.
- **The API speaks HTTP; TLS is ended in front of the cell**, as a cloud's load balancer ends
  it, rather than in the cell, which would take a second server stack for v0. Its routes:
  `GET /api/locations`, `GET …/{id}/replication`, `GET` and `PUT …/{id}/catalog`,
  `POST …/{id}/catalog/check`, and `GET …/{id}/reports/sales?from=&to=`. JSON errors, `401`
  without the token, `404` for a location the cell doesn't serve, `409` for a catalog read
  before another was published (its `base`), `422` for one the kernel refuses, with why.
- **The catalog's JSON is its catalog and menu**: items, variants and prices in minor units,
  modifier groups and their modifiers, and pages; the tax categories the location's rules know
  come with it to choose from. Written, it replaces the profile's catalog and menu, keeping its
  settings, rules and team, and the kernel checks the profile whole. Checking and publishing are
  one job on the location's thread: nothing is published between the two.
- **Reports add up each closed order's ticket**, priced by the kernel as every device prices it:
  sales by business day and by item (by the name its lines show), and captured payments by
  tender.
- **The back office is `apps/web/backoffice`**, as the repository's layout has it: React 19,
  TypeScript, Vite, served by the cell from its built files (`keel-cell --web`). The operator's
  token is kept for the browser's session alone.
- **The end-to-end tests run against `keel-cloud`'s demo**: the cell, a hub and a register in
  one process over real TLS on loopback, the register's sales reaching the cell through the hub,
  and the register writing the version it rings from for the tests to read.

## Consequences

**Positive**
- The cloud speaks the protocol the simulator verified, with the same kernel, store and checks:
  ingest adds no new trust path, and its totals can't disagree with a device's.
- A catalog reaches devices by the replication every device already does, offline included: a
  device that was off when it was published takes it when it reconnects.
- Every piece is testable here: the cell, the hub and a register as runtimes on the host, and the
  back office in Chromium.

**Negative**
- A store per location doesn't scale to a cell's thousands of locations as PostgreSQL would; v0
  serves a pilot's handful.
- A full profile per publication: a 100,000-item catalog is 8 MB of events each time, until
  deltas.
- A port per location, until the cell's listener tells locations apart.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| PostgreSQL as the durable copy from v0 | A second implementation of verification and folding, where the store already has both; it comes with cross-location work. |
| Publishing over a channel of its own (HTTP downloads) | A second path for devices to trust, keep durable and catch up on; events already have all three. |
| The cloud dialing the hub | Hubs sit behind stores' routers. |
| One event holding the whole profile | 256 KiB is too small for a large catalog. |

## References
- [ADR-0004](./0004-client-ui-stack.md), [ADR-0007](./0007-cell-based-cloud-modular-monolith.md),
  [ADR-0014](./0014-pricing-engine-v0.md), [ADR-0020](./0020-hub-sequencing.md),
  [ADR-0023](./0023-register-shell.md), [ADR-0026](./0026-local-hub.md),
  [ADR-0027](./0027-android-app.md); [offline-and-sync §6.5](../architecture/offline-and-sync.md#65-class-e--reference-data-cloud-authored)
