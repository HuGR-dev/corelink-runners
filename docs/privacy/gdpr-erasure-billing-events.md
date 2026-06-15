# GDPR Art. 17 — Erasure coverage: `billing_events`

**Status:** TRACKED FOLLOW-UP — not built. Depends on org-wide erasure
orchestration (owner/privacy to schedule when the erasure SLA is formalized).

**Date logged:** 2026-06-14
**Owner:** HuGR privacy / corelink-runners lead

---

## Why this note exists

CoreLink Cache shipped tenant-data erasure (GDPR Art. 17, "right to be
forgotten") for CAS/AC in its D-8 handoff. That covers the cache layer.
The runners fabric owns a **second tenant-scoped store** — `billing_events` —
that is NOT covered by the cache erasure and must be addressed independently
when the org-wide erasure SLA is defined.

---

## The store

Table `billing_events` (schema in
`crates/corelink-fabric/src/billing_sink.rs`, `const DDL`):

```sql
CREATE TABLE IF NOT EXISTS billing_events (
  tenant         text   NOT NULL,   -- <-- tenant-scoped
  lease_id       text   NOT NULL,
  kind           text   NOT NULL,
  at_ms          bigint NOT NULL,
  exported_at_ms bigint NOT NULL,
  PRIMARY KEY (tenant, lease_id, kind, at_ms)
);
```

The `tenant` column is the first component of the PRIMARY KEY. Every row
belongs to exactly one tenant; there are no cross-tenant rows. Source event
type: `SlotOccupancyEvent` (`crates/corelink-fabric/src/meter.rs`), emitted
by `BilSink::persist` via the `Exporter` loop.

**This is tenant-scoped personal data** for GDPR Art. 17 purposes: it records
which tenant ran which leases, when, and with what outcome. A full Art. 17
erasure request targeting a tenant must cover this table, not only the
CAS/AC cache.

---

## Erasure mechanism sketch

The delete is trivial given the schema:

```sql
DELETE FROM billing_events WHERE tenant = $1;
```

Properties:
- **Tenant-prefix-bounded.** The `WHERE tenant = $1` predicate is an exact
  match against the PK prefix. No cross-tenant row is touched regardless of
  the value of `$1`. Blast radius is strictly one tenant.
- **Fail-closed.** The statement either commits cleanly or fails with an
  error; there is no partial-delete ambiguity at the SQL level. The caller
  must handle the error and retry or surface it to the erasure orchestrator.
- **Auditable.** `DELETE … RETURNING tenant, lease_id, kind, at_ms` can be
  used if the erasure orchestrator needs a manifest of what was deleted for
  its own audit log (the returned rows are the deleted rows).
- **Idempotent.** Re-running the delete for the same tenant after the rows
  are gone is a no-op (0 rows affected), which is safe for orchestrator retry
  logic.

---

## Retention nuance — open legal/product question

`billing_events` is a **usage and billing record** (slot occupancy, the
billable unit). Jurisdictions commonly allow or require retaining aggregate
billing records for tax/audit purposes (e.g., EU VAT rules, typically 7–10
years) even after an Art. 17 erasure of personal data.

This creates a potential tension:

- Art. 17(3)(b) exempts retention "for compliance with a legal obligation"
  — an invoicing/tax retention obligation may qualify.
- If retained for billing/tax, the rows should carry the minimum necessary
  personal data. The `tenant` column is currently a plain org identifier
  (not a natural person's name or email); whether it constitutes "personal
  data" under the applicable controller/processor relationship is a
  legal determination.

**This note does NOT decide the retention policy.** It flags the question for
the owner and privacy/legal:

> *Can `billing_events` rows for a tenant be deleted immediately on Art. 17
> request, or must they be retained (possibly in anonymized/aggregated form)
> for billing/tax compliance? If retained, for how long, and in what form?*

Until this is resolved the erasure implementation should not be shipped
without a legal sign-off on the retention posture.

---

## Dependencies and sequencing

1. **Org-wide erasure orchestration** — who calls the per-store erasure
   endpoints, in what order, and with what SLA. This repo provides the
   per-store delete; it does not own the orchestrator.
2. **Legal/product retention decision** (see above) — may change the
   implementation from a raw delete to a selective delete or aggregation step.
3. **CoreLink D-8 alignment** — the cache-layer erasure (D-8) and this
   runners-fabric erasure should be driven by the same orchestration call for
   a given tenant, so both stores are covered atomically from the tenant's
   perspective. Coordinate with the cache team when the orchestrator is
   designed.

---

## What is NOT a concern here

- **Cross-tenant blast.** The `WHERE tenant = $1` predicate is PK-prefix
  bounded; no other tenant's rows can be touched.
- **Replication lag.** Standard Postgres replication considerations apply;
  no runners-specific amplification.
- **The in-memory journal (`SlotMeter`).** Process-local, bounded, ephemeral.
  It dies with the process and is not a persistent store subject to Art. 17.

---

*This is a decision-record and tracking flag, not an implementation.
File against the erasure orchestration milestone when the SLA is set.*
