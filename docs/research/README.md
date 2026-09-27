# Keel — Research Synthesis

> Status: **v1** · Compiled 2026-09-27 · Six research tracks → one set of design conclusions

## 1. Method and evidence quality

Six parallel research tracks covered the market:

| # | Track | Dossier |
|---|---|---|
| 01 | Restaurant and hospitality POS (Toast, Square, Clover, Lightspeed, TouchBistro, SpotOn, Shift4, Simphony, Aloha, …) | [01-restaurant-pos.md](./01-restaurant-pos.md) |
| 02 | Retail POS (Shopify, Square, Lightspeed R/X, Clover, Dynamics 365 Commerce, Odoo, Xstore, grocery, convenience) | [02-retail-pos.md](./02-retail-pos.md) |
| 03 | Payments, fees, PCI, fiscalization and e-invoicing (25 countries), tax, labor, accessibility, privacy | [03-payments-compliance.md](./03-payments-compliance.md) |
| 04 | Technical architecture: how incumbents are built, outages, sync technology, edge, hardware, client stacks, cloud, security, AI and agentic protocols | [04-technical-architecture.md](./04-technical-architecture.md) |
| 05 | Trends, AI, guest-facing experience, pain points by persona | [05-trends-ai-painpoints.md](./05-trends-ai-painpoints.md) |
| 06 | 25 verticals and 12 global markets; cross-vertical primitives | [06-verticals-global.md](./06-verticals-global.md) |

**Evidence limitations (read before relying on details):**
- The research environment blocked full-page fetching on most domains.
- A shared web-search budget was exhausted during the run.
- As a result, dossiers rely on search-result excerpts and primary documents reachable through
  code-hosting mirrors. Claims that couldn't be re-verified are tagged in each dossier (**[unverified]**,
  **†**, **‡** or **[K]**), and each dossier ends with a verification backlog.
- **Architecture decisions in this repository depend only on well-corroborated patterns.** Specific
  prices, dates and regulatory deadlines must be re-verified against primary sources before they are
  used commercially or legally.

## 2. The findings that shaped Keel

1. **"Offline mode" is a payments feature, not an operations feature.** At major vendors, going
   offline typically stops staff login, loyalty, gift cards, device-to-device sync, close-out and
   reports. Card risk moves entirely to the merchant, with windows of 24 h to 7 days. The only
   architectures that degrade gracefully (Simphony CAPS, TouchBistro's local server, Aloha) use a
   local coordinator, with single-point-of-failure and IT costs. → **Keel: local-first with an
   electable hub and peer fallback** ([offline-and-sync](../architecture/offline-and-sync.md)).
2. **The dangerous outage is "internet up, cloud broken".** Square's 2023 DNS incident (about 14 h)
   left devices online-but-failing, so sellers unplugged cables to force offline mode. Shopify's
   Cyber Monday 2025 login bug blocked POS sign-in. The October 2025 cloud-region outage produced
   "complete system failures" at restaurants. The 2023 ransomware attack on one NCR data center took
   gift cards and back office down for days. → **Circuit breakers, local authentication, no
   single-region dependency.**
3. **Lock-in is the business model, and it's the #1 source of owner pain.** Evidence includes:
   - mandatory processing, with penalties for going outside (a reported ~$400/month, or 0.6–2% of
     sales);
   - non-cancellable leases and multi-year contracts with termination fees or liquidated damages
     (one disputed $45k fee);
   - rate changes on 30 days' notice, and surprise fees, including one billed to guests and reversed
     after backlash.

   → **Processor-agnostic, month-to-month, no guest fees, full export, sunset guarantees.**
4. **Merchants get stranded by corporate events.** Examples: a POS frozen after acquisition and
   merchants converted; up to 200k merchants force-migrated to another platform; a native inventory
   app retired (Stocky, August 2026); hardware sunsets and "legacy device" fees; three incompatible
   product lines under one brand. → **Contractual sunset guarantees and API/hardware lifecycle
   commitments.**
5. **Channel chaos is now a kitchen-capacity problem.** Marketplace "tablet hell", menu drift, and
   86s that don't propagate. Starbucks had to build order sequencing (a 4-minute in-store target) and
   cap mobile order size. → **One queue, capacity-aware pacing, a global 86 in ≤ 10 s.**
6. **Depth vs complexity is a false choice today.** SMB tools cap variants (3 option axes; 250
   variations) and lack lot and expiry control. Enterprise tools have the best promotion engine
   (Dynamics 365's exclusive / best-price / compound concurrency modes) and cash office, but need
   integrators and cap offline data at 10 GB. → **Enterprise-grade primitives with SMB-grade UX,
   explainable pricing, a kernel that runs everything locally.**
7. **Compliance churn outpaces vendors, and sometimes reverses.**
   - France reportedly removed, then restored, self-attestation for cash software. The tracks
     conflict here; see §7.
   - Spain delayed VeriFactu to 2027.
   - The UAE extended its provider deadline.
   - Croatia changed its signature hash.
   - US SNAP eligibility now varies by state and date.
   - The US penny ended with no federal rounding law.
   - Tip reporting changed (qualified-tips deduction).
   - The EU accessibility obligations started in June 2025.

   → **Compliance as signed, effective-dated rule packs; pluggable fiscal adapters; an own immutable
   journal.**
8. **AI is table stakes, but trust is scarce.**
   - All major vendors shipped "ask your business" assistants in 2025–26. Approval-gated agents
     (Square Managerbot) are the frontier.
   - Voice AI succeeds only with humans in the loop. McDonald's ended its IBM drive-thru test, Taco
     Bell reconsidered, and the SEC acted on a vendor's automation claims.
   - Only 26% of operators actually use AI tools.

   → **Verifiable answers, an autonomy dial, approval-gated actions, honest automation metrics.**
9. **Guests punish "efficiency" that shifts work or cost onto them.** 27% tip less when shown
   pre-filled tip screens, 90% prefer printed menus to QR-only, retailers rolled back forced
   self-checkout, and surge pricing triggered a boycott. Kiosks succeed because they give guests
   control. → **Ethical defaults.**
10. **Specialists win verticals on a few deep workflows; real businesses mix verticals.** Fifteen
    generic primitives cover 25 verticals: bookable resources, entitlements, a value ledger, tabs,
    custody jobs, a policy engine, tender eligibility, fiscal documents, and so on. → **Compose
    verticals from primitives; never fork.**
11. **No off-the-shelf sync engine fits POS, and the proven in-store pattern is a local authority.**
    - Zero rejects offline writes.
    - ElectricSQL and PowerSync sync device↔cloud only.
    - Ditto (LAN mesh) is proprietary.
    - MongoDB's Device Sync reached end-of-life in September 2025.
    - Oracle Simphony's on-site service arbitrates **check ownership**, and Aloha's store server kept
      restaurants trading through NCR's 2023 ransomware attack.

    → **An own, hub-sequenced event-log protocol with check ownership, hot-standby failover and
    island mode** ([ADR-0006](../adr/0006-own-sync-protocol.md)).
12. **Configuration changes cause most big outages**: CrowdStrike (July 2024), a cloud DNS
    automation race (October 2025), a CDN configuration change (October 2025), a bot-management
    feature file (November 2025), and a payments platform's firewall/DNS change (2023). →
    **Configuration, rules and content ship through the same staged, validated, rollback-able
    pipeline as code, with last-known-good fallback.**
13. **Platform direction: a Rust core with native shells.** Reliability-focused local-first products
    converge on a Rust core. The flagship React Native POS reportedly moved to native Swift/Kotlin in
    September 2026. → **Rust kernel plus Kotlin/Compose (Android first) and SwiftUI**
    ([ADR-0003](../adr/0003-rust-kernel-everywhere.md), [ADR-0004](../adr/0004-client-ui-stack.md)).

## 3. Flaw → fix matrix

| # | Industry flaw | Evidence (examples) | Keel's fix | Where |
|---|---|---|---|---|
| 1 | Offline stops loyalty, gift cards, login, sync, close-out | Toast offline mode; Lightspeed X "Sell screen only"; D365 gift cards need HQ | Full offline parity; hub + peer sync; local auth; stored-value escrow | [sync](../architecture/offline-and-sync.md), A-01…A-12 |
| 2 | Offline card risk is opaque and entirely the merchant's | Square 72 h expiry; Toast 24 h; Clover 7 days; Lightspeed R offline payments not linked to sales | Risk envelope with BIN rules, exposure meter, recovery workflow; offline sales as full sales | [payments §7](../architecture/payments.md#7-offline-payments) |
| 3 | Online-but-broken outages aren't detected | Square 2023 | Circuit breakers on every dependency; 1.5 s budget for class D | [sync §6.6](../architecture/offline-and-sync.md) |
| 4 | Single points of failure: a cloud region, a data center, a hub device | AWS Oct 2025; NCR 2023; Toast hub rules | Cells with paired-region standby; store independence; elected hub | [architecture §4.3](../architecture/README.md) |
| 5 | Processor lock-in and penalty fees | Lightspeed, Shopify, Toast, Square, Clover | Connector SPI; ≥ 3 US connectors at GA; no processor-conditional pricing; card-vault export | [payments](../architecture/payments.md), I-01 |
| 6 | Contracts, termination fees, liquidated damages, lease traps | Toast, Shift4, SpotOn, Clover | Month-to-month, $0 early-termination fee, 90-day price notice, cancellable-by-return leases | V-05, V-06 |
| 7 | Forced migrations and sunsets | Revel → Shift4 Dine; Payeezy → Clover; Stocky; MPOS | ≥ 12-month sunset notice, no forced product switch, ≥ 24-month API support, ≥ 5-year hardware | V-07, T-11, S-02 |
| 8 | Data lock-in | Balances and liabilities rarely exportable; card tokens trapped | One-click full export (round-trip tested); warehouse sync; card-on-file portability | V-08, O-08, I-11 |
| 9 | Tablet hell and menu drift | Every restaurant vendor | Direct marketplace integrations; one catalog with channel overrides; global 86 | L-04, G-06 |
| 10 | Digital orders overwhelm kitchens | Starbucks mobile order flood | Capacity-aware pacing and promise times; channel complexity caps | D-08…D-10 |
| 11 | Catalog modeling ceilings | 3 axes / 2,048 variants; 250 variations; no lot or expiry | ≥ 5 axes, ≥ 10k variants, serial and lot policies, UoM graph, 500k SKUs on device | E-01…E-06 |
| 12 | Opaque promotions; online and offline price divergence | D365 config complexity; no explanation at counter | One deterministic engine everywhere; concurrency modes; "why / why not" explanations; simulation | H-01…H-07 |
| 13 | Config takes hours to propagate | Up to 24 h in one enterprise suite | ≤ 60 s propagation; scheduled activation offline | G-05 |
| 14 | Duplicate transaction IDs after offline | D365 warning | UUIDv7 everywhere; idempotent ingestion; per-device chains | [domain §4](../architecture/domain-model.md) |
| 15 | Network setup fragility | 10 open ports; subnet rules; client isolation | One port; cloud-assisted discovery; network doctor | S-06 |
| 16 | Support fails at peak; billing disputes | TouchBistro, SpotOn, Shift4, Clover reviews | 24/7 P1 human support < 2 min; billing SLAs; self-serve cancellation | V-03, V-09 |
| 17 | Compliance handled ad hoc | Tips vs service charges; surcharging; fiscal devices; SNAP waivers | Rule packs; fiscal orchestrator; gratuity classification; surcharge policy engine | [compliance](../architecture/compliance.md) |
| 18 | AI over-promises | Presto (SEC); McDonald's/IBM | Honest metrics, human takeover, approval-gated actions | [ai](../architecture/ai.md) |
| 19 | Guest-hostile defaults | Tip screens, QR-only menus, receipt spam, guest fees | Ethical defaults (tips off at counters, QR optional, receipts ≠ consent, no guest fees) | H-09…H-11, B-08 |
| 20 | Fund holds without explanation | Payfac holds and reserves | Explainable holds with appeal (Keel Payments) | I-21 |
| 21 | Vertical silos force multiple systems | Grooming + retail + boarding; museum + café + shop | Primitive-based vertical packs in one location | [domain](../architecture/domain-model.md), W-* |
| 22 | Franchisees last to get features; franchisor control is all or nothing | Beta programs excluding franchises | Franchise model with governed resources and bounds; every feature available to franchisees | Q-02, Q-03 |

## 4. Best-in-class features Keel combines

| Source | Best at | How Keel adopts or surpasses it |
|---|---|---|
| **Square** | Minutes-to-first-sale onboarding; published pricing; one ecosystem | Self-serve setup under 60 min with AI menu import; AI included in the base plan |
| **Toast** | Restaurant depth: handhelds, KDS, modifiers, restaurant-specific AI | Same depth + peer-synced offline + processor choice |
| **Shopify POS** | One catalog, customer and order across online and in-store; POS UI extensions; Functions; agentic storefronts | The same unification plus **offline-capable** Functions and extensions, and an agent-ready catalog |
| **Lightspeed** | Inventory and purchasing depth; serials and work orders; B2B | E-*, K-*, custody jobs |
| **Dynamics 365 Commerce** | Promotion concurrency model; cash office; mixed-fulfillment customer orders; serial capture policy | Adopted as primitives (H-04, N-*, E-12, E-04), plus explanations and SMB-grade UX |
| **Odoo** | Unified loyalty, coupon, gift card and eWallet model; cash-difference threshold; UUID-keyed orders | J-03, N-02, IDs everywhere |
| **Oracle Simphony / NCR Aloha / TouchBistro** | Local-server resilience; hierarchical enterprise configuration | Local-first *without* a mandatory server; hierarchical config with bounds |
| **Clover** | Explicit offline caps; app market; hardware range | Stricter, adaptive offline envelope; fair marketplace; commodity hardware |
| **Starbucks** (in-house) | Order sequencing across channels | Capacity-aware pacing for everyone (D-09) |
| **Owner.com** | Commission-free first-party ordering plus automated marketing | L-01, J-06, J-09 |
| **Square Managerbot / Toast IQ** | Approval-gated AI actions | Generalized: autonomy dial, diff preview, 30-day undo |
| **Mashgin / Zippin** | Bounded computer-vision checkout | Partner integrations (not store-wide camera checkout) |
| **Vagaro / Boulevard / Fresha** | Resource-constrained booking; deposits and no-show fees | F-* with service segments and processing gaps; SCA-compliant no-show charges |
| **Dutchie / Flowhub / Cova** | Seed-to-sale compliance; purchase limits | Rule packs + compliance outbox |
| **ECRS / Toshiba / NCR** (grocery) | Scales, EBT/WIC, lane throughput | W-10 grocery pack |
| **Foodics / Petpooja / StoreHub** | Local compliance (ZATCA, GST, MyInvois) and aggregator integration | Country packs + marketplace connectors |

## 5. Trend radar (condensed from R05)

- **Adopt**:
  - offline-first core;
  - verifiable "ask your business" AI;
  - self-order kiosks;
  - first-party commission-free ordering;
  - AI phone answering;
  - forecasting feeding labor, prep and purchasing;
  - cross-channel pacing.
- **Trial**:
  - proactive approval-gated agents;
  - AI marketing with holdout measurement;
  - upsell prompts (bounded);
  - drive-thru voice via partners with human oversight;
  - tray-based computer vision;
  - agentic storefronts;
  - capped subscriptions;
  - QR *pay* (not QR-only menus).
- **Assess**:
  - agent payment protocols;
  - biometric payment and loyalty;
  - electronic shelf labels with discount-only dynamic pricing;
  - produce recognition;
  - scan-and-go;
  - in-store vision analytics;
  - crypto acceptance.
- **Hold**:
  - unsupervised voice AI;
  - store-wide "just walk out";
  - surge or personalized pricing;
  - QR-only full service;
  - aggressive tip defaults;
  - forced self-checkout;
  - vendor-imposed guest fees;
  - AI-washing.

## 6. Incident timeline and lessons

| Date | Incident | Lesson for Keel |
|---|---|---|
| Apr 2023 | Ransomware in one POS vendor data center: back office and gift cards down for days | Store operations and the stored-value snapshot must not depend on the vendor cloud |
| Sep 2023 | ~14 h payments-platform outage from a DNS change; offline mode didn't engage | Detect degraded services, not just link loss; stage infrastructure changes |
| Jul 2024 | Faulty security-sensor content update crashed Windows fleets worldwide | No kernel-mode code; staged rings for code *and content*; last-known-good boot |
| 2025 (several) | Recurring payment-processing disruptions at a major SMB provider | Processor failover paths; offline envelope |
| Oct 2025 | Cloud-region outage causes "complete system failures" at restaurants | Store independence; paired-region cells; protect online channels |
| Dec 2025 | Cyber Monday login-flow bug blocks POS sign-in for ~5.5 h | Local staff authentication |
| Apr 2025 | DDoS against a major payment processor: intermittent in-person and online failures for ~8.5 h | Multi-processor failover paths |
| Apr 2025 | Help-desk social engineering leads to ransomware at UK retailers | Help-desk verification playbooks; phishing-resistant MFA |
| Oct–Nov 2025 | CDN and edge configuration changes cause multi-hour global outages | Configuration as code; non-panicking parsers; last-known-good |
| 2023–2026 | Acquisitions, forced migrations, app retirements, hardware sunsets | Contractual sunset guarantees |

## 7. What still needs verification

Every dossier ends with a verification backlog. The highest-impact items before commercial launch:
- the outcome of the US interchange settlement fairness hearing (16 Nov 2026) and the Regulation II
  appeal;
- US state surcharge, junk-fee and cashless-ban lists, and penny-rounding rules;
- exact offline limits of each certified payment connector;
- fiscal details for launch-wave-2 markets (Germany, France, Spain), confirmed with local counsel;
- the Australian surcharging decision;
- **France NF525 (conflicting sources):** R03 reports self-attestation restored from 21 Feb 2026;
  R04 and R06 report third-party certification mandatory since 1 Mar 2026. Until resolved, plan for
  third-party certification, which satisfies either reading;
- final passage of Czech EET 2.0;
- pricing for vendors quoted from third-party sources;
- direct operator interviews (Reddit and review mining couldn't be completed in this environment).
  These should be replaced with first-party user research: at least 30 operator and staff interviews
  across 5 verticals before v1 scope lock.
