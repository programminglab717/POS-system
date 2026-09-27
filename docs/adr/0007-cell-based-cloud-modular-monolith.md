# ADR-0007: Cell-based multi-tenant cloud running a modular monolith

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

The cloud must serve from one to hundreds of thousands of locations, respect data residency, and
limit blast radius. Incidents in the research include a DNS change taking down a whole platform, and
a regional outage and a single-data-center ransomware attack each crippling many merchants.

Team velocity matters too. A premature microservice split multiplies failure modes and slows early
development.

## Decision

1. **Cells.** A cell is a complete, independent deployment:
   - the core service and workers;
   - PostgreSQL (multi-AZ);
   - NATS JetStream;
   - object storage and Valkey.

   Each cell serves a subset of tenants (initial sizing is about 5k locations), and each has a
   **warm standby in a paired region** within the same residency zone (RPO ≤ 1 min, RTO ≤ 15 min),
   with quarterly failover drills.
2. **A minimal global control plane**: identity, the tenant directory and cell router, the device CA,
   billing, the marketplace and rollout. Cells keep operating if it's down, because they cache
   routing and verify tokens and certificates locally.
3. **A modular monolith per cell**: one Rust service with strict module boundaries (catalog,
   ordering, payments, fulfillment, inventory, value, bookings, workforce, cash, fiscal, ledger,
   reporting, sync ingest, automations), linking the kernel crates. Modules communicate through
   in-process interfaces and events. A module is extracted into a separate service only when scaling
   or isolation requires it.
4. **Data stores**:
   - PostgreSQL for projections, the hot event window, the outbox and the ledger, with row-level
     security as defense in depth;
   - NATS JetStream for internal events and work queues;
   - Parquet/Iceberg archives in object storage;
   - ClickHouse for analytics, as a regional shared service.
5. **The integration runtime** (TypeScript connector workers) and the analytics and AI gateways are
   regional shared services, isolated per connector with circuit breakers.
6. **Rollouts go cell by cell** (internal → canary → risk tiers) for code, config, rules and
   connectors. Large enterprises can get dedicated cells and custom rings.

## Consequences

**Positive**
- Blast radius is bounded to one cell and one change ring.
- Data residency is natural: cells live in zones.
- Postgres stays within comfortable limits.
- One deployable per cell keeps operations simple.
- Kernel reuse guarantees cloud and device compute identical totals.

**Negative**
- Cross-cell queries (franchisor roll-ups spanning cells) need federation through analytics. Placement
  policy co-locates brand networks where possible.
- Cell rebalancing and migration tooling must be built.
- Paired-region standby roughly doubles the cost of the data tier.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Single global multi-tenant deployment | Unbounded blast radius; residency is harder |
| Microservices from day one | Operational overhead and distributed-systems failure modes without scale justification |
| Globally distributed SQL (Spanner, CockroachDB) as the primary store | Latency and cost; cells + Postgres suffice. Kept as an option for the control plane. |
| Kafka instead of NATS | Heavier operations per cell. Revisit if throughput demands it. |

## References
- [architecture README §4.3, §7, §8](../architecture/README.md), [security.md §5](../architecture/security.md#5-tenant-isolation)
