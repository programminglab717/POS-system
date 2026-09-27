# Track 05: Trends, AI, Customer-Facing Experience, and Real User Pain Points

**Prepared:** 2026-09-27 · **For:** architecture and product leads · **Status:** research input. Evidence is tagged; read Section 0 first.

---

## 0. How to read this report

**Method.** I ran 40 successful web searches covering vendor AI launches, voice AI, computer-vision checkout, pricing, self-service, loyalty, tipping, outages, fees, and review-site complaints. Three environment constraints shaped the evidence base:

1. Direct page retrieval (WebFetch and curl) was blocked by this environment's egress proxy for every external domain tried: news sites, vendor sites, SEC, Capterra, G2, Trustpilot, and Wikipedia.
2. reddit.com is not accessible to the search tool. Reddit sentiment therefore appears only where a secondary source summarized it.
3. The session's shared search budget ran out before the per-persona and per-vendor review mining was finished. Appendix A lists the exact queries still to run.

**Evidence tags**

| Tag | Meaning | How to use it |
|---|---|---|
| **[S]** | Independent or primary source: news, regulator, official announcement, or survey | Rely on it |
| **[V]** | Vendor self-report: press release, case study, or marketing | Directional only |
| **[3P]** | Third-party roundup, reseller, or competitor blog | Possibly biased; corroborate before relying on it |
| **[B]** | Background industry knowledge, not re-verified this session, with no link | A hypothesis to validate |

**Quotes** are reproduced verbatim as captured in search extracts of the cited page. Spot-check them against the live page before using them externally.

---

## Executive summary

1. **Conversational AI is now table stakes; approval-gated agents are the new frontier.** Square (June 2025), Toast (October 2025), Shopify (December 2025), and Lightspeed (January 2026) all shipped "ask your business" assistants. Square's Managerbot (April 2026) and Toast IQ now *take actions*, and Managerbot requires seller approval before it executes changes. We should differentiate on verifiable answers, cross-domain actions (labor, inventory, marketing, and books), and an owner-controlled autonomy dial, not on simply having a chatbot.
2. **Voice AI works only with a human in the loop, and vendors overstate how much it automates.** McDonald's ended its IBM test at more than 100 stores in 2024 after misorders went viral. The SEC found false Presto's claim that Presto Voice "eliminat[es] human order taking": more than 70% of its in-house orders needed humans. Taco Bell, with voice AI at more than 500 stores, now coaches crews to "monitor voice AI and jump in as necessary." Wendy's pilot handled 86% of orders with no staff intervention. The KPI to track is the no-intervention rate, not vendor-defined "accuracy."
3. **Store-wide camera checkout failed; constrained computer vision works.** Amazon pulled Just Walk Out from its Fresh stores in 2024; reportedly about 700 of every 1,000 sales were human-reviewed in 2022. Grabango shut down (October 2024), Standard AI exited checkout, and Amazon is closing its Go stores (2026). Mashgin's tray-based checkout ran 440 million transactions in 2024, with median transaction times "as low as" 7 seconds [V].
4. **Guest-facing "efficiency" backfires when it pushes work or cost onto the guest.** 90% of diners prefer printed menus. 38% of Americans are annoyed by pre-entered tip screens, and 27% tip less or nothing when shown one. Big retailers rolled back self-checkout. Wendy's (on "dynamic pricing") and Toast (on a 99-cent consumer fee) each retreated within days. Kiosks, by contrast, succeed because they give guests control: Yum's CEO said kiosk checks ran about 10% higher.
5. **Digital channels have created a kitchen-orchestration problem.** At Starbucks, mobile orders "come flooding in faster than even our customer can get there." The fix was an order-sequencing algorithm, with a 4-minute target for in-store customers that more than 80% of company-owned cafés now hit, plus a lower item cap on mobile orders. The POS must own capacity pacing across every channel.
6. **The owner's #1 pain is the commercial relationship, not features.** The recurring complaints are contract lock-in, early-termination fees (ETFs) and hardware-lease residuals, mandatory in-house processing, add-on creep ($69 base rising to $300–400 a month), outages, fund holds, and support that makes you "start with square one."
7. **Regulators are catching up with AI and pricing.** Examples: SEC "AI-washing" enforcement against Presto (January 2025), the FTC's surveillance-pricing study (January 2025), and New York's algorithmic pricing disclosure law (in force November 10, 2025), with more state bills in 2026.
8. **Loyalty is moving toward transparent value, and "unlimited" offers get capped.** Panera's Unlimited Sip Club moves to a 30-refill monthly cap. Starbucks' March 2026 tier overhaul drew backlash, yet its new low-threshold 60-star reward became its most popular. Owner.com's $2.3B valuation (August 2026) shows that first-party, commission-free ordering plus CRM is a growth engine.
9. **Agentic commerce turns catalog quality and real-time availability into POS responsibilities.** Shopify's Agentic Storefronts place merchant products inside ChatGPT, Perplexity, and Copilot.
10. **Operators are open to AI but skeptical of its ROI.** 87% say they are comfortable using AI (Toast survey) [V], but only 26% report using AI tools (NRA) [S], 42% were unprofitable in 2025, and just 28% say their tech investments improved profitability [3P]. Every AI feature must prove its dollar impact.

---

## 1. Trend radar

Rings: **Adopt** means it is proven and we build it for v1. **Trial** means we build it behind guardrails and measure it. **Assess** means we watch and prototype. **Hold** means we avoid it or actively design against it.

### ADOPT

| # | Trend | Evidence | Our move |
|---|---|---|---|
| A1 | **Offline-first, resilient transaction core** | Square's Sept 7–8, 2023 outage lasted more than 12 hours, and "some sellers experienced issues accepting offline payments" ([Square](https://squareup.com/us/en/press/an-update-on-last-weeks-outage), [Payments Dive](https://www.paymentsdive.com/news/square-outage-pos-payments-processing-block-smb-merchants/693674/)) [S]. The AWS outage of Oct 20, 2025 caused complete system failures at many Toast restaurants; one manager offered to pay for guests' meals because cards couldn't be processed ([NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling)) [S] | Core architecture requirement (R1–R4) |
| A2 | **Conversational "ask your business" assistant (read-only)** | Square AI public beta June 2025, with ~200,000 questions asked ([PYMNTS](https://www.pymnts.com/artificial-intelligence-2/2025/square-adds-conversational-ai-assistant-to-business-technology-platform/), [Square](https://squareup.com/us/en/press/square-releases-ai)) [S]. Toast IQ assistant Oct 29, 2025 ([QSR](https://www.qsrmagazine.com/news/toast-expands-toast-iq-with-conversational-ai-assistant-to-help-restaurants-run-smarter-and-faster/)) [S]. Lightspeed AI Jan 2026 ([Lightspeed](https://www.lightspeedhq.com/news/lightspeed-commerce-launches-lightspeed-ai-a-new-ai-powered-intelligence-layer-for-retail-and-hospitality/)) [V]. 87% of operators say they are comfortable using AI ([Toast survey](https://pos.toasttab.com/blog/data/voice-of-the-restaurant-industry-2026)) [V] | Ship at v1 with verifiable answers (R32) |
| A3 | **Self-order kiosks (QSR and fast casual)** | Yum's CEO said average kiosk sales run about 10% above the front counter ([Elo](https://www.elotouch.com/news/taco-bell-kiosks-the-ticket-to-qsr-order-customization); date unverified) [S]. A 15–30% ticket lift is widely claimed, e.g. McDonald's +30% ([Restroworks](https://www.restroworks.com/blog/self-ordering-kiosk-restaurant-statistics/)) [3P] | First-party kiosk mode (R29, R31) |
| A4 | **First-party, commission-free online ordering plus CRM** | Owner.com charges a flat $499/month and pitches against 25–35% third-party delivery commissions ([Sacra](https://sacra.com/c/owner/)) [3P]. It raised a $240M Series D at a $2.3B valuation in Aug 2026 ([RTN](https://restauranttechnologynews.com/2026/08/owner-raises-240-million-to-expand-its-ai-powered-technology-and-marketing-platform-for-restaurants/)) [S] | Native web/app ordering, loyalty, and marketing |
| A5 | **AI phone answering** | Square's AI voice ordering "answers 100% of incoming calls" and sends orders to the kitchen or POS ([Block IR](https://investors.block.xyz/investor-news/news-details/2025/Square-AI-Gains-New-Intelligence-Capabilities-Providing-Deeper-Business-and-Neighborhood-Insights-to-Square-Sellers/default.aspx), [TechCrunch](https://techcrunch.com/2025/10/08/square-launches-ai-voice-ordering-and-an-integrated-bitcoin-solution-for-merchants/)) [S]. Vendors claim the average restaurant misses about 150 calls a month ([Loman](https://loman.ai/blog/restaurant-ai-phone-answering)) [V] | Native phone agent tied to the live menu (R40) |
| A6 | **Demand forecasting feeding labor, prep, and purchasing** | Managerbot does inventory forecasting and scheduling ([Square](https://squareup.com/us/en/press/managerbot-open-beta)) [S]. 7shifts added compliance-aware forecasting ([TimeForge](https://timeforge.com/industry-news/7shifts-simplifies-restaurant-labor-compliance-with-new-forecasting-features/)) [3P]. Claims of 2–4 points of labor-cost reduction are common but vendor-sourced ([Nory](https://www.nory.ai/blog/reduce-restaurant-labor-costs-with-ai)) [V] | Build (R37) |
| A7 | **Cross-channel order pacing and sequencing** | Starbucks' sequencing algorithm targets 4 minutes for in-store and drive-thru and 12 minutes for mobile and delivery; more than 80% of company cafés hit the 4-minute average ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/answer-starbucks-mobile-order-problem-has-been-there-all-along)) [S] | Core KDS and channel capability (R20–R23) |

### TRIAL

| # | Trend | Evidence | Guardrail |
|---|---|---|---|
| T1 | **Proactive agents that take actions** | Toast IQ can "take action like updating menus or editing shifts" ([Business Wire](https://www.businesswire.com/news/home/20251029752451/en/Toast-Expands-Toast-IQ-from-Smart-Features-to-Smart-AI-Assistant)) [S]. Managerbot entered open beta in April 2026 and requires seller approval before any change ([VentureBeat](https://venturebeat.com/data/block-introduces-managerbot-a-proactive-square-ai-agent-and-the-clearest)) [S] | Approval gate, diff preview, undo, and audit log (R33, R34) |
| T2 | **AI marketing automation** | Toast IQ Grow reportedly lifted sales 8% for pilot customers ([Yahoo Finance](https://finance.yahoo.com/markets/stocks/articles/toast-q2-earnings-call-highlights-100400369.html)) [V]. Also SpotOn Marketing Assist ([SpotOn](https://www.spoton.com/blog/operator-first-ai-vision-restaurant-tech/)) [V] and Owner's "AI Executives" ([Sacra](https://sacra.com/c/owner/)) [3P] | Measure incremental lift against a holdout |
| T3 | **AI upsell and personalization** | Toast Menu Upsells ([Toast](https://pos.toasttab.com/news/toast-launches-toastiq-superpower-future-of-restaurants)) [V]; SpotOn "Picked for You" ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/tech-tracker-host-new-ai-tools-hit-market)) [S] | One prompt per step, skippable, with time cost measured (R31) |
| T4 | **Drive-thru voice AI with human oversight** | Wendy's pilot handled 86% of orders with no team intervention ([Restaurant Dive](https://www.restaurantdive.com/news/wendys-expand-google-generative-ai-drive-thru-test/702184/)) [S] and planned 500–600 stores by end of 2025 ([Yahoo](https://finance.yahoo.com/news/wendy-deploy-drive-thru-ai-103100226.html)) [S]. Taco Bell reached nearly 900 stores ([NRN](https://www.nrn.com/quick-service/taco-bell-s-drive-thru-voice-ai-expands-to-nearly-900-restaurants)) [S]. SoundHound claims 10,000 locations ([SoundHound](https://investors.soundhound.com/news-releases/news-release-details/soundhound-ai-unveils-next-generation-ai-platform-restaurants)) [V]. Toast launched Toast Drive-Thru in April 2026 ([Yahoo](https://finance.yahoo.com/markets/stocks/articles/toast-tost-ai-drive-thru-080632748.html)) [S] | Enterprise QSR only, via partners, with the safety rules in R35–R36 |
| T5 | **Tray-based computer-vision checkout** | Mashgin: 440M transactions in 2024 at 4,000+ locations, median transactions as low as 7 seconds, and 150 sports venues ([GlobeNewswire](https://www.globenewswire.com/news-release/2025/01/15/3010088/0/en/Mashgin-s-AI-Checkout-Powers-Over-440-Million-Transactions-in-2024.html), [GlobeNewswire](https://www.globenewswire.com/news-release/2025/04/30/3071379/0/en/One-Billion-Served-Mashgin-Racks-Up-Record-AI-powered-Checkout-Transactions.html)) [V] | Partner integration for venues and cafeterias (R52) |
| T6 | **Agentic storefronts (products discoverable by AI agents)** | Shopify Agentic Storefronts expose products in ChatGPT, Perplexity, and Copilot ([The Letter Two](https://thelettertwo.com/2025/12/10/shopify-ai-growth-tools-sidekick-tinker-agentic-storefronts/), [Fintech Times](https://thefintechtimes.com/shopify-unveils-agentic-commerce-era-with-winter-26-edition/)) [S] | Agent allow-list and order caps (R50) |
| T7 | **Capped subscriptions and tiered loyalty** | Panera caps Sip Club at 30 refills a month ([TheStreet](https://www.thestreet.com/restaurants/panera-major-sip-club-change)) [S]. Starbucks' 60-star reward became its most popular ([CNBC](https://www.cnbc.com/2026/04/23/starbucks-loyalty-changes-are-drawing-value-conscious-customers.html)) [S] | Caps, anti-sharing controls, and change notice (R30) |
| T8 | **QR pay-at-table (paying, not menus)** | Sunday cut staff and exited 60% of its markets ([Restaurant Dive](https://www.restaurantdive.com/news/restaurant-payments-app-sunday-cuts-staff-exits-60-of-its-markets/627312/)) [S]; me&u cut 10% of staff while chasing breakeven in 2025 ([Flux](https://www.flux.finance/post/me-u-the-qr-code-ordering-app-cuts-10pc-jobs)) [3P]. QR pay is a feature, not a company | Optional; no app or account required (R26) |

### ASSESS

| # | Trend | Evidence | Why not yet |
|---|---|---|---|
| As1 | **Agent payment protocols** (OpenAI/Stripe Agentic Commerce Protocol, Google AP2 and UCP, Visa Intelligent Commerce, Mastercard Agent Pay) | Announced 2025–Jan 2026 [B]. Fiserv says AI helps it build "products tied to agentic commerce" ([Payments Dive](https://www.paymentsdive.com/news/fiserv-turns-to-ai-for-help-Clover-payments-processors-stock/804578/)) [S] | Standards are still in flux; restaurant fit (modifiers, pacing) is unproven |
| As2 | **Biometric identity-linked payment and loyalty** | Clover and Wink began a 2026 rollout for QSRs, venues, and retail ([Fiserv](https://investors.fiserv.com/news-releases/news-release-details/clover-introduces-identity-based-payments-transform-everyday)) [S] | Biometric privacy law exposure (e.g., Illinois BIPA) [B]; consumer trust |
| As3 | **Discount-only, time-based pricing** via digital boards or electronic shelf labels (ESLs) | Walmart plans ESLs in every US store by end of 2026; Kroger has them in about 1 in 4 of ~2,700 stores ([BTPM](https://www.btpm.org/local/2026-02-17/electronic-shelf-labels-leave-concerns-around-surveillance-pricing-in-grocery-stores)) [S] | Backlash and legal risk (see H3) |
| As4 | **Produce and item recognition at scales and self-checkout** | Several scale and self-checkout vendors market it [B] | Not researched this session |
| As5 | **Smart carts and scan-and-go** | Amazon replaced Just Walk Out with Dash Carts in Fresh ([Retail Dive](https://www.retaildive.com/news/amazon-removes-just-walk-out-tech-amazon-fresh-stores-dash-carts/712150)) [S], but Amazon is now closing its Go stores ([Fast Company](https://www.fastcompany.com/91483585/amazon-go-closing)) [S]. Sam's Club's member-based Scan & Go [B] | Works best when the shopper's identity is known (membership) |
| As6 | **In-store computer-vision analytics** | Standard AI pivoted to "Vision Analytics" in March 2024 ([CSP](https://www.cspdailynews.com/technologyservices/standard-ai-moves-away-autonomous-shopping-toward-ai-cameras)) [S] | Privacy concerns; value for SMBs is unclear |
| As7 | **Crypto acceptance** | Square integrated Bitcoin (Oct 2025) ([TechCrunch](https://techcrunch.com/2025/10/08/square-launches-ai-voice-ordering-and-an-integrated-bitcoin-solution-for-merchants/)) [S] | Low merchant demand [B] |
| As8 | **Social and live commerce** (e.g., TikTok Shop) | [B] | Mainly a retail/e-commerce channel; handle through catalog sync |

### HOLD (avoid or design against)

| # | Anti-pattern | Evidence |
|---|---|---|
| H1 | **Fully autonomous drive-thru voice with no human fallback** | McDonald's/IBM ended (July 2024) ([CNBC](https://www.cnbc.com/2024/06/17/mcdonalds-to-end-ibm-ai-drive-thru-test.html)); Presto: more than 70% of orders needed humans [S]. Details in Section 2.3 |
| H2 | **Store-wide camera "just walk out" checkout** | Just Walk Out: ~700 of every 1,000 sales human-reviewed in 2022 against a goal of 50 (per The Information, via [Washington Times](https://www.washingtontimes.com/news/2024/apr/4/amazons-just-walk-out-stores-relied-on-1000-people/); Amazon disputes this). Grabango shut down ([CNBC](https://www.cnbc.com/2024/10/09/amazon-just-walk-out-rival-shutters-after-failing-to-secure-funding.html)); Standard AI sold GoSkip to PAR ([C-Store Dive](https://www.cstoredive.com/news/par-retail-acquires-goskip-standard-ai/747701/)) [S] |
| H3 | **Surge pricing, or personalized ("surveillance") pricing** | Wendy's backtracked within days [S] (Section 2.3). FTC surveillance-pricing study, Jan 2025 ([Grocery Dive](https://www.grocerydive.com/news/dynamic-pricing-state-laws-ftc-grocery-supermarkets/826529/)); New York disclosure law, up to $1,000 per violation [S] (Section 3.7) |
| H4 | **QR-only menus in full service** | 90% of diners prefer printed menus ([Yahoo/Escoffier](https://creators.yahoo.com/lifestyle/story/the-menu-war-is-over-why-90-of-diners-are-demanding-printed-menus-back-and-rejecting-qr-codes-171319971.html)) [3P]; ([Ipsos](https://www.ipsos.com/en-us/qr-code-menus-are-growing-even-less-popular)) [S] |
| H5 | **Aggressive default tip prompts in counter and retail settings** | 27% of consumers tip less or nothing when shown a pre-entered tip screen ([Bankrate](https://www.bankrate.com/credit-cards/news/tipping-culture-survey/)) [S] |
| H6 | **Forced self-checkout with no staffed alternative** | Dollar General, Target, and Walmart scaled self-checkout back ([NPR](https://www.npr.org/2024/03/18/1239107299/some-big-retailers-reverse-course-and-scale-back-their-use-of-self-checkout)) [S] |
| H7 | **Consumer fees imposed by the POS vendor** | Toast's 99-cent fee was reversed within days of its nationwide rollout ([Restaurant Dive](https://www.restaurantdive.com/news/toast-software-processor-customer-online-order-fee-payment/688381/)) [S] |
| H8 | **"AI-washing"** (claiming automation that humans actually perform) | SEC cease-and-desist against Presto, Jan 2025 ([SEC](https://www.sec.gov/enforcement-litigation/administrative-proceedings/33-11352-s)) [S] |

---

## 2. AI opportunities in POS

### 2.1 What vendors actually shipped, and how it landed

| Vendor | What shipped (date) | What it does | Reception and outcomes |
|---|---|---|---|
| **Toast** | ToastIQ (May 1, 2025): Menu Upsells, Shift at a Glance, Digital Chits, AI Marketing Assistant, Advertising ([Toast](https://pos.toasttab.com/news/toast-launches-toastiq-superpower-future-of-restaurants), [PYMNTS](https://www.pymnts.com/restaurant-technology/2025/toast-adds-intelligence-engine-to-digital-technology-platform-for-restaurants/)) [S]. Conversational assistant (Oct 29, 2025). Toast IQ Grow. Toast Drive-Thru (Apr 2026) | Describes itself as "timely prompts, personalized recommendations, and automated workflows" drawing on 130,000+ locations. The October 2025 release added a "For you" feed, plain-language Q&A, and actions (menu and shift edits), plus a Coca-Cola beverage-optimization feature ([Nasdaq](https://www.nasdaq.com/press-release/toast-expands-toast-iq-smart-features-smart-ai-assistant-2025-10-29)) [S] | Toast IQ Grow is called Toast's fastest-growing launch, with +8% sales for pilot customers. The roadmap extends agentic AI into scheduling, payroll, tax, inventory, bookkeeping, and voice ordering ([Globe and Mail](https://www.theglobeandmail.com/investing/markets/stocks/TOST/pressreleases/3667139/tost-q2-earnings-call-centers-on-ai-led-reinvestment/), [Motley Fool](https://www.fool.com/earnings/call-transcripts/2026/05/07/toast-tost-q1-2026-earnings-call-transcript/)) [V]. *Analysis:* a brand-sponsored optimizer raises a neutrality question (see R38) |
| **Square (Block)** | Square AI (open beta June 2025). October 2025 added weather, events, news, and review data, plus AI voice ordering and Bitcoin ([Block IR](https://investors.block.xyz/investor-news/news-details/2025/Square-AI-Gains-New-Intelligence-Capabilities-Providing-Deeper-Business-and-Neighborhood-Insights-to-Square-Sellers/default.aspx)) [S]. **Managerbot** (open beta Apr 28, 2026) ([Square](https://squareup.com/us/en/press/managerbot-open-beta)) [S] | Managerbot proactively monitors operations and automates inventory forecasting, scheduling, and campaigns. It runs on Claude Sonnet and GPT models, requires seller approval, and costs nothing extra. The open beta is for most "non-franchise" US food and beverage, retail, and health and beauty sellers ([VentureBeat](https://venturebeat.com/data/block-introduces-managerbot-a-proactive-square-ai-agent-and-the-clearest), [Shopifreaks](https://www.shopifreaks.com/block-launches-managerbot-a-proactive-ai-agent-for-square-sellers-that-manages-inventory-scheduling-and-marketing-autonomously/)) [S] | Reportedly reached about 1 million businesses by April 2026 [V]. Launch coverage also reports it came weeks after Block cut more than 4,000 staff, citing AI [S; verify]. *Analysis:* AI-justified support cuts could deepen existing support complaints |
| **Clover (Fiserv)** | AI is mostly internal: a two-year IBM partnership; 40% of engineers use AI daily and 25% of code is AI-written ([Payments Dive](https://www.paymentsdive.com/news/fiserv-turns-to-ai-for-help-Clover-payments-processors-stock/804578/)) [S]. Identity-based biometric payments with Wink (Jan 2026) [S]. Clover Reserve powered by Tabit for fine dining ([Fiserv](https://investors.fiserv.com/news-releases/news-release-details/fiserv-expands-clovers-restaurant-portfolio-new-fine-dining)) [S] | Biometric checkout unifies identity, payment, and loyalty | Merchant-facing AI is less visible than at peers. Fiserv faces a securities suit alleging Clover growth was inflated by forcing Payeezy merchants to migrate ([Payment Expert](https://paymentexpert.com/2025/07/28/fiservs-being-sued-heres-what-you-need-to-know/)) [S] |
| **Lightspeed** | Lightspeed AI (Jan 2026) in Retail, Restaurant, and NuORDER ([Lightspeed](https://www.lightspeedhq.com/news/lightspeed-commerce-launches-lightspeed-ai-a-new-ai-powered-intelligence-layer-for-retail-and-hospitality/)) [V]. AI Showroom ([PR Newswire](https://www.prnewswire.com/news-releases/lightspeed-commerce-unveils-q2-product-innovations-including-ai-showroom-designed-to-empower-independent-businesses-globally-302595686.html)) [V] | Natural-language Q&A "without navigating complex dashboards"; one-click product-photo cleanup; planned evolution toward an "autonomous agent" through 2026 | Too new for reception data |
| **Shopify** | Winter '26 Edition (Dec 2025): agentic Sidekick, Sidekick Pulse, 20 languages, Agentic Storefronts, SimGym, POS Hub hardware ([Shopify](https://www.shopify.com/editions/winter2026)) [S] | Sidekick Pulse surfaces tasks such as suggested bundles or missing return policies ([Fintech Times](https://thefintechtimes.com/shopify-unveils-agentic-commerce-era-with-winter-26-edition/)) [S] | Observers credited Shopify with reducing merchant friction rather than adding "AI noise" ([The Letter Two](https://thelettertwo.com/2025/12/10/shopify-ai-growth-tools-sidekick-tinker-agentic-storefronts/)) [S] |
| **SpotOn** | "Operator-first AI" (May 2025): Marketing Assist, Picked for You, Profit AI ([SpotOn](https://www.spoton.com/blog/operator-first-ai-vision-restaurant-tech/)) [V] | Flags anomalies in sales, labor, and cost of goods sold (COGS); suggests schedules; "pre-reconciling payouts" ([SpotOn](https://www.spoton.com/blog/ai-for-restaurants/)) [V] | Positioning is deliberately anti-hype ("Profit, Not Hype": [SpotOn 2026 outlook](https://www.spoton.com/blog/spoton-2026-restaurant-outlook-profit-not-hype/)) [V]. It pairs AI with "Faster Access to Cash" ([SpotOn Q2 2026](https://www.spoton.com/blog/q2-2026-proactive-ai-faster-cash/)) [V] |
| **Owner.com** | "AI Executives" (CMO, CFO, and CTO chatbots) launched after a $120M Series C (May 2025). Now also sells a POS ([Sacra](https://sacra.com/c/owner/)) [3P] | Website, commission-free ordering, branded app, loyalty, and automated email/SMS ([Software Advice](https://www.softwareadvice.com/retail/owner-com-profile/)) [3P] | $240M Series D at $2.3B (Aug 2026) ([RTN](https://restauranttechnologynews.com/2026/08/owner-raises-240-million-to-expand-its-ai-powered-technology-and-marketing-platform-for-restaurants/)) [S]. A marketing-led platform is now moving down into the POS |

**Adoption reality.** 26% of operators use AI tools ([NRA via Restaurant Dive](https://www.restaurantdive.com/news/national-restaurant-assocation-operator-artificial-intelligence-adoption/812418/)) [S]. 76% say technology is a competitive advantage, but only 28% say it improved profitability ([Restaurant Velocity](https://restaurantvelocity.com/blog/restaurant-technology-statistics-2026/), [Synergy](https://www.synergyconsultants.com/blog-posts/ai-in-restaurants-2026-roadmap-for-owners-where-it-helps-vs-wastes-money)) [3P; verify against the primary source].

### 2.2 Ranked opportunities (value × feasibility, each scored 1–5)

| Rank | Opportunity | Value | Feasibility | Score | Who does it now | Evidence and outcomes | Our stance |
|---|---|---|---|---|---|---|---|
| 1 | **Verifiable business Q&A plus proactive anomaly alerts** (sales, labor, COGS, voids, refunds) | 4 | 5 | 20 | Square, Toast, Lightspeed, Shopify, SpotOn | Universal in 2025–26; ~200k questions in Square's beta [S] | v1. Answers must cite their source report (R32) |
| 2 | **Forecasting that feeds labor schedules, prep lists, and purchase orders** | 5 | 4 | 20 | Managerbot, 7shifts, Fourth, Toast (roadmap) | Labor-saving claims of 2–4 points [V]; Chili's reports +20% scheduling accuracy with Fourth ([Fourth](https://www.fourth.com/article/ai-in-restaurants)) [V] | v1: forecasting. v1.5: auto-drafted schedules (R37) |
| 3 | **Cross-channel pacing and promise-time quoting** | 5 | 4 | 20 | Starbucks (in-house) | 80%+ of company cafés hit the 4-minute target [S] | v1 (R21) |
| 4 | **Back-office automation**: payout reconciliation, invoice capture, variance detection | 4 | 4 | 16 | SpotOn; Toast (roadmap) | Vendors frame reconciliation as a headline pain [V] | v1.5 (R41–R42) |
| 5 | **Marketing automation from POS data** (win-back, slow-day offers) | 4 | 4 | 16 | Toast IQ Grow, SpotOn, Owner | +8% sales in the Toast pilot [V] | v1.5 with holdout measurement |
| 6 | **AI phone answering and ordering** | 4 | 4 | 16 | Square, Slang, Loman, Popmenu, SoundHound | Popmenu reports 329k+ calls and $1.5M+ in online orders for one client ([Back of House](https://backofhouse.io/best-ai-phone-ordering-for-restaurants)) [3P]. Slang often texts callers a link instead of taking the order by voice ([RestaurantTools](https://restauranttools.ai/blog/ai-ordering-for-restaurants-2026)) [3P] | v1.5, native, synced to menu and 86 status (R40) |
| 7 | **Menu and catalog setup automation**: import from PDF or photo; photo cleanup; allergen tagging | 3 | 5 | 15 | Lightspeed (photos) | Onboarding speed is a proven delighter (see Square in Section 5) [B] | v1 (R53) |
| 8 | **Explainable risk decisions** (holds and reserves) | 4 | 3 | 12 | None does this well | Square holds and deactivations ([NerdWallet](https://www.nerdwallet.com/business/software/learn/square-concern)) [3P] | v1 policy, v2 model (R10) |
| 9 | **Menu engineering and pricing recommendations** (margin-aware, never auto-applied) | 4 | 3 | 12 | SpotOn Profit AI; Toast with Coca-Cola | Wendy's backlash shows pricing is sensitive [S] | v2; requires recipe costing |
| 10 | **Upsell prompts** (server, kiosk, online) | 3 | 4 | 12 | Toast, SpotOn | Kiosk lift [S/3P]; prompt fatigue risk [B] | v1.5 with limits (R31) |
| 11 | **Drive-thru voice** | 5 | 2 | 10 | Wendy's/Google, SoundHound, Toast Drive-Thru, Taco Bell | 86% no-intervention (Wendy's). Failures: McDonald's/IBM, Presto [S] | Partner integration only |
| 12 | **Agent-ready catalog and agent order intake** | 3 (rising) | 3 | 9 | Shopify | Agentic Storefronts [S] | v1 API foundations; v2 protocols (R50–R51) |
| 13 | **Store-wide CV checkout** | 3 | 1 | 3 | Amazon (retreating) | Failures listed in H2 [S] | Hold. Tray CV via partner only (R52) |

### 2.3 Failures and backlash, and what each one teaches

- **McDonald's and IBM (2021–July 2024).** The test ran at 100+ drive-thrus. Viral TikToks showed the system adding nine sweet teas and putting butter and ketchup on ice cream. Accents, background noise, and adjacent lanes confused it ([Incident DB](https://incidentdatabase.ai/cite/475/), [ACS](https://ia.acs.org.au/article/2024/mcdonald-s-bins-ai-drive-thru-after-errors-go-viral.html)) [S]. *Lesson:* add quantity sanity checks, lane isolation, readback confirmation, and accent test suites (R36).
- **Presto (SEC, Jan 14, 2025).** Filings claimed Presto Voice "eliminat[es] human order taking," but off-site agents in the Philippines and India processed orders. More than 70% of in-house orders needed intervention, and 100% did at some sites ([SEC](https://www.sec.gov/enforcement-litigation/administrative-proceedings/33-11352-s)) [S]. Presto was delisted from Nasdaq ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/ai-supplier-presto-be-delisted-nasdaq)) [S]. *Lesson:* publish honest automation metrics (R35).
- **Taco Bell (Aug–Sep 2025).** After deploying at 500+ stores, Taco Bell saw glitches, customer discomfort, and pranks such as an order for 18,000 cups of water. Its CDTO said: "we recommend you use voice AI or recommend that you actually really monitor voice AI and jump in as necessary" ([TechCrunch](https://techcrunch.com/2025/08/30/taco-bell-is-having-second-thoughts-about-relying-on-ai-at-the-drive-through/), [PYMNTS](https://www.pymnts.com/news/artificial-intelligence/2025/taco-bell-reconsiders-voice-ai-after-seeing-mixed-results/)) [S]. It still "continues to see the technology as a core part of our future" ([The Next Web](https://thenextweb.com/news/taco-bell-voice-ai-drive-thru-expansion)) [S]. *Lesson:* toggles per store and daypart, plus instant human takeover.
- **Wendy's FreshAI.** Wendy's pilot metric was 86% of orders handled without intervention [S]. Third-party marketing later cites "99% order accuracy" ([Voices](https://www.voices.com/blog/wendys-ai-drive-thru/)) [3P]. *Lesson:* these are different metrics, so standardize on the no-intervention rate plus audited accuracy.
- **Amazon Just Walk Out.** Amazon said its India team is "way less than 1,000" people and reviews only "a small percentage" of cases ([RTIH](https://retailtechinnovationhub.com/home/2024/4/17/amazon-hits-back-at-media-reports-that-its-just-walk-out-technology-relies-on-human-reviewers-watching-from-afar)) [S]. It replaced the system with Dash Carts, "smart-shopping carts, which allows customers all these benefits including skipping the checkout line" ([Retail Dive](https://www.retaildive.com/news/amazon-removes-just-walk-out-tech-amazon-fresh-stores-dash-carts/712150)) [S]. *Lesson:* hidden human labor is a unit-economics problem and a trust problem.
- **Wendy's pricing (Feb 2024).** CEO Kirk Tanner said the chain would "begin testing more enhanced features like dynamic pricing and daypart offerings along with AI-enabled menu changes and suggestive selling." That triggered #BoycottWendys, and within days Wendy's said it would only *lower* prices when traffic was slow ([Restaurant Dive](https://www.restaurantdive.com/news/wendys-backtracks-on-dynamic-pricing-after-consumer-backlash/708799/), [CNN](https://www.cnn.com/2024/02/28/business/wendys-dynamic-pricing-surge-explained/)) [S]. *Lesson:* frame and build any time-based pricing as discounts only (R28).

### 2.4 The AI interaction pattern we should standardize

1. **Show your work.** Every number links to the report, filters, and time range behind it.
2. **Propose, preview, approve, apply, audit, undo.** This is Managerbot's approval gate generalized to every write action.
3. **Autonomy dial per capability,** defaulting to "suggest" for anything touching money, prices, schedules, or guests.
4. **Measured automation.** Publish no-intervention and handoff rates, and never present human-assisted flows as fully automated.
5. **Human takeover in ≤2 seconds** for any customer-facing AI.
6. **Label sponsorship.** If a brand partner shapes a recommendation, say so.

---

## 3. Customer-facing innovations and the backlash against them

### 3.1 Self-order kiosks: what works

- **Evidence.** Kiosk checks run about 10% above the counter at Taco Bell, per Yum's CEO [S]. A 15–30% lift is widely claimed [3P]. Kiosks mostly work because they give guests *control*: unhurried customization, visible options, and no social pressure.
- **Risks** [B]:
  - Tip prompts on kiosks for pickup orders.
  - Accessibility gaps for blind and low-vision users, and for wheelchair reach.
  - Cash-only customers excluded.
  - Upsell screens that slow ordering.
- **Design:** R29, R31.

### 3.2 QR menus versus QR pay

- **QR menus are a backlash category.**
  - 90% of Americans prefer printed menus, a reported "14% jump" from 2023 ([Yahoo/Escoffier](https://creators.yahoo.com/lifestyle/story/the-menu-war-is-over-why-90-of-diners-are-demanding-printed-menus-back-and-rejecting-qr-codes-171319971.html)) [3P].
  - "Customers really don't like QR code menus" ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/customers-really-dont-qr-code-menus)) [S], and "QR Code menus are growing even less popular" ([Ipsos](https://www.ipsos.com/en-us/qr-code-menus-are-growing-even-less-popular)) [S].
  - Reported gripes: 66% dislike pulling out a phone on arrival, 55% find QR menus hard to read or browse, and 50% say they lessen the experience. One group saw check averages drop 10% because diners didn't scroll (via [Eat Your Books](https://www.eatyourbooks.com/blog/2024/05/31/have-we-seen-the-last-of-qr-code-menus) and [Food Network](https://www.foodnetwork.com/fn-dish/news/paper-menu-qr-code)) [3P; verify the primary survey].
- **QR order-and-pay businesses struggled.** Sunday exited 60% of its markets (2022) [S], and me&u, after merging with Mr Yum, cut 10% of staff [3P].
- **Lesson.** Paying at the table via QR is a guest convenience. *Ordering* via QR in full service removes hospitality, and it risks lower checks and tips. Offer QR, never force it (R26).

### 3.3 Mobile order-ahead: a capacity problem disguised as a UX feature

- Starbucks' Niccol said mobile orders "come flooding in faster than even our customer can get there," leaving drinks "sitting on the counter" ([Restaurant Business](https://www.restaurantbusinessonline.com/technology/answer-starbucks-mobile-order-problem-has-been-there-all-along), [Fortune](https://fortune.com/2025/02/20/starbucks-brian-niccol-mobile-drive-through-order-mistakes)) [S].
- The fixes:
  - An order-sequencing algorithm prioritizing in-store and drive-thru customers (4-minute target) over mobile and delivery (12 minutes) [S].
  - A lower mobile order cap, from 15 items to 12 ([Yahoo](https://finance.yahoo.com/news/starbucks-making-changes-mobile-ordering-142346087.html)) [S].
- Quartz's headline captures the staff view: "Starbucks baristas don't just make coffee. They run 3 restaurants at once" ([Quartz](https://qz.com/starbucks-mobile-order-labor-barista-workflow-051926)) [S].
- **Design:** R20–R23.

### 3.4 Loyalty, subscriptions, CRM, and receipts

- **Subscriptions.** Panera's Unlimited Sip Club becomes "My Panera+ Sip Club," capped at 30 self-serve refills a month at the same price. This is part of a loyalty revamp toward predictable points and a broader industry move to curb account sharing and abuse ([TheStreet](https://www.thestreet.com/restaurants/panera-major-sip-club-change), [Yahoo](https://finance.yahoo.com/media-advertising/articles/panera-unlimited-sip-club-no-133000059.html)) [S].
- **Tiered loyalty.**
  - Starbucks reintroduced tiers (Green, Gold, Reserve) on March 10, 2026. Members called it a devaluation, especially those who had earned 2 stars per dollar through reloads ([Newsweek](https://www.newsweek.com/starbucks-revamps-rewards-program-2026-11659352)) [S].
  - Yet the new 60-star "$2 off" reward became the most popular redemption ([CNBC](https://www.cnbc.com/2026/04/23/starbucks-loyalty-changes-are-drawing-value-conscious-customers.html)) [S].
  - *Lesson:* low-threshold, legible rewards win. Rule changes need notice and clear explanation.
- **Digital receipts become marketing spam.** 83% of retailers offering e-receipts did so to capture email (2012 Epsilon data). An industry voice put it bluntly: e-receipt consent "does not give you permission to start blanket marketing to them" ([NBC News](https://www.nbcnews.com/business/consumer/paper-or-email-pros-cons-digital-receipts-n15201)) [S; dated source, but the pattern persists [B]]. **Design:** R27.
- **First-party CRM.** Owner.com's growth rests on moving orders off 25–35% commission marketplaces and into owned channels with automated marketing [3P/S].

### 3.5 Tipping ("tipflation")

- Bankrate 2025 ([Bankrate](https://www.bankrate.com/credit-cards/news/tipping-culture-survey/), [press release](https://www.bankrate.com/press-releases/63-of-americans-have-a-negative-view-about-tipping-with-41-saying-tipping-culture-has-gotten-out-of-control/)) [S]:
  - 63% of Americans hold at least one negative view of tipping, up from 59%.
  - 41% say tipping culture is "out of control," and 41% say businesses should "pay employees better."
  - **38% are annoyed by pre-entered tip screens.**
  - **27% tip less or nothing when shown one** (up from 25%); only 11% tip more (down from 14%).
- Pew found 72% say tipping is expected in more places than five years ago; only about a third find it easy to know whether (34%) or how much (33%) to tip (relayed by [Duck Hub](https://www.duck-hub.com/blog/restaurant-tipping-statistics); the original Pew survey is from 2023 [B]).
- **Implication.** Tip screens are a revenue feature for staff that is *actively backfiring*. The POS vendor controls the defaults, so we own the ethics (R25).

### 3.6 Self-checkout, scan-and-go, and computer-vision checkout

- **Self-checkout rollbacks** ([NPR](https://www.npr.org/2024/03/18/1239107299/some-big-retailers-reverse-course-and-scale-back-their-use-of-self-checkout), [Retail Dive](https://www.retaildive.com/news/dollar-general-eliminate-self-checkout-shrink/717520/), [CX Dive](https://www.customerexperiencedive.com/news/walmart-removes-self-checkout-stores-experience/713885/)) [S]:
  - Dollar General pulled self-checkout from its 300 highest-shrink stores and limited it to 5 items elsewhere.
  - Target limited it to 10 items or fewer and opened more staffed lanes.
  - Walmart removed kiosks at select stores.
  - Target cited nearly $500M more in shrink losses in 2023 than in the prior year.
- **Camera-everywhere checkout** failed economically (H2). **Constrained CV** works: Mashgin identifies multiple items in about half a second, with median transactions as low as 7 seconds [V], and Zippin is "widely used at sporting and concert venues" ([CNBC](https://www.cnbc.com/2024/10/09/amazon-just-walk-out-rival-shutters-after-failing-to-secure-funding.html)) [S].
- **Lesson.** Automate where the problem is bounded (a tray, a venue, a known member). Always keep a staffed path (R29).

### 3.7 Pricing: dynamic prices, ESLs, and the law

- Electronic shelf labels are scaling: Walmart in all US stores by end of 2026, and Kroger in about a quarter of its stores [S].
- Kroger faced "surveillance pricing" scrutiny from lawmakers. Kroger said its shelf-label tests used no facial recognition ([EPIC](https://epic.org/krogers-surveillance-pricing-harms-consumers-and-raises-prices-with-or-without-facial-recognition/), [BTPM](https://www.btpm.org/local/2026-02-17/electronic-shelf-labels-leave-concerns-around-surveillance-pricing-in-grocery-stores)) [S].
- New York's law (effective Nov 10, 2025) mandates the disclosure "THIS PRICE WAS SET BY AN ALGORITHM USING YOUR PERSONAL DATA" ([Jones Day](https://www.jonesday.com/en/insights/2025/11/new-yorks-novel-algorithmic-pricing-disclosure-law-takes-effect), [NY AG](https://ag.ny.gov/press-release/2025/attorney-general-james-warns-new-yorkers-about-algorithmic-pricing-new-law-takes)) [S]. Tennessee and New Mexico introduced bills in 2026 ([Skadden](https://www.skadden.com/insights/publications/2026/01/new-york-algorithmic-pricing-law)) [S].
- **Design.** Scheduled and daypart pricing: yes. Personalized upward pricing: blocked by default, with automated legal disclosure (R28).

### 3.8 Voice from the guest's side

Guests encounter misorders, the discomfort of talking to a bot, and absurd failures that go viral (Section 2.3). Pranks such as the 18,000 cups of water show that adversarial input is normal and must be designed for (R36).

### 3.9 Embedded fintech

- **Market pattern [B].** Integrated POS vendors monetize through payments plus:
  - capital (merchant cash advances repaid from a share of daily sales, e.g. Square Loans, Toast Capital, Shopify Capital)
  - instant payouts for a fee
  - business banking
  - payroll and tip payout
- **Recent signals.**
  - SpotOn pairs AI with "Faster Access to Cash" [V].
  - Square added Bitcoin acceptance [S].
  - Clover added biometric pay linked to loyalty [S].
- **The flip side** is fund holds and deactivations (Section 4.1).
- **Design.** Fintech must be transparent and opt-in: fees in dollars, total repayment disclosed, and holds explained (R10, R43, R44).

### 3.10 Commerce: unified, social, and agentic

- **Unified commerce** is every vendor's framing, e.g. Lightspeed's "unified commerce vision" ([Lightspeed](https://www.lightspeedhq.com/news/lightspeed-commerce-advances-unified-commerce-vision-with-new-ai-payments-fulfillment-and-operations-innovations/)) [V]. Shopify's POS and online store sharing one catalog is the reference model [B].
- **Agentic commerce.** Shopify's Agentic Storefronts put products into ChatGPT, Perplexity, and Copilot "with one setup in the admin" [S]. The payment networks and AI platforms announced agent-payment frameworks in 2025 and early 2026 [B].
- **Implication for merchants' POS and catalogs.** An agent is a new, high-volume, low-patience channel. It needs:
  - machine-readable menus, including modifiers, allergens, and prep-time quotes
  - real-time availability
  - verifiable identity and tokenized payment
  - merchant controls such as an allow-list, spend and quantity caps, and rate limits
  - KDS labeling so staff know an order came from an agent
- Bad catalog data becomes bad orders at scale (R50–R51).
- **Social commerce** is mostly a retail catalog-sync problem [B].

### 3.11 Staff experience

- **Vendors are building server-facing AI.** Toast's first ToastIQ features targeted servers directly ("Shift at a Glance," "Digital Chits," "Menu Upsells") [S].
- **Multilingual is becoming standard.** Shopify's Sidekick works in 20 languages [S]. Restaurant crews are frequently multilingual [B].
- **AI changes staff work rather than removing it.** Taco Bell coaches crews on *when to monitor* the AI [S], and Starbucks baristas juggle three channels at once [S].
- **Turnover.** High restaurant turnover makes training time a hard constraint: every minute of onboarding repeats constantly [B]. Hence the 10-minute onboarding requirement (R13).

---

## 4. Pain-point catalog by persona

Each row gives evidence and, where one exists, a verbatim quote. "→ R#" points to the design response in Section 6.

### 4.1 Owner / operator (strongest evidence)

| Theme | Evidence | Tag | → |
|---|---|---|---|
| **Lock-in: contracts, ETFs, lease residuals** | Toast runs 2–3 year contracts; a restaurant closing a year into a 2-year term may owe $1,000+ in ETF, and auto-renewal needs 30–60 days' written notice ([Sleft](https://www.sleftpayments.com/learning-hub/toast-pos-problems-complaints-2026), [Startup Owl](https://startupowl.com/reviews/toast)). These roundups report that r/restaurateur owners "frequently report feeling trapped" by contracts and leases. Clover ETFs run $295–$595 and can exceed $1,000 with the lease ([PaymentPop](https://paymentpop.com/merchant-accounts/clover-pos-customer-reviews/), [RFP.wiki](https://www.rfp.wiki/payments-fraud/point-of-sale-pos-systems/fiserv-clover)) | 3P | R6 |
| **Mandatory processing** | Toast requires Toast Payments, with no option to bring your own processor [3P, same sources] | 3P | R8 |
| **Add-on creep** | Toast's $69/month base plus online ordering ($75), marketing ($185), and KDS ($35) comes to $300–400/month before processing [3P] | 3P | R12 |
| **Vendor-imposed guest fees** | Toast's 99-cent fee on online orders over $10 (mid-2023) had no opt-out. CEO Chris Comparato: "We made the wrong decision" ([Restaurant Dive](https://www.restaurantdive.com/news/toast-software-processor-customer-online-order-fee-payment/688381/), [Restaurant Business](https://www.restaurantbusinessonline.com/technology/toast-remove-99-cent-fee-after-widespread-backlash)). Fox headline: "Restaurateurs fuming over ominous new fee billed to their customers by Big Tech vendor" ([Fox Business](https://www.foxbusiness.com/lifestyle/restaurateurs-fuming-ominous-new-fee-billed-customers-big-tech-vendor)) | S | R7 |
| **Support quality** | A Toast reviewer: every issue means you "start with square one at support." Others report 20-minute holds and callbacks 24+ hours later ([Capterra](https://www.capterra.com/p/136301/Toast-POS/reviews/)). Toast is reported at 3.1/5 on Trustpilot and 235 BBB complaints [3P]. Clover's most consistent complaint is post-sale support: holds, dropped calls, and callbacks that never come ([Brookside](https://brooksidepayments.com/clover-problems/)) | S/3P | R11 |
| **Outages** | Square's outage ran more than 12 hours: "We apologize for letting you down and for the length of time it took" and "made more difficult by our communication frequency and the delayed support response" ([Square](https://squareup.com/us/en/press/an-update-on-last-weeks-outage)). The AWS outage took down Toast sites [S] | S | R1–R3 |
| **Fund holds and deactivations** | Older reports describe Square reserves of 20–30% imposed with little warning and funds held, sometimes for 90 days, after deactivation; merchants describe layoffs and missed mortgage payments ([NerdWallet](https://www.nerdwallet.com/business/software/learn/square-concern), [House hearing record 2021](https://docs.house.gov/meetings/IF/IF16/20210325/111407/HHRG-117-IF16-20210325-SD045.pdf)) | 3P/S | R10 |
| **Third-party commissions** | Delivery commissions of 25–35% are the core pitch behind Owner.com's rise [3P] | 3P | R27, A4 |
| **AI over-promise and ROI doubt** | Presto AI-washing [S]. Only 28% saw tech improve profitability, and 42% were unprofitable in 2025 [3P] | S/3P | R35 |
| **Setup burden** | "Install is very complicated with little help" (Toast, Capterra) [S] | S | R53 |

### 4.2 General manager / shift manager

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Digital order flood breaks the floor | Starbucks' mobile orders sitting on counters; the sequencing fix [S] | S | R21 |
| Supervising AI adds a new duty | Taco Bell: "monitor voice AI and jump in as necessary" [S] | S | R34, R36 |
| Reporting rigidity | Toast Capterra reviewers cite "no report building wizard" and the "inability to save table column organization" [S] | S | R54 |
| Out-of-stock (86) inconsistency | Out-of-stock items "cannot be sold and do not show price when recalled" (Toast, Capterra) [S]; menu sync across channels [B] | S/B | R22 |
| Scheduling time and labor-law compliance | Vendors claim 5–8 manager hours a week are spent on scheduling [3P]; fair-workweek rules [B] | 3P/B | R19, R37 |
| Approval bottlenecks (voids/comps need a manager swipe on site) | [B] | B | R17 |

### 4.3 Server, bartender, cashier

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Tip screens hurt tips as well as guest goodwill | 27% of guests tip less or nothing when shown a pre-entered screen [S] | S | R25 |
| QR ordering removes the server's upsell moment | Anecdote of a 10% lower check average [3P] | 3P | R26 |
| Juggling channels | Baristas "run 3 restaurants at once" [S] | S | R20 |
| Vendors see servers' needs (table context, upsell, shift overview) | ToastIQ's first features: Shift at a Glance, Digital Chits, Menu Upsells [S] | S | R13 |
| **Hypotheses to validate:** split and re-split after partial payment; seat-based checks; pre-authorized bar tabs and walkouts; handheld dead zones and battery; deep modifier trees and slow search; manager swipes for voids; tip-out and tip-pool opacity; end-of-shift cash-out friction | Domain knowledge; direct Reddit mining was not possible (Appendix A) | B | R13–R18 |

### 4.4 Kitchen staff (BOH, expo, baristas)

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Volume spikes from digital channels, with no pacing | Starbucks: orders "flooding in"; the 15→12 item cap [S] | S | R21, R23 |
| Misorders from AI intake mean remakes | Nine sweet teas; butter and ketchup on ice cream (McDonald's/IBM) [S] | S | R36 |
| **Hypotheses:** tickets routed to the wrong station; unreadable or buried modifiers; allergen flags missed; no coursing or fire-time control; separate third-party tablets; printer outages; KDS freezing when offline; no bump recall | Domain knowledge | B | R20, R24 |

### 4.5 Bookkeeper / accountant

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Payout reconciliation is painful enough that vendors market fixes | SpotOn's AI is "pre-reconciling payouts" [V]. Toast is targeting bookkeeping, tax, and accounting with agentic AI [S] | V/S | R41 |
| Cash-flow unpredictability | Holds and reserves [3P/S] | 3P | R10 |
| **Hypotheses:** deposits netted of fees, refunds, chargebacks, and tips don't match daily sales; tip and gift-card liabilities; multi-jurisdiction sales tax; third-party marketplace payouts; lossy QuickBooks/Xero sync; cash variances; multi-entity consolidation | Domain knowledge | B | R41–R42 |

### 4.6 IT, MSP, and franchise operations

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Blast radius of a cloud provider failure | AWS on Oct 20, 2025 took down Toast sites [S] | S | R2 |
| Offline mode that fails when needed | Square 2023: offline payments had issues during the outage [S] | S | R1, R4 |
| Franchisees get new capabilities last | Managerbot's open beta is for "non-franchise" sellers [S] | S | R46 |
| Forced platform migrations; reseller (ISO) layers with different terms | Alleged forced Payeezy-to-Clover migration [S]; ISO-specific Clover ETFs [3P] | S/3P | R6, R47 |
| **Hypotheses:** legacy on-premises servers (Aloha, Micros) needing Windows patching; ransomware exposure (e.g., the NCR Aloha cloud incident, 2023 [B]); PCI scope; brittle integrations and API fees; menu pushes across hundreds of stores; device fleet visibility; data-ownership disputes between franchisor and franchisee | Domain knowledge | B | R46–R49 |

### 4.7 End customer (strong evidence)

| Theme | Evidence | Tag | → |
|---|---|---|---|
| Tipping fatigue | 63% hold a negative view; 38% are annoyed by pre-entered screens [S] | S | R25 |
| QR menus | 90% prefer printed menus; 66% dislike pulling out a phone [3P/S] | S/3P | R26 |
| Receipt spam | E-receipt "does not give you permission to start blanket marketing" [S] | S | R27 |
| Self-checkout frustration and theft policing | Item caps and removals [S] | S | R29 |
| Price fairness | Wendy's; ESLs; New York law [S] | S | R28 |
| Loyalty devaluation | Starbucks backlash; Panera cap [S] | S | R30 |
| Mobile orders not ready, or cold on the counter | Starbucks [S] | S | R21 |
| Frustration with drive-thru AI | McDonald's; Taco Bell [S] | S | R36 |
| Hidden fees | Toast 99-cent fee [S] | S | R7 |

### 4.8 Per-vendor complaint snapshot (the less-evidenced vendors need validation)

- **Toast:** lock-in, required processing, add-on costs, support, AWS dependency [S/3P].
- **Square:** fund holds and deactivations, the 2023 outage, thin restaurant depth [S/3P; depth is B].
- **Clover:** reseller-driven contracts, ETFs and leases, support, forced migration [S/3P].
- **Lightspeed, Shopify POS, SpotOn, TouchBistro, Revel, Aloha, Micros:** not mined this session. Hypotheses [B]:
  - **Lightspeed:** payments requirements and price increases; fragmented restaurant products.
  - **Shopify POS:** weak restaurant features; per-location POS Pro cost.
  - **SpotOn:** promises made by sales reps.
  - **TouchBistro:** add-on costs; iPad and local-server constraints.
  - **Revel:** support and contracts.
  - **Aloha:** dated UI, on-prem maintenance, NCR corporate churn.
  - **Micros:** cost, complexity, Oracle support.
  - See Appendix A.

---

## 5. Delighters: what users love most, per vendor

| Vendor | Commonly cited delighter | Evidence | What to copy |
|---|---|---|---|
| **Square** | Simplicity: self-serve signup, first sale in minutes, free starter tier, one ecosystem | [B] (hypothesis given in the prompt); Managerbot at no extra cost [S] | Minutes-to-first-sale onboarding; AI included in the base price |
| **Toast** | Restaurant depth: handhelds, KDS, modifiers, payroll, and restaurant-specific AI (server features, marketing). Support is "reachable, helpful, and quick for routine questions" in some reviews | [S] features; [S/3P] support is mixed | Depth of the restaurant domain model |
| **Shopify POS** | Omnichannel: one catalog, inventory, and customer record across online and in-store; ahead on agentic channels (Agentic Storefronts); Sidekick in 20 languages | [S] agentic; [B] omnichannel | One product graph; agent-ready catalog |
| **Lightspeed** | Inventory and catalog depth (variants, purchasing) plus B2B wholesale ordering (NuORDER) and AI photo tools | [S] NuORDER and photo AI; [B] inventory reputation | Deep inventory and purchasing model |
| **Clover** | App market and hardware design; bank-channel distribution; biometric checkout (2026) | [S] biometric; [B] app market | Curated app marketplace |
| **SpotOn** | Operator-first, anti-hype AI (Profit AI, anomaly flags, pre-reconciled payouts); fast access to cash | [V] | Profit-focused AI framing |
| **Owner.com** | Commission-free growth: website, app, loyalty, and marketing for a flat fee | [3P/S] | Built-in growth engine |
| **Mashgin** | Speed: median checkout as low as 7 seconds | [V] | Bounded-CV partner |
| **TouchBistro / Aloha / Micros** | TouchBistro's iPad ease with local-server resilience; Aloha's terminal speed and staff muscle memory at volume; Micros' enterprise and hotel scale | [B] | Offline resilience; high-volume speed |

---

## 6. Implications for our design: concrete, testable requirements

Each requirement below carries an acceptance test ("Test"). Section 4 references these by number.

### A. Resilience

1. **R1 Offline-first core.** With the WAN down, all LAN devices (terminals, handhelds, KDS, kiosks) keep ordering, routing, taking cash, taking cards (store-and-forward within merchant limits), and printing for at least 24 hours.
   - *Test:* cut the WAN during a 500-order simulated peak. Expect zero lost orders, zero duplicate charges, and a full sync within 5 minutes of reconnecting.
2. **R2 No single-region dependency.** The cloud runs active-active across regions or providers, with a degraded mode.
   - *Test:* in a chaos drill that kills the primary region, stores keep transacting and staff notice nothing.
3. **R3 Incident communications.** Owners get an in-app banner plus SMS or email within 5 minutes of a detected incident; a public status page exists; a post-incident report ships within 72 hours.
4. **R4 Offline honesty.** Each device shows its connectivity state and count of queued offline payments; merchants set per-card and total offline limits.
5. **R5 Hardware independence.** Runs on commodity iOS, Android, and Windows devices plus certified terminals.
   - *Test:* a spare tablet replaces a failed device in 10 minutes or less.

### B. Commercial trust

6. **R6** Month-to-month software with **$0 ETF**. Hardware can be bought outright, or leased with total cost and buyout price disclosed before signing.
7. **R7 No vendor-imposed guest fees.** Any surcharge or service fee is created by the merchant, itemized, shown before the guest commits, and checked against junk-fee rules.
8. **R8** Published payment pricing; bring-your-own-processor supported, or a no-penalty exit path.
9. **R9 Free full export** of menu, orders, customers (with consent flags), payments, and staff, in documented CSV/JSON, at any time. The API has parity with the export.
10. **R10 Explainable holds.** Every hold or reserve shows a reason code, amount, release date, and an in-app appeal. A human risk analyst responds within 1 business day. New rolling reserves get 7 days' notice except for suspected fraud.
11. **R11 P1 support.** For "can't take payment" or "kitchen down": 24/7 human support, median time to a human under 2 minutes, one case ID for the life of the issue, and device telemetry visible to the agent.
12. **R12** 60 days' notice for any price change, and a single-page fee schedule.

### C. Staff UX and training

13. **R13 10-minute onboarding.** After 10 minutes or less of in-app guided training, a server who has never used a POS can ring a 4-top: modifiers, a seat split, a manager-approved comp, and card payment.
    - *Test:* at least 90% of test users finish in 4 minutes or less.
14. **R14 Split, merge, and transfer.** Split by seat, item, even share, or amount, and re-split after a partial payment, each in 3 taps or fewer. Checks and items move between tables and servers.
15. **R15 Per-user language.** Every staff surface follows the user's language (at launch: English, Spanish, and at least 6 others). Kitchen tickets print in the cook's language and receipts in the guest's.
16. **R16 Speed.** Core actions respond in under 150 ms on the reference device. No common in-service action needs more than 3 taps from the table view.
17. **R17 Remote approvals.** Voids, comps, and discounts can be approved from a manager's phone with reason codes, and all are audit-logged.
18. **R18 Tip transparency.** Staff see their own tips, tip-outs, and pool shares per shift in real time. Pool and tip-out rules are configurable.
19. **R19 Labor compliance.** Fair-workweek, break, overtime, and minor-labor rules raise alerts *before* a violation happens.

### D. Kitchen and channel orchestration

20. **R20 One queue.** POS, kiosk, web/app, QR, marketplaces, AI phone, and AI agents all feed one KDS queue, tagged by channel and promised time. No separate tablets.
21. **R21 Capacity-aware pacing.** A per-station capacity model quotes ready times. Under load, the system extends quotes, throttles, or pauses channels according to a configurable priority (e.g., in-store first).
    - *Test:* 90% of orders are ready within ±3 minutes of the quote in simulation.
22. **R22 Availability sync.** An 86 or inventory stock-out propagates to every channel, including agent catalogs, in 10 seconds or less.
23. **R23 Complexity caps.** Configurable limits per channel on items and modifiers per order; large orders need staff acceptance.
24. **R24 KDS legibility.** Modifiers and allergens are highlighted, tickets color-code by age, and expo, bump, and recall work offline.

### E. Guest-experience ethics

25. **R25 Ethical tipping.** Tip prompts are off by default for counter service and retail. When on, "No tip" and "Custom" match the presets in size and contrast; presets are calculated on the pre-tax, pre-fee subtotal and shown as both % and $.
    - *Test:* a UI audit checklist, plus tracking of the no-tip rate.
26. **R26 QR is optional.** QR never replaces the server or a printed-menu path. QR pay needs no app and no account.
27. **R27 Receipts are not consent.** Giving an e-receipt email never opts the guest into marketing. The marketing opt-in is separate and unchecked, and the receipt carries no promotions unless the guest opted in.
28. **R28 Price integrity.** Scheduled and daypart pricing are supported. Personalized upward pricing is blocked by default. Any use of personal data in pricing auto-renders the legally required disclosures (e.g., New York's). Channel price differences are disclosed.
29. **R29 Accessible self-service.** Kiosks and self-checkout support screen readers or audio, have accessible reach heights and high contrast, accept cash where required, and answer help calls within 60 seconds. A staffed path always exists in retail mode.
30. **R30 Loyalty guardrails.** Members get notice before program changes (default 30 days) with an auto-generated change summary. Subscriptions support caps, rate limits, and device binding.
31. **R31 Kiosk flow.** A first-time user can order a standard combo in 60 seconds or less, with at most one skippable upsell per step. Attach rate is A/B-tested against time-to-order.

### F. AI

32. **R32 Verifiable answers.** Every AI number cites its report, filters, time range, and data freshness, with one tap to open the source.
33. **R33 Approval-gated actions.** AI can draft menu, schedule, price, purchase-order, and campaign changes. Applying them requires human approval of a diff, allows undo for 30 days, and writes an audit entry naming the approver and model version.
34. **R34 Autonomy dial.** Owners set autonomy per capability (off / suggest / auto with notification / auto). The default is "suggest" for money, prices, schedules, and guest communications.
35. **R35 Honest automation metrics.** Voice and phone AI dashboards show the no-intervention rate, handoff rate, and audited accuracy. Marketing may cite only these measured numbers.
36. **R36 Voice safety.**
    - Readback of every order.
    - Sanity limits: more than 10 of one item, or an order over $X, needs confirmation or a human.
    - Per-store and per-daypart on/off toggles.
    - Human takeover in 2 seconds or less.
    - Accent and multilingual test suites pass before any store goes live.
37. **R37 Forecasting.** 15-minute forecasts by channel and item feed labor, prep, and purchasing. Accuracy (MAPE) is visible, and manager overrides retrain the model.
38. **R38 Sponsorship labels.** Recommendations shaped by a commercial partner are labeled and can be turned off.
39. **R39 Data boundaries.** No cross-merchant model training without opt-in. Benchmarks use k-anonymized cohorts.
40. **R40 AI phone agent.** Answers 100% of calls, orders natively against the live menu (modifiers, 86 status), hands off to staff or texts a link, and stores transcripts. Accuracy is measured.

### G. Back office and fintech

41. **R41 Reconciliation.** Each deposit breaks down into transactions, refunds, chargebacks, fees, tips, and adjustments, auto-matched to bank lines.
    - *Test:* closing a single-location day takes 5 minutes or less.
42. **R42 Accounting sync.** Chart-of-accounts mapping with daily journal entries to QuickBooks Online and Xero, covering gift-card and tip liabilities, sales tax by jurisdiction, and marketplace payouts.
43. **R43 Instant payouts.** Optional, with the fee shown in dollars before each transfer.
44. **R44 Capital offers.** Show total repayment in dollars, holdback %, and an APR-equivalent. Never pre-checked, and never tied to processing terms.
45. **R45 Payroll.** Payroll export or integrated payroll; on-demand tip payout where legal, with a full ledger.

### H. Franchise, IT, security, platform, agentic

46. **R46 Franchise hierarchy.** Menus, prices, and policies inherit brand → region → store, with permitted local overrides and a visible diff. Every AI and analytics feature is available to franchisees. Franchisor and franchisee data rights are configurable.
47. **R47 Fleet management.** Remote device inventory, health, and versions; staged rollouts (1% → 10% → 100%) with automatic rollback; configuration-as-code for enterprise customers.
48. **R48 Security.** No store-level Windows server. Managed, hardened devices; P2PE and tokenization to shrink PCI scope; SSO/MFA; least-privilege roles; tamper-evident audit logs.
49. **R49 Open platform.** Public APIs and webhooks for orders, menu, inventory, customers, and payments; a sandbox; a published rate-limit policy; an app marketplace with security review; no fees for access to merchant-owned data.
50. **R50 Agent-ready catalog.** Real-time structured menu data (modifiers, allergens, prices, availability, prep quotes) via API and emerging agent protocols, with a merchant allow-list, caps, and rate limits. Agent orders are labeled in the KDS and in reports.
51. **R51 Agent payments.** Tokenized, agent-initiated payments with spend limits and clear chargeback rules, adopted as the network frameworks stabilize [B].
52. **R52 Bounded CV.** Tray-based CV checkout through partner integration for venues and cafeterias. No store-wide camera checkout on the roadmap.
53. **R53 AI setup.** Import the menu from a PDF, photo, or competitor POS export.
    - *Test:* a simple café takes its first live transaction within 60 minutes of unboxing.
54. **R54 Reporting.** Saved custom views, a report builder, and scheduled emails. A metrics glossary is shared by reports and the AI.

**UX principles**

1. **Offline is a mode, not an outage.**
2. **The kitchen sets the pace, not the channel.**
3. **Never make the guest do the restaurant's work unless it gives the guest control.**
4. **Every automated decision can be explained and undone.**
5. **Money is sacred.** No fee, hold, or price change without a visible reason and a human path.
6. **Three taps to anything common during service.**

---

## Appendix A: Validation backlog (not run because the search budget was exhausted and Reddit was inaccessible)

1. **Reddit, via an authenticated or alternative channel**, in these subreddits:
   - r/restaurateur: "Toast contract," "switching POS," "SpotOn vs Toast," "Square for Restaurants"
   - r/KitchenConfidential: "KDS," "ticket printer," "online orders"
   - r/Serverlife: "split checks," "handheld," "tip screen," "tip out"
   - r/bartenders: "tabs," "preauth," "walkouts"
   - r/sysadmin and r/msp: "Aloha," "Micros," "Simphony," "POS network"
   - r/shopify: "POS Pro," "offline"
   - r/squareup: "funds held," "deactivated"
   - r/smallbusiness and r/retail: "self checkout," "Lightspeed"
2. **Review sites** (G2, Capterra, Trustpilot, BBB) for Lightspeed Retail and Restaurant, Shopify POS, SpotOn, TouchBistro, Revel, NCR Aloha, and Oracle Simphony. Capture the top 5 complaints and top 5 praise themes, with short quotes and reviewer roles.
3. **Primary sources** to confirm:
   - the QR-menu survey (66% / 55% / 50% figures)
   - the 76% / 28% tech-ROI statistic
   - the McDonald's +30% kiosk figure
   - Pew's 2023 tipping survey
   - Managerbot's ~1M adoption figure
4. **Topics not covered this session:**
   - embedded fintech specifics (capital, instant-payout fees, banking, payroll uptake)
   - AI produce-recognition scales
   - agent-payment protocol details (ACP, AP2, UCP, Visa, Mastercard)
   - social commerce
   - staff turnover and training-time benchmarks
