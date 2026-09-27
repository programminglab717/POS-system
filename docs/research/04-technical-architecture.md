# Track 04: Technical architecture, reliability, sync, hardware and platform

*Research date: 2026-09-27. Audience: the chief architect. This covers sections 1–9 of the brief. Section 10 turns the findings into architecture requirements.*

**How to read the evidence.**
- Sources are vendor docs and help centres, status pages, post-mortems, GitHub repos and specs, and MDN's `browser-compat-data`.
- Partway through, the session's shared web-search quota ran out (about 33 searches were mine). Most vendor domains (Toast, Square, Clover, Oracle, AWS, Cloudflare, Wikipedia) were also blocked for direct fetching.
- To compensate:
  - Many vendor facts come from search-result extracts of the linked pages.
  - GitHub-hosted primary sources (READMEs, licences, specs, compat data) were read directly.
  - A few facts verified by sibling research tracks (01, 02, 06 in this folder) are reused with their links.
- **†** marks claims from prior knowledge (through mid-2026) that were **not re-verified in this session**. Treat them as leads to confirm.

---

## 0. Executive summary

- **Offline gap.** Cloud-first incumbents' "offline mode" means each device works alone and stores card payments to forward later. Coordinating several devices offline is where they fail:
  - Toast needed a separate LAN hub; Square and Clover devices become islands.
  - Only on-prem systems (Aloha's back-office server, Simphony's on-site posting service) keep the whole store coherent.
- **Why systems go down.** Mostly configuration or control-plane changes and shared dependencies (DNS, CDN, login, one cloud region), then vendor ransomware and help-desk social engineering. Stores with **local authority** kept selling: in NCR's 2023 attack, in-restaurant sales continued while the cloud was down.
- **Sync.** No off-the-shelf engine combines POS business rules, a store-LAN hub or peer mode, and open licensing. Zero rejects offline writes; PowerSync and Electric sync device to cloud only; Ditto is a proprietary LAN mesh.
  - **Recommendation:** build a hub-sequenced operation log on SQLite (the hub puts every change into one order) in a shared Rust core. Borrow Replicache's "server reconciliation" (the authority re-runs each change). Use CRDTs (auto-merging data types) only for narrow cases such as inventory deltas.
- **Client.** Rust core with native shells: Kotlin/Compose on Android first, SwiftUI on iPad, Tauri 2 for Windows and Linux lanes, KDS and customer displays.
  - PWAs cannot be LAN peers, and iOS web apps have no USB, serial, HID or Bluetooth APIs.
  - Shopify's September 2026 move from React Native back to native, including POS, supports this.
- **Cloud.** Region-pinned cells with Postgres per cell. NATS JetStream at the store edge feeding Kafka. Idempotency keys end to end, OpenTelemetry from device to payment provider, and TUF-signed ring rollouts with configuration treated as code.

---

## 1. How incumbents are built

### 1.1 At a glance

| Vendor | Client platform | In-store topology | Behaviour when offline | Offline card limits |
|---|---|---|---|---|
| **Toast** | Android on Toast hardware† | Cloud-first. Optional **Local Sync**: an auto-assigned Toast device acts as LAN hub ([Toast](https://doc.toasttab.com/doc/platformguide/platformOfflineModeLocalSync.html)); it must be **hardwired**, serves only its subnet (track 01) | Without the hub, "orders added or updated on one device do not appear on other devices" (KDS excepted) ([Toast](https://doc.toasttab.com/doc/platformguide/platformOfflineMode.html)). Loyalty, gift cards, shift review and login fail offline ([Support](https://support.toasttab.com/en/article/Using-Toast-in-Offline-Mode)) | On by default. Optional per-transaction cap with manager approval ([Toast](https://doc.toasttab.com/doc/platformguide/adminOfflineCCPayments.html)). Authorizations "may expire as early as 24 h" (track 01) |
| **Square** | Native iOS and Android. Square Register, Terminal and Handheld | Pure cloud; each device is an island | Store-and-forward on each device. In the 2023 outage sellers had to **pull the network cable** to force offline mode (track 01) | Upload within 24 h is recommended; payments **expire at 72 h**. Ireland default is €100 per transaction, maximum €50,000. The Mobile Payments SDK allows ≤1,000 payments over ≤24 h per device. The seller is liable for declines ([Square](https://squareup.com/help/us/en/article/7777-process-card-payments-with-offline-mode), [Square IE](https://squareup.com/help/ie/en/article/8106-offline-payments-time-limits), [SDK](https://developer.squareup.com/docs/mobile-payments-sdk/ios/offline-payments)) |
| **Clover** (Fiserv) | Android-based Clover OS devices; apps from the App Market† | Pure cloud | Payments queued on the device and auto-submitted on reconnect | **7 days, fixed.** Defaults: $500 per transaction and $5,000 in total, both configurable ([Clover dev](https://docs.clover.com/dev/docs/handling-offline-payments), [Clover](https://www.clover.com/help/set-up-offline-payments)) |
| **Lightspeed K-Series** | iPad | Marketed as "TrueSync". LAN sync needs about 10 open ports and client isolation turned off ([K-Series networking via track 01](https://k-series-support.lightspeedhq.com/hc/en-us/articles/16154347413275-Networking-for-Lightspeed-Restaurant)) | Keeps sending orders to kitchen and bar. Payments and reporting "may pause" | Offline cards "not guaranteed". U-Series Mobile Tap: 24 h per terminal ([Upserve](https://help.upserve.com/s/article/Lightspeed-Restaurant-U-Series-POS-Mobile-Tap-FAQ-s-and-Functionality-Comparison)). X-Series: $5,000 per transaction and $50,000 in total, both the default and the maximum ([Lightspeed](https://x-series-support.lightspeedhq.com/hc/en-us/articles/25533690950427-Lightspeed-Payments-offline-mode-with-Retail-POS-X-Series)) |
| **Lightspeed L-Series** | iPad | Local Mac **LiteServer** ([Lightspeed](https://resto-support.lightspeedhq.com/hc/en-us/articles/5218871669403-Troubleshooting-the-LiteServer)) | Limited operations | — |
| **Shopify POS** | React Native. In **September 2026** Shopify said it is rebuilding Shopify, Shop, **POS** and Inbox in Swift and Kotlin ([Shopify Eng](https://shopify.engineering/back-to-native), [InfoQ](https://www.infoq.com/news/2026/09/shopify-drops-react-native/)) | Pure cloud. Extension bundles are cached and "run offline, so a device can stay on an older version" ([ui-extensions](https://github.com/Shopify/ui-extensions/blob/2026-10-rc/packages/ui-extensions/src/surfaces/point-of-sale/api/extension-api/extension-api.ts)) | Cash-only by default. Offline card acceptance is opt-in and must be configured **before** the outage | Per-order cap plus per-device daily cap. No swipe, Tap to Pay or MOTO offline. No Interac (CA) or eftpos (AU). Not available in France. Requires POS ≥ 9.14.0 ([Shopify](https://help.shopify.com/en/manual/sell-in-person/shopify-pos/selling-offline/offline-payments)) |
| **Oracle Simphony** | Workstations† plus cloud Enterprise on OCI | **CAPS** (Check and Posting Service) is a **required on-prem service** that posts to the Enterprise and "serves as the arbitrator of check sharing by maintaining a record of check ownership"; scales on IIS plus Oracle DB for stadiums ([Oracle](https://docs.oracle.com/en/industries/food-beverage/simphony/simcg/c_caps.htm)) | Workstations keep posting to CAPS during a WAN outage | Processor-dependent† |
| **NCR Aloha** | Windows FOH terminals | **BOH file server** plus terminal redundancy: a terminal takes over as master ([NCR Voyix](https://docs.ncrvoyix.com/restaurant/aloha-takeout/implementing/enabling_redundancy)) | Redundancy mode can run for **up to 30 days** (red screen border), with offline folders on each terminal ([Scribd](https://www.scribd.com/document/255635737/Aloha-Pos-Redundancy)) | Processor-dependent† |
| **Revel** | iPad | "Hybrid Network Architecture": local network plus cloud ([Revel](https://revelsystems.com/features/always-on-mode/)) | "Always On" mode keeps selling | Card data is captured and queued, so cards may decline when replayed ([Revel blog](https://revelsystems.com/blog/2013/07/30/pos-true-offline-mode-offline-but-not-out-of-commission/)) |
| **TouchBistro** | iPad | A Mac mini or iMac server is required for multi-iPad sites: "the brain" ([TouchBistro](https://www.touchbistro.com/hardware-requirements/)) | iPads keep working against the local server; cloud is used for reporting | TouchBistro Payments has an offline mode (track 01) |
| **PAR Brink** | Cloud-hybrid SaaS | In-store topology not publicly documented in the sources found | Offline mode, then automatic sync ([RDS](https://www.rdspos.com/Brink-POS)) | — |

### 1.2 Topology taxonomy

1. **Dedicated on-prem server:** Aloha BOH, Simphony CAPS, TouchBistro Mac, Lightspeed LiteServer.
   - Best in-store continuity. CAPS explicitly arbitrates check ownership, the key multi-device invariant.
   - The cost is operations: patching, hardware failure, Windows attack surface, and CrowdStrike-class exposure.
2. **Hub elected or assigned among POS devices:** Toast Local Sync.
   - No extra hardware. The hub is a single point of failure, and it inherits network fragility (it must be wired and on the same subnet).
3. **Pure cloud with isolated offline devices:** Square, Clover, Shopify POS, Lightspeed X.
   - Simplest to run. During outages they lose tabs, check transfers, split checks, KDS routing across devices, loyalty, gift cards and close-out.
4. **Peer-to-peer mesh:** Ditto-based designs (see §3).
   - No single point of failure, and works over BLE or P2P Wi-Fi. The engine is proprietary and uses CRDT semantics.

**Takeaways**
- Offline card acceptance everywhere is deferred authorization: the merchant carries decline risk.
- Windows range from **24 h to 7 days** and caps from **$500 to $50,000**.
- No vendor found offers BIN-level offline risk rules or a live offline-exposure dashboard (track 01). That is an open differentiator.
- **Mode detection is itself a failure point.** Square devices stayed "online" while its back end failed.
- **Platform trend.** Shopify ran POS on React Native, with New-Architecture regressions of up to 20% on complex screens ([Shopify Eng](https://shopify.engineering/react-native-new-architecture)) and native threading needed for sync jobs ([Shopify Eng](https://shopify.engineering/five-years-of-react-native-at-shopify)). In 2026 it went native: the Shop rebuild took 6 engineers 12 weeks with AI agents, halved Android startup, cut 109 MB and reduced crashes 10× ([InfoQ](https://www.infoq.com/news/2026/09/shopify-drops-react-native/); [summary](https://github.com/ahastudio/til/blob/main/mobile/shopify-back-to-native.md)).

---

## 2. Incidents and outages

### 2.1 Timeline

| Date | Incident | Root cause | Blast radius and duration |
|---|---|---|---|
| 2017-03-16 | **Square** | A Roster deploy caused a Multipass (auth) cascade. "A specific code path had a high upper bound (500) for retries on optimistic transactions with no backoff," saturating Redis ([Square](https://medium.com/square-corner-blog/incident-summary-2017-03-16-2f65be39297)) | Payments and POS for all sellers were down for about 2 h. SMS 2FA was degraded for longer |
| 2023-04-12 → about 04-17 | **NCR Aloha** ransomware (ALPHV/BlackCat) | Ransomware in a single data centre ([NCR 8-K](https://www.sec.gov/Archives/edgar/data/70866/000119312523103016/d472247dex991.htm)) | Aloha cloud and Counterpoint functions were down: back office, schedules, payroll, inventory, gift cards and Pulse for days. **In-restaurant transactions continued.** NCR built a new cloud environment ([Cybersecurity Dive](https://www.cybersecuritydive.com/news/ncr-pos-ransomware-recovery/648005/), [Computer Weekly](https://www.computerweekly.com/news/365535265/Restaurants-hit-by-IT-problems-after-BlackCat-attack-on-supplier-NCR)) |
| 2023-09-07 13:54 ET → 09-08 05:19 ET | **Square** | A host-firewall policy change expanded into a much larger ruleset. Together with a DNS upgrade, this overloaded internal DNS. **The recovery tooling also depended on DNS** ([Square](https://developer.squareup.com/blog/incident-summary-2023-09-07/), [The Register](https://www.theregister.com/2023/09/11/square_dns/)) | About 15 h. US sellers could not take payments or log in. Remediation included **expanded offline payments** ([TechCrunch](https://techcrunch.com/2023/09/11/square-daylong-outage-dns-error/)) |
| 2024-03-15 | **McDonald's** global | A "third-party provider during a configuration change"† | Restaurants in several countries could not take orders; some closed ([Slashdot](https://it.slashdot.org/story/24/03/15/2033201/mcdonalds-it-systems-outage-shuts-some-restaurants-globally)) |
| 2024-07-19 04:09–05:27 UTC | **CrowdStrike** Falcon Channel File 291 | A content-validator bug let a template instance use a 21st input field while the sensor supplied 20. The result was an out-of-bounds read and a Windows crash ([CrowdStrike hub](https://www.crowdstrike.com/falcon-content-update-remediation-and-guidance-hub/), via [postmortems](https://github.com/icco/postmortems/blob/main/data/fccb0ca5-3db5-4481-98ba-af8f26528a93.md)) | About **8.5 M Windows devices**, including Windows POS, self-checkout and payment back-ends ([USENIX](https://www.usenix.org/publications/loginonline/consequences-compliance-crowdstrike-outage-19-july-2024), [Juniper](https://www.juniperresearch.com/resources/blog/crowdstrike-outage-the-impact-on-banks-and-payments/)). Fixing it needed hands-on Safe Mode or WinRE and BitLocker keys†. About 99% were back by July 29 |
| 2024-11 | **Blue Yonder** ransomware | SaaS supplier compromise ([breach-notes / AP](https://github.com/Vulnetix/breach-notes/blob/main/supply-chain/2024-11_sainsbury-s-blue-yonder.yaml)) | Starbucks (scheduling and payroll†), Morrisons and Sainsbury's supply-chain systems |
| 2025-02-26, 03-14 | **Square** | Not published | About 2–3 h and more than 6 h of card-processing disruption ([StatusGator](https://statusgator.com/services/square/outage-history?page=3), [IsDown](https://isdown.app/status/square/incidents/375949-payments-disruption)) |
| 2025-04-21 18:51 → 04-22 03:20 CEST | **Adyen** | Three DDoS waves ([Adyen](https://www.adyen.com/knowledge-hub/mitigating-a-ddos-april-2025)) | About 8.5 h. EU e-commerce **and in-person** payments failed intermittently ([Payments Dive](https://www.paymentsdive.com/news/adyen-hit-with-cyberattack-in-europe/746064/)) |
| 2025-04 | **M&S, Co-op, Harrods** | Scattered Spider / DragonForce. **Social engineering of the outsourced service desk** for password resets, then NTDS.dit theft ([breach-notes](https://github.com/Vulnetix/breach-notes/blob/main/ransomware/2025-04_marks-and-spencer.yaml), [The Hacker News](https://thehackernews.com/2025/07/four-arrested-in-440m-cyber-attack-on-marks-and-spencer-co-op-and-harrods.html)) | M&S contactless and click-and-collect were disrupted and online orders paused for weeks†. Combined losses were estimated at £270–440 M. Four arrests on 2025-07-10 |
| 2025-07 | **Toast** accounts | Restaurateurs reported account takeovers with changed credentials ([Flyght](https://www.whatisflyght.com/blog/burnt-toast-when-your-pos-becomes-your-biggest-vulnerability)) | Admin control of individual merchants |
| 2025-10-19 23:48 PDT → 10-20 afternoon | **AWS us-east-1** | A DynamoDB DNS Enactor/Planner race deleted all endpoint records; EC2 DWFM then hit congestive collapse ([The Register](https://www.theregister.com/2025/10/23/amazon_outage_postmortem/), [ThousandEyes](https://www.thousandeyes.com/blog/aws-outage-analysis-october-20-2025)) | About 15 h. **Toast restaurants reported "complete system failures"** ([NRN](https://www.nrn.com/restaurant-technology/the-aws-outage-left-many-restaurants-scrambling)); Lightspeed login down ([IsDown](https://isdown.app/integrations/lightspeed/lightspeed-retail-retail-pos)); Square about 2 h in 8 countries (track 02); McDonald's app and Venmo hit |
| 2025-10-29 | **Azure Front Door** | An inadvertent configuration change ([Gremlin](https://www.gremlin.com/blog/reliability-lessons-from-the-2025-microsoft-azure-front-door-outage)) | About 8 h. The Starbucks app could not take mobile orders or show gift-card balances. Costco and Kroger were also affected ([Sangfor](https://www.sangfor.com/blog/cloud-and-infrastructure/microsoft-azure-outage-oct-2025)) |
| 2025-11-18 11:20 UTC | **Cloudflare** | A ClickHouse permission change produced duplicate rows. The Bot Management feature file then exceeded the hard 200-feature limit, and `unwrap()` panicked in the FL2 proxy ([Cloudflare](https://blog.cloudflare.com/18-november-2025-outage/), [Hackaday](https://hackaday.com/2025/11/20/how-one-uncaught-rust-exception-took-out-cloudflare/)) | About 6 h of global 5xx errors, including e-commerce and banking front ends |
| 2025-12-01 (Cyber Monday) | **Shopify** | A bug in the login authentication flow | **POS logins failed** for about 5.5 h at peak ([CNBC](https://www.cnbc.com/2025/12/01/shopify-outage-cyber-monday-shopping.html), via track 02) |
| 2026 | **Toast**; **Clover** | RCAs rarely published | Toast: 10 incidents in 90 days, median about 50 min ([IsDown](https://isdown.app/status/toast)); deposit delays on 2026-09-17/18 ([status](https://status.toasttab.com/history)). Clover: "intermittent issues" ([American Banker](https://www.americanbanker.com/payments/news/some-clover-merchants-suffering-intermittent-issues)) |

### 2.2 Breaches and POS malware

- **The RAM-scraper era (2008–2019):**
  - Hannaford, Schnucks, Michaels, Neiman Marcus, P.F. Chang's, Staples, Kmart and Chipotle ([breach-notes dataset](https://github.com/Vulnetix/breach-notes)).
  - Target 2013: about 40 M cards. Attackers used stolen HVAC-vendor credentials and ran BlackPOS/Kaptoxa on the registers†.
  - Home Depot 2014: 56 M cards, via vendor credentials and malware on self-checkout†.
  - Wendy's 2016, via a third-party remote-access provider†. Wawa 2019, about 850 stores†. Oracle MICROS support-portal compromise in 2016†.
  - **Pattern:** vendor or remote-access credentials, then a flat network, then plaintext card data in POS memory.
- **The current era:**
  - EMV plus P2PE and semi-integrated terminals largely took card data out of POS memory.
  - Attackers moved to cloud admin account takeover (Toast 2025), vendor-SaaS ransomware (NCR 2023, Blue Yonder 2024, CDK Global 2024†), help-desk social engineering (M&S, Co-op) and e-commerce skimming.
  - NFC-blocking malware such as Prilex forces fallback to chip† — the terminal is still an attack surface.

### 2.3 Lessons

1. **Local authority keeps stores selling.** In-store Aloha kept transacting while NCR's cloud was encrypted. Cloud-only Square stopped for about 15 h except where offline was forced by hand.
2. **Detect degraded service, not just a lost link.** Per-dependency health checks should switch to local mode automatically (Square 2023).
3. **Configuration and control-plane changes are the top trigger** (Square firewall, AWS DNS automation, AFD, Cloudflare feature file, CrowdStrike content, McDonald's). Treat configuration as code: schema and size validation, canaries, staged propagation, and parsers that fall back to last-known-good instead of panicking.
4. **Recovery tooling must not share fate** with production (Square's depended on DNS). Keep break-glass access and operator DNS and auth separate.
5. **Retry storms and reconnection herds** (Square 2017, AWS DWFM). 100k+ devices reconnecting effectively DDoS us. Use jittered backoff, retry budgets, admission control, and priority lanes: payments, then orders, then telemetry.
6. **Concentration risk:** one region, one CDN or WAF, one login flow (Shopify), one acquirer (Adyen). Make the payment path multi-region with multiple acquirers, and make staff sign-in work offline.
7. **Endpoint-agent monoculture (CrowdStrike).** Kernel content pushed globally in 78 minutes needed hands-on fixes per device. Lanes should be locked-down Android or Linux appliances with A/B rollback and no third-party kernel agents.
8. **Vendor ransomware and social engineering.** Isolate cells, keep immutable backups, be able to rebuild in a clean room (NCR had to). Require phishing-resistant MFA and help-desk verification.
9. **Merchant account takeover.** Passkeys, holds on payout changes, and notifications to all owners.
10. **Keep the POS out of the card-data path** (P2PE, semi-integrated). Vendor remote access must be just-in-time and recorded.

---

## 3. Local-first and sync technology

### 3.1 What POS data needs

| Data | Required semantics |
|---|---|
| **Orders and checks** | Many devices touch one check. Needs ownership or locking (CAPS-style), deterministic totals, ordered kitchen "fire" events, and auditable voids |
| **Payments** | Immutable facts with exactly-once effects, keyed by idempotency key. **Never merged** |
| **Inventory counters** | Commutative deltas, stock counts treated as snapshots, optional non-negativity or escrow |
| **Menus, catalog, prices** | Cloud-authored and versioned. Large at retail scale (100k+ SKUs) |
| **Receipt and fiscal numbering** | Gapless per-register sequences and hash chains in many countries: DE TSE, FR NF525, ES Verifactu (track 06) |

The tools below were evaluated against these needs.

### 3.2 Evaluation

| Technology | Type | Licence | Maturity (Sept 2026) | Platforms | Conflict model | Offline writes | LAN/P2P | POS fit |
|---|---|---|---|---|---|---|---|---|
| [Automerge 3](https://github.com/automerge/automerge) | JSON CRDT library | MIT† | JS stable; Rust core; v3 uses ~10× less memory | JS/WASM, Rust, C FFI | Operation-based CRDT with full history | Yes | Pluggable transport | Good for collaborative documents (menu or layout drafts). Poor for ledgers and relational reporting |
| [Yjs](https://github.com/yjs/yjs) / [Loro](https://github.com/loro-dev/loro) | Sequence and tree CRDTs | MIT | Yjs very mature; Loro 1.0 | JS plus Rust cores with many bindings | YATA or Fugue text; LWW maps | Yes | Providers or pluggable | Built for text and documents, not transactions |
| [ElectricSQL 1.x](https://github.com/electric-sql/electric) | Postgres **read-path** sync (Shapes over HTTP/CDN) | Apache-2.0 | 1.0 since March 2025 | Any HTTP client; TS client; PGlite | Server-authoritative. Writes go through your API; clients rebase optimistic state ([writes guide](https://github.com/electric-sql/electric/blob/main/website/docs/sync/guides/writes.md)) | Via app patterns | **No** | Good for fanning cloud data out to web and back office. Not a store-LAN answer |
| [PowerSync](https://github.com/powersync-ja/powersync-service) | Postgres, Mongo, MySQL or SQL Server ↔ SQLite | Service is **FSL-1.1-ALv2** (Apache after 2 years); SDKs open | Production; self-hostable | JS (RN, web, Node, Electron, Capacitor), Flutter, Kotlin MP, Swift, .NET, Rust native ([SDKs](https://github.com/powersync-ja/powersync-js)) | Server-authoritative. A client CRUD queue feeds your backend via `uploadData`; writes are checkpointed | Yes (queued) | **No** | Best device↔cloud engine. No hub or peer mode |
| [Zero](https://github.com/rocicorp/mono) (Rocicorp) | Query-driven sync with IVM (incremental view maintenance) | Apache-2.0 | 1.x (npm 1.9.0) | **TypeScript only** | Server-authoritative mutators | **No.** "Zero doesn't support offline writes"; writes are rejected while disconnected ([docs](https://github.com/rocicorp/zero-docs/blob/main/contents/docs/connection.mdx)) | No | **Disqualified** |
| Replicache | Client sync library | Open-sourced† | Maintenance† | JS | **Server reconciliation**: mutators replay on the server and clients rebase | Yes | No | **The pattern is ideal** for POS business rules |
| [Triplit](https://github.com/aspen-cloud/triplit) / [InstantDB](https://github.com/instantdb/instant) | JS sync DB / backend-as-a-service | **AGPL-3.0** / Apache-2.0 | Active | JS, RN | Property merge / server-authoritative | Yes / cache only | No | AGPL friction / cloud-dependent |
| **Ditto** | P2P mesh database | **Proprietary** | Production. Vendor-cited users: Chick-fil-A, Japan Airlines, Alaska, Delta, US Air Force ([alt-clouds](https://github.com/datum-cloud/awesome-alt-clouds/blob/main/src/content/clouds/ditto.mdx)) | iOS, Android, web, Flutter, RN, .NET, C++; Rust core ([safer_ffi](https://github.com/getditto/safer_ffi)) | CRDT documents | Yes | **BLE, P2P Wi-Fi (Wi-Fi Aware), LAN, cloud relay** ([wifi-aware](https://github.com/getditto/wifi-aware-checker)) | Best off-the-shelf LAN mesh. Its [POS/KDS demo](https://github.com/getditto/demoapp-pos-kds) uses "most-advanced state wins" for order status. Carries lock-in and pricing risk |
| Couchbase Lite plus Sync Gateway | Document DB with replication | LiteCore is **BSL 1.1** ([repo](https://github.com/couchbase/couchbase-lite-core)); Community/Enterprise split† | Mature (10+ years) | iOS, Android, .NET, Java, C | Revision trees plus custom resolvers | Yes | P2P replication (Enterprise)† | Proven in field and retail. BSL plus enterprise pricing |
| [RxDB](https://github.com/pubkey/rxdb) / [WatermelonDB](https://github.com/Nozbe/WatermelonDB) | JS databases | Apache plus premium / MIT | Mature | JS, RN, Electron | Custom conflict handler / pull-push LWW† | Yes | RxDB has a WebRTC plugin | JS or RN only |
| [cr-sqlite](https://github.com/vlcn-io/cr-sqlite) | SQLite CRDT extension | MIT | Pre-1.0; last push 2026-08 | Any SQLite binding, WASM | Column LWW plus causal-length deletes. **Inserts are 2.5× slower** | Yes | Pluggable | Elegant, but risky maturity for money data |
| [libSQL](https://github.com/tursodatabase/libsql) → [Turso](https://github.com/tursodatabase/turso) | SQLite fork → Rust rewrite | MIT | libSQL in maintenance; Turso "production-ready but pre-1.0" with MVCC, CDC and offline bidirectional sync | Many, plus WASM | libSQL single-writer; Turso engine-level | Turso yes | No | Watch Turso |
| SQLite session extension ([docs](https://www.sqlite.org/sessionintro.html)) | Changesets and patchsets | Public domain | Very mature (since 3.13, 2016†) | Everywhere SQLite runs | Row changesets. An **application conflict callback** handles DATA, NOTFOUND, CONFLICT, CONSTRAINT and FOREIGN_KEY; needs primary keys† | Yes | Pluggable | A strong low-level building block for replicating hub state tables |
| [LiveStore](https://github.com/livestorejs/livestore) | Event-sourced SQLite | Apache-2.0 | Active | Web, Expo, Node | Event log with rebase and custom merge | Yes | Provider-based | The architecture closest to what POS needs. JS-only |

### 3.3 The MongoDB Realm / Atlas Device Sync deprecation: vendor-risk lessons

- **What happened.** MongoDB deprecated Atlas Device Sync and the Realm SDKs in **September 2024**, with **end-of-life on 2025-09-30** ([MongoDB docs](https://github.com/mongodb/docs-app-services/blob/master/source/sync/device-sync-deprecation.txt), [realm-swift](https://github.com/realm/realm-swift)). It pointed customers to Ditto, PowerSync, ObjectBox, AppSync and others.
- **Lessons.**
  1. A sync engine is load-bearing: its storage, conflict semantics and wire protocol leak into application code.
  2. Vendor incentives change, even at a well-funded public company.
  3. Relicensing is a pattern. CockroachDB now needs a production licence key and telemetry for free use ([LICENSE](https://github.com/cockroachdb/cockroach/blob/master/LICENSE)); Redpanda is BSL ([bsl.md](https://github.com/redpanda-data/redpanda/blob/dev/licenses/bsl.md)). Synadia's attempt to move NATS to BSL was resolved on 2025-05-01, keeping it Apache-2.0 in CNCF ([TNS mirror](https://github.com/rocksun/mwblog/blob/master/microservices/cncf-and-synadia-reach-an-agreement-on-nats/cncf-and-synadia-reach-an-agreement-on-nats.origin.md)).
- **Policy.** Own the protocol and data model of anything on the sell path. Anything bought sits behind our interface with an escrow or exit plan.

### 3.4 Event sourcing, CQRS and conflict resolution for POS

**Model.**
- Every user intent is a **command**. It runs as a deterministic, versioned **mutator** in the shared core and emits **events** (the unit of sync).
- The read side is SQLite **projections** (materialized tables), rebuilt locally.
- The authority (store hub, or the cloud when no hub exists) **re-executes** mutators in canonical order. Clients then **rebase** their pending mutations on the authoritative stream. This is the Replicache/Zero server-reconciliation pattern, extended to work offline at two tiers.
- It handles business-rule conflicts that a CRDT cannot, such as "item 86'd since you added it" or "check closed on another device".

| Aggregate | Authority | Merge semantics | Offline rule |
|---|---|---|---|
| **Check/order** | Single owner per check, with a **lease** (CAPS pattern); transferred explicitly | Line items are an add-wins set keyed by UUID. Status is a monotonic state machine (open → fired → paid → closed; voids are compensating events). Totals are always **derived** from a pinned price-book version | Island devices may edit only checks they own. Taking over another device's check needs a manager override, logged and reconciled on reconnect |
| **Payment** | The initiating device plus the terminal or PSP | **Immutable**. Idempotency key minted **before** calling the terminal; states: initiated → authorized / declined / offline-pending → captured → refunded | Duplicate-payment detection on merge; the policy auto-voids or refunds the duplicate. Reconcile against PSP settlement files |
| **Inventory** | Ledger | Deltas (sale −1, receive +n, waste −k) are commutative (PN-counter). A **count** event sets a baseline at a hybrid-logical-clock (HLC) time T, and later deltas apply on top | Negative stock allowed but flagged. For scarce items, per-device **escrow quotas** (a bounded counter) where strictness matters. "86" flags are LWW by HLC and broadcast with priority |
| **Menu, catalog, pricing** | Cloud | Immutable versioned snapshots plus a local override layer | Apply atomically. Every line records the snapshot version so repricing is deterministic |
| **Drawer, shift, time clock** | Per-drawer owner | Append-only (paid-in/out, drops, counts, punches) | Conflicts go through rules and are surfaced to a manager |
| **Numbering and fiscal** | Per register | Gapless counter plus hash chain for each register as a fiscal device | Matches single-writer-per-register. Never allocate from a shared pool offline |

**Mechanics.**
- **UUIDv7** IDs (RFC 9562†) are minted on the device. D365 Commerce's offline/online duplicate IDs needed "significant manual data fixing" (track 02).
- **Hybrid logical clocks** order events despite clock skew, with the hub as NTP source.
- **End of day (Z-report) is the compaction boundary:** snapshot projections, archive the log to the cloud, prune locally.
- Event schemas are versioned with upcasters, and clients must tolerate unknown event types, because stores run N-1 and N-2 builds during rollouts.

---

## 4. Edge and on-prem patterns at large chains

- **Chick-fil-A** (verified from EdgeCase 2023 talk notes, [RobKenis](https://github.com/RobKenis/edgecase-2023/blob/main/docs/wednesday/2-chick-fil-a.md)):
  - About **3,000 k3s clusters**, one per restaurant, each on **3 Intel NUCs**, with dual routers and separate switches.
  - Workloads: MongoDB as the edge datastore; **MQTT** for kitchen IoT; POS and mobile-order data; a cloud forecasting engine that pushes predictions to the edge.
  - Deployments are GitOps from one repo, rolled out to selected stores **based on error rates** ([gitops](https://github.com/chick-fil-a/gitops)). Bare-metal self-enrolment through AWS SSM ([hoovesup](https://github.com/chick-fil-a/hoovesup)).
  - Vector filters logs at the edge because of bandwidth.
  - Stated principles: "Build things as close to the action as required and no closer", and **recoverability over high availability** (nodes are cattle; zero-touch reprovisioning).
- **McDonald's.** Announced in December 2023 that it would deploy **Google Distributed Cloud** hardware and software to thousands of restaurants from 2024, for local apps and AI that tolerate intermittent connectivity†. Its March 2024 global outage (§2) shows restaurant compute still depended on central configuration.
- **Walmart:** a hybrid "triplet" of public and private cloud plus about 10,000 edge nodes on its Kubernetes platform†.
- **Target:** Kubernetes in every store ("Unimatrix")†.
- **Starbucks:** Azure for digital channels and IoT†. The AFD outage broke mobile ordering (verified, §2).
- **Yum! / Taco Bell:** the "Byte by Yum!" platform, an NVIDIA voice-AI partnership, then a drive-thru retreat† (track 01).
- **Platforms in use:** k3s, Talos, SUSE Edge, Avassa, ZEDEDA/EVE-OS, Azure Local, Outposts, GDC connected†.
- **Why:** trading through WAN loss, under-100 ms kitchen and KDS loops, bandwidth for video and AI, equipment integration, data locality.
- **Cost:** hardware lifecycle, remote hands, patch cadence (CrowdStrike-class risk), limited observability bandwidth.
- **Lesson.** Most merchants cannot run Kubernetes. Offer a hub that runs on a single appliance **or** as a container on a chain's existing edge cluster.

---

## 5. Hardware integration

### 5.1 Printing

- **ESC/POS** (Epson's de-facto standard) is the lingua franca: `ESC @` init, `GS V` cut, `ESC p` drawer pulse, `GS v 0` raster, `GS ( k` QR, `DLE EOT`/`GS a` status. Network printers take raw TCP on port 9100 with **no authentication or TLS**.
- **Code pages break global scale.** Arabic, Hebrew, Thai, Indic and CJK need text shaping. **Render receipts to raster in the core** (HarfBuzz-class) and keep native text mode as a speed optimisation.
- **Star Micronics:**
  - StarPRNT and Line Mode command sets.
  - **StarXpand SDK (StarIO10)** covers iOS, Android, Windows, RN and Web over LAN, Bluetooth, BLE and USB, including drawers, readers, displays and status callbacks ([Android SDK](https://github.com/star-micronics/StarXpand-SDK-Android)). Its web version **uses WebUSB, Chrome/Edge only** ([star-io10-web](https://github.com/star-micronics/star-io10-web)).
  - **CloudPRNT**: the printer polls the server over HTTP(S) using emulation-agnostic Star Document Markup ([SDK](https://github.com/star-micronics/cloudprnt-sdk)). **WebPRNT** prints from the browser ([SDK](https://github.com/star-micronics/starwebprnt-sdk)).
- **Epson:** ePOS SDK for iOS, Android and JS, where the browser talks directly to the printer IP with no driver ([wrapper](https://github.com/rubenruvalcabac/epson-epos-sdk-react)). ePOS-Print XML over HTTP(S) on ports 8008/8043†, and Server Direct Print polling†.
- **Integrated Android printers** (e.g., Sunmi PrinterX) use vendor AIDL services ([sample](https://github.com/shangmisunmi/SunmiPrinterXSample)).
- **Operations.** Kitchen printers fail often (paper, heat, grease). Make KDS primary with the printer as backup. Run a hub spooler with durable jobs, retries, **failover routing** to another printer, and paper and cover alerts.

### 5.2 Device standards

- **UnifiedPOS (UPOS)** defines the device-category semantics: claim / enable / events for POSPrinter, CashDrawer, Scanner, Scale, LineDisplay, MSR, PINPad and more.
  - **OPOS** is the Windows COM implementation, **JavaPOS** the Java one, and POS for .NET is legacy.
  - `Windows.Devices.PointOfService` covers scanners, MSRs, printers, drawers and line displays, including HID-POS scanners†.
- **USB HID POS usage pages:** 0x8C barcode scanner, 0x8D scale, 0x8E MSR†. This gives symbology identifiers and raw data without keyboard-wedge hacks.

### 5.3 Web hardware APIs, per MDN browser-compat-data

| API | Chrome/Edge desktop | Chrome Android | Firefox | Safari (macOS and **all iOS browsers**) | Android WebView |
|---|---|---|---|---|---|
| [WebUSB](https://github.com/mdn/browser-compat-data/blob/main/api/USB.json) | 61+ | Yes | No | No | No ("exposes but does not support") |
| [Web Serial](https://github.com/mdn/browser-compat-data/blob/main/api/Serial.json) | 89+ | **148+** (138–147 Bluetooth RFCOMM only) | **151+ desktop**; not on Android | No | No |
| [WebHID](https://github.com/mdn/browser-compat-data/blob/main/api/HID.json) | 89+ | No | No | No | No |
| [Web Bluetooth](https://github.com/mdn/browser-compat-data/blob/main/api/Bluetooth.json) | 70+ (off by default on Linux) | 56+ | No | No | No |
| [Web NFC](https://github.com/mdn/browser-compat-data/blob/main/api/NDEFReader.json) | No | 89+ | No | No | — |
| [Background Sync](https://github.com/mdn/browser-compat-data/blob/main/api/SyncManager.json) | 49+ | Yes | No | No | No |
| [OPFS sync handle](https://github.com/mdn/browser-compat-data/blob/main/api/FileSystemSyncAccessHandle.json) | 102+ | 109+ | 111+ | 15.2+ | Yes |
| [Local Network Access](https://github.com/mdn/browser-compat-data/blob/main/api/Permissions.json) permission | 142+ (split into `local-network` and `loopback-network` in 145) | — | No | No | — |

**Implications.**
- On iOS and iPadOS a PWA has **no** device APIs.
- WebUSB on Windows needs WinUSB drivers (per Star's README), and protected classes (HID, mass storage) are blocked.
- HTTPS apps calling `http://192.168.x.x` printers hit mixed-content and Chrome's **Local Network Access prompts** ([mixed-content](https://github.com/mdn/browser-compat-data/blob/main/http/mixed-content.json)).
- Browsers **cannot listen on sockets or do mDNS**, so web clients can never be LAN peers or hubs.

### 5.4 Peripherals

- **Cash drawers.** Usually fired from the printer's DK port (RJ-11/12, 24 V) with `ESC p`, with open status read via the printer. Log every open with a reason code.
- **Scales.**
  - Wire formats: RS-232 or USB-COM protocols (Mettler-Toledo 8213/8217-style, NCI)†, scanner-scales, OPOS/JavaPOS or HID-POS.
  - **Legal for trade:** NIST Handbook 44 plus **NTEP** in the US; OIML R76, NAWI 2014/31/EU and WELMEC 7.2 "legally relevant software" separation in the EU†.
  - Required: stable weight only, tare and zero, net weight shown to the customer, manual weights flagged†.
  - **Isolate and version-freeze the weighing module**, because changes can trigger re-evaluation.
- **Barcode scanners.** **HID keyboard wedge** is brittle: layout bugs, dropped characters, no symbology ID. Prefer **serial/CDC-ACM or HID-POS**; on Android, vendor intents (DataWedge, Sunmi)†. **GS1 Sunrise 2027** brings 2D barcodes (DataMatrix, Digital Link QR) to US POS†, so parse GS1 Application Identifiers in the core.
- **Label printers.** ZPL II (stored templates `^DF`/`^XF`; RFID encoding), plus EPL, TSPL, DPL and CPCL†.
- **Customer-facing displays.** Android `Presentation`, an iPad external scene, a second monitor, or VFD pole displays. Better: a **separate CFD client** synced via the hub, so cheap tablets can serve.
- **KDS bump bars.** Programmable USB HID keyboards, so the KDS must be fully keyboard-operable.
- **RFID.** RAIN UHF Gen2v2 via **LLRP** or vendor REST/MQTT†, for inventory accuracy, self-checkout and EAS.

### 5.5 Payment terminals and SDKs

- **Semi-integrated only.** The terminal owns card data and the POS sends the amount and gets the result, keeping us out of PCI scope. Never let the POS touch card data.
- **Connection paths:**
  - cloud-driven (Stripe smart readers, Adyen cloud Terminal API, Square Terminal API, Clover);
  - **local LAN** (Adyen local Terminal API, nexo-based†), which survives WAN loss;
  - on-device payment apps (PAX, Sunmi, Clover);
  - Bluetooth readers;
  - SoftPOS / Tap to Pay (PCI MPoC†).
- **Offline.** Stripe Terminal exposes `OfflineBehavior {PREFER_ONLINE, REQUIRE_ONLINE, FORCE_OFFLINE}` ([stripe-terminal-android](https://github.com/stripe/stripe-terminal-android)); Square's SDK allows 1,000 payments or 24 h. Offline payments must be stored inside the certified terminal or SDK, never in our database.
- **Design.** A `PaymentDevice` capability that prefers the LAN or on-device path, falls back to cloud, then to store-and-forward under a merchant risk policy (caps, BIN and card-type rules, live exposure). Route across **multiple PSPs and acquirers** with failover (Adyen 2025, Square 2023).

### 5.6 Device classes

- **Android all-in-ones** (Sunmi, PAX, iMin, Elo, Zebra): cheapest, with integrated printer, CFD and scanner; PAX models are also PCI PTS payment terminals. Risks:
  - vendor OS lag and old Android versions;
  - **many ship without Google Mobile Services**, so no Play Integrity and no FCM push;
  - supply-chain scrutiny (FBI search of PAX's US office, 2021†).
- **iPad:** consistent hardware and long OS support. Peripherals must be MFi, Bluetooth or network. Tap to Pay is iPhone-only†. Kiosk via Single App Mode or ASAM.
- **Windows:** legacy peripherals and OPOS for enterprise lanes, but the largest attack and update risk (CrowdStrike). If needed, use IoT Enterprise LTSC with Assigned Access and no third-party kernel agents†.

### 5.7 Fleet management and OTA updates

- **MDM:** Android Enterprise dedicated devices (lock-task, zero-touch or QR provisioning, private app tracks, update freeze windows†); **Esper** ([dev docs](https://github.com/esper-io/dev-docs)); Knox E-FOTA†; GMS-less vendor stores (PAXSTORE†); ABM plus Jamf, Kandji or Mosyle for iPad; Intune for Windows.
- **We still need our own cross-MDM device agent** for health, app channel, configuration and remote diagnostics.
- **Rollouts:**
  - rings: internal → pilot stores → 1% → 5% → 25% → 100%;
  - automatic halt on SLO regressions (crashes, payment success, sync lag, print failures), as Chick-fil-A gates on error rates;
  - maintenance windows only, with **per-store cohort updates**;
  - wire-protocol compatibility back to **N-2**.

### 5.8 Recommended hardware abstraction

1. **Capability interfaces** in the core, modelled on UPOS semantics but asynchronous and network-aware: Printer, Drawer, Scanner, Scale, CustomerDisplay, PaymentDevice, LabelPrinter, BumpBar, RFIDReader.
2. **Protocol encoders in the Rust core:** ESC/POS, StarPRNT, ZPL, scale protocols, raster rendering. Test against **golden images**.
3. **Transports per platform:** TCP, USB, serial, BLE, vendor SDK bridges (StarIO10, Epson ePOS, Sunmi AIDL, PAX), and an OPOS/JavaPOS bridge on Windows.
4. **Device-independent document DSL** (in the spirit of Star Document Markup or ePOS XML), compiled against the capability profile of each printer.
5. **Durable spooler on the hub:** idempotent job IDs, retries, routing, status.
6. **Discovery and registry:** mDNS, USB enumeration, vendor discovery, with cloud-managed configuration.
7. **Virtual devices and simulators in CI**, plus remote diagnostics.

---

## 6. Client platform evaluation for POS

**What decides the choice for POS:**
- sub-50 ms input response on cheap hardware;
- durable local SQL;
- **native payment and peripheral SDKs** (Stripe, Adyen, Square, Tap to Pay, Star, Epson and Sunmi are native first);
- the ability to **listen on the LAN** (hub or peer role, mDNS);
- kiosk and background execution;
- a ten-year horizon.

| Stack | UI perf | Offline storage | Hardware access | LAN hub/peer | Velocity | Long-term risk | Verdict |
|---|---|---|---|---|---|---|---|
| Native (Kotlin/Compose; Swift/SwiftUI) | 5 | 5 | 5 | 5 | 3 (two UIs; AI agents narrow the gap, per Shopify 2026) | 5 | Highest quality |
| React Native (New Architecture default since 0.76, [2024-10-23](https://github.com/facebook/react-native/releases/tag/v0.76.0)) | 4 | 4 (JSI SQLite) | 3–4 (needs native modules) | 3 | 4 | 3 (its flagship POS user is leaving) | Viable, declining |
| Flutter | 4–5 | 4 | 3 (platform channels) | 3–4 | 4 | 3–4 | Viable |
| Kotlin Multiplatform plus native UI | 5 | 5 | 5 | 5 | 4 | 4 | Strong |
| Compose Multiplatform (1.8.0, 2025-05-06; JetBrains declared iOS stable†) | 4 | 5 | 4 | 4 | 4–5 | 3–4 | Strong alternative |
| Electron | 3 | 4 | 4 (Chromium APIs plus Node) | 5 | 5 | 4 | Windows lanes only |
| Tauri 2 ([stable 2024-10-02](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.0.0)) | 4 | 5 (Rust) | 4–5 (Rust) | 5 | 4 | 3–4 (WebView variance) | Windows/Linux lanes, KDS, CFD |
| PWA | 3 | 2–3 (evictable; iOS limits) | 1–3 (Chromium-only; **none on iOS**) | **1** | 5 | 3 | Back office; KDS/CFD **served by the hub** |
| .NET MAUI | 3–4 | 4 | 4 (OPOS) | 4 | 3 | 3 | Niche |

**Key judgement.** UI is the cheaper part to duplicate; business logic must be **written once**, because divergent tax, rounding or discount logic across platforms is a classic POS defect. High-reliability sync products converge on a **Rust core with native shells**: Ditto (`safer_ffi`), Automerge, Loro, Turso, and PowerSync's 2025 Rust SDK. One Rust core compiles to Android, iOS, Windows, the Linux hub and **WASM**.

---

## 7. Backend and platform patterns

### 7.1 Tenancy and cells

- **Tenancy.** Pool SMB tenants with row-level isolation (`tenant_id` everywhere, plus Postgres RLS). **Silo** large enterprise brands into dedicated cells.
- **Cells.** Each cell is a full stack slice (API, databases, queues, workers) serving N tenants. Add:
  - a thin, highly available **cell router** whose mapping is cached on hubs;
  - shuffle-sharded shared services;
  - per-cell deploys, configuration pushes and error budgets;
  - cell-evacuation tooling.
  - (AWS cell-based guidance; Slack's cellular migration†.)
- Because stores run island or hub mode anyway, cells protect **cloud** features: online ordering, payment orchestration, reporting, integrations.

### 7.2 Multi-region and data residency

- **Region-pinned cells:** EU cells in the EU, India cells in India (RBI payment-data localisation†), and so on.
- The global control plane holds minimal PII. Card data exists only as PSP tokens.
- **Fiscal adapters** are part of the commit path and run per register offline. Examples: DE KassenSichV TSE (all registers registered via ELSTER since 2025), FR NF525 (third-party certification mandatory since 2026-03-01), ES Verifactu (delayed to 2027-01-01 / 2027-07-01), IT RT, AT RKSV, BR NFC-e, SA ZATCA ([track 06](06-verticals-global.md); [Verifactu](https://github.com/Notifycal/static-landing/blob/main/src/content/blog/verifactu-pospuesto-2027.es.md)). Rules ship as data.

### 7.3 Datastores

| Option | Strength | Concern | Use |
|---|---|---|---|
| **Postgres** (Aurora, AlloyDB, Cloud SQL or self-run; Citus) | Ecosystem, RLS, logical replication, CDC | Single-writer per cluster | **System of record per cell** |
| CockroachDB | Geo-distributed SQL | **Production needs a licence key; telemetry is mandatory** on free use | Avoid for the core (vendor risk) |
| Spanner | TrueTime, external consistency | GCP lock-in and cost | Optional small global control plane |
| TiDB / YugabyteDB | Apache-2.0 distributed SQL (MySQL- or PG-compatible)† | Operational complexity | Alternatives for the control plane |
| Aurora DSQL | Serverless multi-region active-active (GA 2025†) | New; PG subset | Watch |

**Recommendation.** Cells plus region pinning remove the need for globally distributed SQL on the transactional path. Use Postgres per cell with a cross-region standby, and put only the router and identity directory in a globally replicated store.

### 7.4 Eventing, outbox and idempotency

- **Store edge: NATS JetStream** (Apache-2.0, CNCF, runs even on a Raspberry Pi, [nats-server](https://github.com/nats-io/nats-server)). A **leaf node** on each hub store-and-forwards to the cloud via durable streams.
- **Core: Kafka 4.x**, KRaft only with ZooKeeper removed ([docs](https://github.com/apache/kafka/blob/trunk/docs/operations/kraft.md)). Redpanda is Kafka-compatible but **BSL 1.1**: no offering it as a streaming service; converts to Apache-2.0 after 4 years.
- **Transactional outbox** in every service (outbox row in the same DB transaction, then CDC or relay) plus an **inbox/dedupe table** on consumers. This gives exactly-once *effects*.
- **Stripe-style idempotency keys:** one per mutating request; the stored response, including errors, is replayed; reusing a key with different parameters is rejected†.
- For POS, the **operation UUIDv7 minted on the device is the idempotency key end to end**: device → hub → cloud → PSP → webhook. Double charges and duplicate orders become structurally impossible, which also avoids the D365 duplicate-ID class of bug.

### 7.5 API design quality

| | Square | Toast | Shopify | Stripe |
|---|---|---|---|---|
| Style | REST/JSON with SDKs; GraphQL in beta† | REST, **partner-gated**† | **GraphQL Admin API**; REST legacy since 2024-10; new public apps GraphQL-only since 2025-04† | REST (form-encoded), expandable objects |
| Versioning | `Square-Version` date header† | Per-API path versions† | Quarterly `YYYY-MM`, about 12-month support† | Account-pinned dates; named twice-yearly majors (e.g., `2024-09-30.acacia`)† |
| Idempotency | `idempotency_key` on create endpoints† | Not verified | Partial† | Gold standard |
| Webhooks | HMAC-signed† | For partners† | HTTPS, EventBridge or Pub/Sub; HMAC† | Signed; retried up to about 3 days; event destinations† |
| Access | Self-serve plus App Marketplace | Partner approval; Toast Payments required† | Open, with App Store review | Open |
| POS extension | App switching / Point of Sale API† | None public† | **POS UI extensions** (below) | Terminal SDKs |

**Recommendation.** REST/JSON with OpenAPI 3.1, cursor pagination and `expand`, mandatory idempotency on writes, and Stripe-style date versioning. Add **Standard Webhooks** signatures† plus a **replayable event feed** (a cursor-based pull API), so integrators never rely on webhook delivery alone. GraphQL is optional for read-heavy dashboards; use gRPC internally. **No partner gating for data access**: that is Toast's weakness and our openness advantage.

### 7.6 Extensibility

- **Shopify POS UI extensions:** **32 render targets** (home tiles and modals; post-purchase, return and exchange actions; product, order, customer and register details; receipt header and footer blocks). They also include **cart and payment validation "interceptors"** and a session-long background target `pos.app.ready.data` ([targets](https://github.com/Shopify/ui-extensions/blob/2026-10-rc/packages/ui-extensions/src/surfaces/point-of-sale/extension-targets.ts)). Bundles run offline. Shopify Functions run as limited WASM†.
- **Others:** Square has an open App Marketplace†; Toast gates partners and locks in its processor†; Clover runs Android apps on device with revenue share†.
- **WASM sandboxing:** Extism offers capability-based grants (no filesystem or network unless granted), memory limits and timeouts across 14 host languages ([Extism](https://github.com/extism/extism)). Wasmtime with WASI 0.2 components is the alternative†.
- **Our design, in three tiers:**
  1. **UI extensions:** sandboxed web or remote-DOM with a capability manifest.
  2. **Logic hooks** (pricing, discounts, validations): **deterministic WASM, time-boxed (≤5 ms), no network at checkout**. They run **offline on the device**, are version-pinned per store, and fall back to "skip" or "block" as declared.
  3. **Cloud webhooks and APIs.**

### 7.7 Data export, ownership and portability

- The **EU Data Act** has applied since 2025-09-12. Switching charges must be phased out by 2027-01-12†.
- Offer:
  - continuous CDC into the merchant's warehouse as open tables (Iceberg or Parquet);
  - a full event-log export;
  - documented schemas;
  - no export fees;
  - no processor lock-in;
  - an open hardware list;
  - portable card tokens via network tokens or PSP migration support.

### 7.8 Observability and performance budgets

- **OpenTelemetry** (traces, metrics, logs), with W3C trace context carried device → hub → cloud → PSP → webhook.
- On-device ring buffer and **flight recorder**; edge filtering and sampling (Chick-fil-A's Vector pattern); OTel Android and Swift SDKs for real-user monitoring†.
- **The primary SLI is "can sell", measured on the device**, not cloud uptime.
- **Proposed budgets** on the lowest supported device (2 GB RAM Android):

| Metric | Budget |
|---|---|
| Tap to feedback | p95 < 50 ms, p99 < 100 ms |
| Cart render after item add | < 16 ms |
| Search over 100k SKUs | < 50 ms |
| Order fire → KDS paint (LAN) | p95 < 300 ms |
| Print start | < 500 ms |
| Cold start to sell-ready | < 3 s |
| **Automatic** switch to degraded mode | < 2 s, no user action |
| Hub → cloud sync lag (online) | p95 < 5 s |
| Resident memory | < 300 MB |

---

## 8. Security engineering

- **Device identity.** Each device generates a keypair in hardware (Android Keystore/StrongBox, Secure Enclave, TPM 2.0). Enrolment is by attestation: Android Key Attestation (works **without GMS**), Play Integrity where GMS exists, Apple App Attest or TPM†. Our device CA issues **short-lived** (24–72 h) client certificates.
- **mTLS everywhere** (device↔hub, hub↔cloud, services†). **Treat the store LAN as hostile** (guest Wi-Fi, IoT, the Target/HVAC lesson). Put port-9100 printers on an isolated VLAN and prefer HTTPS printing (CloudPRNT, ePOS HTTPS).
- **Zero standing access.** No site VPNs. Support sessions are brokered, just-in-time and recorded. Help desks follow identity-verification playbooks (M&S, Co-op).
- **Secrets on devices.** No long-lived API keys; OAuth tokens are bound to the device certificate (mTLS or DPoP†). **Offline staff authorization** uses cloud-signed role bundles with expiry, PIN hashes sealed with a hardware pepper and rate-limited, and badges or passkeys for managers. Every offline override is logged for review.
- **Data at rest.** SQLCipher or file encryption with keystore-wrapped keys, minimal PII, **never a PAN**.
- **Compliance baseline:**
  - PCI DSS v4.0.1 (future-dated requirements effective 2025-03-31†);
  - PCI Secure Software Standard (PA-DSS retired†); P2PE; MPoC for SoftPOS†;
  - SOC 2 Type II; ISO/IEC 27001:2022 (transition deadline 2025-10-31†);
  - the **EU Cyber Resilience Act**: exploited-vulnerability and incident reporting has been **live since 2026-09-11** (24 h early warning, 72 h notification via the ENISA single reporting platform); full obligations from **2027-12-11** ([Espressif summary](https://github.com/espressif/developer-portal/blob/main/content/blog/2026/09/esp32-cra-obligations-and-deadlines/index.md)). Our hardware and software are in scope.
- **Secure update pipeline.** SLSA-style provenance, Sigstore signing and SBOMs†. **TUF** metadata for device updates (defends against key compromise, rollback, freeze and mix-and-match)†. Reproducible core builds. A/B partitions or last-known-good app with **automatic rollback after failed boots or health checks**, and a minimal "safe-mode POS" that still sells.
- **CrowdStrike and Cloudflare lessons:**
  1. Configuration and content use the **same staged pipeline** as code.
  2. Customers control update rings (CrowdStrike added staged content and customer control afterwards).
  3. Parsers are bounds-checked and **non-panicking**, with last-known-good fallback.
  4. No third-party kernel code on lanes.
  5. Remote recovery works even when the app cannot start.
- **Merchant accounts.** Passkeys required for owners and admins. Step-up authentication, a **cooling-off hold** and all-owner notification for payout changes. Anomaly detection on logins and payouts (Toast 2025).

---

## 9. AI and agentic integration patterns

### 9.1 MCP servers from commerce companies

| Company | Official MCP | Notes |
|---|---|---|
| **Square** | Yes. **Remote** `https://mcp.squareup.com/sse` (OAuth) or local `npx` | Three generic tools (`get_service_info`, `get_type_info`, `make_api_request`) over about 40 services, plus a `DISALLOW_WRITES` read-only switch. Repo created 2025-04 ([repo](https://github.com/square/square-mcp-server)) |
| **Stripe** | Yes. **Remote** `https://mcp.stripe.com` (OAuth) | Also ships `@stripe/ai-sdk` and `@stripe/token-meter` ([stripe/ai](https://github.com/stripe/ai)) |
| **Adyen** | Yes, local `npx @adyen/mcp` with an API key | Payments, links, modifications, **terminal management**, webhooks ([repo](https://github.com/Adyen/adyen-mcp)) |
| **Shopify** | Dev MCP; UCP tooling ([ucp-cli](https://github.com/Shopify/ucp-cli)) | Consumer-agent MCP bridge is archived ([repo](https://github.com/Shopify/consumer-agent-mcp)); Storefront/Checkout MCP† |
| **Toast** | **None found** | Community read-only servers only (e.g., 55 read-only tools, [prime-cost](https://github.com/prime-cost/toast-mcp)). Consistent with partner gating |

### 9.2 Agentic commerce protocols

State as of April 2026, from a neutral CC-BY landscape ([Custena](https://github.com/Custena/agent-payment-protocols)) plus the specs:

- **ACP** (OpenAI and Stripe): Apache-2.0, beta, latest stable 2026-04-17. Covers a Checkout API, delegated payment and Shared Payment Tokens ([spec](https://github.com/agentic-commerce-protocol/agentic-commerce-protocol)). The landscape reports that **ChatGPT Instant Checkout was wound down in March 2026** after about 5 months (single source; verify).
- **UCP** (Google and Shopify): Apache-2.0, transport-agnostic (REST, MCP, A2A). Covers checkout, OAuth identity linking, order webhooks, payment-token exchange and AP2 mandates ([spec](https://github.com/Universal-Commerce-Protocol/ucp)). Merchants publish signed `/.well-known/ucp` profiles ([Magento module](https://github.com/angeo-dev/module-ucp)). Live in Google AI Mode and Gemini.
- **AP2** (Google plus 60+ partners): Intent, Cart and Payment **mandates** as verifiable credentials; v0.1; Revolut live in the UK and EEA since January 2026.
- **Visa:** Intelligent Commerce† plus the **Trusted Agent Protocol** (RFC 9421 signatures bound to domain and page, with replay protection, [repo](https://github.com/visa/trusted-agent-protocol)). The ICC bridge (piloting since 2026-04-08) translates TAP, MPP, ACP and UCP.
- **Others:**
  - Mastercard Agent Pay: live in 9 APAC markets and LatAm; its SD-JWT "Verifiable Intent" delegates identity to Web Bot Auth.
  - Amex ACE (2026-04-14).
  - HTTP-402 rails: x402 (Coinbase), **MPP** (Stripe and Tempo, 2026-03-18), L402.
  - NVIDIA's retail blueprint implements ACP and UCP ([repo](https://github.com/NVIDIA-AI-Blueprints/Retail-Agentic-Commerce)).

**What this means for POS.** None of these protocols covers card-present checkout. Their POS relevance is:
1. **Agent-originated orders** for pickup, delivery or reservations entering the POS as a channel. These need agent verification (TAP / Web Bot Auth) and tokenized payment.
2. **Real-time truth exposed from the POS:** availability, 86'd items, prep times, pickup slots, price and tax. Agents fail when catalogs are stale, so the POS event stream should drive UCP and ACP feeds directly.
3. **Back-office agents through MCP** with scoped, auditable tools.

### 9.3 On-device AI and LLM features

- **On-device:** Apple Foundation Models (about 3 B parameters, iPadOS 26)†, Gemini Nano via ML Kit on flagship Android† (**generally absent on GMS-less or low-end all-in-ones**), Copilot+ NPUs†. The practical offline path is a **hub-hosted small model** (llama.cpp or ONNX).
- **Credible uses:** forecasting and prep (Chick-fil-A pushes cloud forecasts to the edge), loss prevention (void, discount and refund anomalies), natural-language reporting, invoice OCR, menu engineering, support copilots. Voice ordering is mixed: McDonald's ended its IBM test and Taco Bell retreated† (track 01).
- **Guardrails:** LLMs are **never in the money, tax or fiscal path**. Agents act only through typed, permissioned tools with dry-run and approval. Tool outputs such as customer notes are untrusted (prompt injection). Keep audit logs, residency-aware inference and cost caps.

---

## 10. Implications for our architecture

**Reliability and data**

1. **Define reliability at the device.** Target a "can sell" SLO of ≥ 99.999% of trading minutes per location, measured locally. Cloud uptime is secondary. Every feature declares whether it is cloud-required, and **nothing on the sell path is.**
2. **Local-first sale.** Each device can complete a full sale alone: order, tax, fiscal signature, receipt, and card payment through the terminal's store-and-forward. Mode switching is automatic, driven by per-dependency health checks (Square 2023).
3. **Sync approach (recommended): hub-sequenced operation log with island-mode fallback.**
   - **How it works.**
     - Devices apply deterministic mutators locally (optimistic).
     - A **store hub** holds the authoritative per-store log. It re-executes mutators in canonical order (Replicache-style server reconciliation), assigns store sequence numbers, and broadcasts over **mTLS on the LAN** (HTTP/2 or WebSocket; mDNS plus a cloud-signed store roster for discovery).
     - Clients rebase pending operations on the authoritative stream.
     - The hub forwards the log to the cloud through a NATS JetStream leaf node. The cloud sends down versioned catalog and configuration snapshots.
   - **Why this and not the alternatives.**
     - It enforces the invariants CRDT meshes cannot: check ownership, no double payment, gapless fiscal numbering, business-rule validation.
     - It matches the proven incumbents (Simphony CAPS, Aloha BOH, Toast hub, Chick-fil-A edge) while fixing their weaknesses: automatic failover, no Windows server, no same-subnet or wired-only constraints.
     - Web clients can join, because the hub serves them.
   - **Alternatives considered.**
     - Pure P2P CRDT mesh (Ditto): no single point of failure, but proprietary, can't enforce invariants, and costs per device.
     - Device↔cloud engines (PowerSync, Electric): no LAN mode.
     - Zero: no offline writes.
4. **The hub is a role, not a box.** It runs on a dedicated low-cost appliance (recommended above 3 devices; UPS, wired, LTE backup), on an elected always-on terminal or KDS for micro-merchants, or as a container on an enterprise edge cluster. Leader election uses **leases with epoch fencing tokens**, so a deposed hub's writes are rejected. A hot standby keeps failover under 5 s. Follow Chick-fil-A: **recoverability over HA**, with zero-touch re-provisioning.
5. **Island mode.** A device that reaches neither hub nor cloud may sell and pay, edit only checks it owns, and write its own log with its own fiscal counter. On rejoin the hub merges by the §3.4 rules and flags items for a manager (duplicate payments are auto-voided by policy).
6. **Per-aggregate semantics** (§3.4): single-owner checks with leases, immutable payments, an inventory delta ledger with count baselines, versioned catalog snapshots, and LWW-by-HLC flags. UUIDv7 and HLC everywhere; gapless counters per register.
7. **Storage.** SQLite on every node: WAL, STRICT tables, `synchronous=FULL` at commit points, encrypted. Event log plus projections, compacted at end of day. Session-extension or CDC snapshots cold-start new devices. Watch Turso, but not before 1.0.
8. **Own the sync core.** No proprietary engine on the sell path (the Realm EOL lesson). PowerSync (self-hosted) or Electric are optional for **cloud→web back-office read models** only. If time to market forces a purchase, put Ditto behind our sync interface with escrow and an exit plan.

**Clients and hardware**

9. **Client stack (recommended): a shared Rust core with native shells.**
   - The Rust "POS kernel" holds the domain rules (pricing, tax, discounts, fiscal), the sync engine, SQLite storage, device protocol encoders and raster rendering. It is exposed via UniFFI to Kotlin and Swift, via a C ABI, and via WASM.
   - **Android first** (Kotlin plus Jetpack Compose): the all-in-one and payment-terminal ecosystem, dedicated-device management, and hub capability.
   - **iPadOS second** (SwiftUI).
   - **Tauri 2** shells for Windows and Linux lanes, KDS and customer displays.
   - **Why:** one implementation of the money logic; native access to payment and peripheral SDKs; the ability to be a LAN hub or peer; and Shopify's 2026 evidence that dual-native UI is now affordable.
   - **Alternatives:** Kotlin Multiplatform plus Compose Multiplatform (single language, strong on Android and JVM desktop, younger on iOS); Flutter plus a Rust core.
   - **Rejected as primary:** PWA (no LAN role; no iOS hardware APIs) and React Native (the declining flagship precedent).
10. **Web's role.** Back office and analytics run in the browser. KDS and customer displays **may** be browser clients **served by the hub**. Give each store a publicly trusted hostname (e.g., `s123.lan.<domain>` with DNS-01 certificates) so they are secure contexts, avoiding mixed-content and Local Network Access prompts.
11. **Hardware abstraction layer** (§5.8): capability interfaces, Rust encoders, a document DSL rendered to raster for every script, a durable hub spooler with printer failover, a version-frozen legal-for-trade scale module, HID-POS or serial scanners with GS1 parsing, and simulators in CI.
12. **Payments.** Semi-integrated only, with no PAN in our systems. Multiple PSPs and acquirers with failover. Prefer LAN or on-device terminals, which work without WAN. Store-and-forward runs under an explicit **risk policy engine** (caps, BIN, prepaid and foreign-card rules, time window, live exposure dashboard) — a gap no incumbent fills. Tap to Pay is optional.
13. **Devices.** Certify a short list of Android all-in-ones, both GMS and **GMS-less** (no hard dependency on Play Integrity or FCM), plus iPad. Use Windows only for legacy peripherals: IoT LTSC, locked down, **no third-party kernel agents**.
14. **Fleet.** Standard MDMs plus **our own cross-MDM device agent**. SLO-gated rings, per-store cohort updates in maintenance windows, N-2 wire compatibility, TUF-signed artefacts, A/B rollback, and a safe-mode POS.

**Cloud platform**

15. **Cell-based cloud.** Region-pinned cells, siloed cells for enterprise, a thin router cached on hubs, shuffle-sharded shared services, per-cell deploys and configuration, and clean-room rebuild capability.
16. **Datastores.** Postgres per cell as the system of record, with a cross-region standby. Spanner or YugabyteDB only for the router and identity directory. **Avoid CockroachDB** for the core (licence and telemetry terms).
17. **Eventing.** NATS JetStream leaf on each hub → cloud JetStream → Kafka 4.x. Outbox and inbox everywhere. **Device-minted operation IDs serve as end-to-end idempotency keys** through to the PSP.
18. **Reconnection storms.** Jittered backoff, retry budgets and admission control. Priority lanes: payments, then orders, then configuration, then telemetry. Game days that reconnect the whole fleet at once.
19. **No single points of failure.** Staff sign-in works offline (Shopify's Cyber Monday outage). Device APIs are reachable through multiple edges. Break-glass operator access doesn't depend on production DNS or auth (Square 2023).

**Openness and ecosystem**

20. **Open API.** REST/OpenAPI 3.1 with Stripe-grade idempotency and date versioning. Standard Webhooks plus a **replayable event feed**, bulk export, optional GraphQL for reads. No partner gating for data.
21. **Three-tier extensibility.** Sandboxed UI extensions, **deterministic WASM hooks** (Extism or Wasmtime) that run offline, time-boxed and version-pinned, and cloud webhooks. Borrow Shopify's validation-interceptor pattern.
22. **Data portability.** Continuous CDC into merchant-owned Iceberg or Parquet, no export fees, EU Data Act compliant by design, no processor lock-in, open hardware list.
23. **Global compliance modules.** Fiscal adapters in the offline commit path with per-register hash chains and contingency modes; residency via cells; rules shipped as data.

**Operations and security**

24. **Observability.** OTel from device to PSP, an on-device flight recorder and edge sampling. Per-store health covers "can sell", sync lag and printer and terminal status. The §7.8 budgets are enforced in CI on reference low-end hardware.
25. **Security.** Hardware-backed, attested device identity with short-lived mTLS certificates and zero standing access. Passkeys, payout-change holds and help-desk verification for merchants. PCI scope minimisation, SOC 2 Type II and ISO 27001. **CRA reporting is live now** (since 2026-09-11).
26. **Configuration equals code.** Every configuration, menu, rule and content push gets schema and size validation, canaries and staged propagation. Parsers never panic and fall back to last-known-good (CrowdStrike, Cloudflare, AFD).
27. **Verification.** **Deterministic simulation testing** of sync (seeded partitions, clock skew, crashes, duplicate delivery). Property tests for convergence, no double payment and gapless numbering. Fault drills: "cloud gone 24 h", "hub dies at peak", "PSP down".
28. **AI.** An official OAuth-scoped, **read-only-by-default MCP server**. An agent-order channel via UCP and ACP adapters with TAP / Web Bot Auth verification, with catalogs driven by the POS event stream. No LLMs in the money, tax or fiscal paths. Optional hub-hosted small models for offline voice.
29. **Vendor-risk policy.** Anything on the sell path needs an open protocol or our own interface, a local fallback, self-hostable licence terms and an exit plan (lessons: Realm, CockroachDB, NATS, Redpanda).
