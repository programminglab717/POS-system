# Keel — Hardware and Peripherals

> Status: **Draft v1** · Owner: Architecture · Last updated: 2026-09-27
>
> Decision record: [ADR-0010](../adr/0010-hardware-abstraction-and-document-model.md)

## 1. Principles

1. **Bring your own commodity hardware.** Keel runs on iPads and iPhones, Android tablets, phones and
   all-in-ones, Android payment terminals, and Windows, macOS and Linux PCs. Standard peripherals
   work: printers, drawers, scanners, scales and displays. Proprietary lock-in hardware is never
   required.
2. **Certified, not captive.** We publish a certified hardware list and sell reference bundles at
   transparent prices. Buying from us is optional.
3. **Lifecycle commitments.** Certified hardware is supported for at least 5 years from certification,
   with at least 12 months' end-of-life notice. There are no surprise "legacy device" fees. (Recent
   industry counter-examples include hardware sunsets and legacy-device surcharges; see
   [research](../research/02-retail-pos.md).)
4. **The printer is the #1 support call.** Printing, drawers and scanning get first-class
   engineering: auto-discovery, health monitoring, retries, fallback routing and clear diagnostics.
5. **Degrade gracefully.** A missing peripheral never blocks a sale. There's always a documented
   fallback, such as a digital receipt, a reroute to another printer or KDS, a manual weight entry
   with approval, or a manual drawer key.

## 2. Device roles and platforms

| Role | Supported platforms | Notes |
|---|---|---|
| Register (counter POS) | Android (tablets, all-in-ones such as Sunmi, PAX, iMin, Elo; **with or without Google services**), iPadOS; Windows (locked-down IoT Enterprise LTSC) and Linux lanes in v3 | Dual-screen all-in-ones drive the customer display natively. Android is the first platform ([ADR-0004](../adr/0004-client-ui-stack.md)). |
| Handheld / pay-at-table | iPhone, Android phones, rugged handhelds (Zebra, Honeywell), **Android payment terminals** | On Android payment terminals Keel runs *on the terminal itself*, beside the payment app, via the PSP's app platform |
| KDS | Android, iPadOS screens + bump bars (Linux and Windows in v3) | Designed for heat, grease and distance: large type, color-blind-safe status colors, auto-brightness, fully keyboard-operable for bump bars. **KDS is primary; printers are backup.** |
| Display surfaces (order-ready boards, menu boards, secondary displays) | Any modern browser or smart display | Hub-served web apps over the store's trusted local hostname |
| Kiosk | Android, Windows, iPadOS in enclosures | Accessibility per EAA/EN 301 549 and ADA: reach ranges, audio output, tactile navigation, screen reader |
| Customer-facing display | Second screen on all-in-ones, a paired tablet, pole displays (VFD) | Cart, promos, tip, signature, loyalty sign-up, digital-receipt QR |
| Store Hub | **Keel Hub appliance** (recommended) or any hub-eligible device | See §3 |
| Manager / staff app | iOS, Android | Approvals, schedules, time-off, tips, reports |
| Back office | Any modern browser | Web |

Operating modes are **roles**, not separate apps. One Keel device app can be switched between register,
handheld, KDS, kiosk, customer display and hub by an administrator. This simplifies stocking spares.

## 3. The Keel Hub appliance (reference design)

A small, fanless, low-cost box that makes stores more resilient. It is optional but recommended for
full-service restaurants, grocery, venues and multi-lane retail.

- **Compute**: an x86 (Intel N-series class) or ARM (Cortex-A76 class) SoC, 8 GB RAM, 128 GB+
  industrial SSD, TPM 2.0 (hardware root of trust for the hub identity and disk encryption).
- **Networking**:
  - 2× Ethernet;
  - Wi-Fi 6;
  - an **optional LTE/5G modem**, which acts as automatic cellular failover for the whole store's
    card terminals and cloud sync (as a router or as an uplink for hub traffic only);
  - PoE-powered variant.
- **Power**: an internal battery or UPS integration for graceful shutdown and 2+ hours of hub
  operation during power loss. It is the coordination point that keeps battery-powered handhelds and
  terminals working together.
- **Ports**: USB for receipt and kitchen printers, drawers, scales and legacy serial devices (via
  USB-serial). This lets iPad and browser clients use USB peripherals they can't access directly.
- **Software**:
  - an immutable, image-based Linux OS with A/B partitions and automatic rollback;
  - the Keel Hub daemon (the Rust kernel plus hub services);
  - remote management through Keel;
  - no general-purpose shell exposure.

## 4. Peripheral Service

Every Keel runtime includes the **Peripheral Service**. It manages locally attached peripherals, and
the hub's instance also manages shared network peripherals. It exposes one uniform API to the kernel
and the UI:

```text
print(job: KeelDoc, target: PrinterRef | Route) -> JobId
job_status(JobId) -> queued|sending|printed|failed(reason)|unconfirmed
open_drawer(DrawerRef) ; drawer_state(DrawerRef) -> open|closed|unknown
read_weight(ScaleRef) -> { net, tare, unit, stable, legal_for_trade }
scanner_events() -> stream<ScanEvent { symbology, raw, parsed(GS1) }>
display(CustomerDisplayRef, content)
read_id(IdReaderRef) -> IdResult { age_ok, dob?, expiry, method }   // minimal data
rfid_inventory(ReaderRef, session) -> stream<EpcRead>
```

### 4.1 Drivers and transports
- **Drivers** are Rust modules compiled into the runtime. Exotic or regional devices can ship as
  **sandboxed WASM protocol drivers**, which receive I/O only through host-provided transport handles.
  This is how partners add hardware support without Keel releases.
- **Transports**:

| Transport | Where available | Typical devices |
|---|---|---|
| TCP/IP raw (port 9100) | All platforms | Network receipt and kitchen printers |
| HTTP printer APIs (e.g. Epson ePOS-Print, Star WebPRNT) | All | Epson and Star network printers; useful from browsers |
| Cloud printing (Star CloudPRNT, Epson Server Direct Print) | Printer polls the cloud | Online orders printing even when no device is on |
| USB (libusb / WinUSB / Android USB Host) | Android, Windows, macOS, Linux, hub | Printers, drawers, scanners, scales |
| Serial (RS-232 / USB-COM) | Android, Windows, macOS, Linux, hub | Scales, pole displays, legacy devices |
| Bluetooth Classic (SPP) / BLE | Android, Windows, macOS; iOS only via MFi vendor SDKs | Mobile printers, handheld scanners |
| Vendor SDKs (StarIO10, Epson ePOS SDK, Zebra, Honeywell, Sunmi, PAX) | Per platform | iOS printers (MFi), built-in printers and scanners on all-in-ones |
| OS print services (AirPrint, CUPS, Windows spooler) | All | A4/Letter invoices and reports |

- **Platform constraints are handled by topology, not excuses.** iOS can't do raw USB or
  non-MFi Bluetooth, and browsers only get WebUSB/WebSerial/WebHID in Chromium. So Keel routes those
  clients' jobs to **network printers or hub-attached peripherals** through the Peripheral Service on
  the hub, and the experience is identical.

### 4.2 Discovery and configuration
- Auto-discovery on the LAN (mDNS/Bonjour, SNMP, vendor discovery protocols) and on USB/Bluetooth.
  Guided setup with a test print, and printer capability detection (paper width, cutter, drawer port,
  fonts, raster support).
- **Routes are logical.** Kitchen tickets go to "Grill" and receipts to "Front counter". Routes map
  to physical devices with ordered **fallbacks**, e.g. `Grill → Grill-KDS, else Grill-Printer, else
  Expo-Printer`. When a device fails, Keel reroutes automatically and tells the staff.

## 5. KeelDoc — one document model for print and digital

Receipts, kitchen tickets, labels, reports and digital receipts all come from **the same templates**:

- A **device-independent layout**:
  - blocks: text with size, weight, underline, invert and alignment; columns and tables;
  - separators, images (logos, auto-dithered) and 1D/2D barcodes (including fiscal QR codes and
    digital-receipt links);
  - cut, feed, drawer kick, buzzer (kitchen) and label page mode.
- **Templates** use a safe, logic-light templating language over the order snapshot, fiscal payload
  and custom fields. They are localized and brandable, and editable in the back office with a live
  preview for each target printer.
- **Renderers** in the kernel compile KeelDoc to:
  - ESC/POS dialects (Epson, Citizen, Bixolon and generic);
  - StarPRNT and Star Line Mode;
  - ZPL, EPL and TSPL for label printers;
  - PDF (A4/Letter invoices), HTML (digital receipts and email) and plain text (SMS).
- **International text done right.** Printer fonts rarely support Arabic or Hebrew (RTL shaping),
  Thai, Devanagari or complete CJK sets. The renderer shapes text in the kernel (HarfBuzz-class
  shaping) and sends it as **raster graphics** when the printer can't render it natively. Every
  language prints correctly on any printer.
- **Print job lifecycle**:
  - A persistent queue with retries, while none of a job has been sent. Once any of it has, a
    job that fails, or is found sending after a restart, is **unconfirmed**: a printer can't
    always say what it printed, so it is never sent again by itself, and staff decide whether to
    reprint ([ADR-0023](../adr/0023-register-shell.md)).
  - Status polling for paper-out, cover-open and offline states (ESC/POS automatic status back,
    Star status).
  - Staff-visible failures, with a tap-to-reroute.
  - Reprints are explicit and marked "REPRINT"; kitchen reprints are marked so cooks don't double-fire.

## 6. Scanning

- Symbologies:
  - UPC/EAN and Code 128;
  - GS1 DataBar (produce, coupons);
  - **GS1 DataMatrix and QR with GS1 Digital Link**, which carry lot, expiry and serial numbers in
    line with the industry's 2D-barcode transition;
  - PDF417 (driver's licenses);
  - QR (loyalty, app payments, digital IDs).
- **GS1 parsing** of Application Identifiers: 01 GTIN, 10 lot, 17 expiry, 21 serial, 310x net weight,
  392x price, and so on. Price- and weight-embedded barcodes follow configurable prefix rules. Expired
  or recalled lots can be blocked at the counter.
- **Keyboard-wedge robustness**:
  - Burst-timing detection separates scanner input from typing.
  - Prefix and suffix configuration.
  - Protection against the keyboard-layout mismatch that corrupts scans (AZERTY vs QWERTY).
  - The HID-POS usage page or serial mode is preferred when the scanner supports it.
  - Android uses vendor intents and SDKs (e.g. Zebra DataWedge).
- **Camera scanning** on phones and tablets (VisionKit / ML Kit) for low-volume merchants and
  handhelds.

## 7. Scales and weighed goods

- Drivers for common POS scale protocols: Mettler Toledo serial protocols, CAS, Datalogic
  scanner-scales, and NCI-type protocols. Additional protocols can be added as WASM drivers.
- **Legal-for-trade behavior**:
  - Stable-weight detection, zero and tare handling, and minimum-weight rules.
  - The weight is shown on the customer display.
  - The price is computed only from certified scale readings.
  - Manual weight entry requires permission and is flagged.
- **Certification**: legal-metrology requirements for POS that compute price from weight vary by
  jurisdiction (e.g. NTEP and NIST Handbook 44 in the US; OIML R76, NAWI and WELMEC 7.2 rules on
  "legally relevant software" in the EU). They are tracked as a compliance item per market in
  [compliance.md](./compliance.md).
- **The weighing module is isolated and version-frozen** as a separate kernel component with its own
  release pin, because changes to legally relevant software can trigger re-evaluation.
- **Label-printing scales** (deli and bakery) receive PLU and price updates from the catalog through
  scale-integration connectors.

## 8. Cash handling hardware

- Drawers can be printer-driven (DK port), USB or serial. **Open-state sensing** supports the
  "close drawer before next sale" policy and audits long-open drawers. There can be multiple drawers
  per register, each bound to a drawer session.
- **Cash recyclers and dispensers** (e.g. Glory, CashGuard, Cashdro): automatic tendering and change,
  denomination-level counts that feed the drawer session, and a blind close.

## 9. Payment terminals

Terminals are managed through [payments connectors](./payments.md):
- **Pairing and binding**: a terminal can be bound to a register, a station or a shared pool
  (handhelds and pay-at-table).
- **Health and status**: connected, busy, needs update and low battery, visible to staff and to
  remote support.
- **Firmware**: updates follow the PSP's terminal-management system, and the Keel UI blocks
  mid-service updates when the PSP allows it.

## 10. Identity and age verification devices
- **2D imagers** read AAMVA PDF417 (US/Canadian licenses). Supported alternatives:
  - NFC/BLE readers and phone-based verification for **mobile driver's licenses (ISO/IEC 18013-5)**
    and wallet IDs;
  - industry age tokens where adopted (e.g. convenience retail).
- **Minimal data**: the result (over the required age: yes or no), the method and the time are
  recorded, not the document image. The exception is where a regulation requires more; see the domain
  model §14.

## 11. Store-scale integrations
- **RFID**: UHF handheld and fixed readers, with EPC/SGTIN decoding into count sessions and the
  high-throughput ingestion API.
- **Electronic shelf labels**: connectors to the major ESL platforms, so price changes publish to
  shelves and registers atomically at the same effective time.
- **Digital menu boards and order-status boards**: driven by the hub's local API, and live even
  offline.
- **CCTV and video analytics**: transaction event feeds with timestamps for video overlay and loss
  prevention.

## 12. Fleet management
- **Zero-touch enrollment**: Android Enterprise zero-touch, Apple Business Manager with Automated
  Device Enrollment, and Windows Autopilot, plus QR enrollment for bring-your-own devices.
- **Lockdown**: Android lock-task mode, iOS Autonomous Single App Mode / Guided Access, and Windows
  Assigned Access. A dedicated kiosk and KDS lockdown profile.
- **Built-in fleet console**:
  - app, kernel and OS versions, and last-seen time;
  - battery, storage, network quality, peripheral status and terminal status;
  - remote log pull (with consent), remote restart, role change, lock and wipe.

  Integrates with third-party MDMs (e.g. Esper, Jamf, Intune, SOTI) rather than replacing them.
- **Updates**: store-hour-aware windows, staged rings and automatic rollback (see
  [offline-and-sync.md §11](./offline-and-sync.md#11-mixed-versions-and-upgrades)).

## 13. Network guidance (built in, not a PDF)

- The hub runs continuous **network diagnostics**:
  - latency and loss between the hub and each device;
  - Wi-Fi signal per device;
  - DNS and internet reachability;
  - PSP endpoint reachability;
  - and bandwidth.

  Problems are explained in plain language ("The patio handheld has weak Wi-Fi; consider an access
  point near table 30").
- **Recommended topology**:
  - POS devices on a dedicated VLAN or SSID, separate from guest Wi-Fi;
  - wired where possible (KDS, printers, hub);
  - cellular failover via the hub or terminals.

  Setup wizards check these and warn about risky configurations.
