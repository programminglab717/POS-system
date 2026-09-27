# ADR-0010: Peripheral Service and KeelDoc document model

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Printing and peripherals generate a large share of POS support issues. Platforms differ sharply:
- iOS lacks raw USB and non-MFi Bluetooth;
- browsers expose WebUSB, WebSerial and WebHID only in Chromium;
- printer command languages vary (ESC/POS dialects, StarPRNT, ZPL and others);
- most printer fonts can't render Arabic, Hebrew, Thai, Devanagari or complete CJK text.

Proprietary hardware lock-in is a top complaint, so Keel must support commodity devices well.

## Decision

1. A **Peripheral Service** in every Keel runtime exposes one API: print, drawer, weight, scan,
   display, ID read and RFID. It has pluggable drivers (Rust built-ins, plus sandboxed WASM protocol
   drivers for exotic devices) over multiple transports: TCP, HTTP printer APIs, USB, serial,
   Bluetooth, vendor SDKs and cloud printing.
2. **Topology over platform limits**: clients that can't reach a peripheral route jobs through the
   Store Hub's Peripheral Service, with network printers and hub-attached USB devices.
3. **Logical routes with ordered fallbacks** ("Grill → Grill-KDS, else Grill-Printer, else
   Expo-Printer"), with staff alerts on reroute.
4. **KeelDoc**, one device-independent layout model and template system for receipts, kitchen tickets,
   labels, reports and digital receipts. Kernel renderers target ESC/POS, StarPRNT, ZPL/EPL/TSPL, PDF,
   HTML and text.
5. **Kernel text shaping with raster fallback**, so every script prints correctly on any printer.
6. **Fiscal devices** (TSE, signature units, fiscal printers) are handled by the Peripheral Service's
   fiscal device agent.

## Consequences

**Positive**
- Any printer can print any language.
- iPad and web clients get full peripheral access through the hub.
- Printed and digital receipts render from the same template.
- Printer failures reroute instead of stopping service.

**Negative**
- Driver breadth is a long-tail effort, mitigated by partner-contributed WASM drivers and a certified
  hardware list.
- Raster printing is slower than native text on some printers, so it's used only when needed.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| OPOS/UPOS/JavaPOS as the abstraction | Windows- and Java-centric; not viable on iOS, Android or web |
| Vendor SDKs directly in UI code per platform | Duplicated logic; inconsistent behavior; untestable |
| Proprietary Keel-only hardware | Contradicts the no-lock-in promise |

## References
- [hardware.md](../architecture/hardware.md), [R04 §5](../research/04-technical-architecture.md)
