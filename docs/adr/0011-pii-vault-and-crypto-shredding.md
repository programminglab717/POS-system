# ADR-0011: PII vault with per-person keys and crypto-shredding

- **Status:** Accepted
- **Date:** 2026-09-27

## Context

Keel's event log is immutable (ADR-0002) and replicated to devices, hubs, cloud archives and merchant
warehouses. Privacy laws (GDPR Art. 17, CCPA/CPRA deletion and others) grant a right to erasure.
Fiscal law requires keeping transaction records for 6–10 years. Deleting events would break hash
chains and fiscal integrity, and scrubbing replicas and backups is unreliable.

## Decision

1. Events reference people only by opaque tokens (`CustomerRef`, `TeamMemberRef`). **No raw PII in
   event payloads.**
2. PII fields live in the **PII vault**. Each person's record is encrypted with a **per-person data
   key**, wrapped by a tenant key held in a KMS.
3. Devices cache the minimal encrypted subset they need, decryptable only on enrolled devices.
4. **Erasure means destroying the person's data key** (crypto-shredding). Every replica, backup and
   export becomes unreadable for that person's PII, while the financial records remain intact and
   verifiable.
5. Legally required buyer details on B2B invoices are retained under the fiscal-retention policy,
   with pseudonymization where allowed.
6. ID verification stores results, not document images, by default.

## Consequences

**Positive**
- Immutability and the right to erasure coexist.
- Erasure is reliable across all copies, including offline devices once they sync the key revocation.
- Less PII is spread around.

**Negative**
- Customer lookup on devices depends on the encrypted cache, so offline lookup covers only cached
  customers.
- Key management complexity and KMS costs.
- Analytics must work on tokens or consented attributes, not raw PII.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| PII inside events, deleted by rewriting history | Breaks hash chains and fiscal integrity; unreliable across replicas |
| Tombstoning without encryption | Copies in backups, warehouses and devices still expose data |

## References
- [domain-model §14](../architecture/domain-model.md#14-privacy-pii-vault-and-crypto-shredding), [security.md §4](../architecture/security.md#4-data-protection), [compliance.md §8](../architecture/compliance.md#8-privacy)
