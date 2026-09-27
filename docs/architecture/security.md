# Keel — Security Architecture

> Status: **Draft v1** · Owner: Security & Architecture · Last updated: 2026-09-27

A POS is a high-value target. It handles card payments, cash, personal data and tax records, and it
runs on thousands of devices in places nobody guards overnight. Keel's security goal is to be
**boring to attack**:
- there are no card numbers to steal;
- devices can't be impersonated;
- sales records can't be silently edited;
- one tenant can never see another's data;
- and no single bad update can take down the fleet.

## 1. Threat model (top threats → primary controls)

| # | Threat | Examples in the industry | Primary controls |
|---|---|---|---|
| T1 | Card data theft | RAM-scraping malware on Windows POS (the Target and Home Depot breaches) | **Keel never touches PANs**: P2PE/semi-integrated terminals, certified SoftPOS SDKs, processor vaults, hosted payment fields online (§3) |
| T2 | Sales suppression / fiscal fraud | "Zapper" software that deletes cash sales to evade tax | Signed, hash-chained per-device event logs. Deletion or reordering is detectable. Fiscal adapters add jurisdiction seals. |
| T3 | Insider fraud | Void/refund abuse, sweethearting, no-sale drawer opens, discount abuse | ABAC limits, approvals, separation of duties, tamper-evident audit, anomaly detection (AI), optional video overlay |
| T4 | Rogue or stolen device | Stolen iPad used to issue refunds; a laptop on the store LAN posing as a terminal | Hardware-backed device keys, mTLS, attestation, location binding, remote wipe, per-device revocation |
| T5 | Merchant account takeover | Phished owner credentials used to redirect payouts | Passkeys/MFA mandatory for back office. Step-up auth and cool-off periods for payout, bank and permission changes. Anomaly alerts. |
| T6 | Cross-tenant data leakage | A bug returns another merchant's customers | Tenant ID in every key path, Postgres row-level security as defense in depth, cell isolation, automated cross-tenant tests |
| T7 | Supply-chain / bad update | A malicious dependency; a faulty update bricking devices fleet-wide (CrowdStrike, July 2024) | Signed reproducible builds (SLSA L3), SBOMs, dependency review, staged rings, update windows, auto-rollback, last-known-good boot, no kernel-mode drivers |
| T8 | Ransomware / store network compromise | POS vendor infrastructure attacks disrupting thousands of restaurants | Store selling doesn't depend on vendor cloud (local-first). Least-privilege LAN posture. Hub hardening. Immutable cloud backups. |
| T9 | Online fraud | Card testing on online ordering; fraudulent gift card purchases | Processor risk tools, velocity limits, bot protection, 3-D Secure where required, gift card purchase limits |
| T10 | API abuse | Credential stuffing, scraping, token theft by a malicious app | OAuth scopes, short-lived tokens, per-app rate limits, anomaly detection, marketplace review |
| T11 | Prompt injection against AI features | A malicious customer note or review hijacks the copilot | Untrusted-content isolation, tool permission boundaries, human confirmation for writes (see [ai.md](./ai.md)) |

## 2. Identity

### 2.1 Devices
- **Enrollment**:
  1. A manager scans an enrollment QR on the device, or the device is zero-touch provisioned via
     MDM.
  2. The device generates a key pair in secure hardware (Secure Enclave, Android StrongBox/TEE, or
     TPM 2.0).
  3. The device proves integrity where the platform allows it (Apple App Attest, Play Integrity).
  4. It receives an X.509 certificate from the **Keel Device CA**, bound to `(org, location,
     device role)`.
- **Use**: the certificate authenticates all LAN sync (mTLS) and cloud connections, and the device
  key signs every event the device appends. The key itself never rotates: a device that needs a
  new key (after a repair or a reset of its secure hardware) is enrolled again as a new device,
  with a new log ([ADR-0012](../adr/0012-event-wire-format.md)).
- **Short-lived certificates** (24–72 h), rotated automatically. Hub-cached trust material covers
  long offline periods. Revocation propagates through sync. It cuts the device's log after the
  last event the cloud trusts, by sequence number rather than time (a stolen device can set its
  clock back), and replicas quarantine every event beyond the cut
  ([ADR-0012](../adr/0012-event-wire-format.md)).
- **Works without Google services.** Many Android all-in-ones lack them, so enrollment accepts
  **Android Key Attestation** and doesn't depend on Play Integrity or FCM push. Play Integrity and
  Apple App Attest add signal where available.
- **Tokens are bound to the device** (mTLS- or DPoP-bound). No long-lived API keys live on devices.
- **Offline staff authorization** uses cloud-signed role bundles with expiry, plus PIN hashes sealed
  with a hardware-held pepper and rate-limited. Every offline override is logged for review.
- **Remote actions**: lock, wipe, move to a new location (which enrolls the device again, as a new
  device), or change role, all audited.

### 2.2 Staff (on devices)
- A fast PIN login, with optional NFC badge or on-device biometric (never uploaded) as a second
  factor for sensitive roles.
- PINs are stored as salted, memory-hard hashes, and attempts are rate-limited per device and per
  person.
- **Shared-device sessions** with auto-lock after inactivity and "tap to switch user" for busy
  service.
- Every event records the acting team member.
- **Approvals**: manager PIN or badge on the same device, or a push approval to the manager's phone
  with the action context shown. Each approval is recorded as its own event.

### 2.3 Back office, API and apps
- **Passkeys first**, with TOTP or WebAuthn MFA mandatory for owners and admins. SSO (SAML/OIDC) and
  SCIM provisioning for enterprises.
- **Step-up authentication** and a notification plus cool-off window (e.g. 24 h, configurable for
  enterprises) for high-risk changes: bank or payout details, adding an owner, bulk export,
  API-credential creation. **All owners are notified** of payout-destination changes, because
  merchant account takeovers that redirect payouts are a documented attack pattern
  ([R04 §2](../research/04-technical-architecture.md)).
- **Help-desk identity verification playbooks**: support staff can't reset credentials or change
  payout details on a caller's say-so. This addresses the social-engineering vector behind the 2025
  UK retail ransomware attacks.
- **OAuth 2.1** for apps: fine-grained scopes, merchant-visible grants, instant revocation.

## 3. Payments security and PCI scope

**Design goal: Keel's software and infrastructure never store, process or transmit primary account
numbers (PANs).** This makes it possible for merchants to qualify for the smallest PCI self-assessment
questionnaires, and it keeps Keel's own PCI burden limited to what's needed as a platform.

| Channel | Mechanism | Card data path |
|---|---|---|
| In-person card | PCI P2PE-validated or processor-encrypted terminals, **semi-integrated** (the terminal talks to the processor; Keel receives only a result and token) | Card → terminal → processor. Never through the Keel app or cloud. |
| Tap on phone (SoftPOS) | Processor SDKs certified under PCI MPoC or CPoC (e.g. Tap to Pay on iPhone/Android via providers) | Inside the certified SDK's secure boundary |
| Online / QR / kiosk web | Processor-hosted fields or payment pages (iframes), wallet sheets | Browser → processor |
| Card on file / subscriptions | Processor vault or network tokens; Keel stores token references only | — |
| Manual key entry | Only on the terminal's secure keypad, never in Keel UI | Terminal → processor |

Additional controls:
- Receipts and screens show masked PANs only (brand + last 4).
- **No card data in logs, ever.** Log pipelines run PAN-detection scrubbers as a backstop and alert on
  any match.
- If Keel offers embedded payments (a PayFac model via a partner), the payments service runs in a
  **separately segmented cardholder-data environment (CDE)**, with its own accounts, network, access
  and audits. It is the only component in PCI DSS scope as a service provider.

## 4. Data protection

- **In transit**: TLS 1.3 everywhere (LAN included), with mTLS between devices, hub and cloud.
  Certificate pinning to the Keel CA for device connections.
- **At rest on devices**: the SQLite database is encrypted (SQLCipher-class AES-256) with a key
  wrapped by the platform keystore. A device powered off and removed from the store reveals nothing
  without its secure-hardware key.
- **At rest in the cloud**: AES-256 storage encryption, **per-tenant data keys** in a KMS, and
  bring-your-own-key for enterprise tenants.
- **Personal data**: the PII vault with per-person keys and crypto-shredding for erasure (see the
  domain model §14). Data minimization is on by default: collect only what's needed, retain only as
  long as policy says.
- **Backups**: encrypted, immutable (object lock), cross-region, and restore-tested monthly.
- **Data residency**: tenant data (including backups and analytics) stays in the tenant's home region.
  Support access from other regions is policy-controlled and audited.

## 5. Tenant isolation

- **Cells**: each cell is a separate deployment with separate databases. A defect in one cell's data
  layer can't expose another cell's tenants.
- **Within a cell**:
  - every table carries `tenant_id` in its primary key;
  - every query goes through a repository layer that requires a tenant context;
  - and **Postgres row-level security** enforces the tenant predicate even if application code
    forgets it.
- Caches, queues and object-storage paths are tenant-prefixed and key-scoped.
- **Continuous verification**: an automated "canary tenant" test suite tries cross-tenant access
  through every API route on every deploy.

## 6. Authorization (ABAC) and anti-fraud controls

- Permission = action × resource × constraints (amounts, percentages, own records only, time of day,
  revenue center). Evaluated by the kernel on every command, offline included.
- **Separation of duties** is configurable. For example, the person who rings a return can't approve
  it, and whoever edits a time entry can't approve their own.
- **Sensitive actions** always create audit events with reason codes: voids after fire, comps,
  discounts above a threshold, refunds without receipt, no-sale opens, price overrides, drawer counts,
  time-entry edits, permission changes.
- **Loss-prevention analytics**: per-employee rates of voids, refunds, discounts and no-sales vs
  peers, with outlier detection. Optional linkage of events to CCTV timestamps.

## 7. Integrity of records

- Per-device hash chains and signatures make the event history tamper-evident (see the domain model
  §17).
- The cloud periodically computes **Merkle roots** over each location's daily events and stores them
  in write-once storage. Any later alteration of historical data, even by a Keel insider with
  database access, is detectable.
- Fiscal adapters add jurisdiction-mandated seals (TSE signatures, hash chains, authority-issued
  codes) on top.

## 8. Software supply chain and release safety

- **Builds**: hermetic, reproducible (for the kernel), SLSA Level 3 provenance, signed with Sigstore
  cosign. Devices verify signatures before install.
- **Update metadata follows The Update Framework (TUF).** It defends against key compromise, rollback,
  freeze and mix-and-match attacks on the device update channel.
- **Dependencies**: pinned and scanned (vulnerabilities and licenses), with an SBOM for every build.
  New dependencies in the kernel require security review.
- **No kernel-mode code** on any device. Keel runs entirely in user space, and certified lanes run
  **no third-party kernel agents**. Windows is supported for legacy peripherals only, as locked-down
  IoT Enterprise LTSC with Assigned Access.
- **Non-panicking parsers with last-known-good fallback** for all configuration and content
  (see offline-and-sync §11).
- **Rollouts** (lessons from July 2024):
  - Staged rings with automatic health gates (crash rate, payment success, sync lag, UI latency).
  - Per-store **update windows** outside trading hours.
  - Instant rollback, and a last-known-good build kept on every device.
  - A config and content rollout gets the same staging as code: feature flags, rules, tax tables and
    extension versions all roll out in rings.

## 9. Infrastructure security

- Zero-trust service-to-service communication (mTLS with workload identities) and least-privilege IAM
  per service.
- Production access is just-in-time, approved, time-boxed and session-recorded. There are no standing
  admin credentials, and no site-to-site VPNs into merchant networks.
- **Recovery tooling doesn't share fate with production.** Break-glass access, operator DNS and
  operator authentication are independent of production DNS and identity. A lesson from an incident
  where the recovery tools depended on the failing DNS ([R04 §2](../research/04-technical-architecture.md)).
- **Clean-room rebuild capability**: a cell can be rebuilt from immutable backups into fresh accounts
  if it is compromised (the NCR 2023 recovery required a new environment).
- **Store LAN is treated as hostile.** Keel assumes guest Wi-Fi, IoT devices and vendor equipment on
  the same network (the Target/HVAC lesson). All Keel traffic is mTLS. Port-9100 printers go on an
  isolated VLAN, with HTTPS printing (CloudPRNT, ePOS over HTTPS) preferred where available.
- Secrets live in a managed secrets store, rotated automatically, and never in code or images.
- WAF and bot management at the edge. DDoS protection for public endpoints (online ordering, APIs).
- SIEM with detections for admin anomalies, mass exports, unusual API patterns and impossible travel.

## 10. Compliance program

| Framework | Scope | Target |
|---|---|---|
| SOC 2 Type II | All of Keel | Before GA of the paid product |
| ISO/IEC 27001 + 27701 | All of Keel | Within 12 months of GA |
| PCI DSS v4.x | Keel as a service provider (only if operating embedded payments); merchant-facing SAQ guidance | With embedded payments |
| GDPR / UK GDPR / CCPA-CPRA / LGPD / DPDP (India) / PDPL (Saudi Arabia) | Personal data processing | From the first market launch in each region |
| Accessibility (WCAG 2.2 AA, EN 301 549 for EAA) | All staff- and customer-facing UIs | From v1 |
| Fiscal certifications | Per country (see [compliance.md](./compliance.md)) | Per market launch |
| **EU Cyber Resilience Act** | Keel software and the hub appliance as products with digital elements | Vulnerability and incident reporting obligations live since 11 Sep 2026 (24 h early warning, 72 h notification); full obligations from 11 Dec 2027 |
| EU Data Act | Data portability and switching | Met by design (full export, no switching charges) |

Plus an annual third-party penetration test, a continuous public bug bounty, and threat modeling as a
required step in design reviews for new features that touch money, identity or personal data.

## 11. Incident response

- 24/7 on-call with severity definitions tied to merchant impact. Any location unable to sell is SEV-1.
- Kill switches: feature flags per feature, per extension, per integration and per cell.
- Merchant communication:
  - a status page with per-region and per-component status;
  - in-app banners targeted at affected merchants only;
  - post-incident reviews published for SEV-1s.
- Forensics-ready: tamper-evident logs, centralized audit, and device log pull with merchant consent.
