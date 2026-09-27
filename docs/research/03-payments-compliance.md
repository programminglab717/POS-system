# Track 03: Payments, Fees, Security and Regulatory Compliance

*Research input for the POS platform architecture and feature list. Compiled 2026-09-27.*

---

## 0. How to read this report

**Evidence tags.** Every non-trivial claim carries one tag.

- **[V]** Verified in this session through web-search results. The link is the source the search surfaced.
- **[C]** Corroborated through documents that quote a primary source (statute, official notice, technical specification). Where that document quoted a primary URL, I link the primary URL as well as the corroborating document.
- **[K]** Background knowledge (training data to mid-2026) that I could not re-verify in this session. Treat these as leads and verify them before building.

**Tooling limits.** These limits affect how much you can rely on the findings.
- WebFetch was blocked by the network egress proxy for every domain tried, including docs.stripe.com, docs.adyen.com, pcisecuritystandards.org, sec.gov, federalreserve.gov, sede.agenciatributaria.gob.es, help.shopify.com and wikipedia.org.
- The session-wide web-search budget ran out after about 40 of my queries.
- I verified volatile regulatory dates with about 50 targeted GitHub code searches. These surface passages from legal and tax documents that quote official texts: BOE, RIS, EUR-Lex, Porezna uprava, UAE MoF, Dz.U., and Bundestag Drucksachen.
- Counsel or tax advisers should re-read the primary sources before go-live in any market.

**Status date.** Unless stated otherwise, "status" means status as of **2026-09-27**.

---

## Executive summary: what matters most

Details and sources are in the referenced sections.

1. **Processor lock-in is the SMB POS business model (§1).** Square and Toast require their own processing. Shopify charges 0.6–2.0% extra and Lightspeed a reported ~$400/month when merchants use another processor. Clover forces app billing and processing through Fiserv. **Our wedge:** processor-agnostic by design, with our own payments optional.
2. **US acceptance economics are in legal flux (§1.4, §3).** The Visa/Mastercard settlement was preliminarily approved on 9 June 2026; the fairness hearing is 16 November 2026. It would allow card-category acceptance, a 1.25% standard-credit cap for 8 years and ≤3% credit surcharging. Regulation II is vacated but stayed pending appeal, and the Credit Card Competition Act has not passed. Build these as switchable policy, not constants.
3. **Offline risk sits with the merchant everywhere (§2.5).** Limits differ: Square 72 h and ≤$50k per transaction, Stripe $10k total, Clover 7 days, Adyen only with a local integration. Offline mode should be explicit, bounded and adaptive.
4. **SoftPOS is mainstream (§2.2).** Tap to Pay on iPhone (iOS 18.4+) and on Android are MPoC-validated. Treat phones as first-class terminals.
5. **PCI DSS 4.0.1 has been fully mandatory since 31 March 2025 (§4).** The key architectural decision is semi-integration plus P2PE/MPoC, so the POS core never sees a PAN.
6. **Fiscal and e-invoicing cliffs cluster in 2026–2027 (§5).**
   - Already live in 2026: Croatia (January), Belgium Peppol (January), Poland KSeF (February/April), Greece (February/October), Italy's link between card terminals and the RT fiscal register, France e-invoice reception (September).
   - Coming in 2027: UAE (January/July), Spain VeriFactu (January/July), Czech EET 2.0 (January, planned), Portugal qualified e-signature on PDF invoices (January), Germany B2B issuance (2027/2028), France SME issuance and e-reporting (September).
7. **Surprise: rules reverse.** France abolished the NF525 self-attestation in February 2025 and **restored it from 21 February 2026**. Spain postponed VeriFactu twice, and the UAE extended its provider-appointment deadline. The fiscal layer must be configuration-driven, with a release train per country.
8. **Tax rules change mid-year (§6).** Examples: India GST 2.0 (22 September 2025); German restaurant food at 7% from 1 January 2026 (drinks stay at 19%); Brazil CBS/IBS test rates (2026). Japan rounds once per tax rate per invoice, never per line. An effective-dated tax catalogue is mandatory.
9. **US penny (§6.5).** The last circulating pennies were minted in 2025, and a ceremonial final strike was reported on 12 November 2025. There is no confirmed federal rounding law, so cash rounding must be auditable and configurable by jurisdiction.
10. **Tips carry new federal reporting duties (§7.3).** The OBBBA qualified-tips deduction runs 2025–2028. From 2026, W-2s carry code TP and a tipped-occupation code. Voluntary tips must be kept strictly separate from mandatory service charges.
11. **EU accessibility is law (§7.4).** Since 28 June 2025 the European Accessibility Act has covered payment terminals and self-service kiosks.
12. **Account-to-account and QR payments are core in many markets (§2.7).** Pix and UPI dominate Brazil and India. The EU Instant Payments Regulation and Wero reach EU points of sale in 2026. Shopify added USDC (June 2025) and Square added bitcoin (November 2025). All tenders must be plug-ins.

---

## 1. Payments landscape and lock-in matrix

### 1.1 Lock-in matrix

| Vendor | Processing model | Outside processor allowed? | Price of going outside | Hardware portability | Other lock-in levers | Evidence |
|---|---|---|---|---|---|---|
| **Square** | Payment facilitator (Block) | No | n/a | Square hardware works only with Square | Payfac risk holds; offline decline risk on seller | [V] [StartSlice](https://startslice.com/blog/best-pos-systems-integrated-payments/), [TechnologyAdvice](https://technologyadvice.com/blog/sales/square-alternatives/); offline terms [V] [Square](https://squareup.com/us/en/legal/general/payment) |
| **Toast** | Payment facilitator (Toast Payments) | No | n/a | Hardware locked to Toast | Multi-year agreements | [V] [StartSlice](https://startslice.com/blog/best-pos-systems-integrated-payments/), [Beancount](https://beancount.io/blog/2026/07/29/square-toast-clover-pos-comparison-guide) |
| **Clover (Fiserv)** | Fiserv acquiring, sold via Fiserv and bank/ISO partners [K] | **Disputed.** One 2026 comparison says Clover and SpotOn "permit outside processors"; another says Clover hardware is processor-locked | Hardware usually replaced on switch | App Market rule: all app fees and "merchant-customer payment processing must be implemented within the Fiserv or Clover platforms" | [V] [StartSlice](https://startslice.com/blog/best-pos-systems-integrated-payments/); [C] [Clover rule quoted](https://github.com/rob2rhyme/duoCount/blob/ee22e58d4dc7127dab55286f47b370208688724a/docs/monetization-paths-2026.md) |
| **Shopify POS** | Shopify Payments | Online: yes, with a fee. In-person: Shopify Payments hardware [K] | **0.6–2.0%** extra per order depending on plan | Shopify hardware | In-person rates about 2.4–2.9% plus a fixed fee | [V] [Shopify Help](https://help.shopify.com/en/manual/your-account/manage-billing/billing-charges/types-of-charges/third-party-charges/third-party-transaction-fees), [KORONA](https://koronapos.com/blog/shopify-pos-pricing/) |
| **Lightspeed** | Lightspeed Payments | Yes, for a fee | Reported **$400/month** (volume-tiered, unpublished; Service Agreement §5.4) | Hardware processor-locked | Fee pressure to migrate | [V] [The Logic](https://thelogic.co/briefing/lightspeed-begins-charging-customers-for-using-other-payment-platforms/), [Katalyst](https://www.katalystos.com/blog/who-processes-payments-behind-your-pos) |
| **SpotOn** | SpotOn processing | Reported to permit outside processors (single source; verify) | Unknown | Unknown | Unknown | [V] [StartSlice](https://startslice.com/blog/best-pos-systems-integrated-payments/) |
| **Stripe Terminal / Adyen** | Acquirer platforms; the POS vendor builds on them | No (the platform is the acquirer) | n/a | Platform-certified readers or terminals | Stored-card vault portability depends on export policy [K] | [K] |
| **Processor-agnostic ISV stack** (for example Datacap NETePay) | Semi-integrated gateway to many processors | Yes | None | Terminals can be re-boarded to another processor [K] | None | [V] [Datacap](https://datacapsystems.com/netepay) |

### 1.2 Consequences for merchants: what we must fix

1. **Price opacity.** Flat-rate pricing hides interchange, so merchants cannot negotiate interchange-plus (IC+) pricing. The penalty fees in §1.1 make comparison shopping uneconomic.
2. **Switching costs.** Switching means replacing hardware, paying early-termination fees on multi-year contracts, and losing history.
3. **Card-vault lock-in [K].** Stored credentials for tabs, house accounts, subscriptions and card-on-file cannot move unless the incumbent performs a PCI-compliant export. Some payfacs restrict this. Network tokens are also scoped to a token requestor.
4. **Payfac risk actions [K].** Sudden holds, reserves or terminations can freeze working capital.
5. **Ecosystem capture.** App marketplaces mandate in-platform billing (Clover [C]).
6. **Offline risk shifted to merchants** (§2.5).

### 1.3 Pricing models

| Model | Description | Status | Our stance |
|---|---|---|---|
| Flat rate / blended | One % plus fixed fee (e.g., Shopify in-person about 2.4–2.9% plus a fee [V]) | Dominant in SMB | Offer for micro-merchants only, with a rate card that shows the implied margin |
| Interchange-plus (IC+/IC++) | Pass-through interchange and scheme fees plus a fixed markup | Standard for mid-market | **Default**, with line-level cost of acceptance shown |
| Subscription / membership | Monthly fee plus interchange at cost [K] | Niche | Support as a pricing plan |
| Tiered (qualified/mid/non-qual) | Opaque bucketing [K] | Legacy | Do not offer |
| Surcharge / dual pricing ("zero-fee") | Cost passed to the cardholder (§3) | Growing, but enforcement is rising | Support only in compliant form |

### 1.4 Macro drivers to design for (US)

- **Visa/Mastercard injunctive-relief settlement** (MDL 1720, Eastern District of New York, Judge Brian Cogan).
  - The superseding amended agreement was signed on 10 November 2025 ([V] [Visa 10-Q, FY2026 Q2](https://www.sec.gov/Archives/edgar/data/0001403161/000140316126000079/v-20260331.htm), [ABA](https://www.americanbar.org/groups/antitrust_law/resources/newsletters/in-re-payment-card-interchange-fee-merchant-discount-antitrust-litigation/)) and preliminarily approved on **9 June 2026** ([V] [US News](https://money.usnews.com/investing/news/articles/2026-06-09/us-judge-oks-visa-mastercard-38-billion-swipe-fee-settlement)).
  - On 10 September 2026, 978 merchants and trade groups objected, and Walmart objected earlier. The **fairness hearing is 16 November 2026** ([V] [Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/visa-mastercard-settlement-update-june-2026/), [Capitol Forum](https://thecapitolforum.com/the-forum/the-forum-newsletter-september-26-2026/put-visa-and-mastercard-on-trial/)).
  - Terms:
    - A 10 bp cut in the average effective credit rate for 5 years, and standard consumer credit capped at 1.25% for 8 years.
    - Posted credit rates frozen at 31 March 2025 levels while the average-effective-rate (AER) limit runs.
    - "Honor All Cards" relaxed so merchants may decline premium-consumer and/or commercial credit categories while honouring all cards within an accepted category.
    - Brand- or product-level credit surcharging, capped at the lesser of 3% or cost of acceptance, allowed within 90 days of approval with 30 days' notice to the acquirer.
    - Dual pricing is expressly not prevented ([V] [Optimized Payments](https://optimizedpayments.com/insights/industry-news/what-merchants-need-to-know-about-the-new-visa-mastercard-interchange-settlement/), [Datos Insights](https://datos-insights.com/blog/mastercard-visa-settlement-honor-all-cards-surcharge-rules/), [Visa 8-K exhibit](https://www.sec.gov/Archives/edgar/data/1403161/000140316125000093/vex991settlementagreementf.htm)).
- **Credit Card Competition Act.** S.3623 was introduced on 13 January 2026, with House companion H.R. 7035. Attempts to attach it to the GENIUS Act (May 2025) and a housing bill (March 2026) failed. As of late September 2026 there had been no Senate floor vote ([V] [Congress.gov](https://www.congress.gov/bill/119th-congress/senate-bill/3623/titles), [PYMNTS](https://www.pymnts.com/credit-cards/2026/credit-card-competition-act-gains-new-senate-support/)). If enacted, large issuers would have to enable a second, non-Visa/Mastercard network on credit cards [K]. Routing logic must be able to absorb this.
- **Regulation II (Durbin debit cap).**
  - The current cap is 21¢ + 5 bps + 1¢ fraud adjustment. The Federal Reserve's 2023 proposal (14.4¢ + 4.0 bps + 1.3¢) is not finalised ([V] [Federal Reserve](https://www.federalreserve.gov/paymentsystems/regii-interchange-fee-standards.htm), [Federal Register](https://www.federalregister.gov/documents/2023/11/14/2023-24034/debit-card-interchange-fees-and-routing)).
  - On 6 August 2025 the District of North Dakota vacated Regulation II's fee standard but **stayed the vacatur pending appeal** ([V] [Cooley](https://www.cooley.com/news/insight/2025/2025-08-15-district-court-vacates-regulation-iis-debit-card-interchange-fee-standard)).
  - The Eighth Circuit appeal was fully briefed by March/April 2026 and has been argued. A ruling is pending ([V] [ABA Banking Journal](https://bankingjournal.aba.com/2026/04/federal-reserve-files-reply-brief-in-reg-ii-appeal/), [America's Credit Unions](https://www.americascreditunions.org/news-media/news/fed-interchange-rule-challenge-heard-appeals-court)).
  - Separately, the 2022 amendment requiring two unaffiliated networks for card-not-present debit (effective 1 July 2023) remains in force [K].

---

## 2. Payment technology capabilities we must support

### 2.1 Card-present acceptance baseline

- **EMV contact and contactless [K].**
  - Required capabilities: EMVCo Level 1/2 kernels on the payment device, scheme contactless kernels, and CDCVM (consumer device verification) for wallets.
  - Contactless limits and CVM limits vary by market.
  - **US Common Debit AID** selection must be supported so merchants can route debit (§2.6).
- **Wallets [K].** Apple Pay, Google Pay and Samsung Pay (as tokenised EMV contactless), plus scheme QR and local wallets through APM plug-ins (§2.7).
- **Receipts [K].** US FACTA truncation (no more than the last 5 PAN digits and no expiry date on printed receipts) and EMV receipt data (AID, application label, TVR/TSI where required).

### 2.2 SoftPOS / Tap to Pay and PCI MPoC

- **PCI MPoC** is an objective-based standard for payments on commercial off-the-shelf devices (COTS). It covers contactless card reading and **PIN entry on the same COTS device**. It succeeds the separate PCI SPoC (PIN) and CPoC (contactless) standards [K] ([V] [Visa Acceptance](https://developer.visaacceptance.com/docs/vas/en-us/tap-to-phone/integration/all/rest/tap-to-phone/tap-to-phone-intro/ttp-comply-pci-mpoc-intro.html), [Patronus](https://patronusec.com/en/pci-mpoc-certification-spoc-cpoc/)).
- **Tap to Pay on iPhone** on iOS 18.4+ is listed as MPoC-validated. The **Tap to Pay on Android** solution also complies with MPoC ([V] [Visa Acceptance](https://developer.visaacceptance.com/docs/vas/en-us/tap-to-phone/integration/all/rest/tap-to-phone/tap-to-phone-intro/ttp-comply-pci-mpoc-intro.html), [PaymentNerds](https://paymentnerds.com/blog/pci-tap-to-pay-on-iphone-android-what-it-means-for-small-businesses/)).
- MPoC v1.1 certifications now exist, for example MyPinPad ([V] [MyPinPad](https://mypinpad.com/news-and-media/mypinpad-simplifies-the-path-to-compliant-mobile-payments-with-upgraded-pci-mpoc-v1-1-certification/)).
- Mastercard's Tap on Phone implementation guide (September 2025) states that **from January 2027, devices used in fleet deployments must hold at least an EMVCo Reduced Range 1 cm Letter of Approval** ([V] [Mastercard guide](https://www.mastercard.com/content/dam/mccom/shared/business/payments/commercial-payments/accept-payments/mobile-point-of-sale/tap-on-phone/pdf/Implementation-Guide-Sept-2025.pdf), per search summary; verify the exact wording).
- **Design implications.**
  - Integrate SoftPOS through acquirer SDKs (Apple ProximityReader via the acquirer, and Android MPoC SDKs), never through our own kernel.
  - Keep a device-eligibility list with OS version, NFC range certification and attestation status.
  - Provide an accessible PIN-entry flow where schemes require PIN.

### 2.3 Integration patterns and terminal APIs

| Pattern | What the POS touches | PCI impact | Examples |
|---|---|---|---|
| **Fully integrated** | The POS app reads the card (or drives a reader in the clear) and handles the PAN | POS in full scope; payment application needs PCI Secure Software validation [K] | Legacy systems |
| **Semi-integrated** | The POS sends amount and context; the terminal talks to the processor and returns result and token | POS largely **out of cardholder-data scope**; with a validated P2PE solution the merchant can use **SAQ P2PE** | Datacap NETePay/dsiEMVUS/DC Direct ([V] [Datacap](https://datacapsystems.com/blog/meet-netepay-hosted)); Adyen Terminal API; Stripe Terminal; Square Terminal API |
| **SoftPOS** | Sealed MPoC SDK on the phone | Out of scope when the MPoC solution is used as listed | Tap to Pay on iPhone/Android |

Terminal APIs we should abstract over:

- **Adyen Terminal API.**
  - Based on the **nexo Retailer Protocol** ([V] [nexo](https://www.nexo-standards.org/news/adyen-rejuvenates-store-payment-integration-api-pos-solution-powered-nexo-standards)).
  - **Local** mode sends requests to the terminal IP over the LAN and returns results synchronously. **Cloud** mode posts to Adyen's endpoint.
  - **Offline EMV and store-and-forward work only with local communications** ([V] [Adyen Terminal API](https://docs.adyen.com/point-of-sale/design-your-integration/terminal-api), [Adyen offline](https://docs.adyen.com/point-of-sale/offline-payment)).
- **Stripe Terminal.** SDKs (JS, iOS, Android, React Native) and server-driven integration. It supports incremental authorization, extended authorization, overcapture and on-receipt tipping for eligible US merchant category codes (MCCs) ([V] [Stripe incremental auth](https://docs.stripe.com/terminal/features/incremental-authorizations), [extended auth](https://docs.stripe.com/terminal/features/extended-authorizations), [overcapture](https://edge-docs.stripe.com/payments/overcapture), [on-receipt tipping](https://support.stripe.com/questions/terminal-support-for-on-receipt-tipping)).
- **Square Terminal API [K].** Cloud checkout requests pushed to a paired Square Terminal. Square processing only.
- **Datacap [V].** Hardware- and processor-agnostic US/Canada EMV through NETePay (on-premises or hosted), dsiEMVUS, and DC Direct semi-integrated devices, covering EMV, EBT, FSA and prepaid ([V] [Datacap NETePay](https://datacapsystems.com/netepay)).
- **PAX, Verifone, Ingenico [K].** Usually reached through the acquirer or gateway semi-integration protocol that runs on the device, not through a vendor-neutral API. Verifone store-and-forward uses floor limits and a total limit, excludes online-PIN transactions, and leaves approval risk with the merchant [K].

### 2.4 Card-present operations

- **Tips.**
  - Support tip-on-device before authorization, and tip-on-receipt adjustment before capture.
  - Visa lets MCC 5812 (restaurants) and 5813 (bars) add **up to 20%** to the authorized amount without authorization-related chargeback liability. Any tip above that can be charged back for "no authorization" ([V] [Visa restaurant chip guide](https://usa.visa.com/content/dam/VCOM/global/support-legal/documents/chip-payment-acceptance-restaurant-merchants.pdf)).
  - Mastercard reinstated a 20% tolerance for card-not-present restaurants ([V] [Toast support](https://support.toasttab.com/en/article/MasterCard-20-Tip-Tolerance-Reinstated-for-Card-not-present-Restaurants)).
  - **Rule for our engine:** if tip / auth > tolerance(MCC, brand, card-present flag), request an incremental authorization. If that is declined, flag the chargeback risk.
- **Pre-authorization, incremental and extended authorization** (bar tabs, hotels, rentals).
  - Stripe allows up to **10 increments per payment**. Each increment is capped at the higher of **$500 over, or 500% over,** the previously authorized amount. Card-present extended authorizations are available ([V] [Stripe](https://docs.stripe.com/payments/incremental-authorization), [extended auth](https://docs.stripe.com/terminal/features/extended-authorizations)).
  - Authorization validity windows vary by brand and MCC [K]. We need an auth-expiry tracker that re-authorizes or captures before expiry.
- **Partial authorization and split tender.**
  - Visa rules (April 2026 edition) require **all acquirers and all Visa prepaid issuers** to support partial authorization, and mandate it for certain MCCs for debit and prepaid.
  - Acquirers must **not** send the partial-auth indicator from devices that cannot split tender ([V] [Visa Core Rules, 18 Apr 2026](https://usa.visa.com/dam/VCOM/download/about-visa/visa-rules-public.pdf), [Visa Partial Authorization Service](https://usa.visa.com/content/dam/VCOM/global/support-legal/documents/visa-partial-authorization-service.pdf)).
  - The POS must support N-way split tender with the remaining balance carried forward.
- **Refunds without the card [K].**
  - Support referenced refunds to the original payment token, including cross-location and cross-channel refunds. Adyen and Stripe call these referenced and unreferenced refunds.
  - Support refunds to a different card, gift card or store credit, with manager authorization and fraud limits.
  - Support EMV card-present refunds where schemes or markets require the card.

### 2.5 Offline store-and-forward: how incumbents do it

| Vendor | Mechanism | Limits | Time to forward | Who bears the risk | Evidence |
|---|---|---|---|---|---|
| **Square** | Card data encrypted on device, auto-processed on reconnect | Merchant-set per-transaction max between **$1 and $50,000** | Processes on reconnect within 24 h; **expires after 72 h** | **Seller**: declines, expirations, disputes | [V] [Square help](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode), [Square Payment Terms](https://squareup.com/us/en/legal/general/payment) |
| **Toast** | Encrypts and stores on the POS device, then authorizes when back online. The "continue without waiting for authorization" option suits quick-service restaurants | Configurable offline transaction limit (tips excluded); manager approval above it | Authorizations may expire **as early as 24 h** | **Merchant**: "any decline that occurs at that point is final" | [V] [Toast platform guide](https://doc.toasttab.com/doc/platformguide/adminOfflineCCPayments.html), [Toast support](https://support.toasttab.com/en/article/Prepare-to-Operate-in-Offline-Mode-During-Service-Disruptions-or-Outages) |
| **Clover** | Device queues offline payments. The Remote Pay SDK has risk-tiered flags: allow with prompt, approve without prompt, force offline | Per-transaction limit and **total offline limit**; offline acceptance **up to 7 days** by default | Auto-submits on reconnect | **Merchant** accepts the risk of declined or partial payments | [V] [Clover dev docs](https://docs.clover.com/dev/docs/handling-offline-payments), [Clover help](https://www.clover.com/help/set-up-offline-payments) |
| **Stripe Terminal** | SDK stores and forwards | **$10,000 offline maximum** (or local equivalent) plus account-based limits; can fail all offline payments once a stored total is exceeded | Sync **within 24 h** recommended | Merchant: declines as normal | [V] [Stripe fleet offline](https://docs.stripe.com/terminal/fleet/offline-mode), [Stripe offline collect](https://edge-docs.stripe.com/terminal/features/operate-offline/collect-card-payments) |
| **Adyen** | Offline EMV (card-risk-managed) or store-and-forward (approve without verification) | Configured per account [K] | On reconnect | Merchant [K] | [V] [Adyen offline](https://docs.adyen.com/point-of-sale/offline-payment) |

**Gaps we can close.**
- Adaptive offline limits by BIN, issuer country and card type. For example, allow chip with offline data authentication (CDA) but refuse prepaid offline.
- Real-time "offline exposure" on the manager dashboard.
- Queue encryption with device attestation.
- Guaranteed forwarding SLAs, plus fiscal-layer interplay: a fiscal receipt can be issued offline while payment settlement is pending.
- Optional insured or guaranteed offline acceptance when using our own processing [K; business decision].

### 2.6 Routing, orchestration and debit routing

- **US debit routing [K].**
  - Durbin requires at least two unaffiliated networks per debit card, and the merchant or acquirer chooses the route.
  - Terminals should present the **US Common Debit AID** so unaffiliated networks can be used. Card-not-present dual routing has been required since 1 July 2023.
  - Least-cost routing is normally done by the acquirer or gateway. We should expose routing preferences and report the realised savings.
- **Multi-acquirer failover and least-cost routing [K].**
  - For card-not-present, and for card-on-file tabs and subscriptions, use **network tokens or a PCI Level 1 third-party vault** so credentials can be routed to more than one acquirer.
  - Card-present terminals are key-injected for one P2PE/acquirer path. Card-present failover therefore needs either dual-acquirer gateways (Datacap-style) or terminal fleets that can be re-keyed remotely.
- **Network tokens.** Mastercard targets 100% e-commerce tokenisation in Europe by 2030 and reports that about three in five Mastercard e-commerce transactions in Europe are tokenised. Secure Card on File is live in 45 European markets ([V] [Mastercard, 2026](https://www.mastercard.com/news/europe/en/newsroom/press-releases/en/2026/mastercard-advances-europe-s-checkout-transformation-on-the-road-to-2030/)). We should store only network tokens or processor tokens, never PANs.

### 2.7 Alternative payment methods (tender plug-ins)

| Method | Where | Current facts | POS pattern |
|---|---|---|---|
| **RTP / FedNow** | US | RTP limit raised to **$10M (February 2025)**; 343M transactions and $246B in 2024; reaches about 70% of US deposit accounts. FedNow had **1,600+ participating FIs** by January 2026, with a default $100k limit (raisable) ([C] [fintech reference quoting TCH and the Fed](https://github.com/MeridianLZ/copilot-plugins/blob/1e0e726dbf461e53283349fe04eeb4028cfe95cd/docs/governance/The%20Fintech%20Compliance%20Engineering%20Reference%20%28v2%20July%202026%29.md)) | Request-for-pay or QR through a bank or partner; mostly B2B, payouts and bill pay today [K] |
| **Pix** | Brazil | 2025: 178M active users (83.4% of the population); **79.8bn transactions worth R$35.36tn** ([V] [Estado de Minas](https://www.em.com.br/tecnologia/2026/01/7327097-pix-em-2026-pix-automatico-ja-e-realidade-e-novas-funcoes-sao-esperadas.html)). **Pix por aproximação (NFC)** went live on 28 February 2025 with an initial R$500 limit ([V] [gov.br](https://www.gov.br/secom/pt-br/acompanhe-a-secom/noticias/2025/02/pix-por-aproximacao-comeca-a-funcionar-nesta-sexta-feira-28)). **Pix Automático** became mandatory for payer institutions on 16 June 2025 ([V] [Mattos Filho](https://www.mattosfilho.com.br/unico/bcb-divulga-pix-automatico/)) | Dynamic QR (EMV-based BR Code [K]) and NFC; instant confirmation via webhook |
| **UPI** | India | About **23bn transactions/month** by mid-2026: PhonePe processed 10.7bn in May 2026, a 46.5% share ([C] [dossier citing NPCI data](https://github.com/PrakharBeniwal/WhatsApp-Pay-Case-Study/blob/b060e92ba25d2baf577c4d8744f89d49d7c1e404/WhatsApp_Pay_Dossier.md)). Zero merchant discount rate (MDR) policy on UPI/RuPay since the December 2020 announcement ([C] [PwC excerpt](https://github.com/ashaduzzaman-sarker/Fintech-Knowledge-Assistant/blob/2285de43473947dfbc3af74fac5e43ef79290607/data/processed/processed_chunks.json)) | Dynamic QR, collect requests, soundbox confirmation [K] |
| **SEPA Instant / IPR** | EU | Euro-area PSPs must **send** instant euro transfers and offer free **Verification of Payee** since **9 October 2025**. Non-euro PSPs follow in 2027. Instant and standard transfers must be priced equally ([V] [Finextra](https://www.finextra.com/the-long-read/1471/europes-instant-payments-regulation-and-the-9-october-deadline-explained), [Osborne Clarke](https://www.osborneclarke.com/insights/what-are-key-obligations-and-timelines-eu-instant-payments-regulation)) | Pay-by-bank QR; Wero |
| **Wero (EPI)** | DE, FR, BE, … | Person-to-person (P2P) since 2024; e-commerce live in Germany in 2025; **in-store/POS rollout during 2026** ([V] [The Paypers](https://thepaypers.com/payments/news/wero-ecommerce-is-now-live-in-germany), [EPI](https://epicompany.eu/media-insights/wero-announces-first-merchants-in-germany/)) | Acquirer-provided QR or NFC; plug-in |
| **Open banking PIS** | UK/EU | Payment initiation at POS via QR [K] | Plug-in |
| **M-Pesa** | Kenya and others | Lipa na M-Pesa till and paybill; STK push via Daraja API [K] | Phone-number push, then callback |
| **QR schemes** | SG PayNow/SGQR, TH PromptPay, MY DuitNow, ID QRIS, CN Alipay/WeChat Pay and Alipay+ [K] | EMVCo merchant-presented QR [K] | Static or dynamic QR plug-in |
| **BNPL at POS** | Global | Klarna, Affirm and Afterpay reach stores via virtual cards in wallets and through acquirer integrations [K] | Treat as a tender, or as a card via wallet |
| **Crypto / stablecoins** | US, global | Shopify announced USDC payments on Base with Coinbase and Stripe on **12 June 2025** ([C] [growthepie citing Shopify](https://github.com/growthepie/gtp-frontend/blob/edba14c46f18d8638cbabc4b341934444fbab495/lib/quick-bites/qb-shopify-usdc.ts); primary: [shopify.com/news/stablecoins-on-shopify](https://www.shopify.com/news/stablecoins-on-shopify)). **Square Bitcoin payments went live in November 2025** for about 4M sellers, on Lightning, with **no processing fees until 2027** and optional auto-conversion ([C] [crypto-weekly citing Decrypt](https://github.com/iloveburritos/crypto-weekly/blob/7f952f589d9515498ed176513e6913ad66157a2c/posts/11-16-2025.md)). The US GENIUS Act became law in July 2025 [K] | Partner-settled (merchant receives fiat); no crypto custody by us |
| **EBT (SNAP), eWIC** | US | FNS-authorized, PIN-based. States are restricting soda and candy through USDA waivers ("at least 10 states" moving; timelines vary) ([C] [article, June 2026](https://github.com/ariedugan-lab/articles/blob/7a86d205b22785fa945719504d544b82a47f4241/2026-06-20-1202-the-fresh-food-transition-how-snap-junk-food-bans-force-a-new-underwriting-framework-for-small-retail-lenders.md)). SNAP purchases are exempt from sales tax [K] | Item-level eligibility by state with effective dates; split tender |

### 2.8 After the sale: disputes, monitoring, payouts, capital

- **Visa Compelling Evidence 3.0 (CE3.0).** Automated qualification via Visa Secure or Visa Data Only went live on **17 October 2025**. CE3.0 expands to non-disputed fraud (fraud reports with no chargeback, "TC40s") from **18 April 2026** ([V] [Chargebacks911](https://chargebacks911.com/compelling-evidence-3-0-update-april-2026/)).
- **Mastercard First-Party Trust** is expanding after its US launch ([V] [Mastercard, June 2025](https://www.mastercard.com/us/en/news-and-trends/press/2025/june/first-party-trust-countering-friendly-fraud.html)).
- **Visa Acquirer Monitoring Program (VAMP).**
  - Launched on 1 April 2025. It combines fraud reports (TC40) and disputes (TC15) into one ratio.
  - The merchant "Excessive" threshold tightened to **1.5% on 1 April 2026**. Fines are reported at **$8 per fraud/dispute transaction**.
  - Card-testing (enumeration) threshold: 20%, applying above 300k enumerated transactions ([V] [Visa fact sheet](https://corporate.visa.com/content/dam/VCOM/corporate/visa-perspectives/security-and-trust/documents/visa-acquirer-monitoring-program-fact-sheet-2025.pdf), [MRC](https://merchantriskcouncil.org/learning/resource-center/member-news/blog/2026/stricter-vamp-ratio-thresholds-are-now-in-effect-heres-how-to-stay-compliant), [Chargeflow](https://www.chargeflow.io/blog/vamp-visa-acquirer-monitoring-program)).
  - **Implication:** capture evidence elements at sale time (device, IP, customer ID, order history, delivery proof, EMV cryptogram data) and compute VAMP-style ratios per merchant.
- **Payouts and capital [K].**
  - Instant payouts (push-to-card or RTP/FedNow) for a fee.
  - Embedded lending and merchant cash advances offered by Square, Toast, Shopify and Stripe.
  - US state commercial-financing disclosure laws (for example CA and NY) apply to these offers.
  - Implement through licensed partners, with a double-entry ledger and settlement reconciliation per payout.

---

## 3. Fees and surcharging rules

### 3.1 Card-network rules (US)

| Rule | Visa | Mastercard | Evidence |
|---|---|---|---|
| Surcharge cap | Lesser of cost of acceptance or **3%** | Lesser of cost of acceptance or **4%** | [V] [Michigan AG](https://www.michigan.gov/consumerprotection/protect-yourself/consumer-alerts/shopping/credit-debit-card-surcharges), [Mastercard](https://www.mastercard.com/us/en/business/support/merchant-surcharge-rules.html) |
| Debit/prepaid | Surcharging **prohibited**, even when debit is run as credit | Same | [V] [Lifelong POS](https://www.lifelongpos.com/resources/blog/visa-surcharge-rules-2026-compliance-guide) |
| Notice | **30 days'** written notice to the acquirer: business, level (brand or product), amount, and any payfac | Same | [V] [Lifelong POS](https://www.lifelongpos.com/resources/blog/visa-surcharge-rules-2026-compliance-guide) |
| Level | Brand-level **or** product-level, not both | Same | [V] same |
| Disclosure | At store entry and point of sale; surcharge shown as a receipt line [K] | Same | [K] |
| Enforcement | Mystery shopping and transaction-data monitoring; fines escalate for repeat violations; acquirers penalised for supporting non-compliant programs | — | [V] [CCG](https://ccgpays.com/visa-enforcement-on-non-compliant-pricing-programs/), [Stax](https://staxpayments.com/blog/visa-surcharge-rules/), [Digital Transactions](https://www.digitaltransactions.net/acquirers-seek-answers-from-a-visa-surcharging-executive/) |

The settlement would harmonise surcharging at the lesser of **3%** or cost of acceptance for credit, at brand or product level ([V] [ABA](https://www.americanbar.org/groups/antitrust_law/resources/newsletters/in-re-payment-card-interchange-fee-merchant-discount-antitrust-litigation/)).

### 3.2 US state rules

- **Surcharges banned:** Connecticut, Massachusetts, Maine and Puerto Rico ([V] [eBizCharge](https://ebizcharge.com/blog/credit-card-surcharging-a-state-by-state-legal-analysis/), [Nickel](https://www.nickel.com/surcharge-laws)).
- **Colorado:** cap of **2%** or cost of processing, whichever is lower ([V] same).
- **New York:** allowed up to cost of acceptance, with strict price display: cash and card prices shown so customers see the difference before paying ([V] [Merchant Cost Consulting](https://merchantcostconsulting.com/lower-credit-card-processing-fees/credit-card-surcharge-laws-by-state/)).
- **California (SB 478, in force 1 July 2024):**
  - An advertised or listed price must include all mandatory fees except government taxes and shipping.
  - **SB 1524** carved out restaurants and bars, which may add a mandatory fee **if it is clearly and conspicuously displayed on menus**.
  - The Civil Code §1791(u) "clear and conspicuous" standard applied from 1 July 2025 ([V] [CA DOJ](https://oag.ca.gov/hiddenfees), [Greenberg Traurig](https://www.gtlaw.com/en/insights/2024/7/california-junk-fee-bill-sb-1524-becomes-law-what-it-means-for-restaurants), [Kelley Drye](https://www.kelleydrye.com/viewpoints/blogs/ad-law-access/california-junk-fee-statute-now-fully-in-play-with-new-twist-from-last-minute-legislation)).
- **Other states [K].** Several states adopted "junk fee" or all-in pricing rules in 2025–2026, for example Massachusetts AG regulations and laws in Minnesota, Colorado and Virginia. Verify each before launch.

### 3.3 Dual pricing / cash discount

- **Cash discounts are federally protected** and are not surcharges when the posted price is the card price and cash receives a discount [K; Durbin].
- "Dual pricing" shows a cash price and a card price. Compliant programs **display both prices on every item, menu or shelf tag** and never reveal a fee only at checkout ([V] [ProTech](https://protechpayments.com/what-is-dual-pricing-complete-guide/), [Lifelong POS](https://www.lifelongpos.com/resources/blog/visa-surcharge-rules-2026-compliance-guide)).
- The settlement text says nothing prevents dual pricing in which total prices for Visa credit and for cash are disclosed separately ([V] [Visa 8-K exhibit via search](https://www.sec.gov/Archives/edgar/data/1403161/000140316125000093/vex991settlementagreementf.htm)).
- Vendor claims that dual pricing is "legal everywhere" should not be taken at face value. Card-price-by-default programs that fall below the disclosure standard are being treated as disguised surcharges ([V] [CCG](https://ccgpays.com/visa-enforcement-on-non-compliant-pricing-programs/)).

### 3.4 Federal junk-fee rule

- The FTC's **Rule on Unfair or Deceptive Fees** took effect on **12 May 2025**. It covers **live-event tickets and short-term lodging only**, not restaurants.
- Covered sellers must show the total price upfront, including all mandatory fees. Civil penalties are up to **$51,744 per violation** ([V] [FTC press release](https://www.ftc.gov/news-events/news/press-releases/2025/05/ftc-rule-unfair-or-deceptive-fees-take-effect-may-12-2025), [FTC FAQ](https://www.ftc.gov/business-guidance/resources/rule-unfair-or-deceptive-fees-frequently-asked-questions)).

### 3.5 Outside the US [K]

- **EU:** interchange caps under the Interchange Fee Regulation (0.2% debit, 0.3% credit on consumer cards). PSD2 bans surcharges on IFR-capped consumer cards.
- **UK:** consumer-card surcharges banned since January 2018.
- **Canada:** credit surcharging permitted since October 2022 up to 2.4%, except where provincial law forbids it (Quebec).
- **Australia:** the RBA proposed ending surcharges on eftpos, Mastercard and Visa. Verify the final decision and date.

**Architecture takeaway.** Surcharge, dual-pricing and all-in-price logic must be a **policy engine** keyed on jurisdiction, network, card product (from BIN data), MCC, channel and effective date. It must not be a single percentage setting.

---

## 4. Security and PCI

### 4.1 Where PCI stands

- **PCI DSS v4.0.1** replaced v4.0 [K for the retirement date of 31 December 2024]. The **51 future-dated requirements became mandatory on 31 March 2025** ([V] [SecurityMetrics](https://www.securitymetrics.com/blog/a-guide-to-new-requirements-in-pci-dss-4-0-1), [Bluefin](https://www.bluefin.com/bluefin-news/what-is-pci-dss-4-0/)).
  - **6.4.3** requires payment-page scripts to be inventoried, authorized and integrity-assured.
  - **11.6.1** requires change and tamper detection on payment pages, weekly by default ([V] [UpGuard](https://www.upguard.com/blog/pci-compliance)).
  - Other v4 items that matter to a SaaS POS provider [K]:
    - MFA for all access into the cardholder data environment (CDE) (8.4.2).
    - Targeted risk analyses (12.3.1).
    - Automated log review (10.4.1.1).
    - Authenticated internal vulnerability scans (11.3.1.2).
    - Anti-phishing controls (5.4.1).
    - Response procedures for PAN found where it should not be (12.10.7).
    - Inventory of keys and certificates (4.2.1.1).
    - Scope confirmation, every 6 months for service providers (12.5.2.1).
    - Appendix A1 for multi-tenant service providers.
  - PCI SSC revised SAQ A eligibility in early 2025 [K]. Re-check which SAQ applies to our online-ordering iframe model.
- **P2PE [V].** With a PCI-listed P2PE solution, Level 2–4 merchants are automatically eligible for **SAQ P2PE** (about 33 questions versus 329 for SAQ D under v3.2). Level 1 merchants still file a Report on Compliance, with much less effort. Eligibility requires that only listed point-of-interaction (POI) devices handle cardholder data and that the merchant has no access to cleartext PAN. SAQ P2PE is card-present only ([V] [Bluefin](https://www.bluefin.com/bluefin-news/differences-pci-validated-p2pe-non-validated-p2pe-solutions/), [PCI SAQ P2PE v4.0](https://listings.pcisecuritystandards.org/documents/PCI-DSS-v4-0-SAQ-P2PE.pdf)).
- **Non-validated "end-to-end encryption" (E2EE)** does not automatically reduce scope. A qualified security assessor (QSA) must evaluate it ([V] [TrustedSec](https://trustedsec.com/blog/pci-p2pe-vs-e2ee-scoping-it-out)).
- **MPoC** (§2.2) is the SoftPOS counterpart.
- **PA-DSS is retired.** Software that handles PAN needs the PCI Secure Software Standard. Our target is to not need it [K].

### 4.2 Scope-minimising architecture (target state)

```
[Customer card/phone] → [PTS-approved terminal OR MPoC SoftPOS]  ← P2PE/MPoC encrypts at read
          │  (encrypted to processor; POS never sees PAN, track, PIN or CVV)
          ▼
[Acquirer / gateway (Adyen, Stripe, Datacap-routed processors, our payfac)]
          │  returns: approval, auth code, network or processor token, masked PAN, EMV tags
          ▼
[POS app / edge]  ⇄  [POS cloud]   ← out of CDE; stores tokens and masked data only
          ▲
[Online ordering] → hosted fields / iframe from the PSP (6.4.3 and 11.6.1 controls on parent page)
```

Rules:
1. No PAN, track data, CVV or PIN ever touches POS memory, logs, crash dumps, analytics or support tooling. Enforce with automated PAN detectors (Luhn plus BIN heuristics) in CI, in log pipelines and in data-lake ingestion.
2. Card-on-file uses **network tokens** or acquirer tokens. Multi-acquirer portability goes through a PCI Level 1 vault partner [K].
3. Payment devices are managed through the acquirer's terminal management system or a remote key-injection partner. We never hold terminal keys [K].
4. If we run our own payfac or processing, **isolate** the CDE (separate cloud accounts, HSM-backed keys, dedicated CI/CD, segmentation tests every 6 months for a service provider) so the POS product stays out of the service-provider scope [K].
5. For the web and QR-ordering "payment page", keep a script inventory, subresource integrity (SRI) and Content Security Policy (CSP) with reporting, plus a tamper monitor that meets 11.6.1 [V for the requirements].

---

## 5. Fiscalization and e-invoicing by country

### 5.1 Country table (status as of 2026-09-27)

Legend: RT = real-time, NRT = near-real-time, "Clearance" = the authority or its proxy validates before the document is valid.

| # | Country | What applies to POS | Certified device / software | Signing / integrity | Reporting cadence | Deadlines and status | Evidence |
|---|---|---|---|---|---|---|---|
| 1 | **Germany** | KassenSichV/§146a AO: every transaction secured by a **TSE**; receipt obligation; DSFinV-K export. **ELSTER notification of POS systems since 1 January 2025** | BSI-certified TSE (hardware or cloud) | TSE signatures with counters per transaction (BSI TR-03153 [K]) | No live reporting; export on audit (Kassen-Nachschau) | Systems bought before 1 July 2025 had to be reported by 31 July 2025; new ones within one month. DSFinV-K v2.4, v2.5 update pending. B2B e-invoice: receive since 2025, **issue from 1 January 2027 (>€800k turnover), all from 1 January 2028** (small receipts ≤€250 exempt [K]) | [C] [Hessen Finanzamt, quoted](https://github.com/dwahdany/claude-skills/blob/d89b5a25c782227bedb360144450dbe44c48647d/buchhaltungsbutler/references/buchhaltung-de.md); [C] [legal map](https://github.com/perschkramon-ui/Steuern-und-Gesetze/blob/13ccf75ae9332cc0a850807a000d35251afdc4cc/docs/rechtsrahmen-pos-kassensysteme.md); [C] [e-invoice dates](https://github.com/Luyzz22/belegflow-ai-site/blob/67de7a8b49867898b541a8c92c325c47e16f7fbd/src/app/wissen/e-rechnungspflicht/page.tsx) |
| 2 | **France** | CGI 286-I-3° bis: cash software must meet the **ISCA** conditions (inaltérabilité, sécurisation, conservation, archivage: unalterability, security, retention, archiving). Proof by NF525/LNE certificate **or publisher self-attestation (restored from 21 February 2026)**. 6-year retention ([C] [LPF L102B, quoted](https://github.com/Luxyra-fr/luxyra.fr/blob/8703405673c9f3e0b8de306a0070637fa0d50070/blog/nf525-explique-ce-que-tout-salon-doit-savoir.html)) | Certified software or attestation | Hash-chained signatures over tickets, grand totals and closures | Periodic closures (Z); B2C **e-reporting** to the tax authority via PA (Plateformes Agréées, approved e-invoicing platforms) under the e-invoicing reform | **1 September 2026:** all companies receive; large companies and ETI (mid-size companies) issue and e-report. **1 September 2027:** SMEs and micro-businesses issue and e-report. Penalties €50/invoice (capped at €15k/year) and €500 per missing e-report | [C] [QrCommunication](https://github.com/QrCommunication/skills/blob/9b4f687118ef1d32020bf7a41b326052a95d9ddf/skills/isca-nf525-facturation-electronique/SKILL.md), [Neo52000](https://github.com/Neo52000/POS/blob/06abe59ce9d7c098a9b694486e4f384b798b7faf/docs/ATTESTATION-EDITEUR.md); [C] [calendar](https://github.com/jeremyH974/site-ia-pme/blob/04558b259f1df051355bc995ef78380c645dc42a/facturation-electronique-2026.html) |
| 3 | **Italy** | **RT** (registratore telematico) or the tax agency's online commercial-document procedure; daily transmission of receipt totals (corrispettivi). **From 2026, card payment terminals must be logically linked to the RT** via "Gestione collegamenti" on the Fatture e Corrispettivi portal | Agency-approved RT device, or the online procedure | Device-sealed signing [K] | Daily; SDI clearance for B2B invoices [K] | Terminals activated after 1 January 2026: link between the 6th day and the last working day of the second month after activation. Deadline for pre-2026 terminals: verify [K] | [C] [ScontrinoZero](https://github.com/dstmrk/scontrinozero/blob/3fc8fb9ad8a8033ab7190f4cd024f6b83ade7eff/src/app/%28marketing%29/help/normativa-pos-2026/page.tsx) |
| 4 | **Austria** | RKSV/§131b BAO cash register duty (turnover thresholds [K]). Every cash sale, monthly, annual, closing, training and void receipt is **electronically signed**. Start receipt registered via FinanzOnline [K] | Signature or seal creation unit (smart card, HSM or cloud seal) | Machine-readable code with register ID, receipt number, timestamp, amounts **by VAT rate**, **AES-256-ICM-encrypted turnover counter**, certificate serial, **chaining value** to the previous receipt, and signature | On audit (DEP export); annual receipt check [K] | In force since 2016/2017 [K] | [C] [RKSV text](https://github.com/legalize-dev/legalize-at/blob/a0c96aff62b5aa5e12b0b924b13c44d27e3a0312/at/AT-20009390.md), [RIS](https://www.ris.bka.gv.at/GeltendeFassung.wxe?Abfrage=Bundesnormen&Gesetzesnummer=20009390), [pretix AT](https://github.com/pretix/pretix-docs/blob/6451d086e154143636f87c1c4b1537a77d23b42e/docs/trust/fiscal/austria.md) |
| 5 | **Portugal** | Tax-authority-certified invoicing software (Portaria 363/2010); **ATCUD** unique code; **QR code**; SAF-T(PT) | Certified software (Modelo 24 process) | Hash-chained document signatures [K: RSA] | **Monthly** SAF-T(PT) invoicing communication | QR since January 2022; ATCUD since January 2023; B2G e-invoice since January 2024; **qualified electronic signature (QES) mandatory on PDF invoices from 1 January 2027**; SAF-T accounting file in 2028 (for FY2027) | [C] [openaccountants PT](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/portugal/portugal-einvoice.md) |
| 6 | **Spain** | **VeriFactu** (RD 1007/2023; Orden HAC/1177/2024). Invoicing software (SIF) records every invoice or ticket with a hash chain and QR. Either **VERI\*FACTU mode** (records sent to the tax agency AEAT as issued) or a signed-records mode with an event log. **TicketBAI** applies in the Basque provinces | Self-declared compliant software | SHA-256 hash chaining; XAdES signatures in the non-VERI\*FACTU mode [K for the algorithm details] | Real time in VERI\*FACTU mode | Software makers had to offer compliant systems from **29 July 2025**. **RDL 15/2025 (BOE 3 December 2025)** moved obligations to **1 January 2027** (corporate-tax payers) and **1 July 2027** (others). Companies filing under SII and the foral territories are excluded. B2B e-invoicing under Ley 18/2022 is pending its regulation (verify) | [C] [BOE RDL 15/2025](https://www.boe.es/eli/es/rdl/2025/12/02/15), [AEAT note](https://sede.agenciatributaria.gob.es/Sede/iva/sistemas-informaticos-facturacion-verifactu/nota-informativa-ampliacion-plazo-adaptacion-facturacion.html), [lara-verifactu](https://github.com/AichaDigital/lara-verifactu/blob/1fa9e202ea91787e305296d9b99ea81c017b1afc/docs/verifactu/Aproximacion-Tecnica.md) |
| 7 | **Poland** | Online cash registers (kasy online) report to the central repository CRK in many sectors [K]. **KSeF** is mandatory for invoices in the **FA(3)** schema. An invoice gets a KSeF number and UPO receipt only after acceptance | Ministry-approved cash registers [K]; KSeF-integrated software | KSeF authentication by certificate or token | Receipts: CRK in near real time [K]. Invoices: clearance in KSeF | **1 February 2026** (>PLN 200m 2024 sales); **1 April 2026** (all other VAT payers); **1 January 2027** (micro: ≤PLN 10k monthly sales). Legal basis: VAT Act amendment of 5 August 2025 (Dz.U. 2025 poz. 1203). Offline and emergency modes exist | [C] [ksefuj](https://github.com/ksefuj/ksefuj/blob/e3a99a5e623d6b7383601db540dcd699773532f7/apps/web/content/pl/blog/ksef-od-1-kwietnia-2026.mdx), [micode](https://github.com/micode-ai/accounting-ai-agent/blob/db9d814d89b1cd3c180fb379ba86fd78e05b7f47/packages/web/content/blog/pl/ksef-od-kiedy-obowiazkowy-2026.md) |
| 8 | **Hungary** | Certified **online cash registers** connected to the tax authority NAV for specified activities, interfacing with a fiscal device (2025 decree). Real-time reporting of all invoices (RTIR, "Online Számla") [K] | Certified register and fiscal module | Device-level [K] | Real time | New cash-register rules came in 2025 (njt.hu 2025-8-20-2X). A mandatory e-payment option applies where online cash registers are used [K] | [C] [pretix HU](https://github.com/pretix/pretix-docs/blob/6451d086e154143636f87c1c4b1537a77d23b42e/docs/trust/fiscal/hungary.md) |
| 9 | **Croatia** | **Fiskalizacija 1.0:** each receipt gets a **ZKI** (issuer security code) and is sent to the tax authority's system (CIS) for a **JIR** (unique invoice identifier). **Fiskalizacija 2.0** (Fiscalization Act NN 89/25): eRačun (structured e-invoice, EN 16931 national profile CIUS-HR) plus fiscalization and e-reporting of B2B invoices | Any software with an FINA-issued certificate [K] | **RSA-SHA256** under CIS spec **v2.7 (21 July 2026)**. RSA-SHA1 rejected in test from 1 July 2026 and in production from **1 January 2027** | Real time (JIR) | **1 January 2026:** VAT payers must issue and receive e-invoices; non-VAT payers receive. **1 January 2027:** non-VAT payers issue | [C] [CIS spec v2.7](https://porezna-uprava.gov.hr/UserDocsImages/Fiskalizacija/Tehni%C4%8Dke%20specifikacije/Fiskalizacija%20-%20Tehnicka%20specifikacija%20za%20korisnike_v2.7%20%2821.07.2026.%29.pdf), [fiskalizacija2-js](https://github.com/shunkica/fiskalizacija2-js), [fisky](https://github.com/nibzard/fisky) |
| 10 | **Greece** | **myDATA:** income documents, including retail receipts, are transmitted to the tax authority AADE. Card terminals must be interconnected with the cash register or ERP so every card payment ties to a fiscal document [K] | Fiscal devices or certified e-invoicing providers [K] | Provider-signed (unique mark, MARK) [K] | Real time or near real time | **B2B e-invoicing: large companies February 2026, all others October 2026.** Terminal-to-register interconnection deadlines were repeatedly extended (verify) | [C] [Kimai 2026 overview](https://github.com/kimai/www.kimai.org/blob/9526e5f4c1b1e49b05d0b12993230b15ee91adf7/collections/_posts/en/2026-01-14-e-invoicing-2026.md) |
| 11 | **Czech Republic** | EET (electronic sales records) **abolished 2023**. **EET 2.0** covers in-person (cash and card) payments only, through a free smartphone app, with no mandatory printed receipts. An "EET off" regime applies to the smallest flat-tax payers | App or POS integration | Specification pending (technical guidelines planned for mid-2026) | Real time (planned) | **Planned start 1 January 2027** (first month pilot). The bill passed second reading by mid-2026; **verify final passage** | [C] [pretix CZ](https://github.com/pretix/pretix-docs/blob/6451d086e154143636f87c1c4b1537a77d23b42e/docs/trust/fiscal/czech-republic.md), [Chamber summary](https://github.com/michalskop/cz-psp-videoarchive/blob/7de67aacc0bbea7e189c60f247a0ab86b3f6285f/summaries/md/summary_2969_2026-06-02_tiskova-konference-poslaneckeho-klubu-ano.md), [news](https://github.com/honzuejtt-ops/Honzueink/blob/85a3a4dafacd021cfc5681065bccbbe64f701118/eindata/zpravy/aktualni/detail/b4af6b816281.txt) |
| 12 | **Belgium** | Restaurants and catering: certified cash register system **GKS** with **FDM/SCU** (fiscal "black box") plus VAT signing card, above a meal-turnover threshold [K]. **B2B e-invoicing via Peppol** | Certified register and FDM [K] | FDM signatures [K] | FDM on-device journal [K]; Peppol for B2B | **Peppol B2B mandatory 1 January 2026**; self-billing tolerance ended 30 June 2026; Peppol 5-corner e-reporting expected **1 January 2028** | [C] [openaccountants BE](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/belgium/belgium-einvoice.md) |
| 13 | **Sweden** | Cash register (kassaregister) mandatory in most cases; **certified control unit (kontrollenhet)**; registration with the tax agency Skatteverket | Certified control unit | Control unit signs or records receipts [K] | Control-unit journal; audit | In force; B2B e-invoicing still in consultation | [C] [pretix SE citing Skatteverket](https://github.com/pretix/pretix-docs/blob/6451d086e154143636f87c1c4b1537a77d23b42e/docs/trust/fiscal/sweden.md) |
| 14 | **Norway** | Cash Register Systems Act and Regulation (since 2019): **vendor product declaration** to Skatteetaten, electronic journal, **SAF-T Cash Register** export [K] | Product-declared software [K] | Chained signature per receipt [K] | On request (SAF-T) [K] | In force; B2B e-invoicing voluntary (EHF/Peppol) | [K]; [C] [e-invoicing matrix](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/_cross-border/saf-t-realtime-ereporting-matrix.md) |
| 15 | **Brazil** | **NFC-e** (model 65) authorised by state tax authorities (SEFAZ) in real time; contingency modes; SAT/MFE legacy in SP/CE [K]. **Tax reform (LC 214/2025): CBS 0.9% + IBS 0.1% test rates on documents in 2026** | Certificate-holding issuer (ICP-Brasil) [K] | XMLDSig with A1/A3 certificate [K] | Real-time authorisation (clearance) | 2026 test year; full CBS/IBS transition 2027–2033 (reference full rates about 8.8% CBS and 17.7% IBS) | [C] [tribultz](https://github.com/mickbap/tribultz/blob/6cf544216ee17e528d1cf44c82b88d36b36166e8/CLAUDE.md) |
| 16 | **Mexico** | **CFDI 4.0** stamped by an authorised certification provider (PAC). Retail sales are rolled into a **global CFDI** to "PÚBLICO EN GENERAL" (RFC XAXX010101000, tax regime 616, use S01, `InformacionGlobal` with periodicity, months and year). Customers self-invoice from the ticket QR or folio | PAC | Issuer CSD signature plus PAC seal [K] | Per invoice (clearance); global CFDI per period | In force | [C] [Avoqado CFDI design](https://github.com/Joseamica/avoqado-server/blob/e69c77d10982138f8fd1cbc49f07086778a65684/docs/superpowers/specs/2026-06-03-facturacion-cfdi-module-design.md) |
| 17 | **Argentina** | e-invoices through web services of the tax authority ARCA (ex-AFIP) → CAE authorization code, plus QR. **Law 27.743 consumer tax transparency (RG 5614/2024):** consumer invoices show "IVA Contenido" and "Otros Impuestos Nacionales Indirectos" | ARCA-authorised issuing, or fiscal controllers for some retail [K] | Authorization code (CAE) from ARCA | Per document (clearance) | Large companies from **1 January 2025**; everyone else **mandatory 1 April 2025**; simplified-regime taxpayers (monotributistas) excluded; penalty: 2–6 days' closure | [C] [invoice requirements](https://github.com/fedemenossi/asistente-virtual/blob/1371834ff24e1d2636ffa3f70758655a78de85df/requisitos_factura.md) |
| 18 | **Chile** | **Boleta electrónica** with CAF folio ranges authorised by the tax authority SII, e-signed; daily reporting [K] | SII-authorised issuer [K] | Electronic signature plus CAF [K] | Daily or per document [K] | In force since 2021 [K]. **Verify the 2025 change on card vouchers versus boletas** (not verified here) | [K] |
| 19 | **India** | **GST e-invoicing:** B2B invoices registered on the Invoice Registration Portal (IRP) to get an **IRN plus signed QR** (turnover thresholds [K]). B2C retail receipts are not e-invoiced; dynamic QR applies to very large B2C issuers [K] | GSP/ERP integration | IRP-signed QR [K] | Per invoice; 30-day upload limit for larger taxpayers [K] | **GST 2.0 from 22 September 2025:** 12% and 28% slabs abolished, items moved to 5% or 18% (and nil) | [C] [openaccountants IN](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/india/india-gst.md) |
| 20 | **Saudi Arabia** | **ZATCA Fatoora Phase 2:** standard (B2B) invoices cleared in real time; **simplified (B2C) invoices reported within 24 h**; invoice counter (ICV) and **previous-invoice hash (PIH, SHA-256)** chain; QR | Each POS onboarded as an invoice-generation unit with a ZATCA certificate [K] | Cryptographic stamp (ECDSA [K]) and hash chain | Clearance for B2B; reporting within 24 h for B2C | Wave 23 (>SAR 750k) went live January–March 2026; **Wave 24 (>SAR 375k) by 30 June 2026** | [C] [openaccountants SA](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/saudi-arabia/saudi-einvoice.md), [invoicekit map](https://github.com/MuhDur/invoicekit/blob/8a9e8d74e23ab97742d56d1dfabe77a9330ec61e/research/regulatory-map.md) |
| 21 | **UAE** | Peppol **5-corner model** (PINT AE) via **accredited service providers (ASPs)**; B2B and B2G initially | ASP | Peppol plus ASP validation | Near real time via the tax authority's (FTA) data platform (corner 5) | Ministerial Decisions 243 and 244 of 2025. Voluntary phase since 1 July 2026. Revenue ≥AED 50M: **ASP by 30 October 2026** (extended from 31 July 2026), **go-live 1 January 2027**. Under AED 50M: ASP by 31 March 2027, go-live **1 July 2027**. Government: **1 October 2027** | [C] [TFM-ERP](https://github.com/TFM-ERP/TFMERP/blob/b65d2c172076b40fcf26bb59ca7471665f48ee70/backend/src/compliance/compliance.service.ts), [MCP-Dubai](https://github.com/mahdi-salmanzade/MCP-Dubai/blob/febbf18da434ecc83c5f8f9bd4ec0c6791d57453/src/mcp_dubai/biz/tax_compliance/server.py), [MoF ASP list](https://mof.gov.ae/en/about-us/initiatives/einvoicing/einvoicing-accredited-service-providers-asps/) |
| 22 | **Egypt** | Tax authority (ETA) **e-invoice** (B2B, mandatory since 2021 in phases) and **e-receipt** (B2C) rolling out nationally; POS registered with ETA, receipts submitted with a UUID [K] | ETA-registered POS [K] | Signed or hashed receipts [K] | Real time or near real time [K] | e-receipt national rollout in progress; verify your phase | [C] [MENA note](https://github.com/rossaddison/invoice/blob/683452d0b607e712ba2ccc363ec32057201d7f0a/docs/FUTURE_PEPPOL_MENA.md) |
| 23 | **Kenya** | **eTIMS** (tax authority KRA): every sale invoice transmitted; integration via **OSCU** (online, hosted by KRA) or **VSCU** (virtual, on the trader's server, for high volume) | KRA-approved integrator | Control-unit data on the receipt: CU invoice number, receipt signature, internal data, SDC ID, MRC number | Real time (OSCU) or batched through VSCU | In force. Costs without eTIMS invoices are non-deductible [K] | [C] [KRA guide](https://github.com/ondieki1237/employeehr/blob/d0a08b44978e897f92c54be573fcecb33fbba5bb/DOCUMENTATIONS/OSCU_VSCU_Step-by-Step_Guide-on-how-to-sign-up.txt), [eTIMS integration doc](https://github.com/Owinovative/invinceible_core_hms_v2/blob/4b7c4f8acb393d46d085ba0373cca6bd1cd64fa9/docs/integrations/etims.md) |
| 24 | **Turkey** | **New-generation cash registers (Yeni Nesil ÖKC)** integrated with payment and connected to the revenue administration GİB for retail receipts; e-Fatura and e-Arşiv for invoices [K] | Certified ÖKC hardware [K] | Device-level [K] | Real time or near real time [K] | Thresholds change yearly [K] | [K] |
| 25 | **Japan** | **Qualified Invoice System:** POS receipts can be **simplified qualified invoices** (適格簡易請求書) for retail, restaurants, taxis and parking; registration number **T + 13 digits**; totals and tax per rate. **Round once per tax rate per invoice (no per-line rounding)**. Returns under ¥10,000 are exempt from return invoices | None (no fiscal device) | n/a | None (no live reporting) | In force since October 2023 [K]; transitional input-credit percentages step down in 2026 [K] | [C] [shinkoku FAQ](https://github.com/kazukinagata/shinkoku/blob/607574d5e5ce63898b2560e083a555d074ef258d/skills/invoice-system/references/faq.md), [requirements](https://github.com/kazukinagata/shinkoku/blob/607574d5e5ce63898b2560e083a555d074ef258d/skills/invoice-system/references/qualified-invoice-requirements.md), [levy notes](https://github.com/SamuelChien/mega-skills-collection/blob/c2b1fdccc779e99ac814e55fb69ca8d71215dd0f/levy/references/invoice-system.md) |

Adjacent regimes worth tracking:
- Oman e-invoicing, reportedly from July 2026 (Peppol), and Bahrain around 2027 ([C] [ASP vendor deck](https://github.com/Rohan0234/vcmo/blob/3e0b52cba29135eb9b44ad9e091f02bcb7607659/kb_corpus/chunks.jsonl)).
- Denmark: B2B e-invoicing from 2026 under its digital bookkeeping act ([C] [matrix](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/_cross-border/saf-t-realtime-ereporting-matrix.md)).
- The US, UK, Canada and Australia have **no POS fiscalization** [K].

### 5.2 Patterns across the table

1. **Four integrity models.**
   - (a) Hardware secure element or fiscal device: DE TSE, AT signature card, BE FDM, SE control unit, IT RT, TR ÖKC, HU.
   - (b) Software hash chain plus signature: FR, PT, ES VeriFactu (non-VERI\*FACTU mode), NO, KSA.
   - (c) Real-time clearance or authorisation: HR JIR, BR NFC-e, MX PAC, AR CAE, PL KSeF, KSA B2B, IT SDI.
   - (d) Network or intermediary model: UAE ASP, BE and FR Peppol or approved platforms.
2. **Receipts carry machine-readable proofs**: QR codes in AT, PT, ES, KSA, AR and HR, and TSE data in DE. The receipt renderer must be driven by fiscal payloads.
3. **Offline and contingency is universal.** Examples: Croatia issues without a JIR and resubmits; Poland has KSeF offline and emergency modes; Brazil NFC-e has contingency modes; German TSE failures must be documented; KSA reports within 24 h.
4. **Retail B2C and invoice B2B diverge.** POS must issue both a receipt and, on request, a B2B e-invoice through the national channel (KSeF, SDI, Peppol, CFDI, IRP, ZATCA). Links between receipt and invoice must be preserved: PL receipt-to-invoice, MX self-invoicing, FR e-reporting.
5. **Rules change and even reverse.** Examples: France's attestation, Spain's two postponements, the UAE ASP deadline extension, Croatia's hash-algorithm change, Czech EET 2.0.

### 5.3 Architecture: a pluggable fiscal layer

**Core principle.** The sales core emits jurisdiction-neutral, immutable **fiscal events**. Country **fiscal providers** turn events into legally valid documents, signatures, reports and exports. Providers are versioned and certified independently of the core.

```
Sales core ──(FiscalEvent: SaleCompleted, Void, Refund, Tip, Deposit, NoSale, Training,
             ProvisionalBill, DayClose, DeviceChange, TaxRateChange …)──►
Fiscal Orchestrator (per location: jurisdiction resolver + policy + outbox + retry)
   ├─ Provider SPI:  register() · sign(event) → FiscalProof · render(proof) → receipt blocks (QR, legends)
   │                 report(batch|event) · close(period) · export(format, range) · contingency(state)
   ├─ Device Agent (edge): TSE (USB/SD/cloud), smart cards (AT), FDM/SCU (BE), control units (SE),
   │                 RT and fiscal printers (IT/PL/HU/TR), with driver plug-ins and health telemetry
   ├─ Clearance and network connectors: KSeF, SDI, CIS/eRačun, SEFAZ, PAC, ARCA, IRP, ZATCA, Peppol AP,
   │                 FR approved platform, UAE ASP, KRA eTIMS, ETA
   └─ Evidence store: append-only, hash-chained journal (WORM), per-country retention (e.g., FR 6 years)
```

Design rules:
1. **Two-phase commit at checkout.** Order finalised → fiscal `sign()` → print or send receipt → payment capture (or payment first where law allows). Signing must complete within the checkout SLA (target: 95th percentile under 800 ms for local devices, under 2 s for cloud).
2. **Contingency state machine** per provider: `ONLINE → DEGRADED → OFFLINE → CATCH-UP`, with legal deadlines as parameters (e.g., 24 h for the KSA B2C report) and a manager-visible backlog.
3. **Country packs are data plus code.** Tax codes, document types, numbering series (e.g., ATCUD series, CAF folios), legends ("IVA Contenido"; the Japanese registration number), and QR templates are configuration. Cryptography and protocols are code.
4. **Release train per country**, with a certified-version pin, because certification ties to software versions (NF525, PT certification, Norway product declaration).
5. **Immutable journal independent of any provider.** The core keeps its own hash chain (SHA-256), so migration between fiscal providers or cloud TSEs never breaks audit continuity.
6. **Buy versus build.**
   - Build: the orchestrator, the event model and the journal.
   - Buy or partner for the long tail and hardware-heavy markets: cloud-TSE and fiscal-API vendors; clearance APIs such as PACs, Peppol access points and ASPs.
   - Build in-house only for strategic markets.

---

## 6. Tax engine requirements

### 6.1 United States sales tax [K unless tagged]

- Economic nexus has applied post-*Wayfair* (2018). Sales tax is levied by states and **thousands of local jurisdictions** with overlapping boundaries, so address-level (rooftop) rates are needed.
- Product taxability varies by state:
  - Groceries.
  - **Prepared food.** Streamlined Sales and Use Tax Agreement (SSUTA) definitions cover heated food, mixed items and utensils provided by the seller.
  - Candy and soft drinks, clothing thresholds, and services.
- Other complications:
  - **Sales-tax holidays** that are date-bound and threshold-bound.
  - Tax on delivery fees and service charges.
  - Exemption certificates.
  - **No sales tax on the SNAP-funded portion** of a transaction.
- Engines: Avalara AvaTax, Vertex, and TaxJar (Stripe-owned). The POS needs:
  - An **offline rate cache per location**.
  - Deterministic fallback and **post-hoc reconciliation** when online lookups fail.
  - Commit and void APIs that mirror the order lifecycle for returns filing.

### 6.2 EU VAT and ViDA [K]

- VAT-inclusive consumer pricing is required (Price Indication Directive).
- Multiple rates per basket, with takeaway versus eat-in distinctions.
- EU vouchers directive: single-purpose vouchers are taxed at issue; multi-purpose vouchers at redemption.
- **ViDA** (adopted 2025):
  - Member states may mandate domestic e-invoicing without derogation.
  - Digital reporting and e-invoicing for intra-EU B2B from **1 July 2030**.
  - Platform "deemed supplier" rules and single VAT registration from **2028**.
- For POS, ViDA means structured e-invoice output (EN 16931: UBL and CII, Factur-X/ZUGFeRD) becomes a core capability, not an add-on.

### 6.3 Rate changes observed in 2025–2026 (design evidence)

- **India GST 2.0**, effective 22 September 2025 ([C] [openaccountants](https://github.com/openaccountants/openaccountants/blob/2338bb0caeb8709675203319bf1172bdb5727523/packages/india/india-gst.md)).
- **Germany:** restaurant food permanently at **7% from 1 January 2026**; drinks stay at 19%. The split rate on mixed bills and bundled menus must be handled ([C] [Bundestag Drucksache 21/1974](https://github.com/thunderquack/BundestagDigest/blob/300edb9bcb9fe71e04e1a7768c969ce6e44bb3cb/drucksache_texts/Gesetzentwurf/2025-10-06%2021_1974.txt)).
- **Brazil:** CBS and IBS test rates in 2026 ([C] [tribultz](https://github.com/mickbap/tribultz/blob/6cf544216ee17e528d1cf44c82b88d36b36166e8/CLAUDE.md)).
- **Czech Republic:** a proposed cut of VAT on non-alcoholic drinks in restaurants to 12% ([C] [news](https://github.com/honzuejtt-ops/Honzueink/blob/85a3a4dafacd021cfc5681065bccbbe64f701118/eindata/zpravy/aktualni/detail/b4af6b816281.txt)).

### 6.4 Tax-inclusive versus tax-exclusive pricing and rounding

- **Tax-inclusive markets** (EU, UK, AU, JP): tax is extracted from gross prices. **Tax-exclusive markets** (US, CA): tax is added.
- The engine must support both, including mixed baskets with a per-item inclusive flag for cross-border chains.
- **Rounding granularity is a legal rule.** Japan: once per tax rate per invoice, with the merchant choosing round down, round up or round half up ([C] [shinkoku](https://github.com/kazukinagata/shinkoku/blob/607574d5e5ce63898b2560e083a555d074ef258d/skills/invoice-system/references/faq.md)). Other markets allow per-line or per-document rounding with a documented method [K].
- The method must be configurable per jurisdiction, **effective-dated**, and recorded on each document.
- **Tax display legends** (e.g., Argentina "IVA Contenido") come from the fiscal pack.

### 6.5 Cash rounding

- **US:** the final circulating pennies were minted in June 2025, and the ceremonial last strike in Philadelphia was reported on 12 November 2025. Pennies remain legal tender. Some retailers round cash totals to the nickel, and state law and SNAP rules complicate this. Retail groups want federal legislation ([C] [news digest](https://github.com/ensonfun/ensonfun.github.io/blob/c380b71ee94d99ef102165d7864dda983ca26ca2/content/posts/2025-11-12.md)). **No federal rounding statute was confirmed in this research**; verify pending bills.
- **Canada [K]:** since February 2013, cash totals round to the nearest $0.05 (1–2 cents down, 3–4 up to 5, 6–7 down to 5, 8–9 up). Rounding applies after tax and only to cash; electronic payments are exact.
- **Other cash-rounding regimes [K]:** examples include 0.05 rounding in several eurozone states and Switzerland, whole-krona rounding in Sweden, and 0.50 kroner in Denmark.
- **Requirement:** rounding applies to the **cash tender only**, on the final amount due. It is recorded as a separate, auditable rounding line with account mapping and fiscal treatment. It must be symmetric where the law requires and configurable per state or country with effective dates. It must never apply to SNAP or EBT tender where prohibited.

### 6.6 Tax-engine capability list

The full capability set is requirement 30 in §8, extended by requirements 31–32 for the US engine and cash rounding.

---

## 7. Labor, consumer, accessibility, privacy and age-verification rules affecting POS

### 7.1 Cash acceptance and cashless bans [K unless tagged]

- **US bans on cashless stores** in some form include Massachusetts, New Jersey, Philadelphia, San Francisco, New York City, Colorado, Rhode Island and Washington DC. Verify the current list and exemptions.
- **Sweden:** a cash-access law (committee report FiU39) advanced in the Riksdag in 2026; check its final scope ([C] [Riksdag document](https://data.riksdagen.se/dokument/HD01FiU39)).
- An **EU regulation on the legal tender of euro cash** was proposed alongside the digital euro. Its status is pending.
- **Requirement:** a jurisdiction flag disables card-only mode at stores and kiosks where cash must be accepted, and kiosks provide a compliant cash path.

### 7.2 Price transparency (cross-reference §3)

- FTC fee rule for tickets and lodging [V].
- California SB 478 and SB 1524 [V].
- Various state junk-fee laws [K].
- Engine support: all-in price display, clearly disclosed restaurant service fees, and the dual-price display in §3.3.

### 7.3 Tips and labor

- **FLSA tip credit [K].** The federal minimum cash wage is $2.13/h against the $7.25 minimum wage, so the maximum tip credit is $5.12.
  - Since the 2018 amendments, employers, managers and supervisors may not keep tips.
  - Tip pools may include back-of-house staff only if no tip credit is taken.
  - The Department of Labor's 2021 "80/20" dual-jobs rule was vacated by the Fifth Circuit in 2024.
  - Many states forbid tip credits or card-fee deductions from tips.
- **OBBBA qualified-tips deduction** ([C] [guide](https://github.com/webwithhassan-prog/claude-credit/blob/a5fce15153077f54ae28838f237cb4278b14e4fc/src/content/guides/schedule-1a-deductions-explained.md), [taxonomy](https://github.com/gtax0415-max/taxonomy/blob/cc2798a74d73cf0f33d44d76285421e1f634a2e3/federal/deductions/schedule-1-a/qualified-tips.md)).
  - Deduction for tax years 2025–2028, capped at **$25,000**.
  - From 2026, the W-2 reports **Box 12 code TP** (qualified tips) and **Box 14b** (Treasury tipped-occupation code).
  - The act also adds tip reporting on 1099 forms and extends the FICA tip credit to beauty services.
  - Mandatory service charges are **not** tips [K].
  - The POS must classify every gratuity as **voluntary tip (cash or charged)** or **mandatory service charge**, attribute it to employees and roles or occupation codes, and export to payroll.
- **Czech Republic (planned):** voluntary tips exempt from tax and insurance up to **7% of establishment turnover**, alongside EET 2.0 ([C] [Chamber summary](https://github.com/michalskop/cz-psp-videoarchive/blob/7de67aacc0bbea7e189c60f247a0ab86b3f6285f/summaries/md/summary_2969_2026-06-02_tiskova-konference-poslaneckeho-klubu-ano.md)).
- **Predictive-scheduling laws [K].** Examples: Oregon (statewide), New York City, Seattle, Chicago, Philadelphia, San Francisco, Los Angeles, Berkeley, Emeryville, San Jose and Evanston.
  - Rules typically require about 14 days' advance notice and premium pay for changes.
  - They add rest rules between shifts ("clopening") and require offering hours to existing part-timers first.
  - Scheduling and timeclock modules need rule packs with effective dates and premium calculations.

### 7.4 Accessibility

- **European Accessibility Act** (Directive (EU) 2019/882), applying since **28 June 2025**.
  - Covers **payment terminals (hardware and software)** and interactive **self-service terminals**: ATMs, ticketing and check-in machines, and information kiosks.
  - Self-service terminals must allow the use of **personal headsets**, have **adequate contrast and tactilely discernible keys and controls** where keys exist, alert the user through more than one sensory channel, and allow time to be extended.
  - Transitional rules: service contracts agreed before 28 June 2025 may run until they expire, but no longer than 5 years. **Self-service terminals already in use may be kept until the end of their economically useful life, but no longer than 20 years** ([C] [EUR-Lex 32019L0882](https://eur-lex.europa.eu/legal-content/EN/TXT/HTML/?uri=CELEX:32019L0882), [text](https://github.com/DTMC-marketplace/governance/blob/8bb31afb172df084f394b13f6e914db33d5cd4b5/ai_act_articles/Directive_EU_2019_882_Accessibility_Products.txt)).
  - The harmonised standard is EN 301 549 [K].
- **ADA Title III (US) [K].** There is no kiosk-specific rule for private businesses. The 2010 ADA Standards §707 covers ATMs and fare machines. Litigation relies on effective communication. Build to WCAG 2.2 AA plus EN 301 549 and Section 508 for kiosks, customer-facing displays and web ordering, with reach ranges, speech output, a tactile navigation pad and a headphone jack.

### 7.5 Privacy

- **GDPR/ePrivacy [K].** A digital receipt can rest on contract or legal obligation. Marketing use of a captured email needs separate consent, or the soft opt-in where the national law allows it. Apply data minimisation, and reconcile fiscal retention (6–10 years) with erasure by pseudonymising personal data in fiscal records.
- **CCPA/CPRA.** CPPA regulations on risk assessments, cybersecurity audits and automated decision-making technology (ADMT) were adopted on 24 July 2025, approved by California's Office of Administrative Law (OAL) on 22 September 2025, and are **effective 1 January 2026**. The ADMT compliance deadline is **1 January 2027** ([C] [handoff doc](https://github.com/noahwilliamshaffer/CPPA-AUDIT/blob/ddadf15e9e6325e710e6c24224fa430690d98aaa/ShieldAudit_Software_Engineer_Handoff.md)). A notice at collection is needed wherever the POS captures email, phone or loyalty IDs [K].
- **California Song-Beverly Act [K].** A business may not require personal identification information, **ZIP code included**, as a condition of a credit-card transaction. Massachusetts has similar case law. Email capture must never be tied to card authorization.
- **FACTA [K]:** receipt truncation (§2.1).
- **Biometrics [K].** BIPA and similar laws apply to biometric employee timeclocks and palm or face payment. Get consent and set retention schedules.

### 7.6 Age verification

- **Tobacco (US federal).** Tobacco 21. **From 30 September 2024, retailers must check photo ID for anyone under 30** buying cigarettes, smokeless or covered tobacco ([C] [FDA quote](https://github.com/vibewatch/startupv0/blob/d3357bc046f03f71b2fc605099522c1bdf9b9af9/reports/gopuff-diligence-report.yaml)).
- **Alcohol and cannabis [K].** State rules apply.
- **ID-scan data [K].** Limits on retaining scanned data apply in some states.
- **Mobile IDs [K].** Mobile driver's licences (ISO/IEC 18013-5/-7) and EU Digital Identity Wallets (from about late 2026) enable privacy-preserving "over 18/21" proofs.
- **Requirement:** item-level age flags drive a blocking prompt. Verification uses a PDF417 barcode, mDL or manual date of birth. Store only a pass/fail result, the method and the clerk ID, with no ID images by default. Kiosks hand off to a staff-assisted flow.

### 7.7 EBT/SNAP/WIC (US)

- Item-level SNAP eligibility is changing **state by state** under USDA restriction waivers. At least 10 states were moving on this as of mid-2026 ([C] [article](https://github.com/ariedugan-lab/articles/blob/7a86d205b22785fa945719504d544b82a47f4241/2026-06-20-1202-the-fresh-food-transition-how-snap-junk-food-bans-force-a-new-underwriting-framework-for-small-retail-lenders.md)).
- The eWIC approved product list is UPC-driven [K].
- Requirements: effective-dated eligibility lists, EBT balance inquiry, SNAP-first split tender, no sales tax on the SNAP portion, and no surcharges on EBT [K].

---

## 8. Implications for our design: concrete, testable requirements

Format: **requirement**, then *Test:* the acceptance check.

**A. Payments architecture and anti-lock-in**

1. **Processor-agnostic payment SPI.** Checkout calls one `PaymentConnector` interface. At GA, ship at least three connectors: Adyen Terminal API (local and cloud), Stripe Terminal, and one US processor-agnostic semi-integrated gateway such as Datacap. *Test:* one end-to-end suite (sale, tip adjust, void, refund, partial auth, split tender, offline) passes on every connector, and switching a merchant's connector is config-only with no reinstall.
2. **No processing lock.** POS pricing is identical with or without our processing, with no third-party-processor fee. Merchants can export all data, including a **PCI-compliant stored-card export** to a named PCI Level 1 recipient. *Test:* the price-book lint rejects any processor-conditional fee, and an export dry run completes within 10 business days.
3. **Semi-integrated by default; PAN never enters the POS.** *Test:* CI and runtime PAN detectors (Luhn plus BIN) scan logs, databases, crash dumps and analytics. Any hit blocks the release and pages security.
4. **SoftPOS is a first-class terminal.** Support MPoC-listed Tap to Pay on iPhone and on Android through acquirer SDKs, including PIN where required. Keep a device-eligibility registry (OS version, reduced-range certification, attestation). *Test:* certification passes per acquirer, and ineligible devices are blocked with a clear message.
5. **Bounded, explicit offline mode.** Merchants opt in and sign a liability acknowledgement. Limits cover per-transaction amount, cumulative amount and maximum age; the platform caps them per acquirer (e.g., Stripe ≤$10k total; ≤72 h on Square-like models). BIN-risk rules apply (e.g., no prepaid or foreign cards offline). Queued payments auto-forward on reconnect, and a live exposure dashboard shows what is outstanding. *Test:* simulations enforce each limit, the queue is encrypted at rest, and 99.9% of queued items forward within 5 minutes of reconnect.
6. **Tip engine with scheme tolerances.** Supports tip-on-device and tip adjust. When the adjusted total exceeds the brand/MCC tolerance (Visa 20% for 5812/5813), the engine triggers an incremental authorization. *Test:* an MCC × brand × card-present matrix asserts either an incremental auth or a risk flag.
7. **Pre-auth, incremental and extended auth for tabs and hotels, with an auth-expiry tracker.** *Test:* the tracker captures or re-authorizes before expiry, and increment limits hold per connector (Stripe ≤10).
8. **Partial authorization and N-way split tender.** Send the partial-auth indicator only from lanes that can split tender (Visa rule). *Test:* a partial approval on a prepaid card leaves the correct balance across mixed tenders.
9. **Refunds without the card.** Referenced refunds across locations and channels; unreferenced refunds only under policy (limits, manager PIN, fraud velocity). *Test:* token refunds succeed without the card, and unreferenced refunds over the limit are blocked.
10. **Debit routing and orchestration.** Support the US Common Debit AID, merchant routing preferences and least-cost-routing reports. Store card-on-file as network tokens or in a portable vault. *Test:* reports show the network selected per debit transaction, and stored credentials route to a secondary acquirer in a failover drill.
11. **Tender plug-in framework.** One tender model covers cards, cash, gift cards, store credit, QR and account-to-account payments (Pix, UPI, PayNow, PromptPay, Alipay+/WeChat Pay, Wero, request-to-pay), M-Pesa, BNPL, EBT/eWIC, and partner-settled stablecoin or bitcoin. *Test:* a new tender ships without core changes and passes the refund, void and reconciliation contract tests.
12. **Dispute evidence and monitoring.** Capture the data elements for Visa CE3.0 and Mastercard First-Party Trust, plus card-present EMV data and receipts. Compute VAMP-style ratios per merchant and alert at 70% of each threshold. *Test:* dispute packets auto-assemble, and alerts fire on synthetic data.
13. **Double-entry payments ledger.** Covers auth, capture, fees, payouts, instant payouts, reserves and chargebacks, reconciled daily to acquirer settlement files. *Test:* zero unexplained variance on reconciliation fixtures.

**B. Fees, surcharging and price display**

14. **Surcharge policy engine.** Inputs: jurisdiction (bans in CT, MA, ME and PR; Colorado 2% cap; New York display rules), network caps (Visa 3%, Mastercard 4%, the settlement's 3% once effective), card product from BIN (credit only, never debit or prepaid even when run as credit), the merchant's actual cost of acceptance, brand-level or product-level mode, and effective dates. Outputs: disclosures, a receipt line, and a 30-day acquirer-notice checklist. *Test:* a golden table of 200+ cases, including debit-as-credit and a location in a ban state.
15. **Dual-pricing mode.** Both prices appear on every item, menu, online listing and shelf-label export, and checkout never adds an undisclosed fee. *Test:* the display linter blocks publishing any item that lacks a dual price.
16. **All-in pricing mode.** Displayed prices include mandatory fees, as California SB 478 and the FTC ticket/lodging rule require. A restaurant variant discloses fees on menus instead. *Test:* exported prices include mandatory fees.
17. **Card-category acceptance controls (settlement-ready).** Merchants can decline premium-consumer and/or commercial credit categories using BIN category data. The control stays behind a flag until final approval (hearing 16 November 2026) and scheme rule updates. *Test:* a declined category shows a compliant message and prompts for another tender.
18. **Cost-of-acceptance transparency.** Store interchange, scheme fees and markup (IC++) on every transaction. *Test:* statements tie to settlement files, and the surcharge cap uses the merchant's trailing actual cost.

**C. Security and PCI**

19. **PCI scoping by architecture.** The POS product stays out of cardholder-data scope. Merchants on our P2PE or MPoC paths qualify for SAQ P2PE or the SoftPOS equivalent. Any cardholder-data environment we operate as a payfac is isolated. *Test:* scope confirmation (every 6 months for the service-provider environment), segmentation penetration tests and QSA sign-off.
20. **Online-ordering payment pages meet PCI DSS 6.4.3 and 11.6.1.** *Test:* CSP and SRI enforce the script inventory, and tamper alerts fire within 7 days by default (target: near real time).
21. **Identity and access.** Phishing-resistant MFA for all staff and support access, least-privilege merchant impersonation with a customer-visible audit trail, and automated log review. *Test:* access reviews and alert drills.
22. **Tokens only.** Store only network or processor tokens and masked PANs, and handle the network-token lifecycle. *Test:* token update and suspend webhooks update stored cards.

**D. Fiscal and tax**

23. **Fiscal orchestrator with a country-provider SPI** (register, sign, render, report, close, export, contingency). *Test:* conformance suites per country using official validators or golden files: DSFinV-K and TSE (DE), DEP (AT), VeriFactu XML and QR (ES), KSeF FA(3) (PL), ZATCA SDK (KSA), CFDI (MX), PINT AE (UAE).
24. **Append-only, hash-chained fiscal journal**, independent of any provider. *Test:* tampering breaks chain verification, and a provider migration keeps continuity.
25. **Checkout fiscal SLA.** Signing adds ≤800 ms at p95 with local devices and ≤2 s with cloud providers. Contingency mode engages within 3 s of provider failure, with legal catch-up windows as parameters. *Test:* chaos tests per provider.
26. **Receipt renderer driven by fiscal payloads** (RKSV code, ATCUD and QR, VeriFactu QR, TSE data, "IVA Contenido", Japan T-number with per-rate totals, ZATCA QR). *Test:* snapshot tests per country.
27. **Fiscal device agent at the edge.** Drivers for TSEs (USB/SD/cloud), Austrian smart cards, Belgian FDMs, Swedish control units, Italian RTs and PL/HU/TR fiscal printers, with health telemetry. *Test:* a hardware-in-the-loop suite, with failover handled as each law requires.
28. **E-invoice gateway.** Supports EN 16931 (UBL, CII, Factur-X/XRechnung) and the national channels: KSeF, SDI, CIS/eRačun, SEFAZ, PAC, ARCA, IRP, ZATCA, Peppol (Belgium; UAE via an ASP), French Plateformes Agréées, KRA eTIMS and ETA. It can issue a B2B invoice from a POS receipt. *Test:* a sandbox round trip for each channel.
29. **Regulatory release train.** Each country pack has a certified-version pin, effective-dated configuration and a regulatory backlog reviewed monthly. *Test:* no country pack deploys without a compliance sign-off, and scheduled rate changes activate at local midnight.
30. **Effective-dated tax catalogue** with taxability codes, eat-in/takeaway context, bundle apportionment, inclusive and exclusive pricing, deposits, voucher types, exemptions, tax holidays, returns linked to their original tax lines, and rounding granularity per jurisdiction (Japan: once per rate per invoice). *Test:* golden baskets for Japan 8%/10%, Germany 7% food plus 19% drinks on one bill, India GST 2.0, and US prepared food.
31. **US tax integration.** Avalara, Vertex or TaxJar adapters, an offline rate cache, commit/void mirroring, the SNAP exemption, tax holidays and exemption certificates. *Test:* sales made offline reconcile to engine results after reconnect, with any variance logged.
32. **Cash-rounding engine.** Applies to the cash tender only, on the final total, with rules per jurisdiction (Canada 5¢; US nickel rounding optional by state). Rounding is posted as a separate audit line with its own fiscal treatment. *Test:* rounding tables pass, and card and EBT tenders are never rounded.

**E. Labor, consumer, accessibility, privacy, age**

33. **Gratuity model.** Each gratuity is classified as a voluntary tip (cash or charged) or a mandatory service charge and attributed to employees. Tip-pool rules and a per-jurisdiction toggle for passing card fees through to tips are configurable. The payroll export carries the OBBBA fields (code TP amounts, tipped-occupation code). *Test:* W-2 export fixtures, and service charges never appear as qualified tips.
34. **Scheduling rule packs** for predictive-scheduling cities and states, with advance-notice checks and premium pay. *Test:* a schedule change inside the notice window generates premium pay.
35. **Cash-acceptance guard.** A location in a cashless-ban jurisdiction cannot be set to card-only, and kiosks there offer a cash path. *Test:* the config validator rejects card-only mode.
36. **Accessible customer-facing surfaces.** Kiosks, customer displays and web ordering meet the EAA self-service requirements (headset audio, tactile controls, contrast, time extension), WCAG 2.2 AA and EN 301 549. *Test:* automated axe checks plus a manual audit each release, and an EAA conformity file for EU deployments.
37. **Privacy by design.** Email or phone capture is never a condition of card payment (Song-Beverly). Marketing consent is separate from receipt delivery, and receipts are FACTA-truncated. Also provide notice at collection, DSAR tooling, and pseudonymisation that respects fiscal retention periods. *Test:* card checkout has no required PII fields, and DSAR fixtures pass.
38. **Age-restricted sales.** Item flags force a blocking verification (PDF417 barcode, mobile driver's licence or manual date of birth), including the FDA under-30 prompt for tobacco. Only the result and the method are stored. *Test:* restricted items cannot complete unverified, and no ID images persist.
39. **EBT/SNAP.** Effective-dated, state-specific eligibility lists (reflecting the 2026 restriction waivers), SNAP-first split tender, no sales tax on the SNAP portion and no surcharges. *Test:* fixture baskets per state.

---

## Appendix A. Verification backlog before build or launch

These items are [K] or partially verified and are material to design or launch sequencing:

1. **Italy:** deadline and penalties for linking card terminals activated before 2026 to the RT; latest RT technical specification version.
2. **Greece:** current deadline and scope of the terminal-to-register (ERP) interconnection; myDATA B2C transmission rules.
3. **Chile:** status of card vouchers versus boletas since 2025.
4. **Hungary:** the 2025 cash-register decree and its sector list and deadlines.
5. **Belgium:** any extension of GKS beyond current restaurant thresholds; FDM/SCU replacement plans.
6. **Norway and Sweden:** signature algorithms and any 2026 rule updates.
7. **Austria:** current turnover thresholds and receipt-issuing rules.
8. **Portugal:** current monthly SAF-T submission day.
9. **Saudi Arabia:** waves after Wave 24.
10. **Egypt:** e-receipt phase list.
11. **Turkey:** 2026 e-document thresholds.
12. **Spain:** status of the Ley 18/2022 B2B e-invoicing regulation.
13. **Czech Republic:** final passage and technical specification of EET 2.0.
14. **US:** federal or state penny-rounding bills; current cashless-ban list; state junk-fee laws (MA, MN, CO, VA and others); FedNow current limits.
15. **Card networks:** outcome of the 16 November 2026 fairness hearing and the effective dates of any scheme rule changes; Eighth Circuit Regulation II ruling; CCCA floor action.
16. **Australia:** RBA surcharging decision and effective date. **Canada:** surcharge rules by province.
17. **EU:** status of the euro legal-tender regulation; national cash-acceptance laws.
18. **PCI:** confirm the current SAQ A eligibility text and MPoC listing for each target SoftPOS SDK.
