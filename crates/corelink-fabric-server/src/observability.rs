//! Golden-signal counters — a lock-free, always-on operational metric surface.
//!
//! A Stage-C observability primitive: a fixed set of process-lifetime
//! monotonic counters incremented at the load-bearing seams (acquire outcomes,
//! close, mint/revoke, provision capacity, agent-exec, load-shed, suspend). The
//! snapshot rides the already-gated `GET /internal/v1/status` aggregate
//! ([`crate::handlers::status`]) — one obs-key-gated read answers "is the fabric
//! admitting, rejecting, and why, and are the credential legs failing" WITHOUT
//! a shell into the container or a per-tenant scrape.
//!
//! ## Design (deliberate, no framework)
//! - **No new dependency, no metrics runtime.** Each counter is a single
//!   [`std::sync::atomic::AtomicU64`] behind the [`Counter`] newtype; increment
//!   is a relaxed `fetch_add(1)` on the hot path (never a lock, never an
//!   allocation). This is the JSON-light posture the golden-counters wave
//!   ratified — not Prometheus, not `tracing`.
//! - **Labels are fields, not a map.** A labeled signal (an acquire rejection
//!   *reason*) is modelled as one explicit named counter per reason, so a call
//!   site is a greppable `state.counters.acquire_rejected_over_cap.incr()` and
//!   the snapshot has a fixed, wire-stable shape — no dynamic label cardinality,
//!   no lock over a `HashMap`.
//! - **Monotonic, process-lifetime.** Counters only ever increase; they reset on
//!   restart (like the in-memory ledger). A monitor computes rates by diffing
//!   snapshots over time. Non-tenant, non-secret — safe for the ops surface.
//! - **Additive / default-safe.** Wiring these increments changes NO control
//!   flow: an `incr()` is a pure side effect next to an existing branch. The
//!   snapshot is only reachable through the obs-key gate (404 until armed).

use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// A single lock-free, process-lifetime monotonic counter.
///
/// [`incr`](Self::incr) is a relaxed `fetch_add(1)` — the ordering is
/// deliberately `Relaxed`: these are independent statistics with no
/// happens-before relationship to guard, so the cheapest atomic is correct.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    /// Increment by one. Hot-path safe: no lock, no allocation.
    #[inline]
    pub fn incr(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the current value for a snapshot.
    #[inline]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// The fixed set of golden-signal counters. Held as `Arc<Counters>` in
/// [`crate::app::AppState`] and shared across every handler clone.
///
/// Field groups mirror the seams the go-live recon mapped: **admission**
/// (acquired + one counter per rejection reason), **lifecycle** (closed,
/// provision-capacity 503), **credential** (mint/revoke attempt+failure),
/// **agent-exec** (started/done/failed), and **operator** (load-shed 503,
/// trigger dedup, suspend actions).
#[derive(Debug, Default)]
pub struct Counters {
    // ── Admission ────────────────────────────────────────────────────────────
    /// Leases that reached `Held` (a box was provisioned + admitted).
    pub leases_acquired: Counter,
    /// Acquire rejected: tenant is operator-suspended (403).
    pub acquire_rejected_suspended: Counter,
    /// Acquire rejected: image not pinned / not allowlisted (400).
    pub acquire_rejected_invalid_image: Counter,
    /// Acquire rejected: malformed request / runner-mode-off / bad field (400).
    pub acquire_rejected_bad_request: Counter,
    /// Acquire rejected: per-tenant 60s rate ceiling (429).
    pub acquire_rejected_rate: Counter,
    /// Acquire rejected: concurrency cap reached (429).
    pub acquire_rejected_over_cap: Counter,
    /// Acquire rejected: tenant has NO plan on file → zero purchased concurrency
    /// slots (a mis-provisioned tenant, NOT genuine saturation). Kept DISTINCT
    /// from [`acquire_rejected_over_cap`](Self::acquire_rejected_over_cap) so a
    /// config gap (no plan) is never read as a busy tenant hitting its cap — the
    /// de-smear of the `leases.rs` no-plan site (WP-3b).
    pub acquire_rejected_no_plan: Counter,
    /// Acquire rejected: monthly vCPU-h compute ceiling (429).
    pub acquire_rejected_compute_ceiling: Counter,
    /// Acquire rejected: ledger declined the lease (invalid state transition).
    pub acquire_rejected_lease_invalid: Counter,

    // ── Lifecycle ────────────────────────────────────────────────────────────
    /// Leases closed / cancelled (Held→Released, box torn down).
    pub leases_closed: Counter,
    /// Leases the reaper reclaimed at their deadline (Held→Expired) — the client
    /// never sent close. A rising rate vs `leases_closed` means clients are
    /// abandoning leases (crash/timeout) instead of closing cleanly.
    pub leases_expired: Counter,
    /// Leases reclaimed by the crash sweep (Held→Crashed) — a box probed
    /// authoritatively-Dead before its deadline. Distinct from the deadline
    /// backstop (`leases_expired`).
    pub leases_crashed: Counter,
    /// Provision returned capacity-exhausted → acquire 503 (box backend full).
    pub provision_capacity_503: Counter,

    // ── Credential legs (moat) ───────────────────────────────────────────────
    /// CAS/runner PAT mint attempts.
    pub mint_attempts: Counter,
    /// CAS/runner PAT mint failures (the silent-cold-hydration seam).
    pub mint_failures: Counter,
    /// PAT revoke attempts (teardown defense-in-depth).
    pub revoke_attempts: Counter,
    /// PAT revoke failures (revoke degraded to TTL self-expiry).
    pub revoke_failures: Counter,

    // ── Agent-exec ───────────────────────────────────────────────────────────
    /// Agent-exec steps dispatched (202 accepted).
    pub agent_exec_started: Counter,
    /// Agent-exec steps that completed successfully.
    pub agent_exec_done: Counter,
    /// Agent-exec steps that failed (non-zero / signal-killed / capture error).
    pub agent_exec_failed: Counter,

    // ── Operator / load ──────────────────────────────────────────────────────
    /// Requests shed by the global in-flight cap (503 load-shed).
    pub load_shed: Counter,
    /// Requests shed by the introspect admission gate (503 fail-closed) BEFORE
    /// they could enter the blocking pool. Distinct from
    /// [`load_shed`](Self::load_shed) (the tower global-concurrency limiter): this
    /// is the precise backpressure on the auth + plan introspect offload — a burst
    /// of acquires past `FABRIC_INTROSPECT_MAX_INFLIGHT` sheds here CLEANLY (503)
    /// instead of piling into the blocking pool and browning the singleton out to
    /// 000. A rising rate means the box is at its introspect-round-trip ceiling.
    pub introspect_shed: Counter,
    /// W3 introspect circuit-breaker OPEN transitions — the count of times a
    /// sustained corelink-server introspect brownout tripped (or re-tripped) the
    /// breaker OPEN, converting the per-acquire retry storm into an instant
    /// fail-closed. Shared as an `Arc` with the breaker (`introspect_breaker.rs`),
    /// so the breaker increments and this snapshot reads the SAME cell. A rising
    /// rate means the introspect endpoint is in brownout and the fabric is
    /// fast-failing acquires (503) rather than pinning the blocking pool.
    pub introspect_breaker_open: Counter,
    /// Introspect-cache hits: a same-token auth served from the short-TTL cache
    /// WITHOUT an introspect round-trip. A high ratio vs. authed requests means the
    /// cache is absorbing a same-tenant burst (the 2026-07-19 incident's root fix).
    /// Zero while the cache is default-off (`FABRIC_INTROSPECT_CACHE_TTL_MS` unset).
    pub introspect_cache_hit: Counter,
    /// §9 trigger idempotency cache hits (a duplicate delivery answered without
    /// re-executing).
    pub trigger_dedup_hits: Counter,
    /// Operator suspend actions (tenant suspended + leases killed).
    pub suspend_actions: Counter,
}

impl Counters {
    /// Take a point-in-time snapshot of every counter for the status aggregate.
    pub fn snapshot(&self) -> CounterSnapshot {
        CounterSnapshot {
            leases_acquired: self.leases_acquired.get(),
            acquire_rejected_suspended: self.acquire_rejected_suspended.get(),
            acquire_rejected_invalid_image: self.acquire_rejected_invalid_image.get(),
            acquire_rejected_bad_request: self.acquire_rejected_bad_request.get(),
            acquire_rejected_rate: self.acquire_rejected_rate.get(),
            acquire_rejected_over_cap: self.acquire_rejected_over_cap.get(),
            acquire_rejected_no_plan: self.acquire_rejected_no_plan.get(),
            acquire_rejected_compute_ceiling: self.acquire_rejected_compute_ceiling.get(),
            acquire_rejected_lease_invalid: self.acquire_rejected_lease_invalid.get(),
            leases_closed: self.leases_closed.get(),
            leases_expired: self.leases_expired.get(),
            leases_crashed: self.leases_crashed.get(),
            provision_capacity_503: self.provision_capacity_503.get(),
            mint_attempts: self.mint_attempts.get(),
            mint_failures: self.mint_failures.get(),
            revoke_attempts: self.revoke_attempts.get(),
            revoke_failures: self.revoke_failures.get(),
            agent_exec_started: self.agent_exec_started.get(),
            agent_exec_done: self.agent_exec_done.get(),
            agent_exec_failed: self.agent_exec_failed.get(),
            load_shed: self.load_shed.get(),
            introspect_shed: self.introspect_shed.get(),
            introspect_breaker_open: self.introspect_breaker_open.get(),
            introspect_cache_hit: self.introspect_cache_hit.get(),
            trigger_dedup_hits: self.trigger_dedup_hits.get(),
            suspend_actions: self.suspend_actions.get(),
        }
    }
}

/// A serializable, point-in-time copy of [`Counters`]. Embedded in the
/// `StatusReport` under `counters`. Field-for-field with [`Counters`]; every
/// value is a plain `u64` (monotonic since boot).
#[derive(Debug, Serialize)]
pub struct CounterSnapshot {
    pub leases_acquired: u64,
    pub acquire_rejected_suspended: u64,
    pub acquire_rejected_invalid_image: u64,
    pub acquire_rejected_bad_request: u64,
    pub acquire_rejected_rate: u64,
    pub acquire_rejected_over_cap: u64,
    pub acquire_rejected_no_plan: u64,
    pub acquire_rejected_compute_ceiling: u64,
    pub acquire_rejected_lease_invalid: u64,
    pub leases_closed: u64,
    pub leases_expired: u64,
    pub leases_crashed: u64,
    pub provision_capacity_503: u64,
    pub mint_attempts: u64,
    pub mint_failures: u64,
    pub revoke_attempts: u64,
    pub revoke_failures: u64,
    pub agent_exec_started: u64,
    pub agent_exec_done: u64,
    pub agent_exec_failed: u64,
    pub load_shed: u64,
    pub introspect_shed: u64,
    pub introspect_breaker_open: u64,
    pub introspect_cache_hit: u64,
    pub trigger_dedup_hits: u64,
    pub suspend_actions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments_and_reads() {
        let c = Counter::default();
        assert_eq!(c.get(), 0);
        c.incr();
        c.incr();
        assert_eq!(c.get(), 2);
    }

    #[test]
    fn snapshot_reflects_increments() {
        let counters = Counters::default();
        counters.leases_acquired.incr();
        counters.acquire_rejected_over_cap.incr();
        counters.acquire_rejected_over_cap.incr();
        counters.acquire_rejected_no_plan.incr();
        counters.mint_failures.incr();
        counters.leases_expired.incr();
        let snap = counters.snapshot();
        assert_eq!(snap.leases_acquired, 1);
        assert_eq!(snap.acquire_rejected_over_cap, 2);
        // WP-3b: the de-smear counter is distinct from over_cap in all three
        // (Counters → snapshot → CounterSnapshot).
        assert_eq!(snap.acquire_rejected_no_plan, 1);
        assert_eq!(snap.mint_failures, 1);
        assert_eq!(snap.leases_expired, 1);
        // Untouched counters are zero.
        assert_eq!(snap.leases_closed, 0);
        assert_eq!(snap.leases_crashed, 0);
        assert_eq!(snap.revoke_failures, 0);
    }

    #[test]
    fn snapshot_serializes_to_flat_u64_object() {
        let counters = Counters::default();
        counters.load_shed.incr();
        let v = serde_json::to_value(counters.snapshot()).expect("serializes");
        assert_eq!(v["load_shed"], 1);
        assert_eq!(v["leases_acquired"], 0);
        // A representative sample of the fixed key set is present.
        assert!(v.get("acquire_rejected_over_cap").is_some());
        assert!(v.get("agent_exec_failed").is_some());
    }

    /// The full 3-way parity of every counter field: `Counters` → `snapshot()` →
    /// `CounterSnapshot` → serialized JSON. Each of the 23 counters is
    /// incremented a DISTINCT number of times (its 1-based index), so any
    /// copy-paste mis-wiring in `snapshot()` (a field reading the wrong counter)
    /// surfaces as a wrong value, and a field forgotten in `snapshot()` surfaces
    /// as a wrong count. This is the drift-tripwire for the counter surface.
    #[test]
    fn every_counter_maps_one_to_one_through_snapshot_distinct_values() {
        // (json key, &Counter, expected count == its distinct 1-based index)
        let counters = Counters::default();
        // Increment each counter `idx` times using an explicit, ordered list so
        // the mapping is auditable. Order MUST match the struct field order for
        // readability but the assertions are keyed by name, not position.
        let plan: Vec<(&str, &Counter)> = vec![
            ("leases_acquired", &counters.leases_acquired),
            (
                "acquire_rejected_suspended",
                &counters.acquire_rejected_suspended,
            ),
            (
                "acquire_rejected_invalid_image",
                &counters.acquire_rejected_invalid_image,
            ),
            (
                "acquire_rejected_bad_request",
                &counters.acquire_rejected_bad_request,
            ),
            ("acquire_rejected_rate", &counters.acquire_rejected_rate),
            (
                "acquire_rejected_over_cap",
                &counters.acquire_rejected_over_cap,
            ),
            (
                "acquire_rejected_no_plan",
                &counters.acquire_rejected_no_plan,
            ),
            (
                "acquire_rejected_compute_ceiling",
                &counters.acquire_rejected_compute_ceiling,
            ),
            (
                "acquire_rejected_lease_invalid",
                &counters.acquire_rejected_lease_invalid,
            ),
            ("leases_closed", &counters.leases_closed),
            ("leases_expired", &counters.leases_expired),
            ("leases_crashed", &counters.leases_crashed),
            ("provision_capacity_503", &counters.provision_capacity_503),
            ("mint_attempts", &counters.mint_attempts),
            ("mint_failures", &counters.mint_failures),
            ("revoke_attempts", &counters.revoke_attempts),
            ("revoke_failures", &counters.revoke_failures),
            ("agent_exec_started", &counters.agent_exec_started),
            ("agent_exec_done", &counters.agent_exec_done),
            ("agent_exec_failed", &counters.agent_exec_failed),
            ("load_shed", &counters.load_shed),
            ("introspect_shed", &counters.introspect_shed),
            ("introspect_breaker_open", &counters.introspect_breaker_open),
            ("introspect_cache_hit", &counters.introspect_cache_hit),
            ("trigger_dedup_hits", &counters.trigger_dedup_hits),
            ("suspend_actions", &counters.suspend_actions),
        ];
        for (idx, (_name, counter)) in plan.iter().enumerate() {
            for _ in 0..=idx {
                counter.incr();
            }
        }
        let v = serde_json::to_value(counters.snapshot()).expect("serializes");
        let obj = v.as_object().expect("snapshot is a JSON object");

        // 1. The serialized key set is EXACTLY the planned set — no field
        //    forgotten in `snapshot()`, no extra key. This fails the moment a
        //    new `Counters` field is added but not threaded into the snapshot.
        assert_eq!(
            obj.len(),
            plan.len(),
            "CounterSnapshot must serialize exactly {} keys (one per Counters field)",
            plan.len()
        );

        // 2. Every field carries its own distinct value → no cross-wiring.
        for (idx, (name, _counter)) in plan.iter().enumerate() {
            let expected = (idx as u64) + 1;
            assert_eq!(
                obj.get(*name).and_then(|x| x.as_u64()),
                Some(expected),
                "field `{name}` must map to its own counter (expected {expected})"
            );
        }
    }
}
