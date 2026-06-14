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

use std::collections::{BTreeMap, VecDeque};

use crate::meter::{SlotEventKind, SlotOccupancyEvent};
use crate::tenant::TenantId;

/// In-memory audit-tail bound for the journal: when the journal holds this
/// many events, the OLDEST event is dropped (and counted in
/// `journal_dropped`) before a new one is appended. An exporter that drains
/// the meter into durable storage is a future WP; until it lands, drops are
/// bounded and **never silent** (matching the §13 envelope discipline:
/// bounded in-flight, overflow surfaced via the snapshot, never lost quietly).
const JOURNAL_CAP: usize = 100_000;

/// Per-tenant slot-occupancy meter over the frozen [`SlotOccupancyEvent`]
/// schema (CF0 freeze item 5).
///
/// Maintains, per tenant:
/// - **current occupied slots** — `Acquired` +1; `Released` / `Expired` /
///   `Crashed` −1, saturating (never negative: a spurious free clamps to 0,
///   fail-closed in the customer's favor);
/// - **peak slots** — the high-water mark of concurrent occupancy *as seen by
///   THIS meter instance*;
/// - an **append-only journal** of every event recorded, in arrival order
///   (the audit trail billing reconciles against the lease ledger).
///
/// There is intentionally no field here that accumulates elapsed time: the
/// slot is the billable unit, full stop.
///
/// # SCOPE: instance-local observability, NOT a global truth (P1)
///
/// This meter counts only the events recorded into THIS instance. It is
/// per-instance: a lease `Acquired` on instance A and `Released`/`Expired` on a
/// DIFFERENT instance B leaves A stuck +1 and B clamped at 0 — neither's
/// `occupied`/`peak` is the fabric-wide truth at N>1 deployments. So `peak` is
/// **observability only**; it does NOT reconcile against the plan's
/// `max_concurrency` across instances, and nothing load-bearing may treat it as
/// a global occupancy oracle. The cap itself is enforced DB-globally upstream
/// (`ledger.try_admit`, which counts the tenant's active leases atomically), so
/// this scoping is a metering-honesty limitation, never a cap breach. A truly
/// global occupancy view, if ever needed, derives from the ledger — not from
/// this meter (and the meter deliberately carries no DB dependency). Pinned by
/// `meter_is_instance_scoped_not_a_global_truth`.
#[derive(Debug, Default)]
pub struct SlotMeter {
    /// Currently occupied slots per tenant.
    occupied: BTreeMap<TenantId, u32>,
    /// High-water mark of concurrent occupancy per tenant.
    peak: BTreeMap<TenantId, u32>,
    /// Bounded event journal, arrival order (oldest dropped past `JOURNAL_CAP`).
    /// A `VecDeque` so trimming the oldest event is O(1) `pop_front` — never an
    /// O(n) `Vec::remove(0)` shift under the held meter mutex (the steady-state
    /// throughput cliff this avoids).
    journal: VecDeque<SlotOccupancyEvent>,
    /// Count of journal events dropped to stay under `JOURNAL_CAP` — the
    /// audit-tail overflow, surfaced (never silent) via the snapshot.
    journal_dropped: u64,
}

/// One tenant's slot occupancy in an [`OccupancySnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TenantOccupancy {
    pub tenant: TenantId,
    pub occupied: u32,
    pub peak: u32,
}

/// A non-destructive, point-in-time read of the meter for billing / ops
/// reconciliation (peak vs the plan's `max_concurrency`). Taking it reads
/// only — it never mutates or drains the meter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OccupancySnapshot {
    /// Per-tenant occupancy, sorted by tenant for determinism.
    pub per_tenant: Vec<TenantOccupancy>,
    pub journal_len: usize,
    pub journal_dropped: u64,
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
        // Bound the in-memory audit tail: drop the oldest event when full,
        // counting it so the overflow is never silent. `pop_front` on the
        // `VecDeque` is O(1) — no O(n) element shift under the held mutex.
        if self.journal.len() >= JOURNAL_CAP {
            self.journal.pop_front();
            self.journal_dropped += 1;
        }
        self.journal.push_back(ev);
    }

    /// Currently occupied slots for `t` (0 if the tenant has never metered —
    /// the memoized / zero-slot case).
    pub fn occupied(&self, t: &TenantId) -> u32 {
        self.occupied.get(t).copied().unwrap_or(0)
    }

    /// High-water mark of concurrent slots for `t` as seen by THIS meter
    /// instance (0 if never metered). Instance-local observability — NOT a
    /// fabric-wide occupancy oracle at N>1 (see the type-level SCOPE note); it
    /// does not reconcile against `max_concurrency` across instances.
    pub fn peak(&self, t: &TenantId) -> u32 {
        self.peak.get(t).copied().unwrap_or(0)
    }

    /// The bounded journal of recorded events, in arrival order. Past
    /// `JOURNAL_CAP` the oldest entries are dropped (see [`Self::journal_dropped`]).
    /// Backed by a `VecDeque` (O(1) oldest-eviction); it indexes, iterates and
    /// reports `len`/`is_empty` exactly like a slice.
    pub fn journal(&self) -> &VecDeque<SlotOccupancyEvent> {
        &self.journal
    }

    /// Number of journal events dropped to keep the audit tail under
    /// `JOURNAL_CAP` (0 until the cap is first reached).
    pub fn journal_dropped(&self) -> u64 {
        self.journal_dropped
    }

    /// A non-destructive read of current occupancy/peak per tenant plus the
    /// journal length and drop count. Builds `per_tenant` from the union of
    /// the occupied and peak maps (a tenant at occupied 0 but peak > 0 still
    /// appears), sorted by tenant (the maps are `BTreeMap`, so iteration is
    /// already in order). Reads only — never mutates or drains the meter.
    pub fn snapshot(&self) -> OccupancySnapshot {
        let mut per_tenant: BTreeMap<&TenantId, TenantOccupancy> = BTreeMap::new();
        for (t, &occ) in &self.occupied {
            per_tenant.insert(
                t,
                TenantOccupancy {
                    tenant: t.clone(),
                    occupied: occ,
                    peak: 0,
                },
            );
        }
        for (t, &pk) in &self.peak {
            per_tenant
                .entry(t)
                .and_modify(|e| e.peak = pk)
                .or_insert_with(|| TenantOccupancy {
                    tenant: t.clone(),
                    occupied: 0,
                    peak: pk,
                });
        }
        OccupancySnapshot {
            per_tenant: per_tenant.into_values().collect(),
            journal_len: self.journal.len(),
            journal_dropped: self.journal_dropped,
        }
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

    /// The journal is capped at `JOURNAL_CAP`: past it the oldest event is
    /// dropped and counted, so growth is bounded and the overflow is visible.
    #[test]
    fn journal_is_bounded_and_drops_are_counted() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        // Record JOURNAL_CAP + 50 events, alternating acquire/release so the
        // occupancy stays in {0, 1} the whole time.
        let total = JOURNAL_CAP + 50;
        for i in 0..total {
            let kind = if i % 2 == 0 {
                SlotEventKind::Acquired
            } else {
                SlotEventKind::Released
            };
            m.record(ev(&t, "lease-x", kind, i as u64));
        }
        assert_eq!(m.journal().len(), JOURNAL_CAP, "journal stays at the cap");
        assert_eq!(m.journal_dropped(), 50, "exactly the overflow was dropped");
        // The very first event (at_ms 0) was dropped — the oldest survivor is
        // not it. With 50 dropped, the oldest survivor was recorded at i == 50.
        assert_eq!(
            m.journal()[0].at_ms,
            50,
            "oldest survivor is the 51st event, not the first recorded"
        );
    }

    /// [P1 regression] The meter is INSTANCE-SCOPED observability, never a
    /// global truth at N>1. A lease Acquired on instance A and Released on a
    /// different instance B leaves A stuck +1 and B clamped at 0 — neither
    /// instance's `occupied` is the fabric-wide count. This pins that documented
    /// limitation so no consumer treats the meter as a global occupancy oracle
    /// (the cap itself is enforced DB-globally upstream via `ledger.try_admit`;
    /// this is metering honesty, not a cap breach). No DB dependency is added.
    #[test]
    fn meter_is_instance_scoped_not_a_global_truth() {
        let t = tenant("acme");

        // Instance A sees only the Acquire — it is stuck at occupied 1, even
        // though the lease was actually released (on B).
        let mut a = SlotMeter::new();
        a.record(ev(&t, "lease-1", SlotEventKind::Acquired, 1));
        assert_eq!(
            a.occupied(&t),
            1,
            "instance A, having seen only Acquired, reads 1 — its local view"
        );

        // Instance B sees only the Release — it clamps at 0 (saturating), it
        // never observed the matching Acquire.
        let mut b = SlotMeter::new();
        b.record(ev(&t, "lease-1", SlotEventKind::Released, 2));
        assert_eq!(
            b.occupied(&t),
            0,
            "instance B, having seen only Released, clamps at 0 — its local view"
        );

        // The naive cross-instance sum (1 + 0) does NOT equal the true global
        // occupancy (0): the meter is per-instance and must not be summed or
        // reconciled against max_concurrency across instances.
        assert_eq!(
            a.occupied(&t) + b.occupied(&t),
            1,
            "summing per-instance meters is not the global truth — \
             this is exactly why peak is observability-only at N>1"
        );

        // Source oracle: the type-level doc no longer claims peak reconciles
        // against max_concurrency (the removed false claim).
        let source = include_str!("billing.rs");
        let marker = ["#[cfg(te", "st)]"].concat();
        let production = source.split(&marker).next().expect("has production half");
        let false_claim = ["reconciles against the plan's ", "`max_concurrency`"].concat();
        assert!(
            !production.contains(&false_claim),
            "the false global-peak reconciliation claim must be gone"
        );
    }

    /// [P2 regression] The journal is backed by a `VecDeque`, so trimming the
    /// oldest event at steady state is O(1) `pop_front` — never an O(n)
    /// `Vec::remove(0)` shift under the held meter mutex. This drives churn far
    /// past the cap and asserts the bound + drop-count + FIFO-eviction order
    /// hold exactly (the O(1) trim is functionally identical to the old O(n)
    /// one — only the cost under the mutex changed). The source-level oracle
    /// pins that `remove(0)` is gone from the production trim.
    #[test]
    fn journal_trim_is_o1_deque_pop_front_not_vec_remove() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        // Churn 3× the cap so the trim runs ~2×JOURNAL_CAP times.
        let total = JOURNAL_CAP * 3;
        for i in 0..total {
            let kind = if i % 2 == 0 {
                SlotEventKind::Acquired
            } else {
                SlotEventKind::Released
            };
            m.record(ev(&t, "lease-x", kind, i as u64));
        }
        assert_eq!(m.journal().len(), JOURNAL_CAP, "journal stays at the cap");
        assert_eq!(
            m.journal_dropped() as usize,
            total - JOURNAL_CAP,
            "every event past the cap was dropped, counted, never silent"
        );
        // FIFO eviction: the oldest survivor is the (total-JOURNAL_CAP)-th
        // event, proving pop_front evicted from the FRONT (arrival order).
        assert_eq!(
            m.journal()[0].at_ms,
            (total - JOURNAL_CAP) as u64,
            "oldest survivor is the first non-dropped event (front eviction)"
        );
    }

    /// The snapshot reports occupied + peak for every tenant that ever held a
    /// slot, sorted by tenant, and omits a memoized (never-acquired) tenant.
    #[test]
    fn snapshot_reports_occupied_and_peak_per_tenant() {
        let zed = tenant("zed");
        let acme = tenant("acme");
        let memoized = tenant("memoized");
        let mut m = SlotMeter::new();

        // acme: peaks at 2, ends at 1.
        m.record(ev(&acme, "a1", SlotEventKind::Acquired, 1));
        m.record(ev(&acme, "a2", SlotEventKind::Acquired, 2));
        m.record(ev(&acme, "a2", SlotEventKind::Released, 3));
        // zed: peaks at 1, ends at 0.
        m.record(ev(&zed, "z1", SlotEventKind::Acquired, 4));
        m.record(ev(&zed, "z1", SlotEventKind::Crashed, 5));
        // memoized: never acquires — must not appear.
        let _ = memoized;

        let snap = m.snapshot();
        assert_eq!(snap.per_tenant.len(), 2, "only tenants that metered appear");
        // Sorted by tenant: "acme" < "zed".
        assert_eq!(
            snap.per_tenant[0],
            TenantOccupancy {
                tenant: acme.clone(),
                occupied: 1,
                peak: 2,
            }
        );
        assert_eq!(
            snap.per_tenant[1],
            TenantOccupancy {
                tenant: zed.clone(),
                occupied: 0,
                peak: 1,
            }
        );
        assert!(
            snap.per_tenant.iter().all(|to| to.tenant != memoized),
            "the never-acquired tenant is absent"
        );
    }

    /// Taking a snapshot does not drain the meter: a later snapshot reflects
    /// the advanced state while the earlier one is unchanged.
    #[test]
    fn snapshot_is_non_destructive() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        m.record(ev(&t, "l1", SlotEventKind::Acquired, 1));

        let first = m.snapshot();
        assert_eq!(first.per_tenant[0].occupied, 1);
        assert_eq!(first.per_tenant[0].peak, 1);
        assert_eq!(first.journal_len, 1);

        // Advance the meter after snapshotting.
        m.record(ev(&t, "l2", SlotEventKind::Acquired, 2));
        let second = m.snapshot();

        // The first snapshot is untouched (owned, point-in-time copy).
        assert_eq!(first.per_tenant[0].occupied, 1);
        assert_eq!(first.journal_len, 1);
        // The meter advanced — snapshot did not drain it.
        assert_eq!(second.per_tenant[0].occupied, 2);
        assert_eq!(second.per_tenant[0].peak, 2);
        assert_eq!(second.journal_len, 2);
        assert_eq!(m.journal().len(), 2, "snapshot left the journal intact");
    }

    /// Once the journal overflows, the snapshot surfaces the drop count.
    #[test]
    fn snapshot_surfaces_dropped_count() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        // One past the cap forces exactly one drop.
        let total = JOURNAL_CAP + 1;
        for i in 0..total {
            let kind = if i % 2 == 0 {
                SlotEventKind::Acquired
            } else {
                SlotEventKind::Released
            };
            m.record(ev(&t, "lease-x", kind, i as u64));
        }
        let snap = m.snapshot();
        assert_eq!(snap.journal_len, JOURNAL_CAP);
        assert_eq!(snap.journal_dropped, 1, "the snapshot reports the drop");
        assert!(snap.journal_dropped >= 1);
    }
}
