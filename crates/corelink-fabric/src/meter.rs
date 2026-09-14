//! Meter event schema (CF0 freeze item 5; BIL1 emits, CP1/ENV2 produce).
//!
//! **The SLOT is the billable unit — never minutes.** Concurrency pricing is
//! the decided, load-bearing principle: the customer buys N parallel runners
//! flat, minutes are unlimited; per-minute billing is the thing we are
//! replacing. Slot-occupancy events are the billing-relevant signal; COGS
//! counters are strictly internal.

use serde::{Deserialize, Serialize};

use crate::tenant::TenantId;

/// What happened to a billable slot (mirrors the ledger's contract §1
/// lifecycle: a slot is occupied at `Acquired` — the `Pending → Held`
/// transition — and freed by exactly one of the three terminal events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotEventKind {
    /// Lease became `Held`: one slot occupied.
    Acquired,
    /// Lease `Released`: slot freed cleanly.
    Released,
    /// Lease `Expired` (ttl, fail-closed kill): slot freed.
    Expired,
    /// Runner `Crashed` mid-job: slot freed.
    Crashed,
}

/// One slot-occupancy event — the billing-relevant meter (BIL1).
///
/// Billing reconciles slot occupancy against the lease ledger (held leases =
/// occupied slots); a memoized cache hit never acquires a slot, so it bills
/// zero by construction ("never charge for the customer's own compute twice").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotOccupancyEvent {
    /// Tenant whose slot this is (org = tenant, ADR-0002).
    pub tenant: TenantId,
    /// The lease occupying / freeing the slot.
    pub lease_id: String,
    /// What happened to the slot.
    pub kind: SlotEventKind,
    /// Unix epoch ms of the event.
    pub at_ms: u64,
    /// The lease's durable billing-acquire stamp (`LeaseRecord.billing_acquired_at_ms`),
    /// threaded onto a **terminal** event so the billing usage-push can compute
    /// `slot_seconds` even when the in-memory `Acquired→terminal` pairing was lost
    /// to a fabricd restart (revenue-loss fix #3). `None` on `Acquired` events and
    /// whenever the ledger has no stamp; the billing target prefers its in-memory
    /// pairing and falls back to this only on a miss. `#[serde(default)]` so older
    /// journaled/wire events (which never carried it) still deserialize as `None`.
    #[serde(default)]
    pub acquired_at_ms: Option<u64>,
}

/// Internal COGS counters — trust/audit vocabulary.
///
/// **NEVER a billable meter.**
///
/// These quantify what the fabric spent serving a job (cost of goods sold,
/// whitepaper §7 / contract §10 + §13.1's exact-integer pattern). They are
/// never surfaced as a customer meter and never appear on an invoice: the
/// billable unit is the SLOT (concurrency pricing, never per-minute).
/// `cost_usd_micros` follows the frozen `IntentMetrics.cost_usd_micros`
/// exact-integer convention (u64 micro-USD, no floats).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CogsCounters {
    /// CPU time consumed, milliseconds.
    pub cpu_ms: u64,
    /// Memory occupancy integral, MB·ms.
    pub mem_mb_ms: u64,
    /// Derived internal cost, exact-integer micro-USD (audit only).
    pub cost_usd_micros: u64,
}
