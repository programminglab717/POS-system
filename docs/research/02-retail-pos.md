# Track 02: Retail POS research (specialty retail, grocery, convenience)

Prepared 2026-09-27 for the architecture phase. Audience: the architect turning research into architecture and a feature list.

## 0. How to read this report

**Research constraints (important).** This session's shared web-search budget (200 queries across all parallel research agents) ran out after this track had issued about 22 queries. The egress proxy also blocked direct page fetches for every domain tried: squareup.com, help.shopify.com, learn.microsoft.com, techcrunch.com, wikipedia.org and reddit.com. To compensate, I read primary documentation for **Microsoft Dynamics 365 Commerce** (the MicrosoftDocs public docs repo) and **Odoo** (the odoo/odoo source and odoo/documentation repos) through GitHub code search.

**Evidence convention.**
- A claim with an inline link was verified in this session, either from search-result content or from primary docs and source.
- **[unverified]** marks background knowledge from training data up to mid-2026 that I could not re-check here. Treat it as a hypothesis. Appendix A lists what to verify first.
- Many "review" and "pricing" sites are affiliates or competitors. KORONA's blog posts about Square and Clover are an example. I flag this where it matters.

**What this means for coverage.** The evidence is strong for Shopify POS, Square, Lightspeed, Clover (contract complaints only), Dynamics 365 Commerce and Odoo. It is thinner for grocery, convenience and most enterprise vendors. For those, I kept claims to things I'm confident about and tagged them.

---

## 1. Landscape table

| Vendor | Segment | Pricing model (software + processing) | Hardware model | Lock-in | Top strengths | Top weaknesses |
|---|---|---|---|---|---|---|
| **Shopify POS** | SMB to mid-market, commerce-first omnichannel | POS Lite included with Shopify plans. POS Pro is **$89/location/month**, on top of the Shopify plan ([ringly.io](https://www.ringly.io/blog/shopify-pos-reviews), [StartupOwl](https://startupowl.com/reviews/shopify-pos)). Card processing through Shopify Payments **[unverified: rates vary by plan and country]** | Shopify POS Go, POS Terminal, Tap & Chip, Chipper 2X BT, WisePad 3; iOS and Android apps ([Shopify Help](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/selling-offline/offline-payments)) | High. Readers and in-person processing are tied to Shopify Payments **[unverified]** | One catalog, customer base and order ledger across web and store. 2,048 variants per product since Oct 2025. B2B price lists and net terms apply in-store | Maximum of 3 option axes. Forecasting was lost when Stocky shut down (Aug 31, 2026). POS login depends on the cloud (Dec 1, 2025 outage) |
| **Square (Retail)** | Micro to SMB | Since **Oct 6, 2025**, 18 à la carte subscriptions were replaced by three plans. Free: $0, 2.6% + 15¢ in person. Plus: **$49/location/mo**, 2.5% + 15¢ in person, 2.9% + 30¢ online. Premium: **$149/mo** ([Square press](https://squareup.com/us/en/press/unified-pricing-and-packaging), [TSG](https://tsgpayments.com/square-simplifies-commerce-software-with-new-unified-plans-pricing/), [POSUSA](https://www.posusa.com/square-fees-pricing/)) | Square Register, Terminal, Handheld and readers | High. Hardware works only with Square processing **[unverified]**. No long-term contract **[unverified]** | Fast setup, flat published pricing, one ecosystem (loyalty, online, staff) | 250 variations per item (120 in Square Online). No per-unit serial numbers. The merchant carries offline card risk (72 h expiry). Outages in 2023 and 2025 |
| **Lightspeed Retail R-Series** | Specialty SMB and mid-market (bike, sporting goods, apparel, golf) | Basic **$89**, Core **$149**, Plus **$289** per month billed annually; **$109 / $179 / $339** billed monthly ([Capterra](https://www.capterra.com/p/120491/Lightspeed-Retail/pricing/), [Loman](https://loman.ai/blog/lightspeed-pos-pricing)). **Lightspeed Payments is mandatory.** Using a third-party processor triggers an unpublished monthly fee ([StartupOwl](https://startupowl.com/reviews/lightspeed), [Business.com](https://www.business.com/reviews/lightspeed-pro/)) | Browser or iPad, plus Lightspeed payment terminals | Medium-high: processor penalty and two incompatible product lines | Serial numbers, work orders and repairs, complex purchasing, layered costing, multi-location reporting ([Mortar](https://usemortar.com/blog/lightspeed-r-series-vs-x-series)) | Offline card payments are not written to sales history or inventory. Offline caps: $5k per transaction, $50k total |
| **Lightspeed Retail X-Series** (formerly Vend, acquired 2021) | SMB multi-store (fashion, specialty food) | Same plan family ([Lightspeed X-Series plans](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25533686832923-Understanding-Retail-POS-X-Series-plans)) | Browser or iPad | Same as R-Series | Offline selling in the browser covers layaway, quotes and on-account sales ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25534272395163-Selling-in-offline-mode)) | Offline mode is limited to the Sell screen. No gift cards and no editing of existing customers while offline. Backend and API differ from R-Series ([SKUPlugs](https://skuplugs.com/lightspeed-r-series-vs-x-series/)) |
| **Clover (Fiserv)** | SMB, sold through Fiserv, banks and ISOs | Software plan plus processing, priced through Fiserv or ISO quotes. Early termination fees of **$295–$595**. Leases run **36–48 months** ([Brookside](https://brooksidepayments.com/clover-problems/), [PaymentPop](https://paymentpop.com/merchant-accounts/clover-pos-customer-reviews/)) | Clover Station, Mini, Flex and Go (proprietary) | Very high. Leases can't be cancelled. Hardware is tied to Fiserv **[unverified]** | Countertop hardware, app market, bank distribution | Opaque, reseller-dependent pricing. A $99.95/mo per-device charge on legacy hardware was reported in 2025. Cancelling is slow |
| **KORONA POS** | SMB and franchise (liquor, smoke/vape, convenience, attractions) | Per-register subscription, month-to-month **[unverified]** | Commodity hardware **[unverified]** | Low. Processor-agnostic **[unverified]** | Offline mode with the full sale flow; franchise and chain management **[unverified]** | Thin omnichannel; smaller ecosystem **[unverified]** |
| **Heartland Retail** (Global Payments) | SMB and mid specialty | Per-register subscription plus Heartland processing **[unverified]** | iPad **[unverified]** | Medium **[unverified]** | Purchasing, replenishment, multi-store inventory **[unverified]** | Parent company in upheaval (Global Payments' Worldpay deal) **[unverified]** |
| **NCR Counterpoint** (NCR Voyix) | Mid-market specialty (garden, sporting goods, gift) | Licenses sold through VARs **[unverified]** | Windows PCs + SQL Server, on-prem **[unverified]** | Medium: depends on the VAR | Deep specialty features: grid items, serials, house accounts, multi-site **[unverified]** | Windows/SQL upkeep, dated UI, support quality varies by VAR **[unverified]** |
| **Odoo POS** | SMB to mid-market, ERP-first | Community edition is open source; Enterprise is a per-user subscription **[unverified: prices]** | Any browser; IoT Box for peripherals **[unverified]** | Low (open source, self-hostable) | POS runs on the ERP's own inventory, accounting and pricelists. Cash-difference threshold. Loyalty, eWallet and gift cards in one model. Orders keyed by UUID ([odoo source](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/pos_config.py)) | Offline mode is limited. Large catalogs need "limited loading". Usually needs an integrator |
| **Hike** | SMB (AU/NZ, UK, US) | Per-store subscription **[unverified]** | iPad or browser | Low | Layby (layaway), multi-store, e-commerce sync **[unverified]** | Insufficient data this session |
| **Loyverse** | Micro | Free core, paid add-ons **[unverified]** | Phone or tablet | Low | Free and simple | Shallow inventory and no B2B **[unverified]** |
| **Zettle (PayPal)** | Micro (UK/EU) | Free app plus per-transaction fees **[unverified]** | Zettle Reader | High (PayPal processing) | Simple | Shallow retail features. US availability should be checked **[unverified]** |
| **SumUp POS Lite/Pro** | Micro and SMB (EU) | Per-transaction fees plus POS Pro subscription **[unverified]** | SumUp terminals. POS Pro came from the Goodtill acquisition **[unverified]** | High | Cheap entry | Shallow features **[unverified]** |
| **Epos Now** | SMB (UK/US) | Hardware bundles, subscription and payments **[unverified]** | Bundled | Contracts **[unverified]** | Low entry cost | Frequent complaints about billing, contracts and support **[unverified]** |
| **Microsoft Dynamics 365 Commerce** | Enterprise and upper mid-market | D365 per-user licensing plus a systems integrator **[unverified: prices]** | Store Commerce on Windows, Android and iOS, plus Store Commerce for web. Offline mode needs a local SQL Server ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/store-commerce)) | Medium-high (tightly coupled to the ERP) | Most expressive discount engine found: Exclusive, Best price and Compounded, combined with priorities. Full cash office (blind close, safe and bank drops). Orders that mix fulfillment types. Fiscal certifications | Offline DB on SQL Express is capped at 10 GB. Duplicate transaction IDs can occur. Gift cards and pay-by-link need connectivity. Heavy implementation |
| **Oracle Retail Xstore** | Tier-1 specialty and department stores | License or cloud service plus an SI **[unverified]** | Windows/Linux registers, mobile **[unverified]** | High **[unverified]** | Registers keep working when central services are down; deep enterprise features **[unverified]** | Cost, long upgrade cycles **[unverified]** |
| **Retail Pro (Prism)** | Global mid-market and enterprise specialty | Licenses through partners **[unverified]** | Store servers **[unverified]** | Medium | Global localization, apparel matrix **[unverified]** | Quality depends on the partner **[unverified]** |
| **Aptos** | Enterprise specialty | SaaS **[unverified]** | — | — | Cloud unified commerce **[unverified]** | Insufficient data |
| **Manhattan Active POS** | Enterprise | SaaS inside Manhattan Active Omni **[unverified]** | Browser and mobile | High (coupled to Manhattan OMS) | Versionless cloud; POS joined natively to order management and store fulfillment **[unverified]** | Enterprise-only. Offline depth not verified |
| **SAP Customer Checkout** | Enterprises on SAP | Licensed with SAP **[unverified]** | Java POS **[unverified]** | High | Works offline, integrates with S/4 **[unverified]** | Retail depth below the specialists; SAP leans on partners **[unverified]** |
| **ECRS Catapult** | Independent grocers and co-ops | License plus support **[unverified]** | Scanner-scales, self-checkout **[unverified]** | Medium | Grocery depth: scales, EBT/WIC, promotions **[unverified]** | Insufficient data |
| **LOC Software (SMS)** | Independent grocers | — | — | — | Insufficient data | Insufficient data |
| **NCR Emerald** | Grocery | SaaS **[unverified]** | NCR hardware and self-checkout | High | Cloud-native grocery POS **[unverified]** | Insufficient data |
| **Toshiba TCx** | Tier-1 grocery and general merchandise | License plus services **[unverified]** | Toshiba hardware, TCx Sky OS **[unverified]** | High | Proven at tier-1 scale **[unverified]** | 4690-era heritage, complexity **[unverified]** |
| **Diebold Nixdorf Vynamic** | Tier-1 grocery, EU | License plus services **[unverified]** | DN POS and self-checkout **[unverified]** | High | Self-checkout plus vision AI for age and produce **[unverified]** | Emerged from Chapter 11 in 2023 **[unverified]** |
| **Verifone Commander** | Convenience and fuel | Through distributors **[unverified]** | Site controller + Topaz/Ruby registers **[unverified]** | Very high (fuel certification) | Forecourt control, EMV at the pump **[unverified]** | Dated UX; upgrades gated by certification **[unverified]** |
| **Gilbarco Passport** | Convenience and fuel | Through distributors **[unverified]** | Passport + Gilbarco dispensers **[unverified]** | Very high | Integrated with dispensers and tank gauges **[unverified]** | Same as Commander **[unverified]** |
| **Petrosoft** | Convenience back office | SaaS **[unverified]** | Integrates with Verifone and Gilbarco POS **[unverified]** | Low-medium | Cloud back office, fuel pricing, scan data **[unverified]** | Not a POS core; depends on POS exports **[unverified]** |

---

## 2. Best-in-class feature catalog

### 2.1 Catalog: variants, bundles, serials, lots, UoM, weighed items

- **Variant scale.** Shopify raised the limit to **2,048 variants per product on all plans** on Oct 15, 2025, up from 100 ([Shopify dev changelog](https://shopify.dev/changelog/the-product-variant-limit-is-now-2048-for-all-merchants), [Shopify blog](https://www.shopify.com/blog/2048-variants)). Products are still capped at **3 options** and **250 media files** ([Craftshift](https://craftshift.com/shopify-variant-limit-2026/)). Square allows **250 variations per item** on POS and **120 on Square Online**, and cites performance as the reason. Fabric and art-supply stores that need 300–500 colors split products to fit ([Square Community](https://community.squareup.com/t5/Using-Square/What-s-the-maximum-variations-allowed-per-item/m-p/127690), [Square Dev Forums](https://developer.squareup.com/forums/t/square-catalog-only-allows-250-variants-per-item/11902)).
  - Shopify leads on count. No SMB vendor offers more than 3 axes; apparel needs color × size × length × width.
  - D365 has four product dimensions (configuration, size, color, style) plus version **[unverified]**.
- **Serialized items.**
  - D365 has a POS serial-number management page. Inbound and outbound POS operations can register or validate serials, but only when the item's tracking dimension is set to **"Active"** rather than **"Active in sales process"**. That setting decides whether a serial is captured at receipt or only at sale ([MS Learn: serialized items](https://learn.microsoft.com/en-us/dynamics365/commerce/pos-serialized-items)).
  - Lightspeed R-Series supports serial tracking plus work orders and repairs ([Mortar](https://usemortar.com/blog/lightspeed-r-series-vs-x-series)).
  - Square for Retail can't attach serials to received units ([TrustRadius](https://www.trustradius.com/products/square-for-retail/reviews?qs=pros-and-cons)).
  - **Model to copy:** D365's per-item choice of *when* the serial is captured.
- **Weighed, PLU and scale items.** Odoo has a native "To Weigh" product flag, which the POS loads alongside `uom_id`, barcode and `combo_ids` ([odoo product_template.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/product_template.py)). Grocery vendors (ECRS, Toshiba, DN, NCR) handle scanner-scales, PLU and GS1 embedded-price or weight barcodes **[unverified]**.
- **Bundles and kits.** Odoo "combo" products are first-class POS data (`combo_ids`) ([odoo](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/product_template.py)). D365 kits **[unverified]**. Shopify bundles rely on apps **[unverified]**.
- **Lot and expiry.** No SMB vendor was verified to enforce FEFO or block expired items at the register. This is a gap; see §4.
- **Custom and non-inventory items.** D365 allows price overrides only on products configured for them (operation 104) ([MS Learn: POS operations](https://learn.microsoft.com/en-us/dynamics365/commerce/pos-operations)). This per-item permission is the right pattern.

### 2.2 Pricing and promotions engine

- **Best observed: Dynamics 365 Commerce** ([retail discounts overview](https://learn.microsoft.com/en-us/dynamics365/commerce/retail-discounts-overview), [applying multiple discounts](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/Apply-multiple-retail-discounts), [price settings](https://learn.microsoft.com/en-us/dynamics365/commerce/price-settings)). Its building blocks:
  - **Four discount types:** Simple, Mix and Match, Quantity (multi-buy or tiered), and Threshold (spend X).
  - **Three concurrency modes per discount:** Exclusive, Best price and Compounded. Exclusive discounts are always evaluated first and block all others on those lines. Two Exclusive discounts compete on best price.
  - **Pricing priority numbers.** A global "concurrency control model" decides how priorities interact. For example:
    - "Best price and compound within priority, never compound across priorities": once a line is discounted, lower priorities are ignored.
    - "Best price only within priority, always compound across priorities".
  - **Mix-and-match "least expensive"** discounts with deal price, percent or amount off. The "number of least expensive lines" must be more than one and less than the number of products. A **"Multiple occurrences mode"** setting (e.g. Favor retailer) resolves repeated matches ([MS UPM doc](https://learn.microsoft.com/en-us/dynamics365/supply-chain/unified-pricing-management/upm-margin-discount-pricing-rules)).
  - **Why it's the best:** each stacking behavior is an explicit, named setting instead of an implicit rule.
  - **Its flaw:** nobody at the counter can explain an outcome.
- **Customer-specific price books.**
  - Shopify B2B price lists and catalogs now apply at POS. Foundational B2B moved from Plus-only to all paid plans in 2026 ([Ask Phill](https://askphill.com/blogs/blog/b2b-on-shopify), [Makro](https://www.makroagency.com/insights/how-to-create-custom-price-lists-and-net-payment-terms-for-b2b-buyers-on-shopify)).
  - Odoo lets each POS config carry a set of allowed pricelists (`pos_available_pricelist_ids`) ([odoo res_config_settings.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/res_config_settings.py)).
- **Coupons, loyalty and stored value in one model.** Odoo's single `loyalty.program` covers **Coupons, Promos, Gift Cards, Loyalty Cards, eWallets and Discount codes** across POS and e-commerce ([odoo loyalty_program.py](https://github.com/odoo/odoo/blob/master/addons/loyalty/models/loyalty_program.py)). One rules model and one ledger is a good pattern.
- **How promotion engines break at scale.** This is analysis based on the verified designs above.
  1. **Combinatorial search.** Finding the "best price" across overlapping mix-and-match deals is a set-packing problem. Engines fall back on heuristics or priority cut-offs, which is why D365 has priorities and Favor-retailer modes. The results can surprise customers.
  2. **Divergence between online and offline.** If the register runs different code or stale data than the cloud, the same basket prices differently.
  3. **Slow propagation.** In D365, a Commerce-parameter change can take **up to 24 hours** to reach channels unless IIS is reset ([MS Learn: seamless offline](https://learn.microsoft.com/en-us/dynamics365/commerce/seamless-offline-improvements)).
  4. **Returns.** Partially returning a promotional basket needs line-level discount allocation stored at sale time.
  5. **Explainability.** No vendor was found to show cashiers "why this price" or "why this promo didn't apply".

### 2.3 Inventory

- **Purchasing, transfers and counts.**
  - Shopify moved POs, transfers, stocktakes and receiving into the native admin.
  - Stocky lost transfers and min/max forecasting on **July 7, 2025**, left the App Store in **Feb 2026**, and **stopped working Aug 31, 2026**. The native tools do **not** replace demand forecasting, ABC analysis or supplier management ([Finaloop](https://www.finaloop.com/blog/stocky-discontinued-in-2026-what-shopify-merchants-should-do), [Sensible Tools](https://sensible.tools/blog/stocky-deprecated-shopify-inventory-forecasting-alternatives), [Stockful](https://stockful.app/blog/stocky-removed-from-shopify-what-merchants-need-to-do-now)).
  - Lightspeed R-Series is the strongest verified SMB option for complex purchasing and layered costing ([Mortar](https://usemortar.com/blog/lightspeed-r-series-vs-x-series)).
  - Heartland Retail and Counterpoint are reputed to be strong at purchasing **[unverified]**.
- **Replenishment and forecasting.** SMB now has a gap (Shopify's forecasting was dropped). Enterprise gets this from ERP master planning (D365, SAP) **[unverified]**.
- **Negative stock.** No vendor policy was verified. Design for a per-item or per-location allow, warn or block setting.
- **RFID.** Not verified for any vendor. Enterprise suites integrate partner RFID platforms **[unverified]**.
- **Scale behavior.** Odoo POS has a "limited products loading" feature with a tested priority order for which product templates load first. It exists because full catalogs don't fit a browser session ([odoo tests](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/tests/test_pos_basic_config.py)).

### 2.4 Omnichannel (BOPIS, BORIS, ship-from-store, endless aisle)

- **Best verified: D365 customer orders** ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/customer-orders-overview)).
  - One cart can mix carry-out, **"Pick up selected / Pick up all"** at a chosen store, and shipments with a requested ship date.
  - A **"Deposit override"** changes the amount due now.
  - A **card is kept on file** and charged for the balance at invoicing.
  - This is the model for special orders, endless aisle and BOPIS in one flow.
- **Shopify** leads on one catalog, customer base and order ledger for web and store, and now B2B at POS ([Ask Phill](https://askphill.com/blogs/blog/b2b-on-shopify)). Details of the POS Pro fulfillment features were not re-verified **[unverified]**.
- **Manhattan Active** joins POS and OMS natively for ship-from-store and store fulfillment **[unverified]**.

### 2.5 Customers, returns, layaway, special orders, quotes, gift cards, loyalty

- **Layaway, quotes and on-account.** Lightspeed X-Series can create quotes, mark layaways and complete on-account sales **while offline** ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25534272395163-Selling-in-offline-mode)). No other vendor was verified to do this offline.
- **Returns.**
  - D365 exposes "Return transaction" (recall a receipt to return some or all items) and "Show journal" (reprint receipts and gift receipts, recall for return) as distinct operations ([MS Learn: POS operations](https://learn.microsoft.com/en-us/dynamics365/commerce/pos-operations)).
  - Receipt-less return fraud scoring (e.g. Appriss Retail) is an enterprise add-on **[unverified]**.
- **Gift cards: best verified is D365** ([MS Learn: seamless offline](https://learn.microsoft.com/en-us/dynamics365/commerce/seamless-offline-improvements), [What's new 10.0.45](https://learn.microsoft.com/en-us/dynamics365/commerce/get-started/whats-new-commerce-10-0-45)).
  - Internal gift card balances live centrally.
  - The system **locks a gift card as soon as it's added to a transaction**, so it can't be spent on two terminals at once.
  - An admin switch allows concluding gift-card transactions after the POS goes offline.
  - Until 10.0.45, an **external gift card's balance was added before the customer paid**, which was a fraud window. 10.0.45 delays the load until payment succeeds.
- **Loyalty.** Odoo's single program model (loyalty, eWallet, gift card, coupons) is the cleanest verified design ([odoo](https://github.com/odoo/odoo/blob/master/addons/loyalty/models/loyalty_program.py)).

### 2.6 Cash management

- **Best verified: D365** ([cash management](https://learn.microsoft.com/en-us/dynamics365/commerce/cash-mgmt), [shift and drawer management](https://learn.microsoft.com/en-us/dynamics365/commerce/shift-drawer-management), [POS operations](https://learn.microsoft.com/en-us/dynamics365/commerce/pos-operations)).
  - Shift objects with **tender declaration per payment method** (op 1052).
  - **Blind close** (op 1053): the shift is closed to new transactions but still open to drawer operations such as tender removal and declaration.
  - **Suspend shift** (op 1054) and **Manage shifts** (active, suspended, blind-closed).
  - **Safe drop** and **bank drop**. A safe is modelled as its own shift that a manager reconciles and closes.
  - A permission flag, **"Allow blind close"**, set per permission group ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/tasks/create-pos-permission-groups)).
- **Best small-scale pattern: Odoo.**
  - **"Amount Authorized Difference"** is the largest gap a non-manager may accept between counted and theoretical cash at session close. Anything larger forces a manager.
  - Cash control on or off.
  - **"Total Rounding (PoS)"** for cash rounding ([odoo pos_config.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/pos_config.py), [res_config_settings.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/res_config_settings.py)).
- **Why cash rounding matters now.** The US Mint struck its last circulating penny in Nov 2025, so US retailers must round cash totals. State rules and benefit-program rules differ **[unverified]**.

### 2.7 Employees

- **Permissions.** D365 permission groups hold granular flags: allow edit order, retrieve order, password change, blind close, X-report printing ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/tasks/create-pos-permission-groups), [shift mgmt](https://learn.microsoft.com/en-us/dynamics365/commerce/shift-drawer-management)). Odoo's `pos_hr` adds an employee login screen ([odoo pos_config.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/pos_config.py)).
- **Commissions and time clock.** Lightspeed R-Series (commissions), Square (team plans) and KORONA are reputed for these **[unverified]**. Shopify POS offers staff attribution but no native commissions **[unverified]**.

### 2.8 Self-checkout, scan-and-go, mobile POS, line busting

- **Mobile and handheld.**
  - Square Handheld is a first-party device that supports offline payments ([Square Help](https://squareup.com/help/us/en/article/8551-view-offline-payments)).
  - D365 10.0.45 added **Store Commerce offline on iOS and Android (public preview), backed by SQLite** ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/get-started/whats-new-commerce-10-0-45)). That makes offline line-busting handhelds possible in enterprise.
- **Self-checkout.**
  - Tier-1 grocery self-checkout comes from Toshiba, Diebold Nixdorf and NCR **[unverified]**.
  - DN markets vision-based age estimation and produce recognition **[unverified]**.
- **Scan-and-go.** No vendor offering was verified this session.

### 2.9 EBT, SNAP and WIC; age verification

All background knowledge, **[unverified]**.
- **SNAP.** Requires per-item eligibility flags, a SNAP-eligible subtotal and split tender. Sales tax may not be charged on the SNAP-paid portion.
- **WIC.** eWIC requires ingesting each state's Approved Product List and matching against category and subcategory benefit balances.
- **SNAP restriction waivers (new in 2026).** USDA approved state waivers restricting SNAP purchases of items such as soda and candy. The first took effect Jan 1, 2026. Eligibility now varies **by state and date**, not just by federal category, and most SMB POS item models have one global "EBT eligible" flag.
- **Who supports EBT.** Clover supports EBT through Fiserv. Square and Shopify historically did not support EBT natively **[unverified]**.
- **Age verification.**
  - Convenience stores are moving to ID scanning (AAMVA PDF417) and NACS's TruAge token.
  - Mobile driver's licenses (ISO 18013-5) are emerging.
  - Jurisdiction-specific alcohol sale hours apply.

### 2.10 Reporting, analytics, multi-store, enterprise, franchise

- **Verified.** Square's Oct 2025 plan consolidation gates advanced features by tier ([Square press](https://squareup.com/us/en/press/unified-pricing-and-packaging)). Lightspeed R-Series targets multi-location reporting ([Mortar](https://usemortar.com/blog/lightspeed-r-series-vs-x-series)). D365 inherits ERP reporting and Power BI **[unverified]**.
- **Franchise royalty and fee roll-ups.** KORONA **[unverified]**. None verified.
- **Takeaway.** No vendor was verified to offer a full-fidelity event stream (every line, discount allocation, tax and tender) exportable to a customer-owned warehouse. That becomes a design requirement.

### 2.11 B2B (invoices, net terms, tax-exempt customers, house accounts)

- **Best verified at POS: Shopify B2B.** Company profiles with multiple buyers and roles, catalogs and price lists, **net 7, 15, 30, 60 and 90 terms**, now on all paid plans and applied at POS ([Ask Phill](https://askphill.com/blogs/blog/b2b-on-shopify), [Makro](https://www.makroagency.com/insights/how-to-create-custom-price-lists-and-net-payment-terms-for-b2b-buyers-on-shopify)).
- **On-account sales.** Lightspeed X-Series supports them, offline too ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25534272395163-Selling-in-offline-mode)).
- **Invoicing and receivables.** Odoo and D365 get these natively from their ERP layers. Counterpoint has house accounts with statements **[unverified]**.

### 2.12 Offline mode: side-by-side (verified)

| Vendor | Card payments offline | Limits and expiry | Are offline sales full records? | Unavailable offline |
|---|---|---|---|---|
| Shopify POS | Yes, on POS Go, POS Terminal, Tap & Chip, Chipper 2X BT and WisePad 3. Needs app ≥ 9.14.0 and the "Accept offline credit and debit payments" permission. **No** swipe, Tap to Pay on iPhone or Android, or MOTO. **No** Interac (Canada) or eftpos (Australia). Chip & Swipe reader unsupported from Sept 15, 2025 ([Shopify Help](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/selling-offline/offline-payments)). Multi-entity support added ([CedCommerce](https://cedcommerce.com/blog/shopify-adds-offline-payments-support-for-multi-entity-setups/)) | Limits exist; values not verified | Synced on reconnect | Reviewers report that inventory lookup, new-customer creation and discount codes need connectivity **[unverified]** |
| Square | Yes (Register, Terminal, Handheld) | **Upload within 72 h or payments expire.** Upload within 24 h is recommended. Per-transaction limit is configurable (Ireland default €100). **Merchant liable** for declined, expired or disputed payments. No decline alerts while offline. Square won't contact customers ([Square Help](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode), [Square IE](https://squareup.com/ie/en/townsquare/offline-card-processing), [Square Help](https://squareup.com/help/us/en/article/8551-view-offline-payments)) | Yes | — |
| Lightspeed R-Series | Only through the Smart Terminal in standalone or offline mode; insert or tap only | **$5,000 per transaction and $50,000 total**. The defaults are also the maximums ([Lightspeed](https://retail-support.lightspeedhq.com/hc/en-us/articles/27640682216731-Processing-payments-in-standalone-and-offline-mode), [Lightspeed Golf](https://golf-support.lightspeedhq.com/hc/en-us/articles/29102071881115-Processing-offline-payments-through-Lightspeed-Retail-R-Series)) | **No.** Not linked to a sale, and inventory is not updated. They appear only in payments reports | — |
| Lightspeed X-Series | Integrated payments are off in basic offline mode. A separate Lightspeed Payments offline mode exists ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25533690950427-Lightspeed-Payments-offline-mode-with-Retail-POS-X-Series)) | — | Sales are stored in the browser and auto-sync. A "Retry errored sales" button exists ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25534043015195-Syncing-Offline-Sales-in-Retail-POS-X-Series)) | Only the Sell screen works. No editing of existing customers, no selling or redeeming gift cards. Won't start without an initial sync. Lightspeed calls it "not intended as a complete solution" |
| D365 Commerce | Card is on the list of payment types to test offline ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/implementation-considerations-offline)) | Offline DB on **SQL Express has a 10 GB cap; keep it ≤ 8 GB**. Use index compression (10.0.29+) and data exclusion, e.g. drop customers ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/implementation-considerations-cdx), [index compression](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/index-compression)) | Yes (local database) | Internal gift cards need headquarters (offline completion is optional). Pay-by-link is unavailable ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/pay-by-link-overview)). **Duplicate transaction IDs** can occur across offline and online modes and "can require a significant amount of manual data fixing" ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/channel-setup-retail)) |
| Odoo POS | Depends on the terminal **[unverified]** | — | Yes. Orders and lines carry UUIDs with a unique constraint ([odoo pos_order_line.py](https://github.com/odoo/odoo/blob/master/addons/point_of_sale/models/pos_order_line.py)) | "Many POS features rely on backend information and, therefore, are not available offline" ([Odoo docs](https://github.com/odoo/documentation/blob/master/content/applications/sales/point_of_sale/use.rst)) |

**Offline patterns worth copying from D365** ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/implementation-considerations-cdx)):
- **"Allow manual switch to offline before sign in".**
- **"Enable advanced offline switching"**, which trades online-only features for speed.
- An operation-independent **health-check interval** so the POS switches back online quickly.

KORONA and Xstore are reputed to run offline with the full sale flow **[unverified]**.

### 2.13 Hardware, pricing, contracts, lock-in, support, large catalogs, portability

- **Hardware obsolescence.**
  - Shopify's Chip & Swipe reader became unsupported on Sept 15, 2025 ([Shopify Help](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/selling-offline/offline-payments)).
  - Clover merchants reported a **$99.95/month per-device charge on legacy hardware** in 2025 ([KORONA, a competitor](https://koronapos.com/blog/clover-pos-pricing/), [checkthat.ai](https://checkthat.ai/brands/clover/pricing)).
  - D365 deprecated Modern POS and the hybrid apps in **Oct 2023** and told customers to migrate to Store Commerce ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/store-commerce)).
- **Contracts.** A Clover Station that costs about **$1,800 outright can cost $4,000–6,000 over a 48-month lease**. Leases usually can't be cancelled even if the business closes. Cancelling takes multiple departments and 2–4 weeks, and some BBB complaints describe 2–3 months of continued billing ([Brookside](https://brooksidepayments.com/clover-problems/), [Sleft](https://www.sleftpayments.com/learning-hub/clover-pos-review-honest-pricing-2026)).
- **Processor lock-in.**
  - Lightspeed charges an unpublished monthly fee if you use another processor ([StartupOwl](https://startupowl.com/reviews/lightspeed)).
  - Square and Shopify hardware work only with their own processing **[unverified]**.
  - KORONA markets itself as processor-agnostic **[unverified]**.
- **Large catalogs (100k+ SKUs).**
  - Square explicitly caps variations for performance.
  - D365's offline store is bounded by SQL Express's 10 GB.
  - Odoo needs limited product loading.
  - Shopify had to "re-architect a core part of the platform" to reach 2,048 variants ([Shopify blog](https://www.shopify.com/blog/2048-variants)).
  - No vendor publishes register-side performance budgets (lookup latency, sync time) for 100k+ SKUs.
- **Support.** Clover reviews report long hold times and unresolved issues ([Brookside](https://brooksidepayments.com/clover-problems/)). Reviewers say Square support is weekday-focused and plan-dependent **[unverified detail]**.
- **Data portability.** Not verified for any vendor. Background: exports rarely include gift-card liabilities, loyalty balances, open layaways or promotion definitions **[unverified]**.

---

## 3. Flaws and complaints catalog

### 3.1 By vendor

**Shopify POS**
- **Cloud login is a single point of failure.** On **Dec 1, 2025 (Cyber Monday)**, a bug in the login authentication flow blocked admin and **POS logins**.
  - Started about 9:08 AM ET, peaked around 11:00 AM with 4,000+ reports, largely fixed by 2:40 PM ET.
  - Online checkout stayed up, but stores couldn't sign in to POS during peak trading ([CNBC](https://www.cnbc.com/2025/12/01/shopify-outage-cyber-monday-shopping.html), [TechCrunch](https://techcrunch.com/2025/12/01/shopify-resolves-outage-disrupting-merchants-on-cyber-monday), [Tech Times](https://www.techtimes.com/articles/313085/20251202/shopify-outage-disrupts-cyber-monday-merchants-face-checkout-pos-failures.htm), [ALM Corp](https://almcorp.com/blog/shopify-outage-december-1-2025-cyber-monday-analysis/)).
- **Features removed.** Stocky was retired, and forecasting and ABC analysis were not replaced natively.
- **Modelling limits.** 3 option axes and 250 media per product.
- **Offline card gaps.** No swipe, Tap to Pay or MOTO offline, and no Interac or eftpos offline.
- **Per-location cost.** POS Pro at $89 per location scales linearly with store count.

**Square for Retail**
- **Outages.**
  - **Sept 7–8, 2023**: from 1:54 PM ET Sept 7 to 5:19 AM ET Sept 8, sellers couldn't take payments or reach their accounts. Cause: DNS failures after an internal network software update, where a large ruleset destabilized nodes.
  - Afterwards Square expanded offline payments and changed its firewall, DNS and communications ([BleepingComputer](https://www.bleepingcomputer.com/news/technology/square-last-weeks-outage-was-caused-by-dns-issue-not-a-cyberattack/), [TechCrunch](https://techcrunch.com/2023/09/11/square-daylong-outage-dns-error/), [Payments Dive](https://www.paymentsdive.com/news/square-outage-pos-payments-processing-block-smb-merchants/693674/), [Square](https://developer.squareup.com/blog/incident-summary-2023-09-07/)).
  - 2025 incidents: Feb 26 (about 2–3 h, card transactions), Mar 14 (more than 6 h of intermittent warnings), Aug 10 (payments disruption), and **Oct 20** (degradation across 8 countries and 26 components for about 2 h 7 m) ([IsDown](https://isdown.app/status/square/incidents/375949-payments-disruption), [IsDown](https://isdown.app/status/square/incidents/428984-payments-disruption), [StatusGator](https://statusgator.com/services/square/outage-history?page=3), [KORONA](https://koronapos.com/blog/square-pos-outage/)).
- **Offline risk sits with the merchant.** Payments expire after 72 h, the merchant is liable for declines, and there are no alerts while offline.
- **Catalog limits.** 250 variations (120 online). No serialized units.
- **Plan gating.** The October 2025 consolidation moves features between tiers.

**Lightspeed (R- and X-Series)**
- **Mandatory payments.** Lightspeed Payments is required, with an unpublished penalty for using another processor.
- **Offline card payments in R-Series are a side ledger.** They aren't tied to a sale and don't update inventory. That means reconciliation work and phantom stock.
- **X-Series offline is minimal.** Only the Sell screen works, with no gift cards and no customer edits.
- **Two retail products with different backends and APIs.** Integration partners must build twice, and merchants face future migration risk ([SKUPlugs](https://skuplugs.com/lightspeed-r-series-vs-x-series/)).
- **Outage on Oct 20, 2025:** "Lightspeed Software – Login & other systems" ([IsDown](https://isdown.app/integrations/lightspeed/lightspeed-retail-retail-pos), [StatusGator](https://statusgator.com/services/lightspeed/retail-pos)).

**Clover**
- **Distribution and contracts.**
  - Pricing is quote-based and set by the reseller.
  - Leases can't be cancelled and cost 2–3× the purchase price.
  - ETFs of $295–$595.
  - Rates rise through tiered-pricing reclassification.
  - App-market fees stack on top.
  - Cancelling takes several departments ([Brookside](https://brooksidepayments.com/clover-problems/), [PaymentPop](https://paymentpop.com/merchant-accounts/clover-pos-customer-reviews/)).
- **Legacy hardware fee.** A $99.95/mo per-device charge was reported in 2025.
- **Caveat.** Several sources are competitors or payment consultants.

**Microsoft Dynamics 365 Commerce**
- **Offline database size.** SQL Express caps the offline DB at 10 GB. Microsoft recommends keeping it ≤ 8 GB and excluding data such as customers. A self-hosted CSU on Express that hits 10 GB "can cause problems such as loss of data" ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/implementation-considerations-offline)).
- **Duplicate transaction IDs** across offline and online modes, which need manual cleanup ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/channel-setup-retail)).
- **Connectivity-dependent features.**
  - Gift cards depend on headquarters.
  - Pay-by-link doesn't work offline.
  - In 10.0.44, pay-by-link couldn't take partial payments or pay the balance after a gift card (fixed in 10.0.45) ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/pay-by-link-overview)).
- **Config propagation.** Parameter changes can take up to 24 h to take effect.
- **Forced migration.** MPOS was deprecated in Oct 2023.
- **Implementation weight.** CSU and CDX distribution schedules, channel database groups and SQL editions all need configuring ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/implementation-considerations-cdx)).

**Odoo POS**
- **Offline limits.** Offline mode keeps order entry working, but backend-dependent features stop ([Odoo docs](https://github.com/odoo/documentation/blob/master/content/applications/sales/point_of_sale/use.rst)).
- **Large catalogs.** They need partial loading.
- **Background.** Major-version upgrades and integrator dependence **[unverified]**.

**Others (background, [unverified]).**
- *NCR Counterpoint*: on-prem Windows/SQL burden, and support quality depends on the VAR.
- *Epos Now*: recurring billing and contract complaints.
- *Grocery tier-1 (Toshiba, DN, NCR)*: long, SI-heavy upgrades and costly hardware refreshes.
- *Fuel (Verifone, Gilbarco)*: forecourt certification slows change, and EMV-at-pump upgrades were costly.
- *Heartland*: parent-company churn.

### 3.2 Cross-cutting themes

1. **Selling depends on a cloud control plane.**
   - Logins (Shopify, Dec 1, 2025), DNS (Square, 2023) and regional cloud events (Square and Lightspeed both degraded on **Oct 20, 2025**) stopped or hampered stores.
   - Oct 20, 2025 was the day of the widely reported AWS us-east-1 outage **[unverified linkage]**.
2. **"Offline mode" means partial function plus shifting risk to the merchant.** Every verified vendor restricts something offline:
   - card entry methods (Shopify);
   - liability and a 72 h expiry (Square);
   - no sales or inventory posting (Lightspeed R);
   - no gift cards or customer edits (Lightspeed X, D365);
   - database size (D365).
3. **Stored value is the weakest link.** Gift cards and eWallets need a central balance. D365 locks cards to prevent double-spend. Lightspeed X can't redeem gift cards offline. D365 used to load external card balances before payment.
4. **Lock-in comes through payments, hardware and contracts.** Examples: Lightspeed's penalty fee, Clover's leases and ETFs, and a legacy-device fee. Proprietary readers get retired (Shopify Chip & Swipe).
5. **Catalog modeling ceilings.** Shopify allows 3 axes; Square allows 250 or 120 variants. There's no native lot or expiry control in SMB.
6. **Churn and forced migrations.** Examples: the Stocky shutdown, MPOS deprecation, Lightspeed's two product lines, and Square collapsing 18 subscriptions into 3 plans.
7. **Complexity with no explanation.** Configuring promotion concurrency needs a specialist (D365). No vendor explains a price at the counter.
8. **Regulatory churn outpaces vendors** **[unverified]**. Examples: SNAP state waivers from 2026, penny rounding from late 2025, card surcharging rules, and fiscal certification (France NF 525 is verified for D365: the certified version maps to 10.0.48 / Store Commerce 9.58.x, covering Windows, Cloud, Android and iOS) ([MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/localizations/france/emea-fra-cash-registers)).

### 3.3 Notable outages and incidents

| Date | Vendor or event | Impact | Cause | Source |
|---|---|---|---|---|
| 2023-09-07/08 | Square | More than 14 h; sellers couldn't take payments or log in | DNS failure after an internal network update | [BleepingComputer](https://www.bleepingcomputer.com/news/technology/square-last-weeks-outage-was-caused-by-dns-issue-not-a-cyberattack/), [TechCrunch](https://techcrunch.com/2023/09/11/square-daylong-outage-dns-error/) |
| 2023-10 | D365 Commerce | Modern POS and hybrid apps deprecated; forced migration to Store Commerce | Product lifecycle | [MS Learn](https://learn.microsoft.com/en-us/dynamics365/commerce/dev-itpro/store-commerce) |
| 2024-07-19 | CrowdStrike Falcon update | About 8.5M Windows devices crashed, including many Windows POS fleets | Faulty content update | **[unverified]** |
| 2025-02-26 | Square | About 2–3 h card-processing outage | Not published | [StatusGator](https://statusgator.com/services/square/outage-history?page=3) |
| 2025-03-14 | Square | More than 6 h of intermittent disruption | Not published | [IsDown](https://isdown.app/status/square/incidents/375949-payments-disruption) |
| 2025-04/05 | M&S and Co-op (UK) | Contactless payments and click-and-collect disrupted; M&S online orders paused for weeks | Social-engineering ransomware | **[unverified]** |
| 2025-07-07 → 2026-08-31 | Shopify Stocky | Transfers and forecasting removed, then the app shut down | Consolidation into admin | [Finaloop](https://www.finaloop.com/blog/stocky-discontinued-in-2026-what-shopify-merchants-should-do) |
| 2025-08-10 | Square | Payments disruption | Not published | [IsDown](https://isdown.app/status/square/incidents/428984-payments-disruption) |
| 2025-09-15 | Shopify | Chip & Swipe reader support ended | Hardware end of life | [Shopify Help](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/selling-offline/offline-payments) |
| 2025-10-20 | Square; Lightspeed | Square: 8 countries, 26 components, about 2 h. Lightspeed: "Login & other systems" | Same day as the AWS us-east-1 outage **[unverified linkage]** | [StatusGator](https://statusgator.com/services/square/outage-history?page=3), [IsDown](https://isdown.app/integrations/lightspeed/lightspeed-retail-retail-pos) |
| 2025-12-01 | Shopify | POS and admin logins failed for about 5.5 h on Cyber Monday | Bug in the login authentication flow | [CNBC](https://www.cnbc.com/2025/12/01/shopify-outage-cyber-monday-shopping.html) |

---

## 4. Unsolved problems (gaps no vendor handles well)

1. **Offline card acceptance with managed risk.** Vendors either refuse, cap it blindly ($5k/$50k at Lightspeed), or pass the risk to the merchant (Square's 72 h expiry, no decline alerts). No vendor was found that scores offline risk (card type, amount, customer history), shows live exposure, and posts offline card sales as full sales. Lightspeed R doesn't even link them to a sale.
2. **Selling through a cloud outage, including identity.** A login or IdP failure (Shopify, Dec 1, 2025) should never stop a store. Offline staff authentication, price lookup, promotions, returns and stored value need to run from local state.
3. **Stored value offline.** Balances live centrally and cards are locked per transaction (D365). No vendor does *bounded* offline redemption, such as a per-card offline allowance reconciled later.
4. **Large catalogs at the edge.** Pushing 100k–1M SKUs, price books, customers and promotions to every register runs into SQL Express's 10 GB limit (D365), browser memory (Odoo's limited loading) and variant caps (Square). No vendor publishes register latency or sync-time budgets.
5. **Deterministic, explainable promotions.** A powerful engine (D365) means configuration experts and opaque results. Simple engines can't express retail deals. Nobody offers cashier-facing explanations, merchandiser simulation, pre-publish conflict detection, or identical results across cloud, register, self-checkout and web.
6. **Global identity of offline documents.** D365 warns that duplicate transaction IDs across offline and online modes need manual fixes. Odoo solves it with UUIDs. This is still a common failure.
7. **Complex item models in SMB tools.** Apparel with more than 3 axes, paint and fabric colors in the hundreds, lot and expiry with FEFO and recalls, serial capture at receipt versus at sale, and scale-priced items all need expensive enterprise suites or add-ons.
8. **Native forecasting and replenishment for SMB.** Shopify just dropped it (Stocky). Merchants are pushed to third-party apps.
9. **Lock-in and portability.** Processor penalties, leases and legacy-hardware fees are common. No vendor was verified to offer a complete export that includes stored-value liabilities, loyalty balances, open orders and promotions.
10. **Jurisdictional rules as data.** State-by-state SNAP restrictions, cash rounding, surcharge limits, age and hour restrictions, and country fiscal rules (NF 525 and others) change often. Most POS item models have single flags, not effective-dated rules **[unverified for specific vendors]**.
11. **Migrations that don't hurt.** Vendors retire apps and products (Stocky, MPOS, Chip & Swipe) or run two product lines (Lightspeed). Merchants carry the migration cost.
12. **Cross-channel returns with correct net price.** Refunding the right amount on a partial return from a mixed-promotion, mixed-fulfillment order requires stored line-level allocations. This wasn't verified for any SMB vendor.

---

## 5. Implications for our design: testable product requirements

**Resilience and offline**
1. **Offline-first register.** With the WAN down for **≥ 72 h**, a register must support:
   - sales, returns with and without receipt, and exchanges;
   - layaway payments and quotes;
   - customer create and edit;
   - price lookup and promotions;
   - loyalty accrual;
   - gift-card redemption within offline limits;
   - all cash operations.

   *Test:* a store with 10 registers, 150k SKUs and 50k customers runs 72 h with the WAN cut, then reconnects. Expect **0 lost and 0 duplicated documents**, and inventory reconciled within 5 min.
2. **Store-local sync (LAN peer or in-store hub).** Registers must share sales, inventory, customers and gift-card state while the WAN is down. *Test:* sell on register A offline, then return that receipt on register B offline.
3. **Local staff authentication.** Hashed PINs or badges and role grants are cached on the device, so an IdP outage never blocks sign-in (the Shopify Dec 1, 2025 lesson). *Test:* block auth endpoints; sign-in still completes in < 2 s.
4. **Client-generated, globally unique, time-ordered IDs (e.g. UUIDv7) for every document, with idempotent ingestion.** Human-readable receipt numbers use a per-register sequence plus a prefix. *Test:* replaying an offline batch 3× creates 0 duplicates. (This fixes D365's duplicate-ID warning.)
5. **Offline card acceptance with risk management:**
   - configurable limits per transaction, per card, per device and per store;
   - rules by card type or BIN where the acquirer allows;
   - live exposure display;
   - upload retries with alerts at 1 h, 12 h and 48 h, well before the processor's expiry (Square: 72 h).

   Offline card sales **must post as complete sales** (inventory, customer, tax, receipt), never a payments-only side ledger.
6. **Published degraded-mode matrix.** Every feature is documented as available offline or not. The UI shows the reason for anything unavailable, with no silent failures.
7. **Config propagation SLO.** Price, promotion and permission changes reach online registers in ≤ 60 s. Scheduled changes are effective-dated so offline registers switch at local time. (D365 parameters can take up to 24 h.)
8. **Safe rollouts:**
   - ring deployment (canary stores → 10% → all);
   - merchant-set change freezes (e.g. Black Friday through Cyber Monday);
   - rollback in ≤ 10 min;
   - registers keep running if the update service fails.

**Catalog**

9. **Variant model:** ≥ 5 option axes and ≥ 10,000 variants per parent. Grid entry for POs, receiving and counts. Multiple barcodes per variant. (Shopify: 3 axes and 2,048 variants; Square: 250.)
10. **Performance budgets on a mid-range register holding 500k SKUs locally:**
    - barcode lookup ≤ 50 ms p99;
    - text search ≤ 200 ms p95;
    - initial sync ≤ 15 min;
    - delta sync ≤ 60 s.
11. **Native tracking types:**
    - serials captured at receipt *or* at sale, as a per-item policy (the D365 "Active" versus "Active in sales process" pattern);
    - lots with expiry: FEFO prompt, block expired sales, recall lookup from lot to customer;
    - UoM conversions (case, inner, each);
    - weighed items with scale integration, tare and GS1 variable-measure barcodes;
    - kits and bundles that deplete components;
    - service, non-inventory and open-price items, gated by permission.

**Pricing and promotions**

12. **One pricing and promotion engine library** runs identically in cloud, register, self-checkout and web. CI runs a golden suite of **≥ 1,000 baskets** on every platform build, and outputs must match exactly.
13. **Promotion vocabulary:** simple; quantity or tiered; mix-and-match (least-expensive, deal price); threshold; BOGO; bundle price; time-of-day or day-of-week; segment and B2B price lists; serialized single-use coupons; loyalty rewards. Stacking uses exclusive, best-price or compound modes with priorities (the D365 model), plus explicit allow-lists per promotion.
14. **Engine performance:** a 100-line basket against 10,000 active promotions prices in **≤ 100 ms p99** on the register, with deterministic tie-breaking.
15. **Explainability:**
    - each line records its price source and the promotions applied;
    - a "why not applied" diagnostic;
    - a merchandiser simulation sandbox;
    - margin-floor and overlap conflict checks before publishing;
    - line-level discount allocation stored for returns.

**Inventory and omnichannel**

16. **Event-sourced inventory ledger with reason codes.** It tracks on-hand, committed, available and in-transit quantities, plus:
    - a negative-stock policy per item or location (allow, warn, block);
    - blind cycle counts that account for movements during the count, so no store freeze is needed;
    - PO receiving with tolerances;
    - landed and layered costs.
17. **Native replenishment**, not app-dependent: min/max, reorder points, forecasts aware of lead time and seasonality, suggested POs, and ABC analysis. (Stocky's forecasting disappeared.)
18. **RFID-ready count ingestion API.** EPC/SGTIN decoding; ≥ 100k reads per minute per store.
19. **Mixed-fulfillment cart:** carry-out, pickup elsewhere and ship in one transaction, with deposits and a card on file for the balance (the D365 model). Reservations have a TTL. Ship-from-store routing and endless aisle.
20. **Cross-channel returns at any location** use the original line allocations and a policy engine (by days, channel, category, condition, tender and customer tier). Receipt-less returns default to store credit at the lowest price in N days, with ID capture, per-customer velocity limits and a fraud-score hook.

**Customers and stored value**

21. **Stored value** (gift cards, store credit, eWallet) must meet all of these:
    - **locked against concurrent use**;
    - **activated only after payment is captured** (the D365 10.0.45 lesson);
    - bounded offline redemption;
    - liability and breakage reports;
    - expiry and escheat rules per jurisdiction.
22. **Layaway, special orders, quotes, work orders and house-account sales all work offline.** Lightspeed X already supports layaway, quotes and on-account offline; we should match it and add the rest.

**Cash and staff**

23. **Cash office:**
    - float, paid-in and paid-out with reason codes;
    - safe and bank drops;
    - denomination counts;
    - blind close and shift suspend/resume;
    - multiple drawers per register;
    - over/short thresholds by role that force a manager (the Odoo pattern);
    - over/short trend reports per employee.
24. **Cash rounding by jurisdiction and tender** (cash-only nickel rounding), with benefit-program exceptions.
25. **Permission catalog per operation**, with PIN or badge manager override that works offline and a complete audit trail. Commissions are native: per item, category or employee, split sales, clawback on return. Time clock with break rules.

**Regulated selling**

26. **EBT, SNAP and WIC:**
    - eligibility rules keyed by **jurisdiction plus effective date**;
    - SNAP-eligible subtotal, split tender, and tax exemption on the SNAP-paid portion;
    - WIC APL ingestion with category and subcategory matching, and partial approvals.

    *Test:* the same basket prices differently for SNAP in two states.
27. **Age and hours restrictions:** minimum age per category and jurisdiction; sale-hour windows; AAMVA ID scan and mDL; the verification method recorded, but not unneeded personal data.

**Platform and commercial**

28. **Payments abstraction:** processor-agnostic with our own optional processing. No penalty for third-party processors, no leases, month-to-month terms, and hardware reusable across processors.
29. **Hardware policy:** commodity devices (iOS, Android, Windows, Linux) and standard peripherals. Support at least 5 years, with at least 12 months' notice of end of life (contrast Shopify Chip & Swipe and Clover's legacy fee).
30. **Complete self-serve export.** Every entity, including stored-value liabilities, loyalty balances, open layaways and orders, price books and promotions, in documented formats within 24 h. Import tools for competitor exports.
31. **Event stream and warehouse sync** at full fidelity (lines, allocations, taxes, tenders). APIs match the UI. Multi-store and franchise roll-ups with royalty calculation.
32. **B2B:** company accounts with multiple buyers, net terms, credit limits, tax-exemption certificates with expiry, POs at POS, invoices, statements and aging.
33. **Pluggable fiscalization and tax per country** (e.g. France NF 525). Release trains are aware of certifications.
34. **Reliability:** multi-region cloud, and no single-region dependency for authentication, catalog or payment routing. A status page by component. **99.95% cloud SLO**, and the store keeps trading regardless.
35. **Upgrades:** versioned APIs and extension points with a **≥ 24-month deprecation window**. Never retire a feature without native parity or a free migration (lessons from Stocky and MPOS).

---

## Appendix A: Verification backlog (highest value first)

1. **Shopify POS:**
   - the offline transaction limits;
   - whether inventory lookup, customer creation and discount codes fail offline;
   - native serial numbers and layaway;
   - card-rate schedule by country.
2. **Square:** US maximum offline limits; EBT support status; Premium plan rates; which retail features sit in each new plan.
3. **Clover:**
   - that hardware is tied to Fiserv;
   - the legacy-device fee, from a primary source;
   - EBT support;
   - the Fiserv Q3-2025 guidance reset and its effect on Clover pricing.
4. **Grocery and convenience vendors** (ECRS, LOC, NCR Emerald, Toshiba TCx, DN Vynamic, Verifone Commander, Gilbarco Passport, Petrosoft): current product names, offline architecture, SNAP-waiver readiness, TruAge support and ownership.
5. **Enterprise** (Xstore, Retail Pro, Aptos, Manhattan, SAP CCO): offline architecture, SAP CCO maintenance status, pricing model.
6. **Regulation:**
   - SNAP waiver states and effective dates;
   - the US penny end date and state rounding laws;
   - card surcharge caps and state bans;
   - Visa/Mastercard settlement terms (2025);
   - eWIC APL formats.
7. **Incidents:** CrowdStrike retail impact (2024-07-19), M&S and Co-op (2025), and the AWS 2025-10-20 root cause and POS impact.
8. **Portability:** export completeness for each SMB vendor.

## Appendix B: Primary documentation read through GitHub code search

- MicrosoftDocs/Dynamics-365-Unified-Operations-Public, `articles/commerce/`:
  - retail-discounts-overview.md, dev-itpro/Apply-multiple-retail-discounts.md, price-settings.md;
  - cash-mgmt.md, shift-drawer-management.md, pos-operations.md, tasks/create-pos-permission-groups.md;
  - seamless-offline-improvements.md, dev-itpro/implementation-considerations-offline.md, dev-itpro/implementation-considerations-cdx.md, dev-itpro/index-compression.md, dev-itpro/store-commerce.md, dev-itpro/pay-by-link-overview.md;
  - channel-setup-retail.md, customer-orders-overview.md, pos-serialized-items.md, get-started/whats-new-commerce-10-0-45.md;
  - localizations/france/emea-fra-cash-registers.md, localizations/dev-itpro/fiscal-integration-for-retail-channel.md.

  The learn.microsoft.com links above are the public renderings of these files.
- odoo/odoo:
  - addons/point_of_sale/models/pos_config.py, res_config_settings.py, product_template.py, pos_order.py, pos_order_line.py;
  - addons/loyalty/models/loyalty_program.py;
  - addons/point_of_sale/tests/test_pos_basic_config.py.
- odoo/documentation: content/applications/sales/point_of_sale/use.rst.
