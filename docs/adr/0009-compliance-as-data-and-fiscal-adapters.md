# ADR-0009: Compliance as signed rule packs, plus pluggable fiscal adapters

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Regulation changes often, differs by jurisdiction, and sometimes reverses:
- France removed, then restored, self-attestation for cash software;
- Spain postponed VeriFactu twice;
- US SNAP eligibility now varies by state and date;
- Germany cut restaurant food VAT from 1 January 2026;
- India rationalized GST in September 2025;
- surcharging rules await a court decision.

Shipping these as code ties compliance to app release cycles and app-store review, and leaves
merchants exposed between releases. Fiscal regimes fall into four integrity models (device,
software chain, clearance, network), each needing different technical integration.

## Decision

1. **Rule packs.** Rules that change with law are **data**: signed, versioned, jurisdiction-scoped
   and effective-dated. The kernel evaluates them offline with explanations, and every decision
   records the pack version. Packs are distributed as reference data in staged rings, independent of
   app releases, and can be rolled back.
2. **A fiscal orchestrator** in the kernel and cloud emits jurisdiction-neutral fiscal events to
   **country provider adapters** through one SPI (`register`, `sign`, `render`, `report`, `close`,
   `export`, `contingency`). Each adapter has its own contingency state machine and legal catch-up
   parameters.
3. **Keel's own hash-chained journal** is the evidence base, independent of any fiscal provider.
4. **Release trains per country**, with certified-version pins.
5. **Buy vs build**: Keel builds the orchestrator, the journal and the rule engine. It partners for
   hardware-heavy and long-tail markets (cloud TSE, PACs, Peppol access points, accredited service
   providers).

## Consequences

**Positive**
- Regulatory changes deploy in about an hour as data.
- Provider swaps don't break audit continuity.
- The core never forks per country.
- Explanations reduce support load and audit risk.

**Negative**
- A rule language and tooling must be built and governed. Compliance authoring needs specialist
  staff and legal sign-off workflows.
- Some regimes certify specific software versions, so release trains must respect pins. This slows
  kernel changes in those markets.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Hard-coded country logic | Release-bound, forks the core, slow to react to reversals |
| Delegate everything to a third-party fiscal API | Vendor lock-in for audit data; online dependence; gaps in markets |
| Per-country product builds | The fragmentation merchants suffer from today |

## References
- [compliance.md](../architecture/compliance.md), [R03 §5–§6](../research/03-payments-compliance.md), [R06 §2](../research/06-verticals-global.md)
