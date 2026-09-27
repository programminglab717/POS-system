# 06: Vertical-specific requirements and global markets

Research track 06, compiled 2026-09-27, for the POS platform architect.

## How to read this report (method and confidence)

- About 46 web searches were run, favouring 2024–2026 sources. Most vendor, regulator and reference domains were blocked for full-page fetches in this environment. The session's shared search budget ran out before the Japan, China, Africa, Middle East, Australia/New Zealand and Canada sections, and parts of the Mexico and Southeast Asia sections, could be searched.
- **Claims with an inline link** are supported by that source, as shown in its search-result excerpt. Vendor-blog numbers are marketing claims and are labelled that way.
- **Claims marked ‡** come from the researcher's background knowledge (up to mid-2026) and were **not re-verified in this session**. Treat them as hypotheses to confirm before they become hard requirements. The fiscal/e-invoicing researcher should confirm every ‡ regulatory date.

---

## 1. Vertical requirement matrix

Key to the primitives (defined in §3):
- **P1** Catalog and pricing model
- **P2** Measured and variable-price items
- **P3** Bookable resources and capacity
- **P4** Entitlements and recurring agreements
- **P5** Unified value ledger and accounts
- **P6** Open orders with payment holds (tabs)
- **P7** Deferred-fulfillment orders and routing
- **P8** Custody jobs (work orders and rentals)
- **P9** Identity, verification and consent
- **P10** Policy/compliance engine and traceability outbox
- **P11** Tender orchestration and eligibility
- **P12** Fiscal and document engine
- **P13** Org hierarchy, governance and multi-party settlement
- **P14** Staff attribution and earnings
- **P15** Sessions, events and offline-first operation

| Vertical | Must-have capabilities | Leading specialists | Why general POS fails | Primitives |
|---|---|---|---|---|
| Salons, spas, barbers | Staff, room and chair booking with processing gaps; deposits; card-on-file no-show and late-cancel fees; service and retail on one ticket; tiered commissions (service vs retail); tip split across several providers; client formulas and consents; rebooking; online and marketplace booking; memberships and packages; booth renters | Vagaro, Fresha, Boulevard, Mindbody, GlossGenius, Booksy, Square Appointments, Mangomint | Treats services as items; no resource-constrained calendar; one staff member per line; no compliant way to charge a no-show fee later | P3 P4 P5 P9 P14 |
| Fitness and studios | Recurring contracts, freezes and cancellations; class packs with expiry; class capacity and waitlists; late-cancel fees; check-in and door access tied to payment status; dunning | Mindbody, ABC Glofox, Zen Planner | No recurring billing, entitlements or access control | P3 P4 P5 P9 P15 |
| Cannabis dispensaries | Two-way Metrc/BioTrack reporting per package UID; purchase limits with category equivalency (recreational vs medical); ID scan and 21+; patient registry; cash controls and smart safes; compliant payments (ACH, pay-by-bank); live-inventory menus | Dutchie, Flowhub, Treez, Cova, BLAZE, IndicaOnline | Mainstream processors exclude cannabis‡; no package-level traceability or limit engine | P1 P9 P10 P11 P12 P15 |
| Liquor and age-restricted retail | Item-level age gates with ID scan; case and bottle units; mix-and-match and case discounts; deposits as pass-through lines; state reports; hours-of-sale rules‡ | Bottle POS, NRS, POS Nation, LMS POS | Manual age checks; no case breaking; deposits rung up as miscellaneous items | P1 P9 P10 |
| Grocery and supermarkets | Certified scales, tare, PLU and price-embedded barcodes; SNAP and WIC eligibility and split tender; bottle deposits; lane throughput; self-checkout weight checks; shrink controls (lot and expiry); 2D barcodes‡; large catalogs | NCR Voyix, Toshiba, ECRS, LOC‡; VizionPOS | Scale legality, EBT/WIC certification, speed and catalog size | P1 P2 P10 P11 P15 |
| Convenience and fuel | Forecourt control (Conexxus FDC, IFSF‡); lottery pack inventory and state terminals; tobacco age checks (TruAge); scan-data programs‡; fuel loyalty; shift reconciliation across fuel and store | Verifone Commander, Gilbarco Passport, NCR Radiant‡; Petrosoft, NRS Petro | No forecourt or lottery support; age checks on more than 37% of inside transactions | P2 P9 P10 P11 P15 |
| Pharmacy (front end) | Rx pickup linked to the pharmacy system; signature capture (HIPAA notice, counselling); pseudoephedrine (PSE) logging (NPLEx/MethCheck); FSA/HSA via IIAS or the 90% rule; will-call bins | RetailSTARx (Auto-Star), Celerant, PrimeRx | No pharmacy-system link, no IIAS subtotals, no PSE logs | P9 P10 P11 |
| Hotels (food and drink, spa, retail outlets) | Room charge to the PMS folio (OPERA, Mews, Cloudbeds); guest lookup; mapping outlets to PMS transaction codes; paymaster and house accounts | Oracle Simphony, Agilysys‡; Toast and Lightspeed through connectors | No PMS adapters or folio semantics | P5 P6 P13 |
| Stadiums, venues and events | Transactions in under 3 seconds; offline store-and-forward; portables, in-seat ordering, scan-and-pay, kiosks; closed-loop RFID wristbands; menus scoped to an event; per-stand reporting; alcohol cutoffs; revenue share with concessionaires | Shift4 (VenueNext, Appetize), Fiserv (Bypass), Tapin2, MyVenue, GoTab | Speed, offline operation, closed-loop value and settlement between operators | P5 P7 P9 P13 P15 |
| Food trucks, markets, pop-ups | Handheld with cellular and printer; offline cards with risk caps; tax based on location; quick menus | Square, Toast, SumUp, Clover‡ | Offline caps and merchant-borne decline risk | P11 P15 |
| Bars and nightclubs | Tabs with pre-auth and incremental auth; auto-close walkouts with a tip; bottle service minimums and auto-gratuity; ID scanning and capacity counts; business day that runs past midnight | Toast, SpotOn, Shift4 SkyTab, Clover | Card-network rules on authorizations; walkout handling | P6 P9 P14 P15 |
| Quick-service (QSR) and fast casual | Kiosks; drive-thru timers paired with orders; kitchen display (KDS) routing; order-ahead throttling; delivery aggregators; speed-of-service metrics; limited-time offer rollout | PAR Brink, Toast, NCR Voyix Aloha‡, Oracle Simphony‡; HME timers | Speed-of-service instrumentation and franchise governance | P1 P7 P13 P15 |
| Coffee shops | Deep modifiers with upcharges; drink labels; order-ahead; stamp loyalty; subscriptions | Square, Toast, Posso (UK) | Modifier depth, labels, subscriptions | P1 P4 P7 |
| Bakeries and pre-orders | Custom orders with attributes; deposits; pickup slots and daily capacity; production lists; allergen labels‡ | BakeSmart, Orders.co | No future-dated orders or production planning | P3 P5 P7 |
| Catering and events | Quotes and event orders (BEOs); payment schedules; headcount changes; delivery windows; service charge vs gratuity; tax-exempt clients | Toast Catering & Events, Tripleseat, CaterZen‡ | Assumes same-day tickets | P5 P7 P14 |
| Franchises | Multiple legal entities; menu and price locks; price zones; limited-time offer scheduling; royalties on gross sales; settlement of gift-card and loyalty liabilities between entities | PAR Brink, Toast Enterprise | Flat location model; no locked inheritance | P1 P5 P13 |
| Hardware, auto parts, B2B counter | House accounts and receivables; job/PO numbers; tax-exempt certificates; contractor price levels; quotes to orders; special orders; units of measure; ACES/PIES fitment; core charges | Epicor Eagle, Epicor BisTrack, J3; AMS Retail | No receivables, customer pricing or catalogs | P1 P5 P7 P8 P9 |
| Rental businesses | Availability by serialized asset; time-based rate ladders; deposits and holds; damage waivers and e-signed contracts; return inspection; late fees; metered billing; maintenance | Quipli, EZRentOut, Rentman, Rent in Hand; Point of Rental‡ | Goods leave and come back; pricing is by time | P3 P5 P6 P8 P9 |
| Repair shops | Tickets with device intake (serial/IMEI, condition, photos, checklist); estimates and approval; parts reservation; notifications on status change | RepairDesk, RepairShopr | No work-order state machine | P7 P8 P14 |
| Pet stores with grooming | Pet profiles with vaccinations that gate bookings; groomer calendar; pricing by breed and size; deposits; boarding capacity | MoeGo, Gingr (PetExec), DaySmart Pet | No sub-profiles for animals; no vaccination gates | P3 P4 P9 |
| Fashion and apparel | Size and colour matrix; size runs; clienteling; endless aisle; alterations; exchanges and gift receipts | Lightspeed Retail, Shopify POS, Heartland Retail, Celerant | Variant caps; no clienteling | P1 P7 P8 P9 |
| Furniture and appliances | Special-order templates; deposits and layaway; quotes; delivery scheduling across several dates; financing; service orders | STORIS | No split fulfillment or financing flow | P3 P7 P8 P11 |
| Eyewear and healthcare retail | Rx data; insurance allowances and copays; claims; lab orders and tracking; HIPAA | Compulink, MaximEyes, Jelo, iVend | Insurance maths and lab jobs | P8 P9 P11 |
| Campus and education | Student ID as tender; meal swipes and declining balance; card-system integration; kiosks | CBORD, Transact, Atrium, TouchNet (card systems); Volanté, Mashgin | The external stored-value system is the authority | P4 P5 P9 P11 |
| Museums, attractions, nonprofits | Timed-entry capacity; memberships with benefits; donations and receipts; Gift Aid‡; groups and schools; access scanning | Tessitura, Blackbaud Altru, Ticketure, accesso | Capacity inventory and constituent CRM | P3 P4 P5 P9 |

### 1.1 Flaws that recur across incumbents (the gaps to exploit)

1. **Add-on creep and per-location premiums.**
   - Vagaro starts at about $30/month, but realistic add-ons take it to $100–200+/month plus 2.2–3.5% processing ([Koalendar](https://koalendar.com/blog/vagaro-pricing)).
   - Boulevard costs $175+ per location per month, 3–7× Vagaro ([AgentZap](https://agentzap.ai/blog/vagaro-vs-fresha-vs-boulevard-salon-software-2026)).
   - Dutchie starts around $599 per location per month, plus $3–6k of hardware per register ([NextCannaConnect](https://nextcannaconnect.com/blog/cannabis-dispensary-pos)).
   - Franchise technology fees of $200–400/month often rise 8–15% a year with no cap ([FranchiseVS](https://franchisevs.com/guide/franchise-technology-requirements)).
2. **Marketplace fees on the merchant's own clients.** Fresha's marketplace fee is modelled at 20% of the ticket (minimum $6) per new client, against $0 at Vagaro ([GlossGenius](https://glossgenius.com/blog/fresha-vs-vagaro)).
3. **Offline modes push all risk onto the merchant and switch off value features.**
   - Square: 24 hours offline, and the seller absorbs declines and disputes ([Square](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode)).
   - Toast: a decline on reconnect is final ([Toast](https://support.toasttab.com/en/article/Credit-Card-Payments-FAQ)).
   - A third-party review says Toast loses loyalty, gift cards and text-to-pay while offline ([DineOpen](https://www.dineopen.com/blog/best-pos-food-truck-2026.html)).
4. **Hard data-model limits.** A 2026 buyer's guide still cites Shopify's 100-variants-per-product cap ([Runit](https://www.runit.com/feeds/resources/best-pos-womenswear-boutiques/)); Shopify has announced higher limits‡.
5. **Weak APIs.** Blackbaud is described as less integrable than Tessitura, with an API that is not as robust ([Cuberis](https://cuberis.com/the-transactional-museum-website-tickets-memberships-donations-and-more/)).
6. **Siloes between verticals.** Real businesses mix verticals: grooming plus retail plus boarding, a museum with a café and shop, hardware plus rentals, hotel outlets. Specialists force them onto several systems, with duplicate customers and inventory.
7. **Regulatory change moves at the vendor's release speed.** Examples: New York's Metrc deadlines, the end of Brazil's SAT, and Spain's delayed Verifactu. Compliance rules need to ship as data, not code.

### 1.2 Per-vertical notes

**Salons, spas, barbers**
- The most widely used platforms are Vagaro, Fresha, GlossGenius, Mindbody, Boulevard and Mangomint.
- Automated reminders, confirmations and deposits reduce no-shows, and GlossGenius charges deposits and no-show fees automatically ([Zoca](https://zoca.com/post/best-beauty-salon-software-in-2026)).
- Boulevard targets high-end salons and spas; Vagaro positions itself as a broad operating system ([AgentZap](https://agentzap.ai/blog/vagaro-vs-fresha-vs-boulevard-salon-software-2026)).
- Design-critical details (‡):
  - A service is a sequence of segments (apply, then process while the stylist is free, then finish), each needing resources.
  - Several providers can work on one service.
  - Commission is often calculated on net service revenue after product ("backbar") cost; retail commission is separate.
  - Booth renters act as sub-merchants.
  - Consent and patch-test forms are required.
  - In the EEA and UK, a later no-show charge is a merchant-initiated transaction (MIT). The card has to be set up with strong customer authentication (SCA) when the booking is made.

**Fitness and studios**
- Mindbody suits large multi-location operators; Glofox suits mobile-first boutique studios; Zen Planner suits functional-fitness gyms and supports key-fob door entry tied to payment status ([Gymdesk](https://gymdesk.com/blog/zenplanner-alternatives); [Zen Planner](https://zenplanner.com/comparison-blog/which-platform-is-best-for-boutique-fitness-in-2025-zen-planner-or-mindbody/); [Offering Tree](https://www.offeringtree.com/blog/fitness-studio-software-buyers-guide/)).
- Entitlement types to model:
  - unlimited-for-a-period
  - count-based packs with expiry
  - one-time intro offers
  - family plans
  - freezes
  - late-cancel fees
  - automatic waitlist promotion
- ‡ The FTC's federal click-to-cancel rule was vacated in July 2025, but state auto-renewal laws (for example California's, amended for July 2025) still require easy online cancellation. Cancellation policy must be configurable by jurisdiction.

**Cannabis**
- **How reporting works.**
  - POS systems connect to Metrc's API so every register transaction updates inventory and reports the sale ([Metrc](https://www.metrc.com/how-pos-and-erp-systems-integrate-with-metrc/)).
  - The integration must be two-way and real-time, and must enforce purchase limits and age checks ([WebJoint](https://www.webjoint.com/blogs/cannabis-pos-system)).
  - In medical markets the POS can check patient limits against the state registry ([BLAZE](https://www.blaze.me/blog/compliance/dispensary-compliance-guide/)).
- **Regulatory churn.**
  - New York required retail licensees to be credentialed in Metrc by 17 December 2025, and all inventory to carry package UIDs by 12 January 2026 ([Metrc NY webinar](https://www.metrc.com/wp-content/uploads/2025/12/Welcome-to-Metrc-Dispensaries-Webinar-New-York.pdf); [IndicaOnline](https://indicaonline.com/blog/understanding-metrc-compliance-for-new-york-dispensaries/)).
  - New York then moved delinquent-payment reporting into Metrc from 15 September 2026 ([NY OCM](https://cannabis.ny.gov/seed-to-sale)).
- **Vendors.**
  - Dutchie: integrated e-commerce and ACH-based Dutchie Pay.
  - Flowhub: claims debit-accepting stores earn $4,627/month more, and that its 2025 native e-commerce lifted average order value by up to 27% (vendor claims).
  - Treez: added TreezPay and Treez Loyalty.
  - Cova: strong on compliance and in Canada.
  - Smart safes (CashWizard, Tidel) are offered as integrations ([Cova](https://www.covasoftware.com/cova-insights/top-5-dispensary-pos-systems-in-2026-pros-cons); [Distru](https://www.distru.com/cannabis-blog/the-best-dispensary-pos-systems-for-cannabis); [NextCannaConnect](https://nextcannaconnect.com/blog/cannabis-dispensary-pos)).
- **Purchase limits (‡, illustrative).**
  - California adult use: 28.5 g flower and 8 g concentrate per day.
  - Illinois residents: 30 g flower, 5 g concentrate and 500 mg THC in edibles; non-residents get half.
  - Limits therefore need equivalency across categories, windows (daily or rolling) and customer class.

**Liquor and age-restricted retail**
- Must-haves: ID scans that approve or block automatically, mix-and-match deals, case-to-bottle breaking, deposits as separate lines, and state reports ([Bottle POS](https://bottlepos.com/guide-liquor-store-pos-checklist); [LMS POS](https://lmspos.com/blog/liquor-store-pos-age-verification/); [POS Nation](https://www.posnation.com/blog/benefits-of-liquor-store-point-of-sale-system)).
- Deposits are pass-through amounts, not revenue ([Bottle POS](https://bottlepos.com/blog/what-are-bottle-deposits)). California's CRV is $0.05 below 24 oz and $0.10 at 24 oz or more ([Wikipedia](https://en.wikipedia.org/wiki/California_Redemption_Value); [CAW](https://www.cawrecycles.org/how-the-california-bottle-bill-works)).
- ‡ Related rules:
  - US tobacco age 21 (T21)
  - UK Challenge 25
  - Scotland's minimum unit pricing (65p per unit)
  - UK ban on single-use vapes (June 2025)
  - local time-of-sale ("blue") laws

**Grocery**
- **Scales and barcodes.** A scale that sets price needs an NTEP Certificate of Conformance and a state Weights and Measures seal ([ScaleBlog](https://scaleblog.com/retail-scales-guide/)). The POS also needs PLU lookup, tare weights and price-embedded barcodes from deli and meat scales ([RapidCents](https://rapidcents.com/en-ca/resources/grocery-pos-buyers-guide)).
- **Self-checkout** uses weight validation ([VizionPOS](https://vizionpos.com/supermarkets/)).
- **Shrink** is 3–5% in grocery against 1.4% across retail, so lot and expiry capture at receiving matters (vendor-guide figure; [ShelfPerks](https://shelfperks.com/resources/businesstypes/choose-pos-independent-grocery-store)).
- **SNAP.**
  - Retailers need FNS authorization and a permit, plus EBT equipment from a third-party processor. Split tender covers ineligible items ([USDA](https://www.fna.usda.gov/snap/ebt/new-retailers-factsheet)).
  - Online SNAP runs only through approved processors (Fiserv, Worldpay, Forage), using an online PIN-entry API ([USDA](https://www.fna.usda.gov/snap/retailer/online/requirements)).
  - States are moving to chip EBT cards ([Virginia DSS](https://www.dss.virginia.gov/relief/food-assistance/ebt/snap-retailer-information/ebt-chip-card-modernization-retailer-faqs/)).
  - ‡ USDA approved state waivers in 2025 that restrict soda and candy under SNAP from 2026, so SNAP eligibility now varies by state and by date.
- **WIC.** The state's Approved Product List (UPCs and PLUs) must be downloaded daily. Cash-value benefits cover produce. Stores can run integrated or stand-beside systems, and must demonstrate compliance to the state ([Tennessee WIC EBT guide](https://www.tn.gov/content/dam/tn/health/program-areas/wic/Guide-to-WIC-EBT-for-Retailers-v1.1_Tennessee.pdf); [VDH](https://www.vdh.virginia.gov/wic-retailers/faqs/)).
- ‡ GS1 US "Sunrise 2027": point-of-sale scanners should read 2D barcodes (GS1 DataMatrix / Digital Link) by the end of 2027. That puts batch and expiry data at the point of scan.

**Convenience and fuel**
- Conexxus's Forecourt Device Controller (FDC) standard replaces proprietary links between the POS and dispensers, tank gauges, car washes and price poles ([Conexxus](https://www.conexxus.org/abstracts/device-integration-standards)).
- Conexxus's Age Verification API supports NACS TruAge, and more than 37% of inside transactions involve an age-restricted product ([Conexxus](https://www.conexxus.org/public-standards)).
- Purpose-built systems add age prompts, tobacco logs, state lottery terminal integration and fuel-aware shift reconciliation ([NRS](https://nrsplus.com/blog/gas-station-convenience-store-pos-system/); [Petrosoft](https://petrosoftinc.com/smartpos/)).
- ‡ Tobacco manufacturer scan-data programs require weekly item-level uploads.

**Pharmacy**
- Signature logs with HIPAA timestamps ([PrimeRx](https://www.primerx.io/electronic-signature-management-software/)).
- NPLEx and MethCheck for PSE under the Combat Methamphetamine Epidemic Act, plus IIAS for FSA cards ([Celerant](https://www.celerant.com/industries/pharmacy/); [Auto-Star](https://www.auto-star.com/best-pos-software-for-your-industry/pharmacy-pos/)).
- The SIGIS 90% rule applies only to drug stores and pharmacies whose gross receipts are at least 90% medical. Every other merchant category, including online pharmacies, needs IIAS, which also improves approval rates ([SIGIS](https://sig-is.org/programs/iias-vs-90-comparison); [SIGIS FAQ](https://www.sig-is.org/resources/faq/programs/90-registration)).
- ‡ PSE limits: 3.6 g per day and 9 g per 30 days.

**Hotels**
- **OPERA Cloud.** The modern route is the OHIP integration platform. Simphony's "OPERA Connection" supports room charges, city ledger, online and offline tender posting, guest lookup, and assigning guests to a check or seat ([Oracle](https://docs.oracle.com/en/industries/food-beverage/simphony/simcg/c_opera_connection.htm)). The legacy IFC8, FIAS and XML_POS interfaces still exist ([Oracle](https://docs.oracle.com/en/industries/hospitality/integration-platform/ohipu/c_property_interfaces.htm)). Postings need a cashier ID and transaction codes supplied by the property ([RG Nets](https://support.rgnets.com/knowledge/461)).
- **Mews** posts charges as orders to a *customer profile* (a checked-in guest) or to a Paymaster, not to a room ([Mews](https://docs.mews.com/connector-api/use-cases/point-of-sale)).
- **Cloudbeds** posts to a guest folio or a house account ([Cloudbeds](https://developers.cloudbeds.com/docs/point-of-sale)).
- Protocols in use: HTNG, IFC8 and vendor APIs ([OnlineEMenu](https://onlineemenu.com/blog/pms-vs-pos-restaurant-guide.html)).
- Implication: model the PMS as an external account behind one adapter interface.

**Stadiums, venues and events**
- **Consolidation.**
  - SpotOn bought Appetize for about $415M in 2021; Appetize reportedly served 65% of major-league stadiums ([LA Business Journal](https://labusinessjournal.com/technology/appetize-acquisition-spoton-415-million/)).
  - SpotOn then sold its sports and entertainment unit to Shift4 for $100M ([Payments Dive](https://www.paymentsdive.com/news/shift4-acquisition-spoton-sports-entertainment-unit-100-million-appetize-venues-stadium-payments-pos/695437/); [Digital Transactions](https://www.digitaltransactions.net/spoton-retreats-from-stadiums-to-focus-on-the-restaurant-pos-market/)).
  - Shift4 had already bought VenueNext for $72M ([Digital Transactions](https://www.digitaltransactions.net/magazine_articles/here-comes-stadium-pay/)).
  - Fiserv bought Bypass in March 2020, when it served 50+ stadiums and arenas ([Crowdfund Insider](https://www.crowdfundinsider.com/2020/03/159058-global-fintech-solution-provider-fiserv-acquires-bypass-mobile-an-enterprise-point-of-sale-systems-services-provider/)).
- **Requirements.**
  - Stadiums process more transactions in a four-hour window than most businesses handle in a week. Offline processing, RFID and real-time analytics separate tier-one systems from repurposed retail ones ([Billfold](https://www.billfold.tech/blog/what-is-the-best-stadium-pos-system-in-2026)).
  - HF RFID wristbands sit alongside cards and NFC ([Security Sales](https://www.securitysales.com/news/sports-music-venues-reduce-touchpoints-rfid-nfc-payments/133201/)).
  - Tapin2 offers fixed, handheld and in-seat POS plus scan-and-pay ([Tapin2](https://tapin2.com/)).
  - New Zealand's Venues Ōtautahi picked MyVenue for One New Zealand Stadium ([Yahoo Finance](https://finance.yahoo.com/sectors/technology/articles/venues-tautahi-selects-myvenue-power-175700690.html)).
- ‡ Two further constraints:
  - Unspent closed-loop balances raise refund and breakage accounting questions.
  - The FTC fee rule (May 2025) requires all-in prices for live-event tickets.

**Food trucks, markets and pop-ups**
- Square's Mobile Payments SDK allows up to 1,000 offline payments per device for 24 hours, and the seller bears declines ([Square Developer](https://developer.squareup.com/docs/mobile-payments-sdk/android/offline-payments)).
- Toast stores encrypted cards on the device and has an "offline mode with local sync" ([Toast docs](https://doc.toasttab.com/doc/platformguide/platformOfflineModeLocalSync.html)).
- Implications:
  - Local balances for gift cards and loyalty, with caps.
  - Tax set by where the truck is operating that day.
  - Per-event fees.

**Bars and nightclubs**
- A pre-auth of $1–25 when the tab opens. Incremental auths follow as the tab grows, often automatically past a threshold. Walkouts auto-close at a configured tip ([SpotOn](https://www.spoton.com/blog/bar-tabs-pre-authorization-speed-up-transactions-eliminate-walkouts/); [Bonsai](https://www.bonsaipos.com/bar-tabs-and-card-pre-authorisation-holds-walkouts-and-closing-out-at-last-call/); [Shift4](https://shift4.zendesk.com/hc/en-us/articles/6821343653651-Pre-Authorizations-and-Bar-Tabs-on-SkyTab-POS-and-SkyTab-Mobile)).
- **Card-network rules.**
  - Visa: at bars and restaurants, a final amount more than 20% above the authorized total needs an incremental auth. Once any incremental auth has been used, the tolerance no longer applies and the merchant must true up ([Visa](https://usa.visa.com/content/dam/VCOM/regional/na/us/support-legal/documents/authorization-and-reversal-processing-best-practices-for-merchants.pdf)).
  - Mastercard reinstated a 20% tip tolerance for card-not-present restaurant payments ([Toast](https://support.toasttab.com/en/article/MasterCard-20-Tip-Tolerance-Reinstated-for-Card-not-present-Restaurants)).
- **Bottle service.** Packages, minimum spends and auto-gratuity tied to tables, plus ID scanning and capacity tracking ([SZZCS](https://www.szzcs.com/blog/bar-pos-system-the-complete-guide-to-running-a-faster-and-more-profitable-bar.html); [OnlineEMenu](https://onlineemenu.com/bar-pos.html)).

**Quick-service and fast casual**
- HME's ZOOM Nitro timer integrates with POS systems so each car's wait time is paired with its POS transaction number. It also tracks mobile-order pickup and pull-forward areas ([HME](https://www.hme.com/qsr/zoom-nitro-drive-thru-timers/)). PAR sells its own timer ([PAR](https://partech.com/solutions/pos-hardware/drive-thru/drive-thru-timer/)).
- Secondary source: the average US drive-thru took 6 minutes 22 seconds in late 2025, AI-enabled chains are under 3 minutes, and Wendy's reports orders 22 seconds faster with AI voice ([XPR POS](https://www.xprpos.com/posts/ai-voice-ordering-kiosk-speed-of-service-qsr)).
- Square has launched a drive-thru offering ([Kiosk Industry](https://kioskindustry.org/square-for-drive-thru-launch/)), so general-purpose players are moving in.
- ‡ The EU Accessibility Act has applied to self-service terminals since 28 June 2025.

**Coffee shops**
- Modifier depth (milks, sizes, shots, syrups, each with its own price), barista labels, order-ahead, stamp loyalty and subscription billing ([Posso](https://www.posso.co.uk/cafe-epos-system); [Swipe Savvy](https://swipesavvy.com/solutions/cafe/)).
- Vendor claim: loyalty raises visit frequency by 20–30% ([GoSnappy](https://gosnappy.io/blog/best-pos-system-for-coffee-shops/)).

**Bakeries and pre-orders**
- Custom-order details, deposits and future pickup dates ([Menusifu](https://www.menusifu.com/blog/best-bakery-pos-system)).
- Pre-orders that feed the production schedule, daily order limits and pickup times ([Restomas](https://www.restomas.com/blog/pre-order-systems-for-bakeries-smarter-production-faster-pickup); [BakeSmart](https://bakesmart.com/)).
- Catering, bulk and corporate boxes ([Orders.co](https://orders.co/blog/best-pos-for-bakeries/)).
- ‡ UK Natasha's Law requires allergen labels on food prepacked for direct sale.

**Catering and events** (‡ mostly)
- Flow: quote, contract, deposit schedule, final headcount, then invoice.
- Service charges and gratuities are taxed and paid to staff differently.
- Tax-exempt clients, delivery windows and BEO generation.
- Vendors: Toast Catering & Events, Tripleseat, CaterZen.

**Franchises**
- Royalties are usually a percentage of *gross* sales (before discounts and returns), paid monthly ([Tipalti](https://tipalti.com/blog/franchise-royalty-fee/); [Blue Cloud CPA](https://bluecloudcpa.com/guides/franchise-bookkeeping-chart-of-accounts-royalties)).
- Franchisors mandate the vendor ([FranchiseVS](https://franchisevs.com/guide/franchise-technology-requirements)).
- PAR Brink is widely deployed in North American franchise QSR, including 3,200+ Papa Johns US locations as of January 2026 ([POSUSA](https://www.posusa.com/brink-pos-review/)). Toast Enterprise serves multi-unit brands ([Toast](https://pos.toasttab.com/restaurant-pos/enterprise)).

**Hardware, auto parts and B2B counter sales**
- **Hardware and building supply.**
  - Tens of thousands of SKUs, vendor catalogs, contractor charge accounts, customer-specific pricing, POs, special orders, delivery and rentals ([AppIntent](https://www.appintent.com/software/point-of-sale/retail/hardware-store/)).
  - Epicor Eagle offers matrix pricing, special orders and rental tracking ([SYS Solutions](https://syssolutionsllc.com/useful-articles/hardware-store-pos-comparison-find-the-best-fit/)).
  - BisTrack serves lumber and building-materials dealers, with dispatch and contractor pricing ([Epicor](https://www.epicor.com/en-us/solutions/industries/building-supply/)).
- **Auto parts.**
  - ACES (fitment) and PIES (product attributes) are updated annually and required by major retailers ([Auto Care](https://automotiveaftermarket.org/aftermarket-industry-trends/aces-pies-data-explained/)).
  - Core charges are added automatically and refunded through a core-return flow; supersessions refresh on a schedule ([Parts Square](https://square.parts/solutions/automotive-catalog-data)).

**Rental businesses**
- Digital contracts, e-signatures, damage waivers, and partial payment at booking or walk-in ([Quipli](https://www.quipli.com/solutions/rental-equipment-checkout-software/)).
- Inspections with photos and repair deadlines ([EZRentOut](https://ezo.io/ezrentout/)).
- Availability calendar and seasonal availability rules ([EquipDash](https://equipdash.com/blog/equipment-rental-booking-software)).
- Metered and recurring billing ([Integra](https://www.integrarental.com/blog/equipment-rental-software-guide/)).
- Work orders attached to equipment ([Rent in Hand](https://rentinhand.com/features/)).
- Visa has widened eligibility for estimated and incremental authorizations ([Visa](https://www.visa.com.bz/content/dam/VCOM/global/support-legal/documents/ai09108.pdf)).
- ‡ Holds expire under network rules, so multi-week deposits need a re-auth or a card-on-file fallback.
- ‡ Ski rentals capture height, weight, age and ability for binding (DIN) settings.

**Repair shops**
- RepairDesk tickets carry due date, status, technician, linked parts, device tags, private and diagnostic notes, photos and a pre-repair checklist, and send SMS or email on status changes ([RepairDesk](https://www.repairdesk.co/features/repair-ticket-management-software/)).
- RepairShopr pairs ticketing with retail and inventory ([Bytephase](https://bytephase.com/blog/what-are-the-best-repair-ticket-software-options-for-small-businesses/)).

**Pet stores with grooming**
- MoeGo shows vaccination, medication and allergy alerts at booking and check-in, allows unlimited pets per client, and supports deposits and saved cards ([MoeGo](https://www.moego.pet/pet-grooming-software)).
- Gingr and PetExec (now part of Gingr) cover grooming alongside boarding and daycare ([Animalo](https://www.animalo.com/blog/pet-grooming-software-ultimate-2026-guide-for-salons); [PetExec](https://www.petexec.net/service/groomers)).

**Fashion and apparel**
- Lightspeed's matrix inventory by size and colour, with vendor catalogs, POs and transfers ([Lightspeed](https://www.lightspeedhq.com/pos/retail/apparel/)).
- Clienteling profiles with sizes, preferences and notes ([Runit](https://www.runit.com/feeds/resources/best-pos-womenswear-boutiques/)).
- Endless aisle ([Celerant](https://www.celerant.com/blog/top-3-apparel-pos-systems-to-sell-smarter/)).
- ‡ Alterations are small work orders with a tailor and due date.

**Furniture and appliances**
- STORIS covers sales orders, layaways, quotes, exchanges, returns and service orders. Special-order templates capture fabric and finish. Several delivery dates, pickups and direct shipments can sit on one order, with delivery dates checked against logistics calendars ([STORIS](https://www.storis.com/point-of-sale-software/)).
- Synchrony, LendPro and Affirm financing runs inside the POS ([STORIS](https://www.storis.com/blog/consumer-financing-furniture-retail/)).

**Eyewear and healthcare retail**
- Plan rules (VSP, EyeMed, Davis, Spectera) hold frame allowance, lens coverage and copay, and calculate what the patient pays at checkout ([KwickOS](https://kwickos.com/blog/optical-eyewear-shop-pos-guide.html)).
- Lab ordering integrations such as VisionWeb ([iPlum](https://www.iplum.com/blog/best-optometry-ehr-software)).
- Rx fields: sphere, cylinder, axis and PD ([Glasson](https://www.glasson.app/blog/optical-shop-software-why-your-choice-determines-whether-you-scale-or-struggle/)).

**Campus and education**
- Card systems: Atrium, CBORD, TouchNet, Transact and Trove ([Grubhub Onsite](https://onsite.grubhub.com/solutions/partnerships/campus-cards/)).
- Tenders: declining-balance dollars plus meal swipes ([Volanté](https://www.volantesystems.com/blog/volante-meal-plans-declining-balance-for-students/)).
- Mashgin kiosks accept declining-balance funds at 50 new campuses ([CampusIDNews](https://www.campusidnews.com/fifty-new-campuses-deploy-mashgins-ai-powered-kiosks-that-accept-declining-balance-funds/)).
- Buyers' guidance is "integrate, don't replace" ([Dining Connect](https://diningconnect.com/insights/integrate-campus-dining-with-existing-systems/)).

**Museums, attractions and nonprofits**
- Altru ties ticketing (general, timed and event) to donor and membership records, with kiosks and digital membership cards ([Blackbaud](https://www.blackbaud.com/products/blackbaud-altru)).
- Tessitura adds capacity control, packages, group sales and unlimited membership levels ([Tessitura](https://www.tessitura.com/markets/museums-and-galleries)).
- ‡ UK Gift Aid needs a declaration and, for admissions, a donation-inclusive price.

---

## 2. Global market notes

Each market below covers four things: players, payments, regulatory must-haves and localization. Fiscal detail is kept brief because another researcher covers it.

### Europe (fiscalization-driven, local champions)

- **Regulatory.**
  - **Germany (KassenSichV).** Every transaction is signed by a certified TSE. Data stays local and is produced on audit ([fiskaly](https://www.fiskaly.com/blog/kassensichv-and-tss-fiscalization-in-germany)). Since 1 January 2025 all registers and TSEs must be registered through ELSTER; existing systems had until 31 July 2025 ([kassensichv.net](https://kassensichv.net/en/articles/new-reporting-obligations-2025-cash-registers-taximeters-odometers)).
  - **France (NF525).** Self-certification has ended: third-party certification has been mandatory since 1 March 2026.
  - **Austria (RKSV).** A "Cash Register Package 2026" adds digital receipts from October 2026.
  - **Poland.** Online registers report to the central repository (CRK), and KSeF e-invoicing starts in 2026 ([fiskaly Europe](https://www.fiskaly.com/blog/fiscalization-and-tax-compliance-in-europe); [OpenText](https://blogs.opentext.com/e-invoicing-europe-2026-lessons-from-poland-belgium-greece-france-and-germany/)).
  - **Spain (Verifactu).** Delayed by Royal Decree 15/2025 to 1 January 2027 for corporate-tax payers and 1 July 2027 for everyone else. TicketBAI applies in the Basque Country and Navarra ([KPMG](https://kpmg.com/us/en/taxnewsflash/news/2025/12/tnf-spain-verifactu-invoicing-system-delayed-to-2027.html); [VATCalc](https://www.vatcalc.com/spain/spain-verifactu-delay-till-jan-2027-for-certified-e-invoicing/)).
  - **Certification models** differ between third-party and self-certification ([fiskaly](https://www.fiskaly.com/blog/third-party-vs-self-certification-european-fiscalization)).
  - ‡ Italy: registratore telematico, and card terminals must be linked to it from 2026.
  - ‡ Also: Portugal (certified software, ATCUD and QR), Greece (myDATA and terminal linkage), Hungary (online registers), Sweden (control units), Belgium (B2B Peppol from 2026), and the EU's ViDA package (intra-EU e-invoicing by 2030).
  - ‡ Germany made restaurant food 7% VAT from 1 January 2026, with drinks staying at 19%. It is a mid-life rate split by item type.
- **Local champions (‡).**
  - Germany: orderbird, Gastrofix (Lightspeed), Vectron
  - Austria: ready2order
  - France: Zelty, L'Addition, Tiller (SumUp), Innovorder
  - Spain: Revo, Glop
  - Italy: Cassa in Cloud, Tilby
  - Nordics: Caspeco, Trivec
  - Pan-EU: Lightspeed, SumUp, Zettle, Square, Toast (UK and Ireland)
- **Payments (‡).**
  - Domestic methods: girocard, Cartes Bancaires, Bancontact, iDEAL (moving to Wero), Bizum, MB WAY, BLIK, Swish, Vipps MobilePay, TWINT and Satispay.
  - Meal vouchers (Edenred, Pluxee, Swile, Up) only apply to eligible food and carry daily caps.
  - PSD2/SCA: contactless cumulative limits, and MIT for no-show fees.
  - Surcharges are banned on consumer cards in the EU and UK.
- **Localization.**
  - Decimal comma and VAT-inclusive prices.
  - Separate eat-in and takeaway VAT rates.
  - ‡ Cash rounding to 5c in the Netherlands, Belgium, Finland, Ireland and Italy.
  - GDPR.
  - ‡ The EAA's accessibility duties for kiosks and payment terminals.

### UK

- **Players.** Epos Now (from about £25 per terminal per month) and Zonal (Edinburgh-based, also known as Aztec), which gives groups one ecosystem for POS, bookings, loyalty and ordering. Also Lightspeed, Toast, and the British specialists Tevalis and TISSL ([Tech on Toast](https://www.techontoast.community/blog/pos-systems-uk-hospitality-2026); [Switch & Save](https://switch-and-save.uk/blog/best-hospitality-epos-systems-uk-comparison/); [Mobile Transaction](https://www.mobiletransaction.org/restaurant-pos-systems-uk/)).
- **Payments.**
  - Since March 2026, issuers may set their own contactless limits instead of the fixed £100 cap; most kept their existing limits ([BRC](https://brc.org.uk/news-and-events/news/finance/2026/ungated/changes-to-contactless-limits/)).
  - Pay by Bank had more than 15M users in 2025 and about 33M transactions in November 2025 ([Open Banking](https://www.openbanking.org.uk/pay-by-bank/)).
- **Regulatory.**
  - No fiscalization‡.
  - Making Tax Digital for income tax starts April 2026 ([GoCardless](https://gocardless.com/blog/mtd-itsa-income-tax-changes-2026-guide)).
  - ‡ Also: Making Tax Digital for VAT (digital links), the Employment (Allocation of Tips) Act (October 2024), Natasha's Law allergen labels, calorie labelling for large chains, and Scotland's minimum unit pricing.
- **Localization.** en-GB, VAT-inclusive prices, postcodes.

### India

- **Players.**
  - Petpooja: 50k+ restaurants, strong in tier-2/3 QSR.
  - Restroworks (formerly Posist): 20k+ locations and enterprise chains.
  - UrbanPiper: aggregator middleware.
  - Vendor blogs claim these cover about 85% of the organised market ([Forkcast](https://www.forkcast.in/blog/petpooja-vs-urbanpiper-vs-posist); [TechArion](https://techarion.com/blog/restaurant-pos-system-india-comparison-2025); [DineOpen](https://www.dineopen.com/vs/petpooja-vs-posist)).
- **Payments.**
  - UPI processed 228.5B transactions in 2025, about 84% of digital payments ([Petpooja blog](https://blog.petpooja.com/operations-workflows/best-pos-for-my-restaurant/)).
  - August 2026 set a record of 24.51B transactions worth ₹29.82 lakh crore, about 790M a day ([International News & Views](https://www.internationalnewsandviews.com/upi-transactions-august-2026-record-24-51-billion-407686-2/)).
  - There were about 678M QR codes against 11.2M card terminals (H1 2025), and person-to-merchant payments are about 63% of UPI volume ([CoinLaw](https://coinlaw.io/upi-statistics/); [Meetanshi](https://meetanshi.com/blog/upi-statistics/)).
  - Implication: a dynamic UPI QR, confirmed automatically, is the default tender.
- **Regulatory.**
  - E-invoicing (IRN plus QR) applies above ₹5 crore turnover. Without an IRN the buyer cannot claim input tax credit. Taxpayers at ₹10 crore+ must report within 30 days, from 1 April 2025 ([ClearTax](https://cleartax.in/s/e-invoicing-gst); [Tally](https://tallysolutions.com/accounting/e-invoicing-rules-in-india/)).
  - B2C dynamic QR codes are required at ₹500 crore+ ([GST e-invoice portal](https://einvoice6.gst.gov.in/content/e-invoice-b2c-qr-code-applicability-penalty-contents-generation-exemption-list/)).
  - Restaurants pay 5% GST without input credit, or 18% with credit in hotels whose room tariff is ₹7,500+ ([ClearTax](https://cleartax.in/s/impact-gst-food-services-restaurant-business)).
  - ‡ GST rates were rationalized in September 2025.
- **Localization (‡).**
  - Lakh/crore grouping (1,00,000).
  - Many scripts: Devanagari, Tamil, Telugu, Bengali, Gujarati and others.
  - GSTIN, HSN/SAC codes, and the CGST+SGST vs IGST split by place of supply.
  - Swiggy and Zomato integration.

### Brazil

- **Players and payments.**
  - Stone, PagBank and Cielo lead with smart terminals. Fees for paying out card receivables early are a core revenue line, and Pix is squeezing margins ([Brazil Stock Guide](https://brazilstockguide.com/insights/stone-pagbank-face-q3-miss-as-pix-squeezes-margins/)).
  - Pix was 34% of POS transaction value in 2025 (Worldpay). "Pix by Proximity" (NFC) launched in February 2025 ([ClearingPost](https://clearingpost.com/insights/pix-captures-one-third-brazil-pos-payments-2025-worldpay/)).
  - ‡ Interest-free instalments ("parcelado") and meal vouchers (VR/VA) are standard.
- **Regulatory.**
  - Since 1 January 2026, NFC-e (model 65) is the only retail fiscal document in every state. It replaces ECF, SAT and MFe ([Fiscal Solutions](https://www.fiscal-requirements.com/news/4792)); São Paulo's SAT ended on that date ([Fiscal Solutions](https://www.fiscal-requirements.com/news/5095)).
  - NFC-e can no longer be issued to companies, so they need an NF-e ([Fiscal Solutions](https://www.fiscal-requirements.com/news/4821)).
  - Each document is signed XML authorized in real time by SEFAZ, with a QR code on the DANFE receipt.
  - New layouts for the CBS/IBS tax reform started in 2026 ([Fonoa](https://www.fonoa.com/resources/blog/brazil-tax-reform-e-invoicing-2026)).
  - Goiás requires payment data (cards and Pix) to be filled in the fiscal XML automatically, as reported in [Fiscal Solutions](https://www.fiscal-requirements.com/news/4734-brazils-quiet-tax-revolution-what-retailers-need-to-know-about-the-new-era-of-fiscalization) coverage.
- **Localization (‡).** pt-BR; CPF and CNPJ validation; CPF printed on the receipt; NCM/CEST codes per item; ICMS tax codes by state; offline contingency issuing.

### Mexico

- **Players and payments.**
  - Mercado Pago has more than 1M terminals, and Clip is the other major aggregator. Mobile and portable devices were 67.97% of 2025 installations.
  - CoDi had only about 11.9M transactions by Q1 2024 (under 1% of daily flows). DiMo gained 7.5M users, but merchant acceptance is patchy ([Mordor](https://www.mordorintelligence.com/industry-reports/mexico-pos-terminals-market)).
  - Fees: Mercado Pago about 3.5% plus VAT, Clip 3.6% plus VAT ([CDMX Informa](https://cdmxinforma.com/mercado-pago-vs-clip-cual-terminal-de-cobro-conviene-mas-en-mexico-2026/)).
  - Clip Pin Pad (November 2025) targets merchants that run their own POS, a semi-integrated model ([Inmobiliare](https://inmobiliare.com/clip-lanza-clip-pin-pad-para-mejorar-pagos-en-grandes-empresas-en-mexico/)).
- **Restaurant POS.**
  - Soft Restaurant (formerly National Soft) supports CFDI and AutoFactura: a QR on the ticket lets the customer invoice themselves ([Soft Restaurant](https://softrestaurant.com/cfdi); [AutoFactura](https://softrestaurant.com/yoquieroautofactura)).
  - Parrot has 1,500+ restaurants and integrates with bank terminals ([Parrot](https://parrotsoft.mx/)).
- **Regulatory (‡).**
  - CFDI 4.0: an individual invoice on request, otherwise a periodic "factura global" to RFC XAXX010101000.
  - Invoices are stamped by an authorized provider (PAC).
  - VAT is 16%, or 8% in the border zone, plus IEPS excise.
- **Localization.** es-MX, MXN, RFC validation.

### Southeast Asia

- **Malaysia.**
  - StoreHub (20,000+ businesses in the region) was the first POS fully integrated with LHDN's e-invoicing, and consolidates and submits to MyInvois automatically ([StoreHub](https://www.storehub.com/my/einvoice)).
  - Retail flow: the receipt carries a QR, the buyer requests an e-invoice, and LHDN validates it ([JomeInvoice](https://jomeinvoice.my/article/retail-pos-einvoice-workflow-malaysia/)).
  - Qashier costs RM158–238 a month; HitPay and BigPOS compete ([HitPay](https://hitpayapp.com/blog/point-of-sale-software-comparison-malaysia)).
  - ‡ DuitNow QR; phase-in by turnover; consolidated B2C e-invoices.
- **Rest of the region (‡).**
  - Singapore: PayNow/SGQR, NETS, 9% GST, InvoiceNow (Peppol) being phased in.
  - Thailand: PromptPay/Thai QR, 7% VAT, Buddhist-era dates (2026 = BE 2569).
  - Indonesia: QRIS national QR, including QRIS Tap (NFC) and cross-border links.
  - Philippines: registration of POS devices with the BIR (tax bureau); 20% discount plus VAT exemption for seniors and people with disabilities, with their ID numbers logged on the receipt; GCash/Maya.
  - Vietnam: e-invoices generated from cash registers (Decree 70/2025).
  - Regionally: GrabPay.

### Japan (‡, not verified this session)

- **Players.** AirREGI (Recruit; free iPad POS plus the AirPAY multi-payment terminal), Square, Smaregi, Ubiregi, STORES. Chains run on Toshiba TEC and NEC.
- **Payments.**
  - Credit cards including JCB.
  - Transit IC cards (Suica, PASMO).
  - E-money (iD, QUICPay, WAON, nanaco).
  - QR wallets (PayPay, Rakuten Pay, d払い).
  - METI reported a cashless ratio of about 42.8% in 2024.
  - Automatic change machines integrated with the POS are common.
- **Regulatory.**
  - Qualified-invoice system since 1 October 2023: registration number "T" plus 13 digits, and tax totals per rate.
  - Tax is rounded once per rate per invoice.
  - 10% standard rate vs 8% reduced rate (eat-in vs takeout).
  - Tax-inclusive price display is required.
  - Revenue stamps on large paper receipts.
- **Localization.** Kanji and kana with full-width and half-width characters; furigana; family name first; addresses from postcode down; Reiwa-era dates; yen has no decimals.

### China (‡, not verified this session)

- **Players.** Meituan's merchant SaaS, Keruyun, Hualala, 2dfire, Pospal, Youzan.
- **Payments.**
  - Alipay and WeChat Pay dominate. The POS must scan customer-presented codes and also show merchant QRs.
  - UnionPay, plus e-CNY pilots.
  - Scan-to-order through WeChat mini-programs.
  - Redeeming group-buying vouchers bought on Meituan or Douyin is a core POS flow.
- **Regulatory.** Fully digital e-fapiao issued on request; PIPL and data localization; GB 18030 encoding.
- **Localization.** Simplified Chinese; phone-number-centric identity.

### Africa (‡, not verified this session)

- **Kenya.**
  - M-Pesa till and Paybill numbers, with the Daraja API's STK push.
  - KRA eTIMS applies to all businesses, integrating through OSCU or VSCU. Expenses without an eTIMS invoice are not deductible.
- **Nigeria.**
  - Moniepoint is the largest merchant and agent POS network (a unicorn since 2024); OPay and PalmPay also compete.
  - Instant bank transfer (NIP) is a common tender, but confirmation is hard.
  - The tax authority's e-invoicing is phasing in during 2025–26.
- **South Africa.** Yoco, iKhokha and PayShap. Power cuts make offline operation and battery life essential.
- **Fiscal devices.** Tanzania EFD/VFD, Rwanda EBM, Uganda EFRIS.
- **Localization.** English, French, Swahili, Arabic and Portuguese; SMS and USSD receipts; cash rounding where coins are scarce; FX volatility means frequent repricing.

### Middle East (‡, not verified this session)

- **Players.** Foodics (Riyadh; the MENA restaurant leader, which also appears in Indian comparisons: [TechArion](https://techarion.com/blog/restaurant-pos-system-india-comparison-2025)), Marn, Rewaa, Geidea, Sapaad.
- **Payments.**
  - Saudi Arabia: mada debit, very high Apple Pay use, STC Pay.
  - UAE: the Aani instant-payment system and the Jaywan card scheme.
  - Buy-now-pay-later: Tabby and Tamara.
  - Egypt: Meeza and InstaPay.
- **Regulatory.**
  - **Saudi Arabia (ZATCA Fatoora).**
    - Invoice generation since December 2021; integration in waves since 2023.
    - B2C simplified invoices are reported within 24 hours; B2B invoices are cleared before issue.
    - Format is UBL 2.1 XML, with a cryptographic stamp per device.
    - Each device keeps an invoice counter and a chain of previous-invoice hashes.
    - A TLV QR code and Arabic are mandatory.
  - **UAE:** Peppol-based e-invoicing from 2026–27.
  - **Egypt:** e-receipts.
  - **VAT:** 15% in Saudi Arabia, 5% in the UAE.
- **Localization.** RTL mirroring; Arabic shaping on receipts (use raster printing when the printer can't shape); Hijri dates; Friday–Saturday weekend; three-decimal currencies (KWD, BHD, OMR).

### Australia and New Zealand (‡ except where linked)

- **Players.**
  - Square; Lightspeed (bought Kounta, now Lightspeed Restaurant O-Series); Abacus; H&L; Swiftpos; Bepoz and Idealpos for clubs and pubs; ROLLER for attractions; me&u for order-at-table.
  - Terminals from Tyro and Zeller.
  - Venues: MyVenue won New Zealand stadium contracts ([Yahoo Finance](https://finance.yahoo.com/sectors/technology/articles/venues-tautahi-selects-myvenue-power-175700690.html)).
- **Payments.** eftpos on dual-network debit cards with least-cost routing; PayTo/Osko; Afterpay/Zip.
- **Surcharging.**
  - Currently allowed up to the cost of acceptance.
  - The RBA's 2025 review proposed ending surcharges on eftpos, Mastercard and Visa and cutting interchange. The final decision and date need confirming.
  - New Zealand announced a ban on in-store surcharges.
- **Tax.** No fiscalization. GST is 10% in Australia and 15% in New Zealand. Australian tax invoices over A$82.50 need the ABN.
- **Other.** Public-holiday surcharges in hospitality; cash rounding to 5c (Australia) and 10c (New Zealand); club membership and gaming integrations.

### Canada (‡, not verified this session)

- **Players.** Lightspeed (Montréal), Square, Toast, Clover, Moneris, TouchBistro, Maitre'D (Québec), and Cova for cannabis.
- **Payments.** Interac Debit (with Flash contactless) and Interac e-Transfer; tipping at the terminal. Credit surcharges are allowed under a cap, except in Québec.
- **Regulatory.**
  - Tax combinations: GST 5%, HST 13–15%, PST, and QST 9.975%.
  - Point-of-sale rebates, such as Ontario's on prepared food under $4.
  - A GST/HST holiday ran from 14 December 2024 to 15 February 2025, a date-bound rule change.
  - Québec restaurants must use Revenu Québec's sales recording module (SRM), which is moving to MEV-WEB.
  - Québec's Charter of the French Language (Bill 96; provisions effective June 2025) requires French-first documents, and staff are entitled to French work tools.
- **Localization.** Bilingual receipts; fr-CA formatting ("1 234,56 $"); postal codes in A1A 1A1 format; cash rounding to 5c.

### Patterns across markets

1. **Account-to-account and QR rails lead in growth markets.** UPI is about 84% of India's digital payments, and Pix is 34% of Brazil's POS value. A payment is asynchronous (pending, then confirmed), not a synchronous card auth.
2. **Real-time clearance and reporting keeps spreading.** Examples: NFC-e, ZATCA, MyInvois, eTIMS, KSeF, Verifactu. The fiscal step must be part of the commit path, with contingency modes.
3. **Payments and fiscal documents are being coupled.** Goiás requires payment data in the fiscal XML; ‡ Italy and Greece link terminals to registers.
4. **Customers can request an invoice after the sale.** Mexico's autofactura, Malaysia's e-invoice on request, and NF-e for Brazilian company buyers all allow it.
5. **Surcharging, contactless-limit and fee-display rules keep diverging and moving.** They belong in configuration.

---

## 3. Cross-vertical core primitives

These are the smallest set of generic building blocks that express all 25 verticals. Six are foundational and every vertical uses them: **P1, P5, P10, P11, P12, P15**. The other nine enable specific verticals.

**P1: Catalog and pricing model.**
- Items with any number of variant dimensions (sizes, colours, finishes) and deep modifier groups with per-option prices.
- Bundles, kits and combos.
- A graph of units of measure with conversions: case and bottle, board-foot and linear foot, grams.
- Serial, lot and regulated package IDs (Metrc UIDs).
- Price rules scoped by customer level, zone, time window, channel, mix-and-match and contract. Each rule has effective dates and an explanation trail.
- *Serves:* all verticals; critical for apparel, liquor, hardware and B2B, coffee, QSR combos, cannabis, franchise price zones.

**P2: Measured and variable-price items.**
- Weight, volume, length, time or meter readings from certified devices, with tare and stability checks.
- Parsing of price- or weight-embedded barcodes and GS1 Application Identifiers.
- Open-price items with permission rules.
- *Serves:* grocery, deli and bakery by weight, bulk, fuel (volume), hardware (rope and chain by the foot), metered rentals, cannabis (weighed deli in some markets‡).

**P3: Bookable resources and capacity.**
- Resources: staff with skills, rooms, chairs, equipment, kennels, tables, delivery trucks.
- Services are chains of segments with resource needs and gaps.
- Capacity buckets: classes, timed entry, pickup slots, delivery windows, daily production caps.
- Waitlists, recurrence, online and kiosk booking, deposits, and policies.
- *Serves:* salons and spas, fitness, pet grooming and boarding, museums, bakery and catering pickup and delivery, furniture delivery, repair drop-off, eyewear exams, rentals, restaurant reservations, order-ahead throttling.

**P4: Entitlements and recurring agreements.**
- A contract (subscription or membership) grants entitlements: unlimited access for a period, count packs, counters that reset each period (meal swipes a week), percentage benefits, guest passes, access rights.
- Includes freeze, cancellation per jurisdiction, dunning, family sharing and rollover.
- Redemption is idempotent and works offline within bounds.
- *Serves:* fitness, salon and med-spa memberships, coffee subscriptions, museum memberships, campus meal plans, pet daycare packs, car-wash clubs‡, wholesale memberships, loyalty tiers.

**P5: Unified value ledger and accounts.**
- A double-entry ledger per legal entity and currency.
- It holds gift cards, closed-loop wallets (wristbands, campus declining balance), store credit, deposits, loyalty liabilities and house accounts (receivables with limits, statements and aging).
- External accounts (PMS folio, campus card system) are modelled as ledger counterparties.
- Includes breakage and escheat reporting.
- *Serves:* venues, campus, B2B and hardware, hotels, franchises (liabilities between entities), catering and bakery deposits, rentals, clubs.

**P6: Open orders with payment holds (tabs).**
- Long-lived orders that build up over time.
- Pre-auth, incremental auth and card-on-file, with network-tolerance tables.
- Auto-close with a configured gratuity.
- Split by seat, item or amount; transfer between staff; minimum spends.
- *Serves:* bars and nightclubs, full-service restaurants, hotel outlets, venue suites, rental deposits, fuel pre-auth, golf and club houses.

**P7: Deferred-fulfillment orders and routing.**
- Order lifecycle: quote, order, deposit, procurement, receiving, notification, pickup or delivery, balance.
- Several fulfillment groups per order, each with its own date or slot.
- Items are routed to stations, production lists and labels.
- Capacity limits come from P3.
- *Serves:* bakery custom cakes, catering, furniture and appliances, hardware and auto special orders, eyewear lab orders, apparel special orders, QSR and coffee order-ahead, B2B.

**P8: Custody jobs (work orders and rentals).**
- One state machine for goods in someone else's custody.
- *Inbound* (repair, alteration, lab job): intake with serial or IMEI, condition, photos, checklist and signature; estimate and approval; parts and labour; technician; status notifications; pickup.
- *Outbound* (rental): check-out, time-rate ladder, hold or deposit, return with a condition comparison, damage and late fees, maintenance lockout.
- *Serves:* repair shops, alterations, eyewear labs, bike and ski tuning, equipment, ski, bike and party rentals, appliance service orders, pet grooming checklists.

**P9: Identity, verification and consent.**
- A customer graph: people, households, organizations, and dependents or assets (pets, vehicles, devices, students, patients) with typed attributes such as vaccination expiry, Rx, sizes and skier data.
- ID scanning (AAMVA PDF417, ICAO MRZ), age gates, and lookups against patient, student or member registries.
- Waivers and consents, tax-exempt certificates.
- Retention class per attribute.
- *Serves:* liquor, tobacco, cannabis, bars, pharmacy (PSE), rentals, fitness waivers, salon consents, B2B, campus, museums, the Philippine senior discount‡.

**P10: Policy and compliance engine, plus traceability outbox.**
- Declarative rules with versions, jurisdiction and effective dates: age thresholds, purchase limits with equivalency and rolling windows, sale-time windows, quantity caps, required prompts, tender eligibility.
- Rules are evaluated on the device from signed rule packs.
- Every regulated event goes through a durable, idempotent outbox to adapters: Metrc, BioTrack, NPLEx, lottery, scan data, EBT/WIC.
- *Serves:* cannabis, liquor, c-store, pharmacy, grocery (SNAP/WIC waivers), venues (alcohol cutoffs), and every regulated market.

**P11: Tender orchestration and eligibility.**
- Several tenders per order.
- Eligibility per line: SNAP, WIC, FSA/IIAS, meal vouchers, campus plans, restricted gift cards.
- Pluggable payment rails: cards, domestic debit, A2A/QR, push-to-phone, wallets, pay-by-bank, BNPL and financing, EBT.
- An asynchronous confirmation state machine.
- Cash rounding per currency.
- Surcharge and dual-pricing rules.
- *Serves:* every vertical; critical for grocery, pharmacy, campus and all non-card-first markets.

**P12: Fiscal and document engine.**
- Numbering series per device, signature and hash chains, receipt and invoice templates with QR codes and legal fields.
- E-invoice generation, clearance and reporting.
- Contingency mode.
- Buyer-requested invoices after the sale.
- Journal exports (for example Germany's DSFinV-K, SAF-T).
- Documents are immutable, with reversals instead of deletes.
- *Serves:* every country; B2B invoicing in all verticals.

**P13: Org hierarchy, governance and multi-party settlement.**
- Hierarchy: tenant, brand, legal entity, region, location, revenue centre, device.
- Settings are inherited, with locks and overrides per attribute.
- Scheduled rollouts.
- Royalty, ad-fund and revenue-share calculation; settlement between entities.
- *Serves:* franchises, multi-location chains, hotels (outlets), venues (concessionaires), campus (multiple operators), food halls‡.

**P14: Staff attribution and earnings.**
- Several staff per line with split percentages.
- Commission plans: tiered, retail vs service, cost deductions, clawbacks.
- Tip pooling and allocation; classification of service charge vs gratuity.
- Booth-renter and consignment payouts; payroll exports.
- *Serves:* salons, spas, barbers, restaurants, bars, catering, furniture sales, repair technicians, venues.

**P15: Sessions, events and offline-first operation.**
- A business day that is independent of the calendar day.
- Shifts, drawers and blind counts.
- Events that scope menus, prices, inventory and stands.
- Location of sale that drives tax.
- A local-first data store with sync between devices on the LAN.
- Store-and-forward payments with configurable risk caps.
- *Serves:* venues, food trucks and markets, bars, c-stores (fuel shift reconciliation), catering, museums (events), and every market with poor connectivity.

**Worked example:** a pet store with grooming and boarding uses P1 (retail), P3 (groomers and kennels), P4 (daycare packs), P9 (pet profiles and vaccination gates), P11 and P14 (groomer commissions). No bespoke module is needed.

---

## 4. Implications for our design: concrete, testable requirements

**Catalog, units and measured goods**

1. **R1 Variants.** Support at least 3 option dimensions and at least 2,000 variants per product, each with its own SKU, barcode, price and stock, plus size-run pack purchasing.
   - *Test:* create a 3-D item with 1,800 variants; a barcode lookup on the device, offline, takes ≤150 ms.
2. **R2 Units of measure.** Stock, sell and purchase in different units with conversions and fractional quantities; case breaking is an audited inventory event.
   - *Test:* selling 1 bottle from a 12-bottle case leaves 11 bottles, with an audit record.
3. **R3 Certified scales.** Accept only stable, non-zero weights, and apply tare. Manual weight entry needs a role and is logged. Test with motion and zero flags from a scale simulator.
4. **R4 Barcode profiles per market.** Parse UPC/EAN embedded price and weight, GS1 DataBar, and GS1 DataMatrix/Digital Link AIs (01, 10, 17, 3103, 392x).
   - *Test:* an expired or recalled batch is blocked unless overridden with a code.
5. **R5 Price rules.** Customer, zone, time (happy hour, public-holiday surcharge), channel, mix-and-match and case discounts, each with effective dates. Precedence is deterministic, and every price can show why it applies.

**Scheduling and entitlements**

6. **R6 Resource scheduler.** Service segments with resource needs and processing gaps; capacity buckets; waitlists that promote automatically; safe under concurrency across web, kiosk and POS.
   - *Test:* 50 concurrent requests for the last slot produce exactly 1 booking.
7. **R7 Booking policies.** Deposit %, cancellation windows, and no-show fees, with versioned card-on-file consent. In SCA markets, the card is set up as a customer-initiated payment with SCA and charged later as an MIT carrying the network reference.
   - *Test:* an EEA no-show fee is charged with no customer present, and the consent record can be retrieved.
8. **R8 Entitlements engine.** Unlimited, count, period-reset (for example 14 swipes a week), benefit and guest-pass types, with expiry, freeze, sharing and rollover. Redemption is idempotent and works offline with a bounded overdraft.
   - *Test:* the same pack redeemed on two offline devices reconciles to one valid redemption and one flagged exception.
9. **R9 Access hooks.** A change in entitlement status reaches door, turnstile and scanner integrations within 2 seconds. Cancellation and auto-renewal rules are configurable by jurisdiction.

**Money: ledger, tabs and tenders**

10. **R10 Value ledger.** Every stored value and liability sits in a double-entry ledger per entity and currency, with breakage and escheat reports.
    - *Test:* the ledger always balances; refunding an unused wristband balance posts the correct entries.
11. **R11 House accounts.** Credit limits and holds; PO or job number required per account; statements and aging; payments on account at the POS; tax-exempt certificates with expiry and jurisdiction.
    - *Test:* an expired certificate brings tax back on the sale.
12. **R12 Tabs.** Configurable pre-auth. Automatic incremental auth once the tab passes X% of the authorized amount. Network tolerance tables decide whether a true-up is needed. Walkouts auto-close at business-day end with the configured gratuity.
    - *Test:* capturing 125% of the authorized amount at a bar triggers an incremental auth first.
13. **R13 Holds and deposits.** Deposits are liabilities. Holds are tracked against how long an authorization stays valid, with a re-auth or MIT fallback.
    - *Test:* a 21-day rental hold re-authorizes before it expires.
14. **R14 Tender eligibility.** Each line carries eligibility flags (SNAP, WIC APL match, FSA/IIAS, meal voucher, campus plan) evaluated by jurisdiction and date. Split tender allocates the eligible subtotals, and the IIAS healthcare subtotal is sent in the authorization.
    - *Test:* after a state's waiver takes effect, soda is excluded from the SNAP subtotal.
15. **R15 Payment rail plug-ins.** Supported rails:
    - EMV and NFC cards
    - domestic debit: Interac, eftpos, mada, girocard, Cartes Bancaires
    - QR and A2A: UPI, Pix, PromptPay, QRIS, DuitNow, PayNow, SPEI
    - M-Pesa STK push
    - Alipay and WeChat Pay (customer-presented codes)
    - pay-by-bank
    - BNPL and financing
    - EBT

    Every rail shares one pending, confirmed, failed or expired state machine, confirmed by webhook, polling and manual reconciliation.
    - *Test:* if the network drops after a QR is shown, the payment still settles to the right final state with no double charge.
16. **R16 Cash rounding** per currency and jurisdiction, applied only to the cash tender and shown as its own line.
    - *Test matrix:* CAD 0.05, AUD 0.05, NZD 0.10, CHF 0.05, JPY 0 decimals, KWD 3 decimals.
17. **R17 Surcharging, dual pricing and all-in fee display** are configured by jurisdiction and network, and updated without a deploy.
    - *Test:* a Québec store cannot surcharge.

**Orders and jobs**

18. **R18 Deferred-fulfillment orders.** Quote (with validity date and price lock), then order and deposit. Special-order lines raise POs automatically. Several fulfillment groups per order, each with its own date or slot; partial fulfillment; balance-due alerts.
    - *Test:* a furniture order with 3 delivery groups and a financing tender.
19. **R19 Production and fulfillment routing.** Routing to stations and production lists; daily caps per product; slot throttling for order-ahead.
    - *Test:* the 21st custom cake for a date is refused online and warned about at the POS.
20. **R20 Custody jobs.** One state machine for repairs and rentals:
    - intake: asset ID, condition, photos, checklist, signature
    - versioned estimate approval
    - parts reservation that reduces available-to-sell stock
    - technician attribution
    - notifications triggered by status (SMS, email or WhatsApp by market)

    - *Test:* moving a job to "Ready" notifies the customer within 60 seconds, and the notification is logged.
21. **R21 Rentals.** Availability per asset with cleaning buffers; rate ladders with best-price calculation; late and damage fees posted against the deposit; maintenance lockout by meter reading; e-signed waivers.
    - *Test:* a 9-day rental is priced as a week plus 2 days when that is cheaper.
22. **R22 Sessions and events.** Business date decoupled from calendar date; shifts, drawers and blind counts; menus, prices, inventory and stands scoped to an event; alcohol cutoff times.
    - *Test:* a 01:30 sale posts to the previous business day.

**Identity, compliance and fiscal**

23. **R23 Customer graph** with dependents and assets and typed attributes that can gate actions.
    - *Test:* an expired pet vaccination blocks a grooming booking.
24. **R24 ID verification.** Parse AAMVA and MRZ, and store the verification result and age rather than the image by default. Retention is set by jurisdiction, for example for PSE logs.
    - *Test:* an under-21 ID blocks an age-restricted line, and the audit records the operator.
25. **R25 Policy engine.** Signed, versioned rule packs evaluated offline, with decisions that explain themselves. Rule updates reach devices within 5 minutes, with no app release.
    - *Test:* a cannabis cart over the flower-equivalent limit is blocked while offline.
26. **R26 Traceability outbox.** Durable and idempotent, with retries, a dead-letter queue, and reconciliation against state portals.
    - *Test:* a 4-hour outage loses zero submissions and creates zero duplicates.
27. **R27 Fiscal engine.** Pluggable country adapters: Germany TSE, France NF525, Austria RKSV, Italy RT, Spain Verifactu/TicketBAI, Brazil NFC-e/NF-e, Mexico CFDI, India IRN, Saudi Arabia ZATCA, Kenya eTIMS, Malaysia MyInvois. Hash chains per device. A sale is final only when the fiscal step succeeds or a contingency is recorded.
    - *Test:* dropping the network mid-sale in Brazil produces a contingency NFC-e that is authorized later.
28. **R28 Immutability.** Finalized documents are never deleted; corrections are linked reversals; national audit exports are available.
    - *Test:* a delete through the API is rejected.
29. **R29 Invoices after the sale.** A customer can turn a receipt QR into a valid e-invoice within the statutory window, and that sale then drops out of the global or consolidated invoice.
    - *Test:* Mexico autofactura and Malaysia MyInvois flows.
30. **R30 Payment–fiscal coupling.** Payment method, amount and authorization data flow automatically into fiscal documents where required.
    - *Test:* the payment group of a Goiás NFC-e is populated.

**Organization, staff and franchise**

31. **R31 Inheritance with locks.** Settings are inherited down the org tree with locks and overrides per attribute (menu, price, tax, policies, UI), plus price zones and scheduled limited-time-offer rollouts.
    - *Test:* a franchisee cannot change a price the brand has locked, but can add a local item.
32. **R32 Settlement.** Royalty and ad fund on a configurable gross or net base; revenue share per concessionaire and event; gift-card and loyalty settlement between entities.
    - *Test:* a gift card sold at franchisee A and redeemed at B creates a settlement between them.
33. **R33 Staff earnings.** Split attribution, commission plans with clawback, tip pooling, service charge vs gratuity classification, and payroll and tip-reporting exports.
    - *Test:* a 60/40 stylist split reverses correctly on a refund.

**Offline, performance, hardware, localization, privacy**

34. **R34 Offline-first.** Sales, returns, tabs, check-ins, entitlement and gift-card redemption all run for at least 24 hours offline. Devices on the LAN sync peer-to-peer. Store-and-forward caps are set per transaction and cumulatively, with a clear disclosure of the risk.
    - *Test:* a simulated 10,000-transaction event with a 3-hour network cut.
35. **R35 Throughput.** p95 from tap to receipt ≤3 seconds for 1–3 items. Item lookup ≤100 ms with 100k SKUs on the device.
36. **R36 Speed-of-service instrumentation.** Timestamps per order for start, send, bump, ready and handoff, plus a drive-thru timer API that pairs a car with a transaction.
37. **R37 Hardware abstraction.** Certified scales; 1D and 2D scanners; recyclers and smart safes; forecourt controllers (Conexxus FDC); lottery terminals; ID scanners; RFID; label printers. A certification matrix per market.
38. **R38 Accessibility.** Kiosks and self-checkout meet WCAG 2.2 AA and the EAA's requirements for self-service terminals (‡ in force since June 2025), and are ADA-aligned in the US.
    - *Test:* a full order can be completed by screen reader or tactile keypad.
39. **R39 Localization.**
    - ICU/CLDR formats, lakh grouping, three-decimal and zero-decimal currencies.
    - Hijri, Thai Buddhist-era and Japanese-era dates on documents where required.
    - Name and address formats per country.
    - Full RTL mirroring with bidi-correct receipts (Arabic shaping, raster fallback).
    - Bilingual receipts (Canada, Saudi Arabia).
40. **R40 Tax engine.**
    - Tax-inclusive and tax-exclusive pricing.
    - Stacked taxes: GST+PST/QST, cannabis excise.
    - Eat-in vs takeaway rates.
    - Rounding per line or per document (Japan: once per rate‡).
    - Effective-dated rate changes (‡ India 2025, Germany 2026, Canada's tax holiday).
    - Exemptions by customer class (‡ Philippine senior and PWD discounts).
41. **R41 Data residency and privacy.** Storage pinned to a region (EU, India, China, Saudi Arabia); minimal PII; retention by data class; HIPAA-grade handling in the pharmacy and optical modules.
42. **R42 Adapter SDK with contract tests** for:
    - PMS: OPERA via OHIP or IFC8, Mews profiles and paymaster, Cloudbeds folios and house accounts
    - campus card systems
    - pharmacy systems
    - optical labs and insurers
    - ACES/PIES catalogs
    - delivery marketplaces
    - drive-thru timers
43. **R43 External-account tenders** follow one flow: lookup, authorize, post, confirm, void. This covers room charge, campus plans, house accounts and third-party vouchers, with an offline queue and limits.
44. **R44 Receipt QR and digital receipts everywhere,** with legal fields per market (NFC-e, ZATCA, India B2C, Austria 2026 digital receipts) generated by P12.
45. **R45 Regulatory rule packs ship separately from app releases** and can be rolled back. This covers limits, eligibility, tax rates, fiscal dates and surcharge rules.
    - *Test:* a SNAP waiver or Verifactu date change is deployed as data in under an hour.

---

### Sources consulted but not fetchable (for follow-up)

- The Metrc, Mews, Cloudbeds, Oracle, Toast developer, fiskaly, fiscal-requirements.com, ZATCA, KRA, RBA and NTA pages were cited from search excerpts or were blocked for fetching.
- Priority checks for the fiscal researcher (‡):
  - ZATCA wave schedule
  - Kenya eTIMS integration modes
  - RBA final surcharging decision
  - New Zealand surcharge-ban date
  - Japan's invoice rounding rules
  - Italy's POS–RT link
  - Québec MEV-WEB timeline
  - UAE e-invoicing dates
  - Malaysia MyInvois phases
