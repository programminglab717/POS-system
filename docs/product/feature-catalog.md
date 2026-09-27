# Keel — Feature Catalog

> Status: **Draft v1** · Owner: Product & Architecture · Last updated: 2026-09-27
>
> This is the complete feature list for Keel, organized by domain. Every feature has:
> - an **ID**, used for traceability in the roadmap, tickets and tests;
> - a **tier**, meaning when it ships (see the table below);
> - a **why**: either the industry flaw it fixes (🩹) or the best-in-class product it matches or beats
>   (⭐). References point to the research dossiers: [R01](../research/01-restaurant-pos.md) restaurant,
>   [R02](../research/02-retail-pos.md) retail, [R03](../research/03-payments-compliance.md) payments and
>   compliance, [R04](../research/04-technical-architecture.md) technical, [R05](../research/05-trends-ai-painpoints.md)
>   trends, AI and pain points, [R06](../research/06-verticals-global.md) verticals and global.

| Tier | Meaning | Target customer when it ships |
|---|---|---|
| **M** | MVP / pilot | Single-location counter-service café, QSR or specialty shop in the US |
| **1** | v1 / general availability | Full-service restaurants and bars, multi-location retail and restaurant groups, US and Canada |
| **2** | v2 / platform | Services and memberships, omnichannel retail, developer platform, first EU and UK markets |
| **3** | v3 / enterprise and global | Franchises and enterprise chains, grocery, venues, hotels, regulated verticals, global expansion |

---

## A. Always-on operations (resilience)

*The single biggest differentiator. Most "offline modes" keep card payments alive but stop operations:
login, loyalty, gift cards, KDS sync, close-out, reports (🩹 [R01 §2.1], [R02 §2.12]).*

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| A-01 | **Offline parity** | Every in-store function runs on local data. Nothing on the sales path waits on the cloud. Soak-tested for **72 h** WAN-down with zero lost or duplicated documents. | M | 🩹 Toast offline loses loyalty, gift cards, login, shift review; Lightspeed X "Sell screen only" [R01, R02] |
| A-02 | **Store Hub with hot-standby failover** | The hub role runs on an appliance or any eligible device. It sequences the store's events and grants check-ownership leases. A hot standby takes over in < 5 s with epoch fencing, and selling never pauses. Works across subnets and VLANs via cloud-assisted discovery. | M | 🩹 Toast local sync needs a hardwired device of a specific generation on the same subnet [R01]; ⭐ Simphony CAPS check ownership [R04] |
| A-03 | **Island mode** | A device cut off from both hub and cloud keeps selling: new orders, payments, printing and its own fiscal chain. Structural and money operations only on orders it owns; manager override otherwise. Everything reconciles on rejoin. | M | 🩹 devices become isolated "islands" with lost functions in cloud-first POS [R04 §1] |
| A-04 | **Local staff authentication** | PIN, badge and role grants are cached on the device. An identity-provider or cloud outage never blocks sign-in (under 2 s). | M | 🩹 Shopify Cyber Monday 2025 login outage blocked POS sign-in [R02]; Toast staff can't log back in offline [R01] |
| A-05 | **Degraded-service detection** | Circuit breakers trip on latency, error rate or DNS failure, not just on link loss. There's hysteresis and a one-tap manual override. | M | 🩹 Square 2023: sellers had to unplug Ethernet to force offline mode [R01] |
| A-06 | **Offline card acceptance risk envelope** | Caps per transaction, card, device and location, with BIN and card-type rules (e.g. no prepaid or foreign cards offline). Live exposure meter, maximum offline age, alerts before the processor's expiry, and forwarding within 60 s of reconnect. | 1 | 🩹 Square 72 h expiry with merchant liability; Clover $500/$5k defaults are the best found [R01, R02] |
| A-07 | **Offline sales are full sales** | Offline card sales post to inventory, customers, tax, the ledger and receipts like any other sale. | M | 🩹 Lightspeed R offline card payments aren't linked to sales or inventory [R02] |
| A-08 | **Declined-offline recovery** | Optional guest contact capture. Pay-by-link recovery, manager alert and ledger write-off workflow. | 1 | 🩹 Square "cannot provide customer contact details" [R01] |
| A-09 | **Processor failover** | Secondary processor, backup terminal or SoftPOS path when the primary processor is degraded. | 2 | 🩹 processor outages stop sales [R01, R02] |
| A-10 | **Cellular failover** | The hub appliance has an LTE/5G modem that keeps terminals and sync online through ISP outages. | 1 | ⭐ enterprise resilience practice |
| A-11 | **Online-channel protection** | When a store is unreachable, off-premise channels are auto-paused within 60 s, or orders are queued for guaranteed delivery. An order the kitchen never sees is never accepted. | 1 | 🩹 AWS Oct 2025 outage left restaurants with broken online and waitlist flows [R01, R05] |
| A-12 | **Stored-value offline escrow** | Gift card, store credit and loyalty redemption offline within merchant-set bounds, with a compact balance snapshot on the hub. | 1 | 🩹 D365, Lightspeed X and Toast can't redeem gift cards offline [R02] |
| A-13 | **"Backed up" indicator and in-store replication factor 2** | Payment and close events are held by two replicas within about 100 ms. Staff see if a device's sales aren't backed up yet. | M | ⭐ data-loss prevention when a device dies |
| A-14 | **Degraded-mode matrix in the product** | Every unavailable capability says *why* and what to do. No silent failures. | M | 🩹 [R02 §5 req 6] |
| A-15 | **Business-hours-aware updates** | Updates install outside trading hours in staged rings with auto-rollback. Merchants can set change freezes (e.g. Black Friday). | M | 🩹 CrowdStrike July 2024 lessons [R04]; [R02 §5 req 8] |
| A-16 | **Incident transparency** | Status page by component and region, in-app banner within 5 min, SMS/email to affected merchants, and a public post-incident report within 5 business days. | 1 | 🩹 Square 2023 "communication frequency and delayed support response" [R05] |
| A-17 | **Multi-region cloud** | Cells are multi-AZ with a warm standby in a paired region in the same residency zone (RPO ≤ 1 min, RTO ≤ 15 min). Quarterly failover drills. | 2 | 🩹 single-region dependency (us-east-1, Oct 2025) [R01] |
| A-18 | **Ransomware-resilient design** | Immutable cross-region backups. A cloud back-office compromise can't stop stores. Back office restored in under 4 h. | 1 | 🩹 NCR Aloha 2023 ransomware took gift cards and back office down for days [R01] |

## B. Selling and checkout (universal)

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| B-01 | **Instant register** | Every tap responds in under 50 ms p99 locally, with no spinners on the sales path. Three taps to anything common during service. | M | 🩹 lag at peak [R05 R16] |
| B-02 | Search, scan and quick keys | Fuzzy search (FTS) across names, SKUs, barcodes and aliases; configurable button grids; favorites; keyboard shortcuts on desktop | M | ⭐ |
| B-03 | Open-price, custom and non-inventory items | Permission-gated, with reason codes | M | ⭐ D365 per-item override permission [R02] |
| B-04 | Park, hold and recall orders | Across devices | M | |
| B-05 | Split tender | Any mix of tenders, partial payments and re-split after partial payment | M | 🩹 split-check pain [R05] |
| B-06 | Discounts (manual and automatic) | Line and order level, with reason codes, limits and approvals | M | |
| B-07 | Customer attach | Lookup by phone, email, name, card fingerprint or loyalty QR. Create inline. | M | |
| B-08 | Receipts: print, digital, none | Email and SMS receipts **never imply marketing consent**. Reprint. Gift receipts. Localized per guest language. | M | 🩹 receipt spam [R05 R27] |
| B-09 | **Returns and exchanges** | Linked to original lines (refunds at allocated net price and tax), across locations and channels. Policy engine. Receipt-less policy with limits. | M (basic) / 1 (full) | 🩹 over-refunds and wrong net amounts [R02 §4.12] |
| B-10 | **Training mode** | Sandbox transactions on real devices, with no fiscal, payment or inventory effects. Guided exercises. | M | 🩹 training burden with high turnover [R05] |
| B-11 | **10-minute onboarding standard** | A new server rings a 4-top with a seat split, a manager-approved comp and card payment after at most 10 min of in-app training. | 1 | [R05 R13] |
| B-12 | Per-user language | Staff UI in the staff member's language; receipts in the guest's; kitchen in the station's. | M (EN/ES) / 1 (8+) | [R05 R15] |
| B-13 | Price check and item info | Stock by location, attributes, allergens, images | M | |
| B-14 | Notes and tags | Order and line notes, tags, custom fields | M | |
| B-15 | Pre-orders and scheduled orders | `scheduled_for` time, with capacity-aware slots | 1 | |
| B-16 | Offline-safe order numbering | Hub-allocated store numbers, with per-device blocks when partitioned | M | 🩹 D365 duplicate transaction IDs [R02] |
| B-17 | Accessibility for staff | Screen reader support, large text, high contrast, left-hand mode, color-blind-safe status colors | 1 | EAA / WCAG 2.2 |

## C. Restaurant front of house

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| C-01 | Floor plans and live table state | Drag-and-drop editor. Timers per state: seated, ordered, fired, check dropped, paid. Server sections. | 1 | ⭐ [R01 §2.3] |
| C-02 | Seats and courses | Seat-level ordering, course assignment, hold and fire by course | 1 | ⭐ |
| C-03 | **Timed auto-fire and synchronized fire** | Fire course 2 N minutes after course 1 is bumped. Fire so items finish together. | 1 | ⭐ rare outside enterprise KDS [R01] |
| C-04 | **Best-in-class splits** | By seat, item, even N ways, amount, and fractions of a line. Drag and drop. Re-split after partial payment. Each in ≤ 3 taps. | 1 | ⭐ TouchBistro drag-and-drop [R01]; [R05 R14] |
| C-05 | Merge and transfer | Checks, items, tables and servers, with an audit trail | 1 | |
| C-06 | **Bar tabs done right** | Open by dip or tap (name from the card). Configurable pre-auth, incremental auth, auto-close with a disclosed policy, walkout report. | 1 | [R01 §2.2, req 13] |
| C-07 | Handhelds with full parity | Coursing, splits, approvals and pay-at-table on phones and payment handhelds. Works offline. | 1 | ⭐ Toast Go [R01] |
| C-08 | Pay at table | Terminal or handheld at the table, with a tip on the terminal. Split by guest. | 1 | ⭐ EU norm |
| C-09 | **QR order and pay (optional, hybrid)** | Joins the same check the server sees. Group tab, rounds and split by item. No app or account. Server can disable per table. Never replaces service or a printed menu. | 2 | 🩹 QR-menu backlash [R05 H4]; ⭐ me&u group ordering |
| C-10 | Reservations and waitlist (native) | Turn-time-based quotes by party size, SMS paging, deposits and no-show fees, table assignment | 2 | 🩹 guest data owned by Amex/DoorDash-owned networks [R01] |
| C-11 | Reservation-network integrations | OpenTable, Resy, SevenRooms and Tock. The guest profile stays merchant-owned and exportable. | 2 | [R01 req 28] |
| C-12 | Guest profiles for dining | Allergies, preferences, visit history and spend, with consent | 1 | ⭐ SevenRooms [R01] |
| C-13 | Service charges and auto-gratuity | Configurable by party size or event. Tax treatment and **separation from tips** (for tip law and qualified-tip reporting). | 1 | [R01 §2.8] |
| C-14 | Happy hour and dayparts | Scheduled prices and menus by time and day; "discount-only" dynamic pricing | 1 | 🩹 Wendy's surge-pricing backlash [R05] |
| C-15 | Comps, voids and approvals | Reason codes and thresholds. **Remote push approval** to a manager's phone (under 10 s on LAN). Post-fire voids logged as waste. | M (basic) / 1 (remote) | [R01 req 25], [R05 R17] |
| C-16 | Catering and events | Quotes, deposits, BEOs (banquet event orders), prep sheets, invoicing, house accounts, delivery or setup fulfillment | 2 | [R01 req 41] |
| C-17 | Drive-thru | Lane modes, confirmation board, timer integrations, line-busting handhelds, optional voice AI via partners with human takeover | 3 | [R01 req 40], [R05 T4] |
| C-18 | Multi-brand / virtual brands | Several brands in one kitchen, one KDS and separate channel menus | 2 | [R01 §4.15] |

## D. Kitchen, production and fulfillment

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| D-01 | KDS routing engine | Item, modifier, channel, order mode, daypart and revenue center → stations. Item split into components across stations. | M | ⭐ [R01 §2.4] |
| D-02 | Expo views | Consolidated order, dine-in vs to-go expo, handoff marking | M | |
| D-03 | Bump bars and touch | USB/Bluetooth bump bars, recall last bumped, park, rush | M | |
| D-04 | **Cook-time sequencing** | Per-item cook times so a 12-minute steak and a 4-minute salad finish together | 1 | ⭐ QSR Automations-class [R01] |
| D-05 | All-day counts and batch views | Totals per item across open tickets; prep batching | 1 | |
| D-06 | Allergen and modifier legibility | Structured allergens and "No/Extra" rendered in distinct colors. Free-text highlighted. Color-blind safe. | M | 🩹 allergen integrity unsolved [R01 §4.13] |
| D-07 | Ticket aging and speed metrics | Color thresholds, average ticket time per station, speed-of-service reports | M | |
| D-08 | **One queue for every channel** | POS, kiosk, web and app, QR, marketplaces, phone AI and AI agents all feed one KDS queue, tagged by channel and promised time. **No tablets.** | 1 | 🩹 "tablet hell" [R01 §2.5]; [R05 R20] |
| D-09 | **Capacity-aware pacing and promise times** | A per-station load model quotes ready times, then extends, throttles or pauses channels by priority (e.g. in-store first). 90% of orders ready within ±3 min of the quote. | 1 | ⭐ Starbucks sequencing [R05 A7]; [R01 req 23] |
| D-10 | Channel complexity caps | Maximum items and modifiers per order per channel. Large orders need staff acceptance. | 1 | ⭐ Starbucks mobile cap 15→12 [R05] |
| D-11 | Printer and KDS fallback routing | Logical routes with ordered fallbacks. Staff alerted on reroute. Kitchen reprints marked. | M | 🩹 printer = #1 support call |
| D-12 | Order-ready boards and notifications | Status screens driven by the hub's local API; SMS "ready" notices | 1 | |
| D-13 | Pickup and delivery handoff | Courier ETA integration, first-party delivery via on-demand fleets (e.g. DoorDash Drive, Uber Direct), delivery zones and fees | 2 | [R01 §2.5] |
| D-14 | Hands-free KDS voice commands | "Bump 42", "recall", "86 salmon", on-device | 3 | |
| D-15 | Production and prep | Prep lists from forecasts, batch production with hold times and expiry labels | 2 | |
| D-16 | Waste logging | From the KDS (post-fire voids), prep and counts, with reason codes | 1 | [R01 req 32] |

## E. Retail operations

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| E-01 | **Rich variant model** | ≥ 5 option axes and ≥ 10,000 variants per parent, lazily generated. Grid entry for POs, receiving and counts. | 1 | 🩹 Shopify 3 axes / 2,048 variants; Square 250 [R02] |
| E-02 | Large catalogs at the edge | 500k SKUs per register. Barcode lookup ≤ 50 ms p99, search ≤ 200 ms p95, delta sync ≤ 60 s. | 1 | 🩹 D365 10 GB offline cap; Odoo partial loading [R02] |
| E-03 | Multiple barcodes, GS1 parsing | GTIN, PLU, internal codes, embedded price and weight, **GS1 DataMatrix and Digital Link** (lot, expiry, serial) | M (1D) / 1 (2D) | ⭐ industry 2D transition |
| E-04 | Serialized items | Serial capture **at receipt or at sale** (per-item policy). Serial history. Warranty lookups. | 1 | ⭐ D365 pattern [R02] |
| E-05 | Lot and expiry | FEFO prompts, block expired sales, recall lookup from lot to customers | 2 | 🩹 no SMB vendor enforces this [R02] |
| E-06 | Units of measure | Case, inner and each conversions; weighed and measured goods | 1 | |
| E-07 | Kits and bundles | Fixed and choice components, component depletion, price allocation | 1 | ⭐ Odoo combos [R02] |
| E-08 | Labels and shelf tags | Barcode and price labels (ZPL/EPL/TSPL), batch printing from receiving | 1 | |
| E-09 | Layaway and deposits | Partial payments, reserved stock, policies. Works offline. | 1 | ⭐ Lightspeed X offline layaway [R02] |
| E-10 | Special orders | Vendor purchase-need creation, deposit, customer notification on receipt | 1 | |
| E-11 | Quotes and estimates | Expiring quotes that convert to orders in one tap. Works offline. | 1 | |
| E-12 | **Mixed-fulfillment cart** | Carry-out, pickup at another store and ship, in one transaction, with a deposit override and card on file for the balance | 2 | ⭐ D365 customer orders [R02] |
| E-13 | BOPIS / BORIS / ship-from-store / endless aisle | Pick tasks, reservations with time limits, routing rules, cross-channel returns | 2 | [R02 §2.4] |
| E-14 | Clienteling | Customer profile, purchase history, wishlists, size profile, notes, outreach from the handheld | 2 | |
| E-15 | Gift receipts and price-hidden receipts | | 1 | |
| E-16 | RFID counting | EPC/SGTIN decoding, ≥ 100k reads/min ingestion, count sessions | 3 | [R02 req 18] |
| E-17 | Electronic shelf labels | Price publishes to shelves and registers atomically | 3 | |
| E-18 | Self-checkout mode | Retail self-checkout with an always-staffed alternative, item limits, assist alerts | 3 | 🩹 forced self-checkout backlash [R05 H6] |
| E-19 | Scan-and-go (membership-based) | Customer app scanning, exit verification | 3 | [R05 As5] |
| E-20 | Mobile POS / line busting | Handheld checkout with terminal pairing or SoftPOS | 1 | |

## F. Services, bookings, memberships and rentals

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| F-01 | Resources and availability | Staff, rooms, chairs, equipment and courts; skills; calendars | 2 | [R06] |
| F-02 | Appointments | Online and in-store booking, buffers, multi-resource services, reminders, intake forms | 2 | ⭐ Vagaro, Fresha, Boulevard [R06] |
| F-03 | Deposits, no-show and cancellation fees | Card on file via processor vault, policy-driven | 2 | |
| F-04 | Classes and capacity events | Capacity, waitlists, check-in | 2 | |
| F-05 | Memberships and entitlements | Plans, billing cycles, freezes, entitlements (counted, discount, access), dunning, caps and anti-sharing | 2 | ⭐ Panera Sip Club cap lesson [R05] |
| F-06 | Packages and prepaid services | "5 massages", class packs | 2 | |
| F-07 | Rentals | Serialized units, deposits, due times, late and damage fees, return inspection | 3 | [R06] |
| F-08 | Work orders and service tickets | Stages, photos, signatures, parts, labor lines, customer status notifications | 2 | ⭐ Lightspeed R work orders [R02] |
| F-09 | Staff commissions for services | Per service or product, tiered, split, clawback | 2 | |
| F-10 | Tickets and timed entry | Attractions and events, capacity by time slot, scanning at the gate | 3 | [R06] |

## G. Catalog and menu management

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| G-01 | **Hierarchical catalog** | Org → brand → region → location → revenue center → channel, with inheritance, overrides, locked fields and bounds | 1 | ⭐ Simphony EMC [R01] |
| G-02 | **Deep modifiers** | Unlimited nesting (≥ 4 levels tested), min/max, free-N, size-price matrix, pre-modifiers, halves and quarters | M | [R01 req 19] |
| G-03 | Structured allergens and dietary tags | Structured data end to end: menu → online → KDS → labels → receipts | M | 🩹 allergen integrity [R01] |
| G-04 | Channel menus and dayparts | Per-channel structures, prices and availability; daypart schedules | 1 | |
| G-05 | **Drafts, scheduled publishing, preview and rollback** | Immutable versions, effective-dated activation (works offline), one-click rollback, audit | 1 | 🩹 D365 config takes up to 24 h to propagate [R02] |
| G-06 | **Global 86 in ≤ 10 s** | Availability events reach every device, channel, marketplace and agent catalog within 10 s p95. Auto-86 at zero stock with scheduled un-86. | M (in-store) / 1 (all channels) | [R01 req 21], [R05 R22] |
| G-07 | Bulk edit, import and export | Spreadsheet round-trip, API bulk jobs | M | |
| G-08 | **AI menu and catalog import** | From photo, PDF, website, spreadsheet or competitor export. The merchant reviews a diff before publishing. | M | [R05 R53] |
| G-09 | Images and media | Auto background cleanup, multiple sizes, per channel | 1 | ⭐ Lightspeed AI photo tools [R05] |
| G-10 | Nutrition and labels | Calories and nutrition panels where required, and label generation | 2 | |
| G-11 | Custom fields on catalog objects | Typed, validated, searchable | 1 | |
| G-12 | Vendor catalogs | Vendor items, pack sizes, costs, catalog import | 1 | |

## H. Pricing, promotions and fees

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| H-01 | **One deterministic pricing engine everywhere** | The same kernel code on device, hub, cloud, kiosk and web. A golden suite of ≥ 1,000 baskets must produce identical output on every platform build. | M | 🩹 online/offline price divergence [R02] |
| H-02 | Layered price lists | Base, location, channel, daypart, customer group and contract; effective-dated | 1 | |
| H-03 | **Promotion vocabulary** | Simple, quantity or tiered, mix-and-match (least expensive, deal price), threshold, BOGO, bundle price, time-based, segment, single-use coupons, loyalty rewards | 1 | ⭐ D365 [R02] |
| H-04 | **Explicit stacking model** | Exclusive / best-price / compounded concurrency modes with priorities, plus explicit allow-lists per promotion | 1 | ⭐ D365 concurrency model [R02] |
| H-05 | **Explainable pricing** | Per-line price source and promotions applied; "why not applied" diagnostics shown to cashiers and in the back office | 1 | 🩹 no vendor explains prices at the counter [R02] |
| H-06 | Promotion simulation and conflict checks | A sandbox that prices historic baskets; margin-floor and overlap warnings before publish | 2 | [R02 req 15] |
| H-07 | Promotion engine performance | 100-line basket × 10,000 active promotions ≤ 100 ms p99 on a register | 2 | [R02 req 14] |
| H-08 | **Surcharge, dual pricing and fee compliance engine** | Rules by jurisdiction and card type (credit vs debit), caps and bans encoded, disclosures on menus and receipts, junk-fee checks | 1 | [R01 req 14], [R03] |
| H-09 | **Merchant-controlled guest fees only** | Keel never adds a fee to a guest's bill. Any fee is created by the merchant, itemized and disclosed before commitment. | M | 🩹 Toast 99¢ fee reversal [R05 H7] |
| H-10 | **Ethical tipping** | Off by default for counter service and retail. "No tip" and "Custom" as prominent as presets. Presets computed on pre-tax, pre-fee subtotal, shown as % and $. | M | 🩹 27% tip less on pre-entered screens [R05 H5] |
| H-11 | **Price integrity** | Scheduled and daypart pricing only. Personalized upward pricing blocked by default. Legally required disclosures (e.g. NY algorithmic pricing) rendered automatically. | 1 | [R05 R28] |
| H-12 | Cash rounding | Per jurisdiction and tender (e.g. nickel rounding for US cash after the penny's end), with benefit-program exceptions | M | [R02 §2.6] |
| H-13 | Price rules as extensions | Custom pricing logic via WASM Functions that run offline | 2 | ⭐ Shopify Functions, made offline-capable |

## I. Payments

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| I-01 | **Processor-agnostic by design** | Bring any supported processor. Switching needs no new hardware and loses no data. No penalty fees. | M (1 processor) / 1 (≥ 3 US) | 🩹 universal processing lock-in; Lightspeed penalty fee; SpotOn $995 switch fee [R01, R02] |
| I-02 | Semi-integrated card terminals | Card data never touches Keel. Minimal PCI scope. Terminal APIs over LAN or cloud. | M | [R03] |
| I-03 | Tap to Pay on phone (SoftPOS) | Via certified PSP SDKs on iPhone and Android | 1 | ⭐ |
| I-04 | Run on Android payment terminals | The Keel app runs on the PSP's Android terminal alongside the payment app | 1 | ⭐ |
| I-05 | Tips: on-terminal and on-screen, tip adjust | Tip adjust windows, tip on pre-tax base, reporting per staff member | M | |
| I-06 | Pre-auth, incremental auth, capture | Tabs, hotels, rentals | 1 | |
| I-07 | Partial approvals and split tender | Including gift card + card, cash + card | M | |
| I-08 | Refunds | To original tender, cross-location, or to store credit; linked refunds; manager limits | M | |
| I-09 | **Offline payments with risk envelope** | See A-06 | 1 | |
| I-10 | Card-on-file and network tokens | Vaulted at the processor. Account updater. Consent records. | 2 | |
| I-11 | **Card-on-file portability** | Card data migrated to a new PCI DSS Level 1 processor on request within 10 business days | 2 | [R01 req 12] |
| I-12 | Least-cost and smart routing | Debit routing preferences where supported, routing by BIN, amount or brand | 3 | [R03] |
| I-13 | Multi-processor routing | Per location, per card brand, per channel, with failover | 2 | |
| I-14 | Alternative payment methods (by region) | A2A/instant (Pix, UPI, FedNow request-for-payment, SEPA Instant), QR wallets, mobile money, BNPL, meal vouchers, EBT | 2–3 | [R03], [R06] |
| I-15 | Gift cards (native) | Physical, digital and wallet passes; locked during a transaction; **activated only after payment captured**; offline escrow; liability reports | 1 | ⭐ D365 lock; 🩹 D365 pre-payment load bug [R02] |
| I-16 | Third-party gift card providers | Givex, SVS, Blackhawk and others via connectors | 2 | |
| I-17 | House accounts | Credit limits, net terms, statements, aging, invoices, payments on account | 1 | [R02 req 32] |
| I-18 | **Automatic reconciliation** | Deposits broken down into transactions, refunds, chargebacks, fees, tips and adjustments, and matched to bank lines. Single-location day close in ≤ 5 min. | 1 | [R05 R41] |
| I-19 | **Fee transparency** | Effective rate per transaction and method. Fees audited against contracted pricing. | 1 | 🩹 opaque, reseller-set pricing [R02] |
| I-20 | Disputes and chargebacks | Evidence packets (itemized receipt, signature, delivery proof), deadlines, win-rate reporting | 2 | |
| I-21 | Keel Payments (optional) | Embedded processing with published interchange-plus pricing, next-day or instant payouts (fee shown in $), **explainable holds** with an appeal path. Never mandatory. | 2 | 🩹 fund holds without explanation [R05 R10] |
| I-22 | Capital offers (optional) | Total repayment in $, holdback %, APR-equivalent. Never pre-checked and never tied to processing. | 3 | [R05 R44] |
| I-23 | Multi-currency cash acceptance | Tourist areas and border towns; change given in local currency | 2 | |
| I-24 | EBT/SNAP and WIC | Eligibility rules by **jurisdiction + effective date** (state SNAP restriction waivers), SNAP subtotal, tax exemption on the SNAP portion, eWIC APL matching | 3 | [R02 §2.9] |

## J. Customers, loyalty and marketing

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| J-01 | Unified customer profile | Across dine-in, retail, online, bookings and memberships; merchant-owned and exportable | 1 | ⭐ Shopify one customer record [R05] |
| J-02 | Consent management | Per channel and purpose; receipts ≠ consent; DSAR tooling; crypto-shred erasure | M (basic) / 1 | 🩹 [R05 R27] |
| J-03 | Loyalty programs | Points, visits, item-based, tiers, cashback, paid tiers. Low-threshold rewards. One model for coupons, loyalty, eWallet and gift cards. | 1 | ⭐ Odoo unified program model [R02]; ⭐ Starbucks 60-star reward [R05] |
| J-04 | Card-linked enrollment | Enroll and earn with the payment card, no typing | 2 | ⭐ Thanx [R01] |
| J-05 | Loyalty guardrails | Change notices to members (default 30 days) with an auto summary; subscription caps and device binding | 2 | [R05 R30] |
| J-06 | Marketing automation | Win-back, birthday, slow-day offers; email and SMS; **holdout-measured lift** | 2 | ⭐ Toast IQ Grow +8% (vendor-reported) [R05] |
| J-07 | Segments | Rule-based and predictive (churn risk, VIP), usable in promotions and pricing | 2 | |
| J-08 | Feedback and reviews | Post-visit surveys, review routing, AI summaries | 2 | |
| J-09 | First-party ordering growth engine | Website, ordering, app, loyalty and marketing, commission-free (see L-01) | 2 | ⭐ Owner.com model [R05 A4] |

## K. Inventory and purchasing

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| K-01 | **Movement-sourced inventory ledger** | Reason codes; on-hand, reserved, available, in-transit and on-order | M | [R02 req 16] |
| K-02 | Negative stock policy | Allow, warn or block per item or location | 1 | |
| K-03 | **Counts without closing the store** | Blind counts recorded "as of" a timestamp; movements during the count reconciled; mobile count sheets | 1 | [R02 req 16] |
| K-04 | Purchase orders | Vendor minimums, pack sizes, partial receipts, tolerances, landed costs | 1 | |
| K-05 | Transfers | Store ↔ store ↔ warehouse, in-transit, discrepancy handling | 1 | |
| K-06 | Receiving | Scan to receive, label printing, cost updates | 1 | |
| K-07 | **Recipes and theoretical usage** | Recipes and sub-recipes, yields, **modifier-level depletion**, actual vs theoretical variance | 1 | ⭐ MarginEdge, xtraCHEF [R01] |
| K-08 | **Native replenishment** | Min/max, reorder points, lead-time- and seasonality-aware forecasts, suggested POs, ABC analysis | 2 | 🩹 Shopify Stocky shutdown (Aug 2026) removed forecasting [R02] |
| K-09 | AI invoice capture | Photo or email invoices → lines matched to catalog and PO → cost updates, three-way match | 2 | [R05 R42] |
| K-10 | Cost methods | Moving average or FIFO, per org setting; margin reports | 1 | |
| K-11 | Commissary and central kitchen | Production at a central kitchen, transfers to stores, internal pricing | 3 | ⭐ Restroworks [R01] |
| K-12 | Integrations | MarginEdge, Restaurant365, ERP inventory via API | 2 | [R01 req 32] |

## L. Omnichannel and digital ordering

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| L-01 | First-party online ordering | Web ordering on the same catalog, pricing and pacing engine; pickup, delivery and curbside; no commission | 1 | ⭐ Owner.com [R05] |
| L-02 | Branded app | White-label ordering and loyalty app | 3 | |
| L-03 | **Self-order kiosk** | Same menu engine. Accessible mode. At most one skippable upsell per step. First-timer orders a combo in ≤ 60 s. | 2 | ⭐ Yum kiosk +10% [R05 A3] |
| L-04 | **Marketplace integrations without tablets** | DoorDash, Uber Eats and Grubhub at launch, then Deliveroo, Just Eat, Wolt, Talabat, Zomato, Swiggy, iFood, Rappi and others by market. Two-way menu and status sync, per-order payout reconciliation. | 1 | 🩹 tablet hell [R01] |
| L-05 | E-commerce connectors | Shopify, WooCommerce, BigCommerce and Adobe Commerce: unified inventory and orders | 2 | |
| L-06 | Unified inventory availability (ATP) | Reservations per channel, safety stock, oversell protection | 2 | |
| L-07 | **Agent-ready catalog** | Real-time structured menu or catalog (modifiers, allergens, prices, availability, prep quotes) for AI agents. Allow-lists, caps, rate limits. Agent orders labeled on the KDS. | 2 | [R05 R50] |
| L-08 | Agentic checkout | Delegated or tokenized agent payments per emerging protocols | 3 | [R05 R51] |
| L-09 | AI phone ordering | Answers every call, orders against the live menu and 86 status, human handoff or SMS link, transcripts, measured accuracy | 2 | ⭐ Square AI voice ordering [R05 A5] |
| L-10 | Order-ahead for scheduled pickup | Slot capacity by kitchen load | 1 | |

## M. Workforce

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| M-01 | **Roles and ABAC permissions** | Action × resource × limits; per-location grants; separation of duties | M | ⭐ D365 permission groups [R02] |
| M-02 | Approvals | Same-device PIN or badge, or remote push to a manager's phone | M / 1 | [R05 R17] |
| M-03 | Time clock | Job codes, breaks, attestations, geofence or photo (optional), cash-tip declaration | 1 | |
| M-04 | **Biometrics off by default** | On-device only when enabled; BIPA-grade consent and retention | 1 | 🩹 BIPA liability [R01] |
| M-05 | Scheduling | Templates, availability, time off, swaps, publishing, labor budget vs forecast | 2 | ⭐ 7shifts [R01] |
| M-06 | **Labor compliance packs** | Predictive scheduling, breaks, overtime and minors per jurisdiction. Alerts *before* violations. | 2 | [R01 req 29], [R05 R19] |
| M-07 | **Tip engine** | Pools weighted by hours × role points; tip-outs by sales category; service charges separate; FLSA rules; qualified-tip reporting by occupation | 1 | [R01 req 30] |
| M-08 | Tip transparency for staff | Own tips, tip-outs and pool shares per shift, in real time | 1 | [R05 R18] |
| M-09 | Commissions | Item, category or margin rules, tiers, splits, clawbacks | 2 | |
| M-10 | Payroll integration | ADP, Gusto, Paychex and others; on-demand tip payout where legal | 2 | [R05 R45] |
| M-11 | Staff app | Schedules, swaps, time off, tips, announcements, training | 2 | |
| M-12 | Certifications tracking | Food handler and alcohol service certificates with expiry, blocking where required | 3 | |

## N. Cash management

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| N-01 | Drawer sessions | Float, paid-in and paid-out with reasons, drops, no-sale audit, multiple drawers per register | M | ⭐ D365 cash office [R02] |
| N-02 | **Blind close with over/short thresholds** | A variance above the role's threshold forces a manager | M | ⭐ Odoo "Amount Authorized Difference" [R02] |
| N-03 | Shift suspend and resume; shared drawers | | 1 | |
| N-04 | Safe and bank deposits | Bag numbers, expected vs actual by business date | 1 | |
| N-05 | Cash recyclers | Automatic tendering and denomination counts | 3 | |
| N-06 | Cash-less and cash-required compliance | Enforce cash acceptance where the law requires it (e.g. cashless-ban cities) | 1 | [R03] |

## O. Reporting and analytics

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| O-01 | Real-time dashboard | Sales, labor %, ticket times, top items; mobile | M | |
| O-02 | **Local reports offline** | X/Z reports, shift and drawer close, sales by hour, from local data | M | 🩹 Toast shift review and Lightspeed reporting stop offline [R01] |
| O-03 | **Semantic layer and metrics glossary** | One definition of every metric, shared by reports, API, exports and AI | 1 | [R05 R54] |
| O-04 | Report builder and saved views | Custom columns, filters, pivots, saved layouts, scheduled email | 1 | 🩹 "no report building wizard" (Toast) [R05] |
| O-05 | Product mix with modifiers | Including modifier attach rates | 1 | |
| O-06 | Loss-prevention reports | Voids, comps, refunds, discounts, no-sales by employee vs peers | 1 | |
| O-07 | Benchmarks | Opt-in, k-anonymized cohort benchmarks | 3 | [R05 R39] |
| O-08 | **Warehouse sync** | Snowflake, BigQuery, Databricks, Redshift, and S3/GCS as Iceberg tables, full fidelity | 2 | [R02 req 31] |

## P. Finance and accounting

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| P-01 | **Double-entry commerce ledger** | Posting rules per event; liabilities for gift cards, tips, loyalty and deposits | 1 | |
| P-02 | Accounting sync | QuickBooks Online, Xero, then NetSuite, Sage, DATEV and others. Chart-of-accounts mapping. Daily journals incl. marketplace payouts and liabilities. | 1 | [R05 R42] |
| P-03 | Multi-entity consolidation | Per legal entity books, inter-entity gift card settlement | 3 | |
| P-04 | Marketplace payout reconciliation | Commission, promo funding, error charges per order | 1 | [R01 §2.5] |
| P-05 | Sales tax reports | By jurisdiction, with filing exports and tax-engine integrations | 1 | |

## Q. Multi-location, enterprise and franchise

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| Q-01 | Location groups and roll-ups | Arbitrary hierarchy for config and reporting | 1 | |
| Q-02 | **Governance with bounds** | Locked fields, allowed ranges (e.g. price ±10%), visible diffs of local overrides | 2 | [R05 R46] |
| Q-03 | **Franchise model** | Franchisee = own account; the franchisor gets governed resources and read scopes; royalty and ad-fund reports; **every feature available to franchisees** | 3 | 🩹 franchisees get features last [R05] |
| Q-04 | Config-as-code | Git-managed configuration for enterprise, with plan and apply | 3 | [R05 R47] |
| Q-05 | SSO and SCIM | SAML/OIDC, provisioning | 2 | |
| Q-06 | Fleet console | Versions, health, peripherals, remote actions (see [hardware](../architecture/hardware.md)) | 1 | |
| Q-07 | Enterprise cells | Dedicated cells and custom rollout rings for large tenants | 3 | |

## R. Compliance and trust

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| R-01 | Tax engine | Inclusive and exclusive, compound, thresholds, holidays, exemptions, takeout vs dine-in; US rate tables; tax-engine connectors | M (store-location US) / 1 | [R03] |
| R-02 | **Fiscalization modules** | Pluggable per country: Germany (TSE, DSFinV-K), France (NF525), Italy (RT), Austria (RKSV), Spain (VeriFactu/TicketBAI), Portugal, Poland and others; Saudi Arabia (ZATCA), Brazil (NFC-e), Mexico (CFDI), India (GST) | 2–3 | [R01 req 38], [R03] |
| R-03 | Tamper-evident records | Signed, hash-chained event logs; daily Merkle roots in write-once storage | M | 🩹 sales-suppression "zappers" |
| R-04 | Age verification | ID scan (AAMVA PDF417), mobile driver's licenses, recording outcome only; category and time-of-day restrictions | 1 | [R02 req 27] |
| R-05 | Regulated goods | Purchase limits, traceability connectors (e.g. cannabis seed-to-sale), restricted-hours rules | 3 | [R06] |
| R-06 | Accessibility | WCAG 2.2 AA staff and guest UIs; EN 301 549 / EAA for kiosks and self-service | 1 | [R03], [R05 R29] |
| R-07 | Privacy | Consent, DSAR, retention policies, crypto-shredding, data residency | 1 | |
| R-08 | Junk-fee and price-transparency rules | Menu and receipt disclosure templates (e.g. California SB 478), all-in pricing modes | 1 | [R01 req 14] |
| R-09 | Labor law | See M-06 and M-07 | 2 | |
| R-10 | Legal-for-trade weighing | Certified-scale integration rules per market | 3 | |

## S. Hardware and devices

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| S-01 | **Bring your own commodity hardware** | iPad, iPhone, Android, Android payment terminals, Windows, macOS, Linux; certified peripheral list | M | 🩹 proprietary hardware lock-in [R01, R02] |
| S-02 | Lifecycle commitments | ≥ 5 years support and ≥ 12 months' EoL notice; no legacy-device fees | M | 🩹 Clover $99.95/mo legacy fee; Shopify reader sunset [R02] |
| S-03 | Keel Hub appliance | Fanless, TPM, LTE/5G failover, UPS, USB peripheral host, A/B OS updates | 1 | |
| S-04 | **Printing done right** | Auto-discovery, test print, health polling, queue with retries, fallback routes, raster rendering for every script (Arabic, Hebrew, Thai, CJK and more) | M | 🩹 printer = #1 support call |
| S-05 | Scanners, scales, drawers, displays | See [hardware.md](../architecture/hardware.md) | M / 1 | |
| S-06 | **Network doctor** | Diagnoses client isolation, split subnets, weak Wi-Fi and unreachable printers in < 60 s. Works on consumer routers at defaults. ≤ 3 documented ports. | M | 🩹 Lightspeed K 10 ports; Toast subnet rule [R01] |
| S-07 | Zero-touch enrollment and kiosk lockdown | Android Enterprise, Apple ADE, Windows Autopilot | 1 | |
| S-08 | Spare swap in ≤ 10 min | Any enrolled spare takes over a failed device's role | M | [R05 R5] |
| S-09 | Customer-facing display | Cart, tips, loyalty, digital-receipt QR, weight display for scales | M | |

## T. Platform and extensibility

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| T-01 | **Public REST API with parity** | Everything the UI does; OpenAPI; idempotency keys; date versions; **free access to a merchant's own data** | 1 | 🩹 partner-gated APIs [R05 R49] |
| T-02 | Webhooks, event log API and replay | Signed, ordered per aggregate, 72 h retries, replay | 1 | |
| T-03 | **Store Hub local API** | LAN REST and WebSocket for menu boards, timers, CCTV and ESL; works offline | 2 | ⭐ unique |
| T-04 | **Functions (WASM) that run offline** | Pricing, validation, routing, loyalty and returns hooks, deterministic and sandboxed | 2 | ⭐ Shopify Functions, offline-capable |
| T-05 | POS UI extensions | Sandboxed, host-rendered components on defined targets | 2 | ⭐ Shopify POS UI extensions |
| T-06 | Automations (no-code) | Trigger → condition → action; cloud and local (hub) runners | 2 | ⭐ Shopify Flow |
| T-07 | Custom fields and objects | On every resource | 1 / 2 | |
| T-08 | App marketplace | Reviewed apps, transparent permissions, fair revenue share, clean uninstall | 2 | ⭐ Clover app market [R05] |
| T-09 | SDKs, CLI, sandbox with simulators | Simulated terminals, hub, delivery partners and fiscal authorities | 1 / 2 | |
| T-10 | **Keel MCP server** | Scoped AI-agent tools with approvals and audit | 2 | |
| T-11 | **API and feature deprecation policy** | ≥ 24 months for API versions; no feature removal without native parity or a free migration | 1 | 🩹 Stocky shutdown, MPOS deprecation [R02] |

## U. AI

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| U-01 | AI catalog and menu import | See G-08 | M | |
| U-02 | Contextual staff help | "How do I…" grounded in docs and screen state, in the staff member's language | 1 | |
| U-03 | **Verifiable business Q&A** | Every number cites its report, filters, time range and freshness | 1 | [R05 R32] |
| U-04 | Proactive anomaly alerts | Sales, labor, COGS, voids and refunds | 1 | ⭐ SpotOn Profit AI [R05] |
| U-05 | **Autonomy dial per capability** | Off / suggest / auto with notification / auto. Default "suggest" for money, prices, schedules and guest communications. | 1 | [R05 R34] |
| U-06 | **Approval-gated actions** | Diff preview, approve, apply, 30-day undo, audit with approver and model version | 1 | ⭐ Square Managerbot approval gate [R05] |
| U-07 | Forecasting | 15-min granularity by item and channel; MAPE (mean absolute percentage error) visible; overrides feed back | 1 | [R05 R37] |
| U-08 | Ordering agent | Draft POs from forecasts, pars, lead times, pack sizes | 2 | |
| U-09 | Schedule agent | Law-aware draft schedules | 2 | |
| U-10 | Loss-prevention sentinel | Anomaly detection with evidence packets | 2 | |
| U-11 | Menu engineering | Profitability × popularity, recipe cost drift, test suggestions (never auto-applied) | 2 | |
| U-12 | Marketing assistant | Campaign drafts with holdout measurement | 2 | |
| U-13 | Phone AI | See L-09 | 2 | |
| U-14 | Voice drive-thru (partner) | No-intervention rate published; readback; sanity limits; human takeover ≤ 2 s | 3 | 🩹 McDonald's/IBM, Presto SEC [R05 H1, H8] |
| U-15 | **Honest automation metrics** | No-intervention and handoff rates; no AI-washing | 2 | [R05 R35] |
| U-16 | Sponsorship labels | Commercially influenced recommendations are labeled and can be turned off | 2 | [R05 R38] |
| U-17 | Data boundaries | No cross-merchant training without opt-in; regional processing | M | [R05 R39] |

## V. Onboarding, support and commercial promises

| ID | Feature | Detail | Tier | Why |
|---|---|---|---|---|
| V-01 | **Self-serve setup** | A simple café takes its first live transaction within 60 min of unboxing. A 3-terminal, 2-KDS, 2-printer site goes live in ≤ 2 h without a vendor call. | M | [R05 R53], [R01 req 18] |
| V-02 | **Competitor importers** | Menus, catalogs, customers, gift card and loyalty balances from Toast, Square, Clover, Lightspeed and Shopify exports | 1 | [R01 req 18] |
| V-03 | **24/7 human support for P1** | "Can't take payments" or "kitchen down": median < 2 min to a human, one case ID, device telemetry visible to the agent | 1 | 🩹 [R05 R11], [R01 req 37] |
| V-04 | Proactive support | Telemetry detects printer, terminal and network problems and opens tickets before the merchant calls | 2 | |
| V-05 | **Month-to-month, $0 early-termination fee** | No liquidated damages; hardware bought outright or on a lease cancellable by returning it | M | 🩹 Toast, Shift4, Clover, SpotOn contracts [R01] |
| V-06 | **Price-change notice** | ≥ 90 days' notice for any price change, with a penalty-free exit; a single-page fee schedule | M | 🩹 Toast 30-day rate changes [R01] |
| V-07 | **Sunset guarantee** | ≥ 12 months' notice, no forced migration to a different product, security fixes throughout, contractual data escrow | M | 🩹 Revel frozen after acquisition; Clover forced migration [R01] |
| V-08 | **Leave anytime, take everything** | One-click full export in an open schema (incl. modifier trees, balances and liabilities), verified by a round-trip import test | M | [R01 req 35], [R02 req 30] |
| V-09 | Self-serve cancellation | Billing stops on the cancellation date | M | 🩹 billing after cancellation [R01] |
| V-10 | In-app diagnostics | One-tap "health check" bundle for support (with consent) | 1 | |

## W. Vertical packs

Vertical packs are **configurations of the core primitives plus a few specialized modules**, not forks.
Details and per-vertical research: [R06](../research/06-verticals-global.md).

| ID | Vertical | Key capabilities (beyond the core) | Tier |
|---|---|---|---|
| W-01 | Café and coffee | Speed layout, modifier-heavy drinks, mobile order-ahead with pacing, loyalty, subscriptions | M |
| W-02 | Quick service and fast casual | Kiosk, order-ready boards, pacing, combo builders | M → 2 |
| W-03 | Full-service restaurant | Floor, courses, splits, handhelds, pay-at-table, reservations | 1 |
| W-04 | Bars and nightlife | Tabs and pre-auth, speed screens, ID scanning, bottle service, walkout handling | 1 |
| W-05 | Specialty retail | Variants, serials, layaway, special orders, labels, clienteling | 1 |
| W-06 | Salons, spas and barbers | Appointments, deposits, commissions, product + service checkout | 2 |
| W-07 | Fitness and studios | Memberships, class packs, check-in, access control | 2 |
| W-08 | Bakeries and catering | Pre-orders, production planning, deposits, invoices | 2 |
| W-09 | Food trucks, markets and pop-ups | Single-device hub, cellular, offline, SoftPOS, event mode | 1 |
| W-10 | Grocery | Scales, PLU, GS1 variable measure, EBT/SNAP/WIC, bottle deposits, lane throughput, self-checkout | 3 |
| W-11 | Convenience and fuel | Age checks, lottery, forecourt integration (via certified partners) | 3 |
| W-12 | Liquor and age-restricted retail | ID scanning, hours restrictions, case discounts | 2 |
| W-13 | Stadiums, venues and events | Revenue-center partitioning, hub appliances, high-throughput handhelds, cashless and RFID wristbands | 3 |
| W-14 | Hotels and resorts | PMS room charge (OPERA, Mews, Cloudbeds and others), outlets, folio posting | 3 |
| W-15 | Cannabis | Seed-to-sale reporting, purchase limits, ID, cash-heavy operations | 3 |
| W-16 | B2B counter sales (hardware, auto parts) | House accounts, contractor price lists, quotes, special orders, POs at the counter | 2 |
| W-17 | Rentals | Serialized units, deposits, returns, damage | 3 |
| W-18 | Repair and service shops | Work orders, stages, parts and labor, notifications | 2 |
| W-19 | Apparel and footwear | Size runs, matrix grids, clienteling, alterations work orders | 1 → 2 |
| W-20 | Furniture and appliances | Special orders, delivery scheduling, financing (BNPL) | 3 |
| W-21 | Museums, attractions and nonprofits | Timed tickets, memberships, donations with receipts | 3 |
| W-22 | Campus and corporate dining | Student and employee ID tenders, meal plans, subsidies | 3 |

## X. Global readiness

| ID | Feature | Detail | Tier |
|---|---|---|---|
| X-01 | Localization framework | ICU messages, plurals, per-user locale, number, date and address formats | M |
| X-02 | RTL layouts and printing | Arabic, Hebrew and others in UI and on print (raster shaping) | 2 |
| X-03 | Multi-currency | Currency per location, exponent-correct money, FX for cash tenders | 1 |
| X-04 | Regional payment packs | Local methods per market (see I-14) | 2–3 |
| X-05 | Fiscal and e-invoicing packs | See R-02 | 2–3 |
| X-06 | Data residency | Tenant home region | 2 |
| X-07 | Market launch playbook | Legal, tax, fiscal, payments, language, support hours, hardware availability | 2 |
