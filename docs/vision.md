# Keel — Vision, Promises and Principles

> Status: **Draft v1** · Last updated: 2026-09-27
>
> *Keel* is a working codename. A keel is the backbone of a ship that keeps it upright in any
> weather.

## 1. The one-liner

**Keel is the point-of-sale platform that never stops selling, never locks you in, and runs any kind
of business, anywhere, as one system.**

## 2. Why the world needs another POS

The research ([summary](./research/README.md)) found an industry that has solved "take a card payment"
and little else reliably. The same failures show up across every major vendor and every vertical:

1. **Cloud dependence dressed up as "offline mode".**
   - Offline modes keep card payments alive but stop the business. Staff can't log in; loyalty, gift
     cards, close-out and reports stop; devices can't see each other.
   - Merchants carry all the risk of declined offline payments.
   - Single outages (a DNS change in 2023, a cloud region in October 2025, a ransomware attack on one
     data center in 2023) stopped thousands of businesses for hours or days.
2. **Lock-in as the business model.**
   - Mandatory in-house processing, often with penalty fees for using another processor.
   - Proprietary hardware and non-cancellable leases.
   - Two- to three-year contracts with termination fees or liquidated damages, and rate changes on 30
     days' notice.
   - Surprise fees, some billed to the merchant's own guests.
3. **Merchants stranded by corporate events.** Products are frozen after acquisitions, merchants are
   force-migrated, apps retired, hardware sunset, and multiple incompatible product lines run under
   one brand.
4. **Channel chaos.** A tablet per delivery marketplace, menus drifting between channels, 86s that
   don't propagate, and mobile orders flooding kitchens with no pacing.
5. **Thin depth, or ruinous complexity.** SMB tools cap variants, skip lot tracking, lack real
   promotions or inventory. Enterprise tools do everything but need integrators, run 10 GB offline
   databases, and take a day to propagate a setting.
6. **Compliance left to the merchant.** Tip pools, surcharging rules, fiscal devices, e-invoicing,
   SNAP eligibility, age checks, labor law and accessibility are handled with side systems and hope.
7. **Support that disappears at 8 pm on a Friday**, and billing disputes that outlast the contract.
8. **AI theater.** Chat boxes over reports, voice bots that need humans behind the curtain (one vendor
   faced SEC action for it), and pricing experiments that triggered consumer boycotts.
9. **Guests treated as a revenue surface.** Tip screens that make 27% of people tip less, QR-only
   menus that 90% dislike, receipts that silently sign people up for marketing.

No vendor is bad at everything. Many are excellent at something:
- Square's minutes-to-first-sale simplicity;
- Toast's restaurant depth;
- Shopify's single catalog, customer and order across channels;
- Lightspeed's inventory;
- Dynamics 365's promotion engine and cash office;
- Oracle Simphony's on-prem resilience;
- Odoo's unified loyalty model;
- Clover's offline card caps;
- Starbucks' order sequencing;
- Owner.com's commission-free growth engine.

**Keel's job is to combine those strengths in one coherent architecture and design out the failures.**

## 3. Who Keel serves

| Persona | What they need from a POS | Keel's answer in one line |
|---|---|---|
| **Owner / operator** | Keep selling, know the numbers, pay fair prices, leave if unhappy | Never-down selling, transparent pricing, no contracts, full export |
| **Manager** | Run a shift without firefighting | Remote approvals, pacing that protects the kitchen, reports that explain themselves |
| **Frontline staff** (server, bartender, cashier, barista) | Fast, obvious, forgiving tools | 10-minute onboarding, three taps to anything common, their own language |
| **Kitchen and production** | Tickets in the right place, at the right pace, readable | One queue across all channels, cook-time sequencing, allergens that can't be missed |
| **Bookkeeper / accountant** | Numbers that reconcile | A double-entry ledger, deposits matched to sales, 5-minute day close |
| **IT / franchise ops** | Fleet control, safe changes, no surprises | Staged rollouts, config-as-code, governance with bounds, health telemetry |
| **Developer / partner** | Open, stable, powerful APIs | API parity, offline-capable extensions, a local store API, free access to merchant data |
| **Guest / customer** | Speed, fairness, respect | Honest tipping, optional QR, receipts that aren't marketing consent, accessible self-service |

## 4. The Keel promises

These are product commitments with measurable targets. They are the acceptance criteria for the
whole program.

| # | Promise | Measurable target |
|---|---|---|
| 1 | **Never stop selling** | In-store functions all work with no internet, no Keel Cloud, no hub or no processor, within documented bounds. Soak-tested for 72 h offline with zero lost or duplicated documents. |
| 2 | **Instant** | < 50 ms p99 from tap to UI locally. Order to KDS in < 250 ms p99. No spinners on the sales path. |
| 3 | **No lock-in, ever** | Any supported processor with no penalty; commodity hardware; month-to-month with a $0 early-termination fee; one-click full export, round-trip verified. |
| 4 | **Honest money** | No fees on guests, ever. 90 days' notice of price changes. Fees shown in dollars. Holds explained, with an appeal path. |
| 5 | **Learn in minutes** | A new server rings a 4-top with a split and a comp after ≤ 10 minutes of training. A simple café makes its first live sale within 60 minutes of unboxing. |
| 6 | **One system for everything you sell** | Food, retail, services, rentals, memberships and events, in one checkout, one customer record and one set of books, across every channel. |
| 7 | **Every channel in sync** | One catalog and one kitchen queue. An 86 reaches every channel in ≤ 10 s. Kitchen-capacity pacing across channels. |
| 8 | **Compliant anywhere** | Tax, fiscal, labor, accessibility, privacy and payments rules shipped as signed data, updated without app releases. |
| 9 | **Programmable** | Public API parity, webhooks, a local store API, WASM Functions that run offline, UI extensions, automations, and an MCP server for AI agents. |
| 10 | **AI that does real work, safely** | Every AI number cites its source. Actions are approval-gated with undo. Autonomy is set per capability by the owner. Automation rates are published honestly. |
| 11 | **Humans when it matters** | 24/7 human support for "can't take payments" or "kitchen down", with median time-to-human < 2 minutes, including Friday night. |
| 12 | **No forced migrations** | ≥ 12 months' notice for any sunset, no forced product switch, ≥ 24-month API version support, ≥ 5-year hardware support. |

## 5. Principles

### Product and UX
1. **Offline is invisible.** Staff shouldn't need to know whether the internet is up. Where
   capability genuinely degrades, the UI says exactly what and why.
2. **Money is sacred.** No fee, hold, price change or refund happens without a visible reason and a
   human path. Every amount is explainable to the cent.
3. **The kitchen sets the pace, not the channel.** Digital channels adapt to production capacity,
   never the other way around.
4. **Never make the guest do the business's work,** unless it gives the guest more control (kiosks:
   yes; forced QR menus: no).
5. **Three taps to anything common during service.** Power features are progressive and never in the
   way.
6. **Every automated decision can be explained and undone.**
7. **Defaults are ethical.** Tips off by default at counters, marketing consent never implied,
   personalized upward pricing blocked. Merchants can change settings, but Keel never makes the
   worst choice the easiest one.
8. **Accessible by default.** WCAG 2.2 AA for staff and guest interfaces. EAA-grade self-service.

### Engineering
1. **The store is primary for the store; the cloud is primary for history.** The local-first
   architecture is the foundation, not a feature ([offline-and-sync](./architecture/offline-and-sync.md)).
2. **One kernel, everywhere.** The same Rust core computes prices, taxes and state on every device,
   hub and cloud node. There is no "the cloud says a different total".
3. **Facts are immutable.** An event-sourced, signed, hash-chained history of everything that touched
   money.
4. **Compliance is data, not code.** Rule packs are versioned, signed, effective-dated and
   rollback-able.
5. **Compose verticals; never fork.** Fifteen generic primitives cover twenty-five verticals.
6. **Own the core, integrate the edges.** Keel owns what defines reliability and correctness (kernel,
   sync, pricing, ledger). It integrates processors, marketplaces, payroll, accounting and ERPs through
   open adapters.
7. **Prove it in simulation.** Sync, payments and ledger correctness are verified by deterministic
   simulation with fault injection, not by hope.
8. **Blast radius is a design parameter.** Cells, staged rollouts, update windows and kill switches.
   No change, whether code, config, content or rules, reaches the whole fleet at once.

## 6. Non-goals

- **We won't require our own payment processing.** Keel Payments exists as an option with published
  pricing. It's never a condition, and never discounted in exchange for a long contract.
- **We won't build proprietary, locked hardware.** We certify commodity devices and offer a reference
  hub appliance.
- **We won't add fees to guests' bills**, or build surge or personalized upward pricing.
- **We won't build store-wide camera checkout.** We'll integrate bounded computer-vision checkout
  (trays, venues) through partners.
- **We won't claim AI automation we don't deliver.** Voice and phone AI always report no-intervention
  and handoff rates.
- **We won't become an ERP, payroll provider or accounting system** in the first phases. We'll keep a
  best-in-class commerce ledger and integrate deeply with the systems merchants already use.
- **We won't run a marketplace that charges merchants a commission on their own customers.**

## 7. Business model (sketch, for alignment)

- **Software**: transparent per-location plans with published prices. The first register is
  inexpensive, and add-on creep is avoided by bundling essentials. AI features are included in the
  base plan, subject to fair use.
- **Payments (optional)**: interchange-plus pricing with a published markup, and instant payouts with
  the fee shown in dollars. Merchants can bring their own processor at no penalty.
- **Hardware**: certified bundles and the hub appliance near cost, bought outright. Any lease is
  cancellable by returning the hardware.
- **Platform**: marketplace revenue share that's generous to developers. API access to a merchant's
  own data is always free.

> The strategic bet: merchants stay because Keel is better, not because leaving is expensive. A
> platform that is reliable, open and honest wins on retention in a market where the dominant
> complaint is feeling trapped.

## 8. Success metrics

| Type | Metric | Target |
|---|---|---|
| North star | **Merchant-hours with full selling capability** (measured at the edge) | ≥ 99.999% |
| Reliability | Sales lost to Keel-attributable failures | 0 |
| Speed | p99 local interaction latency; order-to-KDS latency | < 50 ms; < 250 ms |
| Adoption | Time from signup to first live sale (self-serve) | < 60 min median |
| Staff | Time to competence for new staff | ≤ 10 min |
| Support | P1 median time-to-human | < 2 min, 24/7 |
| Trust | Share of churned merchants who cite "trapped" or billing disputes | ~0 (qualitative review of every churn) |
| AI | Draft acceptance rate; forecast error; time saved per week per location | Per capability; published internally |
| Platform | Active third-party apps and Functions; webhook delivery success | Growth; ≥ 99.99% delivered within 72 h |
