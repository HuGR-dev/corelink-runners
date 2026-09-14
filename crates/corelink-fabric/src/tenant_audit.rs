//! Tenant lifecycle audit row contract (M1 WAVE-0 frozen anchor).
//!
//! Every tenant tier change (create / upgrade / downgrade) on the admin
//! lifecycle API must leave a durable, attributable trail: WHICH key made the
//! change, for WHICH tenant, from WHICH tier to WHICH, WHEN. This module freezes
//! the audit row ([`TenantAuditRow`]) + the table DDL ([`TENANT_AUDIT_DDL`]) so
//! WP-TENANT-LIFECYCLE-API builds against a stable shape.
//!
//! ## Frozen skeleton only — impl is WP-TENANT-LIFECYCLE-API
//!
//! WAVE-0 declares the type + DDL; there is no append / query repo here yet
//! (that is WP-TENANT-LIFECYCLE-API, mirroring
//! [`PgLedger`](crate::pg_ledger::PgLedger) for the insert path).
//!
//! `old_tier` is `Option<String>` — `None` on a tenant's first plan (creation),
//! `Some(prev)` on a change. Tiers are stored as their flat string token (the
//! same vocabulary the `tenant_plans.tier` column carries), so the frozen
//! plan/tier types need no DB derive.

use crate::tenant::TenantId;

/// One tenant-tier-change audit entry — the durable lifecycle trail.
//
// M1 WAVE-0 frozen anchor (tenant_audit) — impl in WP-TENANT-LIFECYCLE-API.
// The append/query repo (and thus the field reads) land in that WP; the struct
// + DDL are the frozen seam dependents build against.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantAuditRow {
    /// The PAT / admin key id that made the change (attribution).
    pub key_id: String,
    /// The tenant whose tier changed (org = tenant, ADR-0002).
    pub tenant: TenantId,
    /// The prior tier token — `None` on a tenant's first plan (creation).
    pub old_tier: Option<String>,
    /// The new tier token after the change.
    pub new_tier: String,
    /// Unix epoch ms the change was recorded.
    pub at_ms: i64,
}

/// Idempotent schema for the tenant-lifecycle audit trail.
///
/// `IF NOT EXISTS`, safe to apply on every connect (mirrors the
/// [`PgLedger`](crate::pg_ledger) DDL convention). Append-only: a synthetic
/// `id bigserial` PRIMARY KEY (a single tenant has many ordered changes — the
/// natural key is not the tenant), indexed by `(tenant, at_ms)` for the
/// per-tenant chronological lifecycle view.
//
// M1 WAVE-0 frozen anchor (tenant_audit) — impl in WP-TENANT-LIFECYCLE-API.
#[allow(dead_code)]
pub const TENANT_AUDIT_DDL: &str = "\
CREATE TABLE IF NOT EXISTS tenant_audit (
  id        bigserial PRIMARY KEY,
  key_id    text      NOT NULL,
  tenant    text      NOT NULL,
  old_tier  text,
  new_tier  text      NOT NULL,
  at_ms     bigint    NOT NULL
);
CREATE INDEX IF NOT EXISTS tenant_audit_tenant_idx
  ON tenant_audit (tenant, at_ms);
";
