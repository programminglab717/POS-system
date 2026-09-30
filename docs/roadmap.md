# Keel — Roadmap

> Status: **Draft v1** · Last updated: 2026-09-27
>
> Phases are gated by **exit criteria**, not dates. Feature IDs refer to the
> [feature catalog](./product/feature-catalog.md), where tiers M, 1, 2 and 3 map to the phases below.

## 1. Strategy: win a wedge with the hardest problem solved first

The architecture's hardest and most differentiating part is **never-down, multi-device, money-correct
operation**: the kernel, sync, pricing, ledger and payments. It has to exist before any vertical
feature matters, so Keel builds it first and proves it in simulation before real merchants rely on it.

The market wedge is **counter-service food & beverage and specialty retail in the US**:
- They are high-volume and highly dissatisfied (lock-in, outages, online-order chaos, tipping backlash).
- The workflows are simpler than full service, so the pilot stays focused.
- They exercise the full core: catalog, modifiers, KDS, payments, cash, inventory, offline.
- The US has no fiscalization, so compliance scope stays manageable while the core matures.

Keel expands from there:
1. full service and multi-location;
2. services, omnichannel and the developer platform;
3. enterprise, franchise, regulated verticals and global markets.

## 2. Phase 0 — Foundations (internal)

**Goal:** the kernel and sync are correct under failure, provably, before any merchant uses them.

| Track | Scope |
|---|---|
| Kernel | `keel-types` (Money, Decimal, UUIDv7, HLC, BusinessDate), `keel-events` (envelope, CBOR, chaining, signing), `keel-domain` (order, check, payment, kitchen ticket, stock, drawer session), `keel-pricing` (price resolution, modifiers, basic discounts, US sales tax at store location, allocation, rounding, trace), `keel-store` (SQLite events + projections + outbox), `keel-policy` (roles, limits, approvals) |
| Sync | `keel-sync`: version-vector replication, hub sequencing, ownership leases, hub election with standby and fencing, island mode, device↔cloud |
| Simulation | `keel-sim`: deterministic multi-device, hub and cloud simulator with fault injection and the invariant suite ([offline-and-sync §12](./architecture/offline-and-sync.md#12-verification-deterministic-simulation-testing)) |
| Cloud (minimal cell) | Sync ingest, event store, projections, tenant/org/location model, device enrollment and CA, catalog publishing |
| Android shell | Register mode skeleton on the kernel via UniFFI; performance harness on a 2 GB reference device |
| Tooling | Monorepo, CI across targets, golden-basket harness, schema registry and compatibility checks |

**Exit criteria**
- The simulator runs **1M+ seeds nightly** with zero invariant violations. The scenarios are
  partitions, crashes, clock skew, hub failover, split brain and unknown payment outcomes.
- The golden-basket suite matches byte for byte on Android, JVM, Linux and WASM.
- Tap-to-UI p99 < 50 ms on the reference 2 GB Android device with a 100k-SKU catalog.

## 3. MVP / Pilot — counter service, one location, US

**Goal:** 10–20 pilot merchants (cafés, quick-service, specialty shops) run their whole business on
Keel, and **never lose a sale to Keel**.

**Scope** (tier **M**):
- **Resilience**: offline parity, hub role (embedded or appliance), local auth, degraded-service
  detection, the "backed up" indicator, update windows (A-01…A-05, A-07, A-13…A-15).
- **Selling**: the instant register, search/scan/quick keys, split tender, discounts, customer attach,
  receipts (print, digital, none), basic returns, training mode, EN/ES (B-01…B-10, B-12, B-16).
- **Catalog**: deep modifiers, structured allergens, global 86 in-store, bulk import/export,
  **AI menu import** (G-02, G-03, G-06, G-07, G-08).
- **Kitchen**: KDS routing, expo, bump bars, legibility, ticket aging, printer and KDS fallback
  (D-01…D-03, D-06, D-07, D-11).
- **Payments**: one connector (Stripe Terminal or Adyen, chosen by pilot needs), semi-integrated
  terminals, tips, split tender, refunds (I-01, I-02, I-05, I-07, I-08).
- **Pricing ethics**: no guest fees, ethical tipping defaults, cash rounding (H-01, H-09, H-10, H-12).
- **Cash**: drawer sessions, blind close with thresholds (N-01, N-02).
- **Inventory**: the movement ledger (K-01).
- **Reports**: dashboard, local X/Z reports (O-01, O-02).
- **Hardware**: Android all-in-ones plus standard printers, drawers and scanners; network doctor;
  printing done right; spare swap (S-01, S-02, S-04…S-06, S-08, S-09).
- **Commercial promises**: self-serve setup, month-to-month terms, price-change notice, sunset
  guarantee, full export, self-serve cancellation (V-01, V-05…V-09).
- **Trust**: tamper-evident records, data boundaries (R-03, U-17). US tax at store location (R-01).
- **Also in MVP**:
  - price check and item info, notes and tags (B-13, B-14);
  - 1D barcodes with GS1 parsing (E-03);
  - roles and ABAC permissions (M-01);
  - AI catalog import (U-01, as G-08);
  - basic consent (J-02);
  - the localization framework (X-01);
  - café and quick-service basics (W-01, W-02).
- **Back office (web)**: catalog, staff and roles, settings, reports, export.

**Exit criteria**
- **Zero Keel-attributable lost sales** across the pilot, including at least one real ISP outage per
  site or a scheduled cut-the-cable drill.
- Median time from signup to first live sale < 60 min (self-serve).
- A new staff member rings a standard order after ≤ 10 minutes of in-app training (the 90th
  percentile of test users).
- Pilot merchant NPS ≥ 50. Every support contact is categorized, and the top 5 causes have fixes
  planned.

## 4. v1 — General availability: full service and multi-location (US and Canada)

**Scope** (tier **1**):
- **Full service**: floor plans, seats and courses, timed and synchronized fire, best-in-class
  splits, tabs with pre-auth and incremental auth, handhelds, pay-at-table, service charges vs tips,
  remote approvals (C-01…C-08, C-12…C-15).
- **Kitchen**: cook-time sequencing, all-day counts, **one queue for all channels**,
  **capacity-aware pacing**, complexity caps, ready boards, waste (D-04, D-05, D-08…D-10, D-12, D-16).
- **Channels**: first-party online ordering, **marketplace integrations without tablets** (DoorDash,
  Uber Eats, Grubhub), order-ahead (L-01, L-04, L-10). Online-channel protection during outages (A-11).
- **Payments**:
  - **≥ 3 connectors**: Adyen, Stripe Terminal, Datacap-class.
  - The offline risk envelope and decline recovery; SoftPOS; Android payment terminals.
  - Gift cards with offline escrow; house accounts.
  - Automatic reconciliation and fee transparency.
  - (I-03, I-04, I-06, I-09, I-15, I-17…I-19, A-06, A-08, A-12)
- **Retail depth**: rich variants, large catalogs, serials, UoM, kits, labels, layaway, special
  orders, quotes, mobile POS (E-01, E-02, E-04, E-06…E-11, E-15, E-20).
- **Catalog and pricing**:
  - Hierarchical catalog, channel menus, drafts, scheduled publishing and rollback.
  - Promotions vocabulary and stacking model; explainable pricing.
  - Surcharge and fee compliance engine; price integrity.
  - (G-01, G-04, G-05, G-09, G-11, G-12, H-02…H-05, H-08, H-11)
- **Customers**: unified profile, consent, loyalty (J-01…J-03).
- **Inventory**: counts, POs, transfers, receiving, **recipes and theoretical usage**, cost methods
  (K-02…K-07, K-10).
- **Workforce**: time clock, biometrics off by default, **tip engine**, tip transparency
  (M-02, M-03, M-04, M-07, M-08).
- **Finance**: double-entry ledger, QuickBooks and Xero sync, marketplace payout reconciliation,
  sales tax reports (P-01, P-02, P-04, P-05).
- **Reports**: semantic layer, report builder, product mix, loss prevention (O-03…O-06).
- **Platform**: public REST API with parity, webhooks and event log, deprecation policy
  (T-01, T-02, T-07, T-09, T-11).
- **AI**: staff help, verifiable business Q&A, anomaly alerts, autonomy dial, approval-gated
  actions, forecasting, quote times (U-02…U-07).
- **Apple**: iPad and iPhone apps (SwiftUI), and Tap to Pay on iPhone.
- **Hub appliance**: LTE failover and UPS (S-03, A-10).
- **Operations**: incident transparency, 24/7 P1 human support, competitor importers, in-app
  diagnostics (A-16, V-02, V-03, V-10).
- **Compliance**: age verification, accessibility (WCAG 2.2 AA), privacy tooling, junk-fee
  disclosures (R-04, R-06…R-08). **Canada**: GST/HST/PST/QST, bilingual receipts, Interac via
  connectors.
- **Also in v1**:
  - ransomware-resilient design (A-18);
  - the 10-minute onboarding standard, pre-orders, staff accessibility (B-11, B-15, B-17);
  - 2D GS1 barcodes (E-03);
  - shift suspend/resume, safe and bank deposits, the cash-acceptance guard (N-03, N-04, N-06);
  - location groups and the fleet console (Q-01, Q-06);
  - zero-touch enrollment and kiosk lockdown (S-07);
  - multi-currency (X-03);
  - vertical packs for full service, bars, specialty retail, food trucks and apparel basics
    (W-03, W-04, W-05, W-09, W-19).

**Exit criteria**
- A certified multi-device, full-service stack survives a **72-hour offline soak** (10 terminals, 4 KDS
  screens, 6 handhelds) with zero lost documents.
- The "can-sell" SLO is ≥ 99.999% of trading minutes across the fleet, measured at the edge.
- SOC 2 Type II report issued. Accessibility audit passed.
- Measured P1 support median time-to-human < 2 min.

## 5. v2 — Platform, services, omnichannel, first European markets

**Scope** (tier **2**):
- **Developer platform**: Store Hub local API, **WASM Functions**, POS UI extensions, automations,
  custom objects, marketplace, **MCP server** (T-03…T-06, T-08, T-10).
- **Services**: resources, appointments with segments and processing gaps, deposits and no-show fees
  (with SCA/MIT support), classes, memberships and entitlements, packages, work orders (F-01…F-06,
  F-08, F-09).
- **Omnichannel retail**: mixed-fulfillment cart, BOPIS, BORIS, ship-from-store, clienteling,
  e-commerce connectors, ATP (E-12…E-14, L-05, L-06). **Lot and expiry** (E-05).
- **Restaurant extras**: QR order and pay (hybrid), native reservations and waitlist with network
  integrations, catering, multi-brand kitchens (C-09…C-11, C-16, C-18). Kiosk (L-03).
- **Growth**: marketing automation with holdouts, segments, reviews, the first-party growth engine
  (J-04…J-09). **Agent-ready catalog** (L-07). **AI phone ordering** (L-09).
- **AI agents**: ordering, scheduling, loss prevention, menu engineering, marketing, invoice capture,
  honest automation metrics, sponsorship labels (U-08…U-13, U-15, U-16, K-09).
- **Workforce suite**: scheduling, labor compliance packs, commissions, payroll integrations, staff
  app (M-05, M-06, M-09…M-11).
- **Inventory**: native replenishment and forecasting, integrations (K-08, K-12).
- **Payments**:
  - multi-processor routing and processor failover;
  - card-on-file portability;
  - disputes;
  - **Keel Payments (optional)**;
  - regional APMs;
  - multi-currency cash.
  - (A-09, I-10, I-11, I-13, I-14, I-16, I-20, I-21, I-23)
- **Cloud**: multi-region cells with paired-region standby (A-17). Warehouse sync (O-08).
  Governance with bounds, SSO/SCIM (Q-02, Q-05).
- **Global**: **UK** first (no fiscalization), then **Germany, France and Spain** fiscal packs (TSE,
  NF525/ISCA, VeriFactu), with the rule-pack pipeline in production (R-02). Data residency (X-06).
  RTL groundwork (X-02). Regional payment and fiscal packs, and the market launch playbook
  (X-04, X-05, X-07).
- **Also in v2**:
  - first-party delivery handoff and production/prep (D-13, D-15);
  - nutrition and labels (G-10);
  - promotion simulation, engine performance, and pricing Functions (H-06, H-07, H-13);
  - labor law via rule packs (R-09);
  - proactive support (V-04);
  - vertical packs: QSR full (kiosk, boards), salons and spas, fitness, bakeries and catering,
    liquor, B2B counter, repair, apparel full (W-02, W-06, W-07, W-08, W-12, W-16, W-18, W-19).

**Exit criteria**
- At least 50 third-party apps or Functions in production.
- First fiscal certifications passed.
- Regulatory changes are shipped as rule packs within 1 hour of sign-off.

## 6. v3 — Enterprise, franchise, regulated verticals, global

**Scope** (tier **3**):
- **Enterprise and franchise**: the franchise model with royalties and inter-entity settlement,
  config-as-code, enterprise cells, benchmarks (Q-03, Q-04, Q-07, O-07, P-03).
- **Verticals**: grocery (scales, EBT/SNAP/WIC, self-checkout, ESL, RFID), convenience and fuel,
  venues and stadiums, hotels (PMS), cannabis, rentals, furniture, campus, museums and attractions
  (W-10, W-11, W-13, W-14, W-15, W-17, W-20, W-21, W-22, E-16…E-19, F-07, F-10, I-24, R-05, R-10).
- **Also in v3**:
  - hands-free KDS voice commands (D-14);
  - least-cost routing (I-12);
  - commissary and central kitchen (K-11);
  - branded app (L-02);
  - certification tracking (M-12);
  - cash recyclers (N-05).
- **Windows and Linux lanes** (Compose Multiplatform desktop).
- **Drive-thru** with partner voice AI (C-17, U-14).
- **Agentic checkout** (L-08). **Capital offers** via partners (I-22).
- **Global expansion** by market attractiveness and partner readiness: Italy, Portugal, Austria,
  Poland, Nordics; Brazil (NFC-e, Pix); Mexico (CFDI); India (GST, UPI); GCC (ZATCA, UAE Peppol);
  Southeast Asia; Australia and New Zealand.

## 7. Cross-cutting tracks (run continuously)

| Track | Continuous work |
|---|---|
| **User research** | ≥ 30 operator and staff interviews across 5 verticals before v1 scope lock, then ongoing. Shadow shifts. Replaces the persona hypotheses the research couldn't verify (see [research §7](./research/README.md#7-what-still-needs-verification)). |
| **Reliability** | Simulation seeds grow with every bug found. Quarterly game days: cloud gone for 24 h, hub dies at peak, processor down, fleet-wide reconnect. |
| **Security and compliance** | Threat model per feature; pentests; bug bounty; SOC 2 → ISO 27001/27701; PCI for Keel Payments; CRA reporting readiness |
| **Hardware certification** | Certified device list grows by demand. Hardware-in-the-loop lab. Hub appliance ODM program. |
| **Partnerships** | Payment connectors, delivery marketplaces, accounting and payroll, fiscal providers, ESL, RFID and PMS vendors |
| **Regulatory watch** | Monthly country backlog reviews. Decision-date alerts (e.g. the US interchange settlement hearing on 16 Nov 2026). |

## 8. First engineering milestones (the next build steps)

1. **Monorepo scaffold**: Rust workspace (`core/crates/`), Android project (`apps/android/`), web
   workspace (`apps/web/`), `schemas/`, CI with Rust targets and Android emulator jobs. *Status: the
   Rust workspace and its CI are done; the Android, web and schema directories arrive with their
   first code.*
2. **`keel-types`**: Money with ISO 4217 exponents, Decimal, Quantity, UUIDv7, HLC, BusinessDate,
   rounding rules, with property tests. *Status: done
   ([`core/crates/keel-types`](../core/crates/keel-types/)); `Locale` follows with the first UI.*
3. **`keel-events`**: the envelope, deterministic CBOR, hash chaining, signing (a software key first,
   hardware keys behind a trait), and a schema registry with compatibility checks. *Status: done
   ([`core/crates/keel-events`](../core/crates/keel-events/)), with the wire format in
   [ADR-0012](./adr/0012-event-wire-format.md). The schema registry was built with the first domain
   events, in `keel-domain` ([ADR-0013](./adr/0013-event-payloads-and-schema-evolution.md)).*
4. **`keel-domain` (order/check/payment) + `keel-pricing` (v0)**: lines, modifiers, checks and
   allocations, discounts, a US sales-tax rule set, the calculation trace, and a golden-basket suite
   of 100 baskets growing to 1,000. *Status: done, in four slices, each reviewed: payload
   codecs, the schema registry and the order's lines; pricing v0
   ([ADR-0014](./adr/0014-pricing-engine-v0.md)), with 140 golden baskets; checks and splits;
   and payments and closing ([ADR-0015](./adr/0015-checks-and-payments.md)). See
   [progress](./progress.md).*
5. **`keel-store`**: the SQLite event store, projections and outbox in a single transaction, with
   crash-safety tests. *Status: done, in three slices, each reviewed: the event log store
   ([ADR-0016](./adr/0016-device-store.md)); projections and the outbox
   ([ADR-0017](./adr/0017-projections-and-outbox.md)); and encryption at rest and integrity
   checks ([ADR-0018](./adr/0018-encryption-at-rest-and-integrity-checks.md)). See
   [progress](./progress.md).*
6. **`keel-sim` + `keel-sync` (v0)**: two devices, one hub and one cloud in simulation. Replication,
   hub sequencing, ownership leases, failover. The invariant suite runs in CI. *Status: in
   progress: design. See [progress](./progress.md).*
7. **Android register shell**: ring an order with modifiers, pay cash, print a receipt (ESC/POS over
   TCP), running on a reference all-in-one against a local hub.
8. **Cloud cell v0**: sync ingest, catalog publishing, a minimal back office (catalog editor, reports).

## 9. Decision gates

| Gate | When | Decision |
|---|---|---|
| G1 | End of Phase 0 | Continue with the own sync protocol, or re-evaluate. Criterion: simulation results and performance budgets met. |
| G2 | Before MVP | First payment connector (Stripe Terminal vs Adyen), based on pilot merchant needs and offline capabilities |
| G3 | Before v1 | Hub appliance: build a reference design with an ODM, or certify off-the-shelf units |
| G4 | Before v2 | Keel Payments partner selection; first EU market order (Germany / France / Spain) |
| G5 | Before v3 | Compose Multiplatform desktop vs Tauri for Windows/Linux lanes; which regulated verticals come first |
