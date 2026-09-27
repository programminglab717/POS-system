# ADR-0005: Processor-agnostic payments; card authorization never transits Keel Cloud

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Processor lock-in is the most consistent owner complaint. It shows up as:
- mandatory in-house processing;
- penalty fees for outside processors (a reported ~$400/month, or 0.6–2% of volume);
- processor-locked hardware;
- trapped card-on-file tokens.

Cloud-routed payment paths also make the POS vendor's cloud a single point of failure for taking
money. PCI DSS v4.x makes any system that touches PANs expensive to operate and audit.

## Decision

1. A **connector SPI** in the kernel ([payments.md §3](../architecture/payments.md#3-the-connector-spi))
   abstracts all processors and terminals. Capability flags advertise the features each supports.
2. **Semi-integrated terminals and certified SoftPOS only.** Keel never reads, stores or transmits
   PANs. PAN detectors in CI and log pipelines block any leak.
3. The card authorization path is **device → terminal → processor** (local terminal APIs where
   available). Keel Cloud is never in the authorization path.
4. **GA ships at least three connectors**: Adyen Terminal API (local and cloud), Stripe Terminal, and
   a processor-agnostic US gateway (Datacap-class).
5. **Software pricing never depends on processor choice.** Keel Payments (a PayFac-as-a-service
   partner) is optional.
6. Card-on-file uses network tokens or a PCI Level 1 portable vault. Keel guarantees a card-vault
   export on request.
7. Every tender, from cards and cash to A2A/QR, mobile money, benefits, external accounts and BNPL,
   is a plugin on one state machine, including `Pending` for asynchronous rails.

## Consequences

**Positive**
- Removes the #1 lock-in lever.
- Merchants can keep existing processor contracts on day one.
- Keel's cloud outages can't stop card payments.
- Minimal PCI scope for Keel and its merchants (SAQ P2PE eligible on validated solutions).

**Negative**
- Keel forgoes the dominant SMB POS revenue model (processing margin as the primary revenue line) and
  must earn on software and optional payments.
- Connector breadth costs engineering and certification effort. Mitigated by aggregator gateways,
  partner-built connectors and the contract test suite.
- Feature parity varies by connector (e.g. incremental auth, offline limits). Capability flags
  surface this honestly in the UI.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Mandatory Keel Payments (industry norm) | Contradicts the core promise; the research shows it's the top churn and trust driver |
| Fully integrated card reading in Keel apps | Pulls Keel into full PCI scope; EMV Level 3 certification per processor per device |
| Route all payments through Keel Cloud to a PSP | Cloud becomes a SPOF for taking money |

## References
- [payments.md](../architecture/payments.md), [security.md §3](../architecture/security.md#3-payments-security-and-pci-scope), [R03 §1–§4](../research/03-payments-compliance.md)
