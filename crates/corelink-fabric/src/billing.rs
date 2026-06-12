//! Slot-occupancy metering (BIL1) — **concurrency, never minutes**.
//!
//! THE INVARIANT: billing counts SLOTS (parallel runners held), never
//! duration. This type deliberately has NO duration/minutes accumulator —
//! there is no timestamp arithmetic anywhere in it; `at_ms` is journaled for
//! audit and never subtracted. A memoized / cache-hit job that never acquires
//! a slot produces no [`SlotOccupancyEvent`]s and therefore meters nothing
//! ("never charge for the customer's own compute twice" — memoized bills
//! zero by construction). Per-minute billing is the thing we are replacing
//! (whitepaper §7; contract §10); the customer buys `max_concurrency`, flat,
//! and minutes are unlimited.

use std::collections::BTreeMap;

use crate::meter::{SlotEventKind, SlotOccupancyEvent};
use crate::tenant::TenantId;

/// Per-tenant slot-occupancy meter over the frozen [`SlotOccupancyEvent`]
/// schema (CF0 freeze item 5).
///
/// Maintains, per tenant:
/// - **current occupied slots** — `Acquired` +1; `Released` / `Expired` /
///   `Crashed` −1, saturating (never negative: a spurious free clamps to 0,
///   fail-closed in the customer's favor);
/// - **peak slots** — the high-water mark of concurrent occupancy (the
///   number that reconciles against the plan's `max_concurrency`);
/// - an **append-only journal** of every event recorded, in arrival order
///   (the audit trail billing reconciles against the lease ledger).
///
/// There is intentionally no field here that accumulates elapsed time: the
/// slot is the billable unit, full stop.
#[derive(Debug, Default)]
pub struct SlotMeter {
    /// Currently occupied slots per tenant.
    occupied: BTreeMap<TenantId, u32>,
    /// High-water mark of concurrent occupancy per tenant.
    peak: BTreeMap<TenantId, u32>,
    /// Append-only event journal, arrival order.
    journal: Vec<SlotOccupancyEvent>,
}

impl SlotMeter {
    /// A fresh meter: every tenant at zero, empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one slot-occupancy event: update the tenant's occupancy and
    /// high-water mark, and append the event to the journal unconditionally
    /// (even a spurious free that clamps at 0 is journaled — the journal is
    /// the audit record of what was reported, not of what was counted).
    pub fn record(&mut self, ev: SlotOccupancyEvent) {
        let occ = self.occupied.entry(ev.tenant.clone()).or_default();
        match ev.kind {
            SlotEventKind::Acquired => {
                *occ = occ.saturating_add(1);
                let peak = self.peak.entry(ev.tenant.clone()).or_default();
                *peak = (*peak).max(*occ);
            }
            // All three terminal lifecycle events free the slot identically;
            // saturating: occupancy never goes negative.
            SlotEventKind::Released | SlotEventKind::Expired | SlotEventKind::Crashed => {
                *occ = occ.saturating_sub(1);
            }
        }
        self.journal.push(ev);
    }

    /// Currently occupied slots for `t` (0 if the tenant has never metered —
    /// the memoized / zero-slot case).
    pub fn occupied(&self, t: &TenantId) -> u32 {
        self.occupied.get(t).copied().unwrap_or(0)
    }

    /// High-water mark of concurrent slots for `t` (0 if never metered).
    pub fn peak(&self, t: &TenantId) -> u32 {
        self.peak.get(t).copied().unwrap_or(0)
    }

    /// The append-only journal of every event recorded, in arrival order.
    pub fn journal(&self) -> &[SlotOccupancyEvent] {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(s: &str) -> TenantId {
        TenantId::new(s).expect("valid tenant id")
    }

    fn ev(t: &TenantId, lease: &str, kind: SlotEventKind, at_ms: u64) -> SlotOccupancyEvent {
        SlotOccupancyEvent {
            tenant: t.clone(),
            lease_id: lease.to_string(),
            kind,
            at_ms,
        }
    }

    /// Two leases with wildly different durations (1 ms vs ~115 days implied
    /// by the timestamps) meter IDENTICALLY: one slot acquired, one slot
    /// freed. Duration is invisible to the meter — and the type itself has
    /// no field that could encode it (source-inclusion check below).
    #[test]
    fn slots_count_concurrency_not_minutes() {
        let t = tenant("acme");

        let mut short = SlotMeter::new();
        short.record(ev(&t, "lease-short", SlotEventKind::Acquired, 1_000));
        short.record(ev(&t, "lease-short", SlotEventKind::Released, 1_001));

        let mut long = SlotMeter::new();
        long.record(ev(&t, "lease-long", SlotEventKind::Acquired, 1_000));
        long.record(ev(
            &t,
            "lease-long",
            SlotEventKind::Released,
            10_000_000_000,
        ));

        // Identical metering despite a ~10^7x difference in held time.
        assert_eq!(short.occupied(&t), long.occupied(&t));
        assert_eq!(short.peak(&t), long.peak(&t));
        assert_eq!(short.occupied(&t), 0);
        assert_eq!(short.peak(&t), 1);

        // Source-inclusion oracle: in the production code of this module
        // (everything before the test cfg attribute), the word built by
        // `dur_needle` never appears outside comments — there is no
        // duration field — and the word built by `min_needle` appears only
        // in doc lines / comments, never in code.
        let source = include_str!("billing.rs");
        let marker = ["#[cfg(te", "st)]"].concat();
        let production = source
            .split(&marker)
            .next()
            .expect("split always yields at least one piece");
        let dur_needle = ["dur", "ation"].concat();
        let min_needle = ["min", "utes"].concat();
        for line in production.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue; // doc comment or comment: prose may say the words
            }
            // Strip any trailing line comment; what's left is code.
            let code = trimmed.split("//").next().unwrap_or("");
            assert!(
                !code.to_lowercase().contains(&dur_needle),
                "billing meter must have no duration field/code: {line:?}"
            );
            assert!(
                !code.to_lowercase().contains(&min_needle),
                "billing meter code must not reference minutes: {line:?}"
            );
        }
    }

    #[test]
    fn release_and_expiry_and_crash_all_free_the_slot() {
        let t = tenant("acme");
        for free in [
            SlotEventKind::Released,
            SlotEventKind::Expired,
            SlotEventKind::Crashed,
        ] {
            let mut m = SlotMeter::new();
            m.record(ev(&t, "lease-1", SlotEventKind::Acquired, 1));
            assert_eq!(m.occupied(&t), 1);
            m.record(ev(&t, "lease-1", free, 2));
            assert_eq!(m.occupied(&t), 0, "{free:?} must free the slot");
            assert_eq!(m.peak(&t), 1, "peak survives the free");
        }
    }

    #[test]
    fn meter_never_goes_negative() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        // Release without a prior Acquire: clamps at 0, never negative.
        m.record(ev(&t, "lease-ghost", SlotEventKind::Released, 1));
        assert_eq!(m.occupied(&t), 0);
        m.record(ev(&t, "lease-ghost-2", SlotEventKind::Expired, 2));
        m.record(ev(&t, "lease-ghost-3", SlotEventKind::Crashed, 3));
        assert_eq!(m.occupied(&t), 0);
        assert_eq!(m.peak(&t), 0);
        // The journal still records every reported event (audit trail).
        assert_eq!(m.journal().len(), 3);
        assert_eq!(m.journal()[0].lease_id, "lease-ghost");
        // A subsequent real acquire still counts correctly from 0.
        m.record(ev(&t, "lease-real", SlotEventKind::Acquired, 4));
        assert_eq!(m.occupied(&t), 1);
        assert_eq!(m.peak(&t), 1);
    }

    #[test]
    fn peak_is_high_water_mark() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        m.record(ev(&t, "l1", SlotEventKind::Acquired, 1));
        m.record(ev(&t, "l2", SlotEventKind::Acquired, 2));
        m.record(ev(&t, "l3", SlotEventKind::Acquired, 3));
        assert_eq!(m.occupied(&t), 3);
        assert_eq!(m.peak(&t), 3);
        m.record(ev(&t, "l2", SlotEventKind::Released, 4));
        m.record(ev(&t, "l3", SlotEventKind::Crashed, 5));
        assert_eq!(m.occupied(&t), 1);
        assert_eq!(m.peak(&t), 3, "peak holds after frees");
        m.record(ev(&t, "l4", SlotEventKind::Acquired, 6));
        assert_eq!(m.occupied(&t), 2);
        assert_eq!(m.peak(&t), 3, "re-acquiring below the peak never lowers it");
    }

    /// A memoized / cache-hit job never acquires a slot, so it emits no
    /// events — and a tenant with no events meters NOTHING: occupied 0,
    /// peak 0, no journal entries. Memoized bills zero by construction.
    #[test]
    fn memoized_zero_slot_job_meters_nothing() {
        let busy = tenant("busy-tenant");
        let memoized = tenant("memoized-tenant");
        let mut m = SlotMeter::new();
        m.record(ev(&busy, "l1", SlotEventKind::Acquired, 1));
        m.record(ev(&busy, "l1", SlotEventKind::Released, 2));

        assert_eq!(m.occupied(&memoized), 0);
        assert_eq!(m.peak(&memoized), 0);
        assert!(
            m.journal().iter().all(|e| e.tenant != memoized),
            "journal must be empty for the zero-slot tenant"
        );
    }
}
