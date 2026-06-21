//! Billing export-target seam (M1 WAVE-0 frozen anchor) — the vendor/Stripe edge.
//!
//! [`BillingSink`](crate::billing_sink::BillingSink) is the DURABLE store for
//! raw [`SlotOccupancyEvent`]s (Postgres `billing_events`, exactly-once by
//! PRIMARY KEY). [`BillingExportTarget`] is the COMPLEMENTARY seam: pushing a
//! billing event OUT to an external billing system (Stripe / a metering vendor)
//! for the concurrency SKUs. Generic over the event — no vendor coupling in
//! this crate.
//!
//! ## Reuses the frozen event type
//!
//! The exported event IS [`SlotOccupancyEvent`] (the slot is the billable unit —
//! concurrency pricing, never per-minute). This module imports it; it never
//! redefines a billing-event type. The target performs NO duration / cost /
//! minutes arithmetic here — it forwards the raw occupancy event; any vendor-
//! side metering is the vendor adapter's concern (WP-BILLING-TARGET).
//!
//! ## Sync trait, fail-closed
//!
//! Sync (`&self`, `anyhow::Result`) to match the crate's other persistence
//! seams ([`BillingSink`](crate::billing_sink::BillingSink) /
//! [`LeaseLedger`](crate::ledger::LeaseLedger)). The default-off impl is
//! [`NoopBillingTarget`]; the real vendor adapter lands in WP-BILLING-TARGET.

use crate::meter::SlotOccupancyEvent;

/// Pushes a billing event OUT to an external billing system (the Stripe/vendor
/// seam). Generic over the event; one event per call so a target may batch or
/// stream as it sees fit.
///
/// Fail-closed by the crate convention: a vendor / transport error is an `Err`
/// (the caller decides retry / dead-letter), never a silent drop.
pub trait BillingExportTarget {
    /// Export one raw slot-occupancy event to the external billing system.
    fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()>;
}

/// The default-OFF [`BillingExportTarget`]: logs the event and succeeds.
///
/// The composition root wires this until a real vendor adapter (WP-BILLING-
/// TARGET) is configured — so the export path is present and exercised end to
/// end without coupling the fabric to any billing vendor. It performs NO cost /
/// minutes arithmetic (raw occupancy only).
#[derive(Debug, Default)]
pub struct NoopBillingTarget;

impl NoopBillingTarget {
    /// A fresh no-op target.
    pub fn new() -> Self {
        Self
    }
}

impl BillingExportTarget for NoopBillingTarget {
    fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()> {
        // Default-off: observe the event and succeed. No vendor call, no cost
        // math — just a trace so the export path is visible in dev/test.
        eprintln!(
            "NoopBillingTarget: export tenant={} lease_id={} kind={:?} at_ms={}",
            event.tenant.as_str(),
            event.lease_id,
            event.kind,
            event.at_ms,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meter::SlotEventKind;
    use crate::tenant::TenantId;

    /// The no-op target accepts any well-formed event and reports success — the
    /// default-off export path never fails on a valid event.
    #[test]
    fn noop_target_exports_ok() {
        let target = NoopBillingTarget::new();
        let event = SlotOccupancyEvent {
            tenant: TenantId::new("acme").expect("valid tenant id"),
            lease_id: "lease-1".to_string(),
            kind: SlotEventKind::Acquired,
            at_ms: 1_000,
        };
        target.export(&event).expect("noop export succeeds");
    }
}
