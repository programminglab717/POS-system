# Restaurant and Hospitality POS: Market and Product Research (Track 01)

Prepared 2026-09-27 as input to the architecture and feature-list phase of a new POS platform.

---

## 0. How to read this report

**Evidence tags**

- A claim followed by a markdown link was found in this session through web search. The link points to the page the search engine summarized. Many pricing figures come from third-party review sites or from competing processors' blogs rather than from the vendor, and are labeled "third-party" where that matters.
- A claim marked **†** comes from the researcher's background knowledge (training data through mid-2026) and was **not** re-verified in this session. Treat † items as leads to confirm, not as facts to put into a contract, a price sheet, or marketing copy.

**Constraints that shaped this report**

- The environment's egress proxy blocked full-page fetching for every domain tried (toasttab.com, squareup.com, docs.oracle.com, nrn.com, wikipedia.org, reddit.com). The evidence therefore rests on search-engine summaries of the cited pages, not on full reads.
- The session-wide web-search budget ran out after 27 searches by this agent. Reddit threads could not be opened directly. For operator voice on the 2023 Square outage, Square's own community forum stands in.
- Vendors covered only at † depth: PAR Brink, Qu, Lavu, Epos Now, HungerRush, Owner.com, Olo, Sunday, me&u, Zonal, orderbird, Restroworks, Petpooja, Foodics.
- A follow-up pass with fetch access should prioritize four things: the full Toast, Square and Clover help-center pages on offline limits; G2 and Capterra complaint distributions; Reddit operator threads on the October 2025 AWS outage; and pricing for the non-US vendors.

---

## 1. Landscape

### 1.1 Major US and global restaurant POS vendors (evidence-backed)

| Vendor | Segment | Pricing model (software + processing) | Hardware model | Lock-in | Top strengths | Top weaknesses |
|---|---|---|---|---|---|---|
| **Toast** | US SMB to mid-market: full-service, quick-service, bars. Enterprise tier exists† | Starter Kit at $0/mo with 3.09% + 15¢ per transaction. "Point of Sale" plan at $69/mo with 2.49% + 15¢. Card-not-present about 3.50% + 15¢ ([Labrador, third-party](https://www.labrador.ai/blog/toast-pos-fees-2026), [POSUSA](https://www.posusa.com/toast-pos-pricing/)). Payroll, marketing, loyalty and Tables modules cost extra† | Proprietary Android terminals, Toast Go handhelds, KDS screens. Hardware only works with Toast† | Toast Payments only†. Two-year terms are typical and some promotions need three. The early-termination fee is the remaining subscription; some guides say $495 plus the remaining contract value. Third-party readings of the merchant agreement say processing rates can change on 30 days' notice ([Merchant Insiders](https://merchantinsiders.com/blogs/toast-fees/), [Sleft Payments](https://www.sleftpayments.com/learning-hub/toast-pos-raised-fees-options-2026)) | Broadest native suite. Handhelds. Offline mode keeps order entry, card payments and KDS routing running. Optional LAN "local sync" hub | Processing lock-in and renewal hikes (a competing processor reports 2026 renewals 15%+ above 2023 pricing). Standard offline mode has no device-to-device sync. Loyalty, gift cards and shift review are unavailable offline. Hit by the AWS us-east-1 outage in October 2025 |
| **Square for Restaurants** | SMB counter service, cafés, small full-service | Free plan; Plus at $49 per location per month; Premium at custom pricing. In person: 2.6% + 15¢ on Free, down to 2.4% + 15¢ on Premium. Sources disagree on the Plus rate; one lists 2.5% + 15¢ ([NerdWallet](https://www.nerdwallet.com/business/software/reviews/square-for-restaurants), [UpMenu](https://www.upmenu.com/blog/square-pricing/)) | Square Register, Terminal, Stand (bring your own iPad), Handheld and Kiosk†. Square-only processing | Month-to-month with no early-termination fee†. Processing is locked to Square | Published pricing. Self-serve onboarding. CSV exports of customers and the item library†. Large app ecosystem† | 14-hour outage in September 2023. Offline payments allowed for 24 h with a 72 h hard stop, and the seller is liable for declines. In 2023, offline mode did not engage on its own. Less depth for complex full-service† |
| **Clover (Fiserv)** | SMB across many verticals. Sold directly and through banks and independent sales organizations (ISOs)† | Varies by reseller; not verified in this session | Proprietary Android devices (Station, Mini, Flex)†. Fiserv processing | Reseller contracts and equipment leases†. Plaintiffs allege fees "buried in complex contracts" ([PaymentPop](https://paymentpop.com/merchant-accounts/clover-pos-customer-reviews/)) | Most generous offline card policy found: up to 7 days, with default caps of $500 per transaction and $5,000 total, both configurable ([Clover docs](https://docs.clover.com/dev/docs/handling-offline-payments), [Limelight](https://www.limelightpayments.com/blog/how-to-take-clover-pos-payments-during-internet-outages)) | Up to 200,000 merchants on Fiserv's Payeezy gateway were forcibly migrated to Clover from late 2023 through H1 2024, followed by "very significant churn" and investor suits ([Payments Dive](https://www.paymentsdive.com/news/fiserv-sued-over-clover-migration/754200/), [PYMNTS](https://www.pymnts.com/earnings/2025/fiserv-shares-fall-clover-growth-slows-merchant-payments-processing-revenues-dip/)) |
| **Lightspeed Restaurant (K-Series)** | SMB to mid-market full-service, bars, hotel outlets. Strongest in Europe† | US tiers as of July 2026 (third-party): Starter $69, Essential $189, Premium $399 per month; Enterprise custom. Lightspeed Payments: 2.6% + 10¢ in person, 2.9% + 30¢ online ([UpMenu](https://www.upmenu.com/blog/lightspeed-pos-pricing/), [Lightspeed pricing](https://www.lightspeedhq.com/pos/restaurant/pricing/)) | Bring-your-own iPad or iPhone, plus certified peripherals | Lightspeed Payments is presented as the standard; third-party write-ups imply it is required (not confirmed) | Runs on the merchant's own iPads. Multiple revenue centers. Raw API access on Premium. Offline order-taking and kitchen routing ("TrueSync") | Offline card payments are "not guaranteed". Needs 10 open ports and wireless client isolation turned off ([K-Series networking](https://k-series-support.lightspeedhq.com/hc/en-us/articles/16154347413275-Networking-for-Lightspeed-Restaurant)). Three separate restaurant product lines (K-, L- and O-Series) |
| **TouchBistro** | iPad full-service SMB, North America | From $69/mo. Online ordering adds $50/mo. Annual contracts. TouchBistro Payments (powered by Chase) required for new customers ([POSUSA](https://www.posusa.com/touchbistro-pos-review/), [business.com](https://www.business.com/reviews/touchbistro/)) | iPads plus an on-site Mac server | Annual contract plus mandatory payments | Hybrid local server keeps service running with no internet ([loman.ai](https://loman.ai/blog/touchbistro-review)) | Slow support, billing that continued after cancellation, rigid contracts ([Trustpilot](https://www.trustpilot.com/review/touchbistro.com), [B2B Reviews](https://www.b2breviews.com/reviews/touchbistro/)) |
| **SpotOn** | US SMB full-service, quick-service and bars; also venues† | "All-In" plan: $0 per station per month with 2.79% + 20¢ and a two-year minimum. "POS Essentials" plan is month-to-month. Hardware bundles from about $700 ([tech.co](https://tech.co/pos-system/spoton-pos-review), [POSUSA](https://www.posusa.com/spoton-restaurant-pos-review/)) | Vendor-supplied bundles | Two-year term on the $0 plan. A $995 fee to switch processors has been reported ([Rezku](https://rezku.com/blog/top-spoton-pos-alternatives-for-restaurants-in-2026/)) | Bundles POS, marketing and labor tools†. Handhelds† | Billing-discrepancy complaints recur across BBB and Trustpilot. Complaints that deposits arrive about two days later because of a 6 pm cutoff ([Sonary](https://sonary.com/b/spoton/spoton+pos/)) |
| **Shift4 Dine (formerly SkyTab)** | SMB full-service and bars | Headline price $29.99/mo ([POSUSA](https://www.posusa.com/skytab-pos-system/)) plus Shift4 processing | Proprietary SkyTab terminals and handhelds | Multi-year processing agreements with liquidated damages. Third-party illustration: $37,500 to leave at month 13 of a 36-month contract for a restaurant processing $50k/month ([Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/shift4-review/)) | Low sticker price. Fast growth: active merchants up more than 40% year over year ([POS Insider](https://www.posinsider.com/blog/pos-pulse-weekly-briefing-the-brand-reset-is-on-shift4-retires-skytab-global-payments-doubles-down-on-genius-may-13-2026)) | BBB average of 1.02 stars ([BBB](https://www.bbb.org/us/pa/center-valley/profile/credit-card-processing-services/shift4-0241-235983222/complaints)). A $45,000 cancellation fee disputed after a merchant cancelled within a 30-day trial ([Reforming Retail](https://reformingretail.com/index.php/2025/12/30/to-catch-a-predator-shift4-termination-fees/)). Surprise fees such as $150/mo "premium support" ([PaymentPop](https://paymentpop.com/merchant-accounts/skytab-customer-reviews/)) |
| **Revel Systems (Shift4)** | iPad POS for quick-service and multi-unit operators | Legacy contracts. New sales go to Shift4 Dine | iPad | Shift4 processing | Mature multi-unit iPad feature set† | Acquired for $250M; the deal closed June 2024 with about 18,000 locations ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/payment-processor-shift4-acquire-revel-systems-250m), [PYMNTS](https://www.pymnts.com/acquisitions/2024/shift4-acquires-majority-stake-in-vectron-completes-purchase-of-revel/)). Bug fixes reportedly stopped. Merchants are being converted to Shift4 Dine per the August 2026 earnings call ([KORONA](https://koronapos.com/blog/what-happened-to-revel-pos/), [Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/revel-pos-merchant-updates/)) |
| **Oracle MICROS Simphony** | Enterprise chains, hotels, venues | Quote-based subscription† | Oracle workstations or supported third-party hardware† | Choice of processor through integrations†. Heavy dependence on integrators† | An on-premises Check and Posting Service (CAPS) leaves workstations "largely unaffected" by WAN outages ([Oracle CAPS](https://docs.oracle.com/en/industries/food-beverage/simphony/simcg/c_caps.htm)). Hierarchical enterprise configuration† | Cost and complexity†. Enterprise-grade (dated) UX† |
| **NCR Voyix Aloha** | Multi-unit full-service and quick-service | Quote† | Windows terminals plus an in-store server† | Varies† | In-store server architecture tolerates loss of the internet link† | An April 2023 ransomware attack on one NCR data center took Aloha's cloud services (back office, gift cards, the Pulse dashboard) down for days ([Cybersecurity Dive](https://www.cybersecuritydive.com/news/ncr-pos-ransomware-recovery/648005/), [SiliconANGLE](https://siliconangle.com/2023/04/17/ransomware-attack-causes-outages-payments-giant-ncr/)) |

### 1.2 Specialists, enterprise and non-US vendors (lower evidence depth, mostly †)

| Vendor | Segment | Pricing / model | Hardware | Lock-in | Strengths | Weaknesses / notes |
|---|---|---|---|---|---|---|
| PAR (Brink POS) | Enterprise quick-service and fast casual† | Quote† | Various† | Also sells PAR Pay, Punchh loyalty (acquired 2021†) and MENU ordering (2022†) | Cloud-native enterprise POS with its own loyalty and ordering stack† | Not verified this session |
| Qu POS | Enterprise quick-service and fast casual† | Quote† | Various† | n/a | "Unified commerce" across POS, kiosk, online and drive-thru† | Not verified |
| Lavu | iPad SMB restaurants† | Subscription plus processing† | iPad† | n/a | n/a | Not verified |
| Epos Now Hospitality | UK and US SMB† | Hardware bundle plus subscription† | Vendor bundles† | Contract and finance terms† | n/a | Not verified |
| HungerRush | Pizza and delivery chains (formerly Revention)† | Quote† | n/a | n/a | Pizza builder and delivery dispatch† | Not verified |
| Owner.com | First-party online ordering, website, app, automated marketing† | Flat monthly fee (about $499/mo†), no commission† | None | Low | Commission-free ordering plus SEO and automated marketing for independents† | Not a POS; depends on POS integrations |
| Olo | Enterprise digital ordering: Ordering, Rails (marketplace injection), Dispatch (routing to delivery providers), Pay† | Per order or subscription† | None | Enterprise contracts† | Marketplace injection and throttling at chain scale† | Taken private by Thoma Bravo (about $2B, announced 2025†) |
| Sunday | QR pay-at-table (Paris)† | Per transaction† | Guest phones | Low | Pay-at-table QR flow† | Scaled back after its 2021–22 hypergrowth† |
| me&u / Mr Yum | QR order-and-pay (AU, UK, US)† | Per transaction† | Guest phones | Low | Group ordering onto one tab† | Diner backlash against QR-only service and against fees† |
| Zonal | UK EPoS for pub and restaurant groups† | Quote† | n/a | n/a | Order-at-table apps for large pub estates† | Not verified |
| orderbird | German iPad POS (Nexi)† | Subscription† | iPad | n/a | Built-in German fiscalization (the TSE security module)† | Not verified |
| Restroworks (formerly Posist) | Cloud platform for chains in India and elsewhere† | Subscription† | n/a | n/a | Central kitchen and commissary plus multi-outlet management† | Not verified |
| Petpooja | Indian SMBs and chains† | Low annual subscription† | Windows and Android† | n/a | Zomato and Swiggy orders land inside the POS; strong offline operation† | Not verified |
| Foodics | Saudi Arabia and MENA† | Subscription† | iPad† | n/a | ZATCA e-invoicing compliance; delivery-aggregator integrations† | Not verified |

### 1.3 What the landscape says

1. **Processing is the business model.** Toast, Square, Clover, SpotOn and Shift4 Dine all tie the device to their own processing. Lightspeed and TouchBistro allow bring-your-own iPads but require their payments (TouchBistro for new customers; Lightspeed per reviewers). Only the enterprise suites (Simphony, Aloha†) are commonly processor-agnostic.
2. **Published in-person rates cluster at 2.4–3.09% plus 10–20¢.** Card-not-present rates run 2.9–3.5% plus 15–30¢. Software fees range from $0 to $399 per location per month. The $0 plans carry the highest rates and the longest terms: Toast Starter at 3.09% + 15¢, and SpotOn All-In at 2.79% + 20¢ with a two-year minimum.
3. **Two architectures.**
   - Cloud-first with an offline fallback: Toast, Square, Clover, Lightspeed K, Shift4 Dine.
   - Local-server hybrids: TouchBistro's Mac server, Simphony's CAPS, Aloha's in-store server†, and Lightspeed L-Series' "LiteServer" ([Lightspeed L-Series](https://resto-support.lightspeedhq.com/hc/en-us/articles/5218871669403-Troubleshooting-the-LiteServer)).

   The hybrids degrade more gracefully but cost more to install and maintain.
4. **Consolidation creates instability for merchants.** Revel was folded into Shift4, and SkyTab was renamed Shift4 Dine on May 12, 2026 ([Shift4 release notes](https://releasenotes.shift4.com/announcements/skytab-is-becoming-shift4-dine)). Clover force-migrated Payeezy merchants. Reservation networks now belong to payment and delivery giants: Resy and Tock to American Express†, SevenRooms to DoorDash (2025)†. Olo went private†.

---

## 2. Best-in-class feature catalog

For each functional area: what "best" concretely looks like, and where the strongest implementations are. Vendor attributions without links are †.

### 2.1 Offline operation and resilience (the most differentiating area)

| Vendor | What keeps working offline | Device-to-device sync while offline | Offline card window and caps | What stops | Evidence |
|---|---|---|---|---|---|
| Toast (standard offline) | Order entry, card payments, printing and KDS routing, scales for weighed items, receipt printing | **None.** "Orders added or updated on one device do not appear on other devices." Toast advises one employee per device | Authorizations "may expire as early as 24 hours" after the transaction | Loyalty accrual and redemption; gift card activation and redemption; shift review; logging in (staff are told not to log out, because they cannot log back in). Staff are told to keep paper receipts and merchant copies for reconciliation and chargebacks | [Toast Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode), [Toast prep guide](https://support.toasttab.com/en/article/Prepare-to-Operate-in-Offline-Mode-During-Service-Disruptions-or-Outages) |
| Toast (offline with local sync) | Same as above, plus orders shared across devices | Through an auto-assigned **local hub**. Needs at least one *hardwired, non-Elo V1* Toast device. The hub only talks to devices on its own subnet | Same as above | The hub cannot send orders to the Toast cloud while offline | [Toast docs](https://doc.toasttab.com/doc/platformguide/platformOfflineModeLocalSync.html) |
| Square | Card payments in offline mode, if enabled in advance | Not verified | Payments accepted for 24 h and must be uploaded within 72 h of the start of the offline session. Seller is liable for expired, declined or disputed payments. Square "cannot provide customer contact details" for them | Cloud-dependent features† | [Square Support](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode), [Devicefield](https://devicefield.com/blog/square-offline-mode) |
| Clover | Card payments, if enabled in advance | Not verified | Up to 7 days (not changeable). Defaults of $500 per transaction and $5,000 total, both configurable | Nothing works unless enabled before the outage | [Clover docs](https://docs.clover.com/dev/docs/handling-offline-payments), [Limelight](https://www.limelightpayments.com/blog/how-to-take-clover-pos-payments-during-internet-outages) |
| Lightspeed K-Series | Order taking and sending to kitchen and bar. Data stored locally and synced back ("TrueSync") | Yes, over the LAN, which needs the listed ports open | Offline card payments "not guaranteed" | "Payments, reporting, and cloud-based services may pause" | [POSUSA](https://www.posusa.com/lightspeed-restaurant-stuck-offline-mode/), [K-Series networking](https://k-series-support.lightspeedhq.com/hc/en-us/articles/16154347413275-Networking-for-Lightspeed-Restaurant) |
| TouchBistro | Full POS through the on-site Mac server; iPads talk over the local network | Yes, through the server | TouchBistro Payments has a "built-in offline mode" | Cloud reporting until sync | [POSUSA](https://www.posusa.com/touchbistro-pos-review/), [loman.ai](https://loman.ai/blog/touchbistro-review) |
| Oracle Simphony | During a WAN outage, workstations keep posting checks to the on-premises CAPS. Oracle states "Team Service operations are available when CAPS is offline" | Yes, through CAPS | Depends on the processor† | Enterprise-level functions until reconnection | [Oracle CAPS](https://docs.oracle.com/en/industries/food-beverage/simphony/simcg/c_caps.htm), [Oracle online/offline modes](https://docs.oracle.com/cd/F32325_01/doc.192/f32329/c_workstation_online_offline_modes.htm) |
| NCR Aloha | In-store server runs the terminals† | Yes† | Depends on the processor† | In 2023: cloud back office, gift cards and the Pulse dashboard were lost for days | [SiliconANGLE](https://siliconangle.com/2023/04/17/ransomware-attack-causes-outages-payments-giant-ncr/) |

**What best looks like, and the lessons**

1. **Use a local authority tier, not devices fending for themselves.** Simphony (CAPS), TouchBistro (Mac server), Aloha (in-store server†) and Toast's local hub all converge on one local coordinator. The cost is a single point of failure plus topology rules: Toast requires a hardwired device of a specific hardware generation on the same subnet. The better design is a peer mesh with automatic leader election, where any device can become coordinator.
2. **Detect degraded service, not just a lost link.** During Square's September 2023 outage, devices still had internet access but Square's services were failing. Sellers had to pull the Ethernet plug or turn off Wi-Fi to force offline mode ([Square Community](https://community.squareup.com/t5/Troubleshooting/How-did-people-get-Offline-Mode-to-work-thru-the-outage/m-p/681363)). Square then deferred processing of offline payments for hours "as a precautionary measure" ([Square incident summary](https://developer.squareup.com/blog/incident-summary-2023-09-07/), [Square press](https://squareup.com/us/en/press/an-update-on-last-weeks-outage)).
3. **Offline card acceptance is a risk-policy problem.** Windows range from about 24 h (Toast authorization expiry) through 72 h (Square hard limit) to 7 days (Clover). Only Clover exposes explicit per-transaction and aggregate caps. No vendor found offers rules by card number range (BIN), for example refusing prepaid or foreign cards offline. None offers a live offline-exposure dashboard. None captures guest contact details so that a later decline can be recovered, and Square says it cannot supply those details ([Square Support](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode)).
4. **The offline blind spots sit next to money.** At Toast, loyalty, gift cards, shift review and login all fail offline ([Toast Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode)). Those are exactly what staff need at close.
5. **Cloud concentration is real.** In the October 20, 2025 AWS us-east-1 outage, many restaurants on Toast saw "complete system failures, disrupted scheduling systems, problems with waitlist management". JD's Hamburgers in Fort Worth had to add its transactions manually ([NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling)). Most vendors run scheduling, waitlist and online ordering only in the cloud.
6. **Complex site networks break offline modes.** Lightspeed K-Series needs ports 22, 80, 443, 7373, 8080, 8883, 9140, 8443, 9100 and 9880 open, and client isolation disabled ([Lightspeed](https://k-series-support.lightspeedhq.com/hc/en-us/articles/16154347413275-Networking-for-Lightspeed-Restaurant)). Toast's hub ignores devices on other subnets. Guest Wi-Fi isolation and VLAN splits are common in restaurants, so these rules fail silently in the field.

### 2.2 Front of house

- **Menus and nested modifiers.** Reference behavior:
  - Modifier groups with minimum and maximum selections, forced or optional.
  - Nesting at least three levels deep, e.g. Burger → Side → Fries → Size → Seasoning.
  - Size-dependent pricing (a price matrix) and pre-modifiers: No, Light, Extra, On the side.
  - Portion pricing for pizza: whole, half, quarter.
  - Allergen tags that follow the item to the KDS and the receipt.

  Strongest implementations: Toast (nested groups, portion-based pizza pricing)†, HungerRush (built pizza-first)†, and Simphony and Aloha (deep condiment hierarchies)†. Square historically offered only single-level modifiers† (check the current state).
- **Coursing and firing.** Each item carries a course. Staff hold and fire by course from a terminal or handheld, and the KDS shows held courses greyed out. Every full-service vendor supports manual firing†. What is rare is timing-aware auto-fire, such as "fire course 2 N minutes after course 1 is bumped" or "fire so all items finish together"; this shows up in enterprise KDS layers such as QSR Automations†.
- **Seats, split, merge, transfer.** Reference behavior:
  - Seat-level ordering.
  - Split by seat, item, even N ways, or custom amount, including fractional splits of a single item.
  - Merge and transfer of checks, items and tables between servers, with an audit trail.

  TouchBistro's drag-and-drop split screen is widely cited as the easiest†.
- **Tabs and pre-authorization for bars.**
  - Open a tab by dipping or tapping a card; the name comes from the card.
  - Configurable pre-authorization with incremental re-authorization as the tab grows.
  - Auto-close at a set time under a disclosed gratuity policy.
  - A walkout report.

  Card networks let restaurants adjust for tips only within a tolerance over the authorized amount (commonly cited as about 20%†), so sizing the pre-authorization matters.
- **Comps, voids and discounts.**
  - Reason codes and dollar or percent thresholds.
  - Manager approval by PIN, badge or remote push.
  - Voids after an item has fired are recorded as waste.
  - Live alerts, plus loss-prevention analytics (voids after payment, "no sale" drawer opens).

  The enterprise privilege models (Simphony, Aloha)† are the most granular.
- **Handhelds.** Toast Go, Shift4 Dine handhelds, Square Handheld†, SpotOn†, and Lightspeed on iPhone. The best keep full parity with the terminal (coursing, splits, approvals), read cards on the device for pay-at-table, and work offline.

### 2.3 Floor plans, table management, reservations and waitlist

- **Reference behavior.**
  - A live floor map with a timer on each table state: seated, ordered, entrée fired, check dropped, paid.
  - Server sections and a seating rotation that balances covers.
  - Waitlist quotes learned from actual turn times by party size, with SMS paging.
  - Reservations with deposits and no-show fees.
  - A guest profile showing allergies, notes and lifetime spend from the POS.
- **Strongest.** SevenRooms (guest CRM, auto-tags, POS spend in the profile)†. OpenTable and Resy (network demand)†. Native waitlists: Toast Tables, SpotOn Reserve, Yelp Guest Manager†.
- **Market structure risk.** Resy and Tock belong to American Express†, SevenRooms to DoorDash†, and OpenTable to Booking Holdings†. Guest data sits with platforms that have their own agendas.
- **Evidence of cloud dependency.** Waitlist management failed at Toast restaurants during the AWS outage ([NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling)).

### 2.4 Kitchen display systems

Reference behavior (an amalgam of the strongest KDS products†):

- **Routing.**
  - Item-to-station rules, with modifier-driven routing (for example, "add side salad" goes to the cold station).
  - Filters by channel and order type.
  - An expo screen that consolidates the order.
  - Separate expo views for dine-in and to-go.
- **Timing.**
  - Per-item cook times.
  - Cook-time sequencing, so a 12-minute steak and a 4-minute salad finish together.
  - Course holds.
  - Colored thresholds by ticket age.
  - Average ticket time per station.
- **Throughput tools.** "All-day" counts (the total of each item across open tickets), batch views, recall of the last bumped ticket, park, and rush or priority.
- **Input and hardware.** Bump bars as well as touch. Heat- and grease-tolerant screens.
- **Safety.** Allergens and "No/Extra" modifiers rendered in a distinct color. Free-text requests highlighted.
- **Throttling.** Off-premise intake pauses, or quoted times lengthen, based on the live queue.
- **Offline autonomy.** Toast documents special "Offline Autofire devices" ([Toast docs](https://doc.toasttab.com/doc/platformguide/platformOfflineAutofireDevices.html)); only the title was seen this session.

Strongest: QSR Automations ConnectSmart (enterprise; cook-time sequencing and load balancing)†. Fresh KDS (iPad; bump bars; all-day counts)†. Simphony KDS†. Toast KDS, which is tightly coupled to online-order status updates†.

### 2.5 Online ordering and delivery ("tablet hell")

- **Reference behavior.**
  - Marketplace orders injected straight into the POS and KDS, with no tablets.
  - Two-way menu and availability sync.
  - Order status pushed back to the marketplace.
  - Per-order payout reconciliation covering commission, promotion funding, adjustments and error charges.
  - Commission-free first-party ordering on web and app.
  - First-party delivery routed to on-demand delivery providers (DoorDash Drive, Uber Direct)†.
  - Throttling by kitchen capacity, with dynamic prep times.
  - A single 86 (sold-out) state that reaches every channel.
- **Strongest.**
  - Enterprise: Olo, with Rails for marketplace injection, Dispatch for delivery routing, and throttling†.
  - Middleware: Otter, Deliverect, Chowly, ItsaCheckmate†.
  - Native marketplace integrations: Toast, Square†.
  - Independents' first-party ordering and marketing: Owner.com†.
  - Indian aggregators straight into the POS: Petpooja†.
- **Economics that drive requirements.** Marketplace commission tiers of 15%, 25% and 30% (DoorDash Basic/Plus/Premier; Uber Eats Lite/Plus/Premium)†. City caps such as New York City's permanent 15% delivery / 5% other / 3% processing, and San Francisco's 15%†. Toast's attempt to add a 99¢ diner fee to online orders over $10 (February 2023) was withdrawn within weeks after backlash†. Merchants want control over diner-facing fees.

### 2.6 Menu management across locations and channels

- **Reference behavior.**
  - One master catalog with inheritance: brand → region → store → revenue center → channel, with overrides at any level.
  - Channel-specific prices (for example, a marketplace markup), time-based price levels (happy hour) and dayparts.
  - Changes staged with preview and scheduled publishing, plus an audit log and one-click rollback.
  - Bulk edit and import/export.
- **Strongest.** Simphony's Enterprise Management Console, with hierarchical inheritance and overrides†. Toast Enterprise publishing and menu versions†. Square's item library with per-location availability and prices†. Lightspeed Premium for multiple revenue centers ([UpMenu](https://www.upmenu.com/blog/lightspeed-pos-pricing/)).
- **Common gap.** Marketplace menus drift from the POS menu, and nested modifiers map poorly onto marketplace schemas†.

### 2.7 Inventory, recipe costing and waste

- **Reference behavior.**
  - Recipes and sub-recipes with yields and unit conversions.
  - Modifier-level depletion ("add bacon" depletes bacon).
  - Invoice OCR that updates ingredient costs.
  - Actual-versus-theoretical food-cost variance.
  - Mobile count sheets, par-based ordering, and transfers between stores and commissary.
  - Waste logging, including voids after an item has fired.
- **Strongest.** Specialist back-office tools rather than POS vendors: MarginEdge, xtraCHEF (Toast), Restaurant365, MarketMan, Craftable, Apicbase (EU)†. Restroworks and Petpooja bundle central-kitchen and inventory tools for Indian chains†. Inventory built into POS products is usually shallow†.

### 2.8 Labor: scheduling, clock-in, breaks, tip pooling and tip-out, payroll

- **Reference behavior.**
  - Schedules built from sales forecasts, with shift swaps and availability.
  - Live labor cost as a percentage of sales, and overtime alerts.
  - Clock-in that selects role and wage, with break attestations and rules per jurisdiction: California's meal break before the end of the fifth hour†; predictive-scheduling ordinances in NYC, SF, Seattle, Chicago, Philadelphia, Oregon and LA†; limits on minors.
  - Tips: pools weighted by hours × role points; tip-outs as a percentage of a sales category (for example, a share of alcohol sales to bartenders); cash-tip declaration; service charges kept separate from tips; instant payouts; payroll export.
- **Compliance anchors.**
  - Federal wage law (FLSA, as amended in 2018): managers and supervisors may not keep tips, and back-of-house staff may join pools only when no tip credit is taken†.
  - The "One Big Beautiful Bill Act" (July 2025) created a federal deduction for qualified tips of up to $25,000 a year for 2025–2028, with new employer reporting duties by occupation†. The POS must separate voluntary tips from mandatory service charges.
  - Illinois' biometric privacy law (BIPA): in *Cothron v. White Castle* (2023), each fingerprint scan counts as a separate violation†. Fingerprint clocks are a liability.
- **Evidence that tip management is valuable and contested.** A federal judge let Gratuity Solutions' trade-secret suit against Toast proceed. The suit alleges Toast encouraged members of its customer advisory board (who were Gratuity customers) to obtain configurations of Gratuity's PayDayPortal tip-management system ([Bloomberg Law](https://news.bloomberglaw.com/litigation/toast-will-have-to-face-lawsuit-over-alleged-trade-secret-theft), [Bloomberg Law IP](https://news.bloomberglaw.com/ip-law/toast-emboldened-its-advisory-board-to-steal-secrets-suit-says)).
- **Strongest.** 7shifts (scheduling, tip pooling, compliance)†. Toast Payroll and Team, with the Sling scheduling acquisition in 2021†. Homebase†.

### 2.9 Reporting, multi-location and franchise operations

- **Reference behavior.**
  - Real-time dashboards and product mix that includes modifiers.
  - Audit reports on voids, comps and discounts.
  - Labor versus sales, ticket times, table turns and server performance.
  - Rollups and comparisons across locations.
  - Franchise royalty and ad-fund reports, with franchisee data rights respected.
  - Warehouse export and an open API.
- **Strongest.** Simphony Reporting & Analytics†. Toast's peer benchmarking†. PAR Data Central†. Lightspeed's raw API access on Premium ([UpMenu](https://www.upmenu.com/blog/lightspeed-pos-pricing/)).
- **Failure pattern.** Reports and close-out stop working offline: Toast shift review cannot be completed offline ([Toast Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode)), and Lightspeed says reporting "may pause" ([POSUSA](https://www.posusa.com/lightspeed-restaurant-stuck-offline-mode/)).

### 2.10 Loyalty, gift cards and marketing

- **Reference behavior.**
  - Points, visits and tiers.
  - Card-linked enrollment, so the guest never types a phone number (Thanx†).
  - Automated win-back and birthday campaigns.
  - One guest profile across dine-in, online and reservations, with consent tracking.
  - Gift cards (physical and e-gift) with reload, balance lookup, liability and breakage reports, and unclaimed-property compliance†.
- **Strongest.** Punchh (PAR) and Paytronix for enterprise†. Thanx for card-linked loyalty†. SevenRooms as a CRM†. Owner.com for independents' automated marketing†. Toast and Square native programs†.
- **Failure pattern.** Toast loyalty and gift cards are unavailable offline ([Toast Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode)). During NCR's 2023 ransomware outage, some Aloha customers could not accept gift cards ([SiliconANGLE](https://siliconangle.com/2023/04/17/ransomware-attack-causes-outages-payments-giant-ncr/)). The stored-value ledger usually lives only in the cloud.

### 2.11 Pay at table and QR order-and-pay

- **Reference behavior.**
  - Card terminals brought to the table (the norm in Europe†).
  - QR ordering that attaches to the *same* check the server sees.
  - Guests join one tab, order rounds, split by item, tip and pay, with no app download.
  - A server can turn QR ordering off per table.
  - An accessible fallback.
- **Strongest.** Sunday (QR pay-at-table)†, me&u (group ordering)†, Toast Mobile Order & Pay†, and Lightspeed contactless ordering on Essential ([UpMenu](https://www.upmenu.com/blog/lightspeed-pos-pricing/)).
- **Risk.** Diners push back on QR-only service and on surcharges†. The winning pattern is a hybrid: staff take the first order, QR handles repeat rounds and payment†.

### 2.12 Drive-thru, kiosks and catering

- **Drive-thru.**
  - Reference behavior: dual lanes, order-confirmation boards, drive-thru timer integration, and line-busting handhelds.
  - AI voice ordering is unproven. McDonald's ended its IBM voice-ordering test in June 2024†. Taco Bell said in 2025 it was rethinking its approach after viral failures†. The SEC charged Presto Automation (January 2025) over misleading claims about its drive-thru AI†.
- **Kiosks.** Upsell rules, loyalty sign-in, an ADA-accessible mode, and the same menu engine as the POS. Examples: Toast Kiosk, Square Kiosk†, Grubbrr†.
- **Catering.** Quotes, deposits, banquet event orders, an event calendar, prep sheets, tax-exempt customers, and invoicing with house accounts. Examples: Toast Catering & Events† and the ezCater marketplace†.

### 2.13 Hardware

- **Proprietary and tied to the vendor's processing:** Toast, Square, Clover, SpotOn, Shift4 Dine.
- **Bring your own device:** iPad (Lightspeed K, TouchBistro, Revel, orderbird†); Windows or Android (Aloha†, Simphony with supported third-party hardware†, Petpooja†).
- **Trade-off.** Bring-your-own lowers the cost of leaving. Proprietary hardware allows integrated card readers and hardened, spill-resistant devices. Network requirements (Lightspeed's port list, Toast's hardwired hub) are a hidden hardware cost.

### 2.14 Commercials: pricing, contracts, support, data portability

- **Best commercial terms.**
  - Square: published pricing, month-to-month†.
  - Lightspeed: published tiers.
  - SpotOn POS Essentials: month-to-month.
- **Worst commercial terms.**
  - Shift4: liquidated damages and disputed cancellations.
  - Toast: two- to three-year terms, early-termination fees, rate changes on 30 days' notice.
  - SpotOn All-In: two years.
  - TouchBistro: annual term plus mandatory payments.
  - Clover through resellers: leases and fee opacity.
- **Support.** No vendor stands out in the evidence gathered. Complaints about billing disputes, long holds and continued charges after cancellation appear for TouchBistro, SpotOn and Shift4 (see section 3).
- **Data portability.**
  - Square exports customers and the item library as CSV†.
  - On-premises enterprise systems give owners the most direct database access†.
  - Card-on-file tokens are tied to the processor. Stripe publishes a policy of migrating card data to another PCI-compliant processor on request†. POS vendors that are also processors rarely advertise the same.
  - Gift-card liabilities and loyalty balances are rarely exportable in a usable form†.
  - The Revel sunset and the Clover forced migration show why a guaranteed exit path matters.

---

## 3. Flaws and complaints catalog

### 3.1 By vendor

**Toast**
- **Lock-in and price creep.** Two- to three-year terms, early-termination fees equal to the remaining subscription, and processing-rate changes on 30 days' notice. A competing processor reports 2026 renewals 15%+ above 2023 pricing, which is biased but directional ([Sleft Payments](https://www.sleftpayments.com/learning-hub/toast-pos-raised-fees-options-2026), [Merchant Insiders](https://merchantinsiders.com/blogs/toast-fees/), [Labrador](https://www.labrador.ai/blog/toast-pos-fees-2026)).
- **Offline gaps.**
  - No sync between devices without the hub, and the hub has strict requirements (hardwired, a specific device generation, same subnet).
  - No loyalty, no gift cards, no shift review.
  - Staff cannot log back in once logged out.
  - Authorizations may expire in 24 h ([Toast Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode), [Toast docs](https://doc.toasttab.com/doc/platformguide/platformOfflineModeLocalSync.html)).
- **Cloud dependency.** Complete system failures at many restaurants during the October 2025 AWS outage ([NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling)).
- **Trust.** A 99¢ diner fee was introduced and then withdrawn in 2023†. The Gratuity Solutions trade-secret suit is proceeding ([Bloomberg Law](https://news.bloomberglaw.com/litigation/toast-will-have-to-face-lawsuit-over-alleged-trade-secret-theft)).

**Square for Restaurants**
- **Reliability.**
  - September 7, 2023, 1:54 PM ET to 5:19 AM ET the next day, caused by DNS ([TechCrunch](https://techcrunch.com/2023/09/11/square-daylong-outage-dns-error/), [Payments Dive](https://www.paymentsdive.com/news/square-outage-pos-payments-processing-block-smb-merchants/693674/)).
  - Further payment disruptions logged in February, March and August 2025 and February 2026 ([KORONA](https://koronapos.com/blog/square-pos-outage/), [IsDown](https://isdown.app/status/square/incidents/375949-payments-disruption), [StatusGator](https://statusgator.com/services/square/point-of-sale)).
- **Offline design.** Offline mode had to be forced by unplugging the network. Offline payments are allowed for 24 h with a 72 h hard stop, the seller bears declines, and Square gives no guest contact details for them ([Square Community](https://community.squareup.com/t5/Troubleshooting/How-did-people-get-Offline-Mode-to-work-thru-the-outage/m-p/681363), [Square Support](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode)).

**Clover (Fiserv)**
- **Forced migration.** Up to 200,000 Payeezy merchants were moved late 2023 through H1 2024. Many left for Square or Toast, citing fees and service ([Payments Dive](https://www.paymentsdive.com/news/fiserv-sued-over-clover-migration/754200/), [PaymentExpert](https://paymentexpert.com/2025/07/28/fiservs-being-sued-heres-what-you-need-to-know/)).
- **Fees.** Merchants allege fees they never agreed to, "buried in complex contracts and monthly billing statements" ([PaymentPop](https://paymentpop.com/merchant-accounts/clover-pos-customer-reviews/)).
- **Corporate instability.**
  - In one 2025 quarter, Clover revenue growth slowed to 27% (from 29%) and annualized payment-volume growth to 8% (from 14%) ([PYMNTS](https://www.pymnts.com/earnings/2025/fiserv-shares-fall-clover-growth-slows-merchant-payments-processing-revenues-dip/)).
  - Fiserv shares fell about 44% in October 2025, with leadership changes ([Bloomberg Law](https://news.bloomberglaw.com/securities-law/fiserv-top-brass-misrepresented-clovers-prospects-suit-says), [FinTech Weekly](https://www.fintechweekly.com/magazine/articles/fiserv-shares-fall-earnings-miss-leadership-shakeup-fintech)).
  - Securities class actions followed ([Kessler Topaz](https://www.ktmc.com/new-cases/fiserv-inc/)).

**Lightspeed Restaurant**
- Network setup burden: 10 ports, and client isolation disabled ([Lightspeed](https://k-series-support.lightspeedhq.com/hc/en-us/articles/16154347413275-Networking-for-Lightspeed-Restaurant)).
- Offline card payments not guaranteed. Third-party guides exist for devices "stuck in offline mode" ([POSUSA](https://www.posusa.com/lightspeed-restaurant-stuck-offline-mode/)).
- Three restaurant product lines with separate help centers: K-Series, L-Series ([LiteServer](https://resto-support.lightspeedhq.com/hc/en-us/articles/5218871669403-Troubleshooting-the-LiteServer)) and O-Series ([O-Series offline](https://o-series-support.lightspeedhq.com/hc/en-us/articles/31329361292571-Working-with-Lightspeed-Offline)). This is a migration risk for merchants.

**TouchBistro**
- Support quality: poor response times, lack of follow-through, difficulty reaching dedicated reps.
- Billing continued after cancellation; one reviewer had been trying to cancel since December 2025.
- Contracts are inflexible, and payments are mandatory for new customers ([Trustpilot](https://www.trustpilot.com/review/touchbistro.com), [B2B Reviews](https://www.b2breviews.com/reviews/touchbistro/), [business.com](https://www.business.com/reviews/touchbistro/)).

**SpotOn**
- Billing charges that do not match agreed rates, with the complaint pattern consistent across BBB and Trustpilot.
- A merchant moved onto a contract when forced to replace equipment.
- A reported $995 fee to switch processors.
- Deposits about two days out because of the 6 pm cutoff ([Sonary](https://sonary.com/b/spoton/spoton+pos/), [Rezku](https://rezku.com/blog/top-spoton-pos-alternatives-for-restaurants-in-2026/), [tech.co](https://tech.co/pos-system/spoton-pos-review)).

**Shift4 Dine (SkyTab) and Revel**
- Liquidated-damages clauses; BBB average of 1.02 stars.
- A merchant disputes a $45,000 fee after cancelling within a 30-day trial.
- Unexpected fees such as $150/mo "premium support" and unexplained $500 charges.
- Long holds and tickets closed without a fix ([BBB](https://www.bbb.org/us/pa/center-valley/profile/credit-card-processing-services/shift4-0241-235983222/complaints), [Reforming Retail](https://reformingretail.com/index.php/2025/12/30/to-catch-a-predator-shift4-termination-fees/), [PaymentPop](https://paymentpop.com/merchant-accounts/skytab-customer-reviews/), [Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/shift4-review/)).
- Revel: development reportedly frozen after the acquisition, with merchants now being converted to a different product ([KORONA](https://koronapos.com/blog/what-happened-to-revel-pos/)).

**Oracle MICROS Simphony**
- Complexity and cost, and reliance on certified integrators†.
- Oracle's MICROS support portal was breached in 2016†.

**NCR Voyix Aloha**
- April 2023 ransomware took cloud back office, gift cards and Pulse offline for days ([Cybersecurity Dive](https://www.cybersecuritydive.com/news/ncr-pos-ransomware-recovery/648005/), [Security Affairs](https://securityaffairs.com/144866/cyber-crime/ncr-blackcat-alphv-ransomware.html)).
- Corporate restructuring: NCR split into NCR Voyix and NCR Atleos in 2023†.

**Delivery marketplaces (all vendors)**
- One tablet per marketplace, and menu drift between the tablets and the POS.
- Commissions of 15–30%†.
- Guest identity withheld by marketplaces†.

### 3.2 Cross-cutting themes

1. **Software is the bait; processing is the business.** Lock-in comes through the device, the contract or the processing requirement. It enables unilateral rate changes, such as Toast on 30 days' notice.
2. **Contracts are the moat.** Two- to three-year terms, early-termination fees, liquidated damages, leases, and $995 switch fees.
3. **"Offline" is a payments feature, not an operations feature.** Loyalty, gift cards, login, close-out, reporting, online orders, waitlist and scheduling all fail. Store-and-forward windows run from 24 h to 7 days, and the merchant carries the risk.
4. **Single points of failure:** one cloud region (AWS us-east-1), one local hub (Toast), one data center (NCR 2023), and DNS (Square 2023).
5. **Merchants are stranded by M&A and migrations:** Revel frozen and converted; Clover's forced migration; SkyTab renamed; Lightspeed's three lines.
6. **Support fails at peak and billing dominates complaints.** The pattern is disputes, charges after cancellation and surprise fees.
7. **Weak data ownership.** Guest data sits in marketplaces and reservation networks. Card tokens are tied to the processor. Gift and loyalty liabilities are hard to move.
8. **Setup fragility.** Port lists, subnet rules and client isolation cause silent failures.
9. **Compliance is left to operators.** Tip rules, service-charge disclosure, surcharging and fiscalization are handled ad hoc.

### 3.3 Notable outages and incidents

| Date | Vendor / provider | Cause and scope | Impact | Source |
|---|---|---|---|---|
| 2023-04-12 to at least 04-17 | NCR (Aloha, Counterpoint) | Ransomware in one NCR data center; BlackCat/ALPHV claimed it | Aloha cloud services down; some customers lost back office, gift-card acceptance and the Pulse dashboard for days | [SiliconANGLE](https://siliconangle.com/2023/04/17/ransomware-attack-causes-outages-payments-giant-ncr/), [Cybersecurity Dive](https://www.cybersecuritydive.com/news/ncr-pos-ransomware-recovery/648005/) |
| 2023-09-07 13:54 ET to 09-08 05:19 ET | Square | DNS error from a routine internal network software update | About 14–15 h. Sellers could not log in or take payments, and Square Online checkout failed. Offline mode needed a manual disconnect. Processing of offline payments was deferred | [Square incident summary](https://developer.squareup.com/blog/incident-summary-2023-09-07/), [TechCrunch](https://techcrunch.com/2023/09/11/square-daylong-outage-dns-error/), [Square Community](https://community.squareup.com/t5/Troubleshooting/How-did-people-get-Offline-Mode-to-work-thru-the-outage/m-p/681363) |
| 2024-07-19 | CrowdStrike (Windows endpoints) | Faulty security-sensor update crashed Windows machines worldwide | Some Windows-based retail and restaurant POS and back-office systems affected† | † (not verified this session) |
| 2025-02-26 | Square | Card-processing disruption | About 2 h | [KORONA](https://koronapos.com/blog/square-pos-outage/) |
| 2025-03-17 and 03-29 | Square | Warning-level issues; payment acceptance for some sellers | About 1 h 19 min on March 17 | [IsDown](https://isdown.app/status/square/incidents/375949-payments-disruption), [KORONA](https://koronapos.com/blog/square-pos-outage/) |
| 2025-08 | Square | Payments disruption | Not quantified | [IsDown](https://isdown.app/status/square/incidents/428984-payments-disruption) |
| 2025-10-20 | AWS us-east-1 (affecting Toast and others) | Regional cloud outage | Many Toast restaurants saw "complete system failures", scheduling and waitlist failures, and manual transaction entry. DoorDash, McDonald's and Starbucks apps also hit | [NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling) |
| 2026-02-04 and 02-20 | Square | Warning-level disruptions | About 1 h 10 min and 1 h 14 min | [StatusGator](https://statusgator.com/services/square/point-of-sale) |

**Business and legal incidents that affect merchants**

| Date | Vendor | Event | Merchant impact | Source |
|---|---|---|---|---|
| 2023-02 | Toast | 99¢ diner fee on online orders over $10 | Withdrawn within weeks after backlash | † |
| Late 2023 to H1 2024 | Fiserv / Clover | Forced migration of up to 200,000 Payeezy merchants | Churn; investor suits in 2025 | [Payments Dive](https://www.paymentsdive.com/news/fiserv-sued-over-clover-migration/754200/) |
| 2024-06 | Shift4 / Revel | Revel acquisition closes: $250M, about 18,000 locations | Reported freeze on fixes; conversions to Shift4 Dine in 2026 | [PYMNTS](https://www.pymnts.com/acquisitions/2024/shift4-acquires-majority-stake-in-vectron-completes-purchase-of-revel/), [Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/revel-pos-merchant-updates/) |
| 2025-10 | Fiserv | Shares fall about 44% on the Clover slowdown; leadership change | Platform uncertainty | [Bloomberg Law](https://news.bloomberglaw.com/securities-law/fiserv-top-brass-misrepresented-clovers-prospects-suit-says) |
| 2025-11-18 | Toast | Judge lets the Gratuity Solutions trade-secret suit proceed | Dispute over tip-management IP | [Bloomberg Law](https://news.bloomberglaw.com/litigation/toast-will-have-to-face-lawsuit-over-alleged-trade-secret-theft) |
| 2025-11/12 | Shift4 | $45,000 cancellation fee disputed after cancellation within a 30-day trial | Exit cost | [Reforming Retail](https://reformingretail.com/index.php/2025/12/30/to-catch-a-predator-shift4-termination-fees/) |
| 2026-05-12 | Shift4 | SkyTab renamed Shift4 Dine | Brand churn; no functional change | [Shift4 release notes](https://releasenotes.shift4.com/announcements/skytab-is-becoming-shift4-dine) |

---

## 4. Unsolved problems (gaps no vendor handles well)

1. **Full operational parity offline beyond 24 h.** No vendor documents login, loyalty, gift cards, close-out, reporting, online-order intake and multi-device sync all working offline together. Toast lacks several of these; Lightspeed pauses reporting.
2. **Automatic detection of degraded service.** Offline modes trigger on a lost link, not on "the cloud is up but broken" (Square 2023). The manual fallback of unplugging the network is unacceptable during a rush.
3. **Bounded, observable offline card risk.** No one combines caps, card-number-range (BIN) rules, an exposure dashboard, and guest contact capture to recover declines. Square explicitly cannot help recover declines.
4. **Coordination without a single hub.** Local-sync designs rely on one hub or server with topology constraints (hardwired, same subnet, specific hardware generation).
5. **Independence from any one cloud region.** Cloud-first vendors lose scheduling, waitlist and online ordering in a regional outage (AWS, October 2025).
6. **Consistent 86 and throttling across marketplaces.** Tablets and middleware remain, marketplace menus drift, and throttling uses static time-slot caps rather than live kitchen load†.
7. **Real portability.** Nobody offers a standard schema for menus with deep modifiers, or transfer of card tokens, gift-card liabilities and loyalty balances. Revel's sunset and Clover's migration show merchants have no exit plan.
8. **Commercial predictability without lock-in.** Rate changes on short notice, early-termination fees and liquidated damages are the norm. Month-to-month SMB offers carry higher rates and fewer features.
9. **Tip and fee compliance as data.** Tip pools, tip-outs, the new federal qualified-tip reporting†, and service-charge-versus-tip treatment across states are handled with side systems. The Toast versus Gratuity suit shows how valuable, and contested, this is.
10. **A guest identity the merchant owns across reservations, marketplaces and POS.** Reservation networks are owned by Amex and DoorDash†, and marketplaces withhold guest data†.
11. **Networks that survive real restaurant IT:** guest-network isolation, VLANs, consumer routers, printers on DHCP.
12. **Support during peak hours and billing integrity.** This is the dominant complaint for TouchBistro, SpotOn and Shift4.
13. **Allergen integrity end to end,** from menu to online ordering to KDS to label. No vendor evidence found; treat it as unsolved.
14. **AI voice ordering accuracy.** Public retreats and enforcement (McDonald's, Taco Bell, Presto†).
15. **Franchise data rights and multi-brand kitchens.** Franchisor visibility versus franchisee ownership; virtual brands sharing one kitchen and one KDS†.
16. **Global compliance as modules:** fiscalization (Germany's TSE, France's NF525, Italy's RT, Austria's RKSV, Saudi Arabia's ZATCA) and e-invoicing without forking the product†.

---

## 5. Implications for our design: testable requirements

Numbers are proposed targets for the architect to confirm. Each requirement names the evidence it answers.

### Resilience and offline

1. **Offline parity for 72 h.** With the internet fully disconnected, all of the following keep working for at least 72 h with no loss of function:
   - staff login (cached credentials and PIN)
   - order entry with full modifiers, coursing and firing, seats
   - split, merge and transfer of checks; tabs
   - comps and voids with manager approval
   - KDS routing and bump; printing
   - cash, and card store-and-forward
   - gift-card and loyalty redemption within offline limits
   - shift close and cash-out; local reports

   **Test:** 72-hour soak on a site with 10 terminals, 4 KDS screens and 6 handhelds. Zero orders lost, and everything reconciled within 15 minutes of reconnecting. *Answers:* the Toast offline gaps and Lightspeed's paused reporting.
2. **Peer-to-peer LAN sync with no single hub.** Devices elect a coordinator automatically. Losing any one device, including the current coordinator, interrupts service for at most 5 seconds and loses no orders. Sync works across subnets and VLANs through configured peers, and does not require a particular hardwired device. *Answers:* Toast's hub constraints.
3. **Degraded-service detection.** A device enters offline mode automatically when:
   - cloud API p95 latency exceeds 3 s for 30 s, or
   - the error rate exceeds 20% over 30 s, or
   - DNS resolution fails.

   It returns online with hysteresis, and there is a one-tap manual override. **Test:** inject a DNS failure with the network link still up. *Answers:* Square 2023.
4. **An offline payment risk envelope.**
   - Configurable caps per transaction, per card, per device and per location; defaults at least as protective as Clover's $500 per transaction and $5,000 total.
   - Rules by card number range (for example, refuse prepaid and foreign cards offline).
   - Queued authorizations forwarded within 60 s of reconnecting.
   - An alert at 20 h of queue age.
   - A live dashboard of offline exposure.
5. **Decline recovery.** Offline card payments can optionally capture a guest phone number or email. Itemized check detail and signatures are kept for chargeback defense. *Answers:* Square cannot provide contact details for declined offline payments.
6. **No single-region dependency.**
   - In-store operation needs no cloud at all.
   - Cloud services (scheduling, waitlist, online ordering, back office) run active-active in at least 2 regions, with at most 1 minute of data loss (RPO) and at most 15 minutes to recover (RTO).
   - A region-failover drill runs every quarter.

   *Answers:* the October 2025 AWS outage.
7. **Online orders during a store outage.** Within 60 s of a store going offline, the system either queues cloud-accepted orders for guaranteed delivery to the store, or automatically pauses every off-premise channel, marketplaces included. No order may be accepted that the kitchen never sees.
8. **Ransomware-resilient design.**
   - Immutable, cross-region backups.
   - A compromise of the cloud back office must not stop in-store service.
   - The gift-card and loyalty ledger is replicated to each store.
   - Back office restored within 4 h.

   *Answers:* NCR 2023.
9. **Incident transparency.**
   - A public status page broken down by component and region.
   - An in-app banner within 5 minutes of an incident being declared.
   - A written post-incident report within 5 business days.

### Payments and commercial terms

10. **Processor-agnostic.**
    - At least 3 US processors certified at launch.
    - A merchant can change processor without replacing hardware or losing data.
    - Published rates, with interchange-plus pricing available.

    *Answers:* universal processing lock-in.
11. **No-lock-in contract.**
    - Month-to-month, with no early-termination fee and no liquidated damages.
    - Price changes need at least 90 days' notice and allow a penalty-free exit.
    - Hardware can be bought outright, or leased on a lease cancellable by returning the hardware.

    *Answers:* Toast's early-termination fee and 30-day rate changes, Shift4's liquidated damages and $45k dispute, SpotOn's $995 switch fee.
12. **Card-on-file portability.** Use network tokens where possible. On request, card data is migrated to a new PCI DSS Level 1 processor within 10 business days.
13. **Bar tabs done right.**
    - Configurable pre-authorization, with incremental re-authorization when a tab exceeds a set share of the authorized amount.
    - Auto-close at a configured time under a disclosed gratuity policy.
    - A walkout report.
14. **Fee and surcharge engine.**
    - Surcharges, service charges and dual pricing configurable by jurisdiction and by card type (credit versus debit), with caps and bans encoded (for example, the card-brand cap of about 3%† and the Connecticut and Massachusetts bans†).
    - Disclosure templates for menus and receipts (for California's SB 478 and SB 1524†).
    - Tax, tip and service charge stored and reported separately on every transaction, to support laws that exempt those portions from interchange, such as Illinois' Interchange Fee Prohibition Act, scheduled for July 1, 2026 and under litigation† (verify status).
15. **Merchant controls diner fees.** The platform never adds a diner-facing fee without the merchant opting in. *Answers:* the Toast 99¢ fee†.

### Hardware and setup

16. **Commodity hardware.** Runs on commodity iPadOS, Android and Windows devices. Certified third-party card readers and handhelds are supported. Any device can be wiped and reused elsewhere if the merchant leaves.
17. **Networking that just works.**
    - Works on a consumer router at default settings.
    - At most 3 documented ports.
    - A built-in "network doctor" that diagnoses client isolation, split subnets and unreachable printers in under 60 s.

    *Answers:* Lightspeed's 10-port list and Toast's subnet rule.
18. **Self-install and switching kit.**
    - A single location with 3 terminals, 2 KDS screens and 2 printers goes live in 2 hours or less without a vendor call.
    - Importers for menu and customer exports from Toast, Square, Clover and Lightspeed.

### Menu, channels and kitchen

19. **Menu model.**
    - Nested modifier groups at least 4 levels deep, with minimum and maximum selections per group.
    - Pre-modifiers (No, Light, Extra, Side) and a size-price matrix.
    - Portion pricing: whole, half, quarter.
    - Combo builders.
    - Allergen and dietary tags as structured data.
    - Recipe depletion at the modifier level.
20. **Hierarchical catalog.**
    - Brand → region → location → revenue center → channel, with overrides at every level.
    - Scheduled publishing, preview, a full audit log, and one-click rollback.
21. **Global 86.** A stock change reaches POS, KDS, kiosk, QR, first-party web and app, and marketplaces within 30 s at p95. Items auto-86 when their count reaches 0 and come back automatically on schedule.
22. **Marketplace integrations without tablets.**
    - Direct integrations: DoorDash, Uber Eats and Grubhub at launch; regional marketplaces (Deliveroo, Just Eat, Zomato, Swiggy, Talabat, HungerStation) as markets require.
    - Orders go straight to the KDS, with two-way status.
    - Per-order payout reconciliation: commission, promotions, error charges.
23. **Throttling based on kitchen load.** Every off-premise channel throttles and quotes times from live KDS load (items queued per station, rolling average ticket time), not only from static time-slot caps. Adjustments take effect within 60 s.
24. **Front-of-house core.**
    - Seats; coursing with hold, fire and timed auto-fire.
    - Split by seat, item, even or amount, including fractional item splits.
    - Merge and transfer with an audit trail.
    - Full parity on handhelds.
25. **Controls.**
    - Reason codes and thresholds.
    - Manager approval by PIN, badge or remote push to the manager's phone, completing within 10 s on the LAN.
    - Voids after an item has fired are logged as waste automatically.
    - Loss-prevention reports.
26. **KDS.**
    - Routing rules by item, modifier, channel and daypart.
    - Expo, bump bar support, all-day counts, cook-time sequencing.
    - Allergen highlighting, colored timers, recall.
    - Runs peer-to-peer offline.
    - Order-to-KDS latency of 500 ms or less at p95 on the LAN.
27. **Pay at table and QR ordering.**
    - Card payment on handhelds at the table.
    - QR orders attach to the same check the server sees.
    - Guests join a tab, order rounds, split by item, tip and pay, with no app download.
    - The server can disable QR ordering per table.

### Guests, labor and back office

28. **Floor, reservations and waitlist.**
    - A native floor plan with live table state; a waitlist with SMS; reservations with deposits.
    - Open integrations with OpenTable, Resy, SevenRooms and Tock.
    - A guest profile that the merchant owns and can export at any time.
29. **Labor compliance.**
    - Rule packs for predictive scheduling (NYC, SF, Seattle, Chicago, Philadelphia, Oregon, LA†).
    - Break rules, such as California's meal-break timing†.
    - Limits on minors.
    - Overtime alerts.
    - Schedules built from forecasts.
30. **Tip engine.**
    - Pools weighted by hours × role points.
    - Tip-outs as a percentage of a sales category or of net sales.
    - Cash-tip declaration.
    - Service charges kept separate from tips.
    - FLSA rules encoded (managers excluded; tip-credit constraints†).
    - Per-employee occupation code and qualified-tip totals for W-2 reporting†.
    - Payroll export to ADP, Gusto and Paychex.
31. **Biometric clock-in off by default.** When enabled, it requires Illinois-BIPA-grade consent and retention controls†.
32. **Inventory.**
    - Recipes and sub-recipes, with depletion at the modifier level.
    - Actual-versus-theoretical variance.
    - Waste logging from the KDS.
    - An API for invoice ingestion, so tools like MarginEdge and Restaurant365 plug in.
33. **Reporting.**
    - Real-time reports.
    - Local reports and shift close available offline.
    - Multi-location rollups and franchise royalty reports.
    - A daily warehouse export (Parquet or CSV to S3, BigQuery or Snowflake).
    - A full API with webhooks.
34. **Loyalty and gift cards.** Native, working offline within limits, with an optional card-linked mode. A liability and breakage report. Balances are exportable.

### Data, exit and support

35. **One-click full export in a documented open schema** covering:
    - the menu, with the full modifier tree
    - customers, with consent flags
    - loyalty balances and gift-card liabilities
    - employees, time punches and tips
    - line-item sales history

    **Test:** round trip. Exporting, then importing into a fresh tenant, reproduces the original.
36. **Sunset guarantee.**
    - At least 12 months' notice before discontinuing a product.
    - No forced migration to a different product.
    - Security fixes continue through the notice period.
    - Contractual data escrow.

    *Answers:* Revel and the Clover/Payeezy migration.
37. **Support service levels.**
    - Human support 24/7.
    - Median time to reach a human of 2 minutes or less on Friday and Saturday, 5–10 pm local time.
    - Billing disputes resolved within 5 business days.
    - Self-service cancellation in the app, with billing stopping on the cancellation date.

### Global, drive-thru, kiosk and catering

38. **Fiscalization as country modules** that plug in without forking the core: Germany (TSE and DSFinV-K), France (NF525), Italy (RT), Austria (RKSV), Saudi Arabia (ZATCA Phase 2), India (GST)†.
39. **Kiosk.** An accessible mode (audio and screen reader, reach ranges), upsell rules and loyalty sign-in, on the same menu engine as the POS.
40. **Drive-thru.**
    - Timer integrations, confirmation boards, line-busting handhelds.
    - AI voice ordering is optional, with a published accuracy threshold and a human fallback.
41. **Catering.** Quotes, deposits, banquet event orders, prep sheets, invoicing and house accounts.
