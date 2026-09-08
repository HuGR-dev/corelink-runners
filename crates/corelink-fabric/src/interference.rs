//! CP4 — non-interference measurement SURFACE (contract §6, X6/X10).
//!
//! Per-tenant wait statistics that make "other-tenant latency unmoved under
//! the external consumer load" **provable, not assumed**: a bounded histogram plus
//! nearest-rank p50/p95 over each tenant's own completed dispatch waits.
//!
//! Composition with CP3: [`crate::scheduler::TickReport::waits_ms`] is the
//! raw feed — the composition root forwards each tick's `(tenant, wait_ms)`
//! pairs into [`TenantWaitStats::record`] (or wholesale via
//! [`TenantWaitStats::observe_tick`]). The per-tenant ring REUSES the
//! scheduler's bound ([`crate::scheduler::WAIT_RING_CAPACITY`]) and its
//! nearest-rank percentile method, so the two surfaces can never disagree on
//! method — only on window contents.
//!
//! # STRICT tenant scoping (the load-bearing property)
//!
//! [`TenantWaitStats::snapshot`] of tenant A is computed ONLY from samples
//! recorded for A. There is **no global ring, no shared counter, no
//! all-tenants aggregate** anywhere in this module: state is exactly one
//! bounded ring per tenant, keyed by [`TenantId`]. Recording any volume of
//! waits for tenant B therefore cannot move a single field of A's snapshot —
//! pinned by `snapshot_is_tenant_scoped_no_cross_leak` below and by the
//! CP4 acceptance suite (`acceptance_cp4.rs`).

use std::collections::{BTreeMap, VecDeque};

use crate::scheduler::{TickReport, WAIT_RING_CAPACITY};
use crate::tenant::TenantId;

/// Exclusive upper bounds (ms) of the first five histogram buckets:
/// `<10ms · <50ms · <250ms · <1s · <5s`; the sixth bucket is `>=5s`.
pub const WAIT_BUCKET_BOUNDS_MS: [u64; 5] = [10, 50, 250, 1_000, 5_000];

/// Point-in-time view of ONE tenant's wait distribution — the wire-facing
/// value the metrics endpoint serves (the server crate owns the DTO; this is
/// the pure-core shape).
///
/// All fields are computed over the same window: the tenant's last
/// [`WAIT_RING_CAPACITY`] recorded waits. `count == 0` means "no samples
/// yet"; the percentiles and histogram are then all zero (count is the
/// disambiguator between "no data" and "0 ms waits").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaitSnapshot {
    /// Nearest-rank p50 (median) wait, ms. 0 when `count == 0`.
    pub p50_ms: u64,
    /// Nearest-rank p95 wait, ms. 0 when `count == 0`.
    pub p95_ms: u64,
    /// Bucket counts: `[<10ms, <50ms, <250ms, <1s, <5s, >=5s]`
    /// (bounds in [`WAIT_BUCKET_BOUNDS_MS`]).
    pub histogram: [u64; 6],
    /// Samples in the window (≤ [`WAIT_RING_CAPACITY`]).
    pub count: u64,
}

/// Per-tenant wait recorder: one bounded ring per tenant, nothing shared.
///
/// Memory is O(tenants × [`WAIT_RING_CAPACITY`]), never O(history) — the
/// same bound the CP3 scheduler applies to its own ring.
#[derive(Debug, Clone, Default)]
pub struct TenantWaitStats {
    /// THE only state: tenant → bounded ring of that tenant's waits. No
    /// global accumulator exists (see module doc — strict tenant scoping).
    rings: BTreeMap<TenantId, VecDeque<u64>>,
}

impl TenantWaitStats {
    /// Empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one completed wait for `tenant` into ITS ring only.
    pub fn record(&mut self, tenant: &TenantId, wait_ms: u64) {
        let ring = self.rings.entry(tenant.clone()).or_default();
        if ring.len() == WAIT_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(wait_ms);
    }

    /// Record every wait a scheduler tick completed — the CP3 → CP4
    /// composition seam ([`TickReport::waits_ms`] is documented as this
    /// surface's raw feed).
    pub fn observe_tick(&mut self, report: &TickReport) {
        for (tenant, wait_ms) in &report.waits_ms {
            self.record(tenant, *wait_ms);
        }
    }

    /// Snapshot ONE tenant's distribution, computed ONLY from that tenant's
    /// own ring (strict scoping — module doc). A tenant that never recorded
    /// gets the zeroed snapshot (`count == 0`).
    pub fn snapshot(&self, tenant: &TenantId) -> WaitSnapshot {
        let Some(ring) = self.rings.get(tenant).filter(|r| !r.is_empty()) else {
            return WaitSnapshot::default();
        };
        let mut sorted: Vec<u64> = ring.iter().copied().collect();
        sorted.sort_unstable();
        let mut histogram = [0u64; 6];
        for &wait in &sorted {
            histogram[bucket_of(wait)] += 1;
        }
        WaitSnapshot {
            p50_ms: nearest_rank(&sorted, 50),
            p95_ms: nearest_rank(&sorted, 95),
            histogram,
            count: sorted.len() as u64,
        }
    }
}

/// Histogram bucket index for one wait (bounds are exclusive uppers).
fn bucket_of(wait_ms: u64) -> usize {
    WAIT_BUCKET_BOUNDS_MS
        .iter()
        .position(|&bound| wait_ms < bound)
        .unwrap_or(WAIT_BUCKET_BOUNDS_MS.len())
}

/// Nearest-rank percentile over a sorted, non-empty slice — the SAME method
/// as [`crate::scheduler::FairScheduler::p95_wait_ms`], so the two surfaces
/// can never diverge on percentile semantics.
fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = (sorted.len() * percentile).div_ceil(100).max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    /// Bucket edges are exact: each bound's predecessor lands below it, the
    /// bound itself lands in the next bucket, and >=5s is the catch-all.
    #[test]
    fn histogram_bucket_edges_are_exact() {
        let mut stats = TenantWaitStats::new();
        let a = tid("a");
        // One sample per side of every edge: 0|9 → b0, 10|49 → b1,
        // 50|249 → b2, 250|999 → b3, 1000|4999 → b4, 5000|90000 → b5.
        for w in [0, 9, 10, 49, 50, 249, 250, 999, 1_000, 4_999, 5_000, 90_000] {
            stats.record(&a, w);
        }
        assert_eq!(stats.snapshot(&a).histogram, [2, 2, 2, 2, 2, 2]);
        assert_eq!(stats.snapshot(&a).count, 12);
    }

    /// Strict scoping: any volume recorded for B moves NOTHING in A's
    /// snapshot — full-struct equality before/after.
    #[test]
    fn snapshot_is_tenant_scoped_no_cross_leak() {
        let mut stats = TenantWaitStats::new();
        let (a, b) = (tid("a"), tid("b"));
        for w in [5, 6, 7, 8, 9] {
            stats.record(&a, w);
        }
        let before = stats.snapshot(&a);
        for i in 0..10_000u64 {
            stats.record(&b, 7_000 + i);
        }
        assert_eq!(
            stats.snapshot(&a),
            before,
            "B's records must not move any field of A's snapshot"
        );
        // And the unseen tenant reads zeroed, never A's or B's data.
        assert_eq!(stats.snapshot(&tid("c")), WaitSnapshot::default());
    }

    /// The ring is bounded at the scheduler's capacity: old samples evict.
    #[test]
    fn ring_is_bounded_and_evicts_oldest() {
        let mut stats = TenantWaitStats::new();
        let a = tid("a");
        // Fill the ring with large waits, then overwrite with small ones.
        for _ in 0..WAIT_RING_CAPACITY {
            stats.record(&a, 9_999);
        }
        for _ in 0..WAIT_RING_CAPACITY {
            stats.record(&a, 1);
        }
        let snap = stats.snapshot(&a);
        assert_eq!(snap.count, WAIT_RING_CAPACITY as u64);
        assert_eq!(snap.histogram, {
            let mut h = [0u64; 6];
            h[0] = WAIT_RING_CAPACITY as u64;
            h
        });
        assert_eq!((snap.p50_ms, snap.p95_ms), (1, 1));
    }

    /// Percentiles use nearest-rank, byte-for-byte the scheduler's method:
    /// 100..=2000 step 100 → p95 = 1900 (the scheduler's own pinned case)
    /// and p50 = 1000.
    #[test]
    fn percentiles_match_scheduler_nearest_rank() {
        let mut stats = TenantWaitStats::new();
        let a = tid("a");
        for i in 1..=20u64 {
            stats.record(&a, i * 100);
        }
        let snap = stats.snapshot(&a);
        assert_eq!(snap.p95_ms, 1_900);
        assert_eq!(snap.p50_ms, 1_000);
    }

    /// `observe_tick` feeds every (tenant, wait) pair of a tick report into
    /// the per-tenant rings — the CP3 composition seam.
    #[test]
    fn observe_tick_records_per_tenant() {
        let mut stats = TenantWaitStats::new();
        let report = TickReport {
            dispatched: vec!["x".into(), "y".into()],
            skipped_over_cap: 0,
            waits_ms: vec![(tid("a"), 40), (tid("b"), 400)],
        };
        stats.observe_tick(&report);
        assert_eq!(stats.snapshot(&tid("a")).histogram, [0, 1, 0, 0, 0, 0]);
        assert_eq!(stats.snapshot(&tid("b")).histogram, [0, 0, 0, 1, 0, 0]);
    }

    /// No samples → the zeroed snapshot, count disambiguates.
    #[test]
    fn empty_snapshot_is_zeroed() {
        let stats = TenantWaitStats::new();
        let snap = stats.snapshot(&tid("ghost"));
        assert_eq!(snap, WaitSnapshot::default());
        assert_eq!(snap.count, 0);
    }
}
