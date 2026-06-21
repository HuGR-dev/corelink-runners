//! Cross-instance fair-admission queue row contract (M1 WAVE-0 frozen anchor).
//!
//! The single-process [`FairScheduler`](crate::scheduler::FairScheduler) is the
//! in-memory deficit round-robin dispatcher; it is NOT cross-instance fair —
//! two control planes each running their own scheduler can starve a tenant the
//! other is serving. This module freezes the **durable queue row** that lets a
//! deficit-ordered admission queue be shared across instances in Postgres.
//!
//! ## Frozen skeleton only — impl is WP-CROSS-INSTANCE-QUEUE
//!
//! WAVE-0 declares the row type ([`PendingAdmission`]) + the table DDL
//! ([`PG_ADMISSION_QUEUE_DDL`]) so the WAVE-1/WAVE-2 work-packages compile
//! against a stable shape. There is deliberately NO enqueue / dequeue / repo
//! impl here yet (that is WP-CROSS-INSTANCE-QUEUE, which fills the
//! advisory-locked deficit-ordered dequeue against this exact DDL, mirroring
//! [`PgLedger`](crate::pg_ledger::PgLedger)).
//!
//! Ordering is `(deficit, enqueued_at_ms)`: lowest deficit first (the tenant
//! owed the most service), ties broken by oldest enqueue (FIFO within a deficit
//! tier) — the cross-instance analogue of the in-memory deficit round-robin.

use crate::tenant::TenantId;

/// One pending admission waiting in the cross-instance fair queue.
///
/// `lease_request_id` is the natural key (the table PRIMARY KEY). It is a
/// `String` to match the crate's `lease_id` convention everywhere
/// ([`LeaseRecord.lease_id`](crate::ledger::LeaseRecord) is `String`; there is
/// no `uuid` dependency in this crate — the id is opaque text on the wire).
///
/// `deficit` is the deficit-round-robin counter (lower = owed more service);
/// `(deficit, enqueued_at_ms)` is the total dequeue order.
//
// M1 WAVE-0 frozen anchor (admission queue) — impl in WP-CROSS-INSTANCE-QUEUE.
// No constructor / dequeue logic yet, so the fields read as dead until that WP
// wires the repo; the struct + DDL are the frozen seam dependents build on.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAdmission {
    /// Tenant the queued admission belongs to (org = tenant, ADR-0002).
    pub tenant: TenantId,
    /// Opaque admission-request id — the queue's natural key (PRIMARY KEY).
    pub lease_request_id: String,
    /// Unix epoch ms the request was enqueued (FIFO tiebreaker within a deficit).
    pub enqueued_at_ms: i64,
    /// Deficit-round-robin counter — lower = owed more service, dequeued first.
    pub deficit: i64,
}

/// Idempotent schema for the cross-instance fair-admission queue.
///
/// `IF NOT EXISTS` so it is safe to apply on every connect, mirroring the
/// [`PgLedger`](crate::pg_ledger) DDL convention. The dequeue index pins the
/// frozen `(deficit, enqueued_at_ms)` ordering — lowest deficit first, oldest
/// enqueue breaking ties.
//
// M1 WAVE-0 frozen anchor (admission queue) — impl in WP-CROSS-INSTANCE-QUEUE.
#[allow(dead_code)]
pub const PG_ADMISSION_QUEUE_DDL: &str = "\
CREATE TABLE IF NOT EXISTS pg_admission_queue (
  tenant           text   NOT NULL,
  lease_request_id text   PRIMARY KEY,
  enqueued_at_ms   bigint NOT NULL,
  deficit          bigint NOT NULL
);
CREATE INDEX IF NOT EXISTS pg_admission_queue_order_idx
  ON pg_admission_queue (deficit, enqueued_at_ms);
";
