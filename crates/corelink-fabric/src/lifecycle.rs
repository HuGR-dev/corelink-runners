//! Lease lifecycle wiring — expiry + crash surfacing over the ledger (WP-CP1b).
//!
//! The LIFECYCLE half of CP1: the sweeps that take the authoritative state
//! machine ([`LeaseLedger`], CF0d) from `Held` to the two non-green terminal
//! states, **through the contract §1 legal-transition matrix only** — never
//! bypassing it, never writing state directly.
//!
//! ## Hermeticity: port-traits, no runner dep
//!
//! The fabric ORCHESTRATES the runner (the draft's layering), but this module
//! deliberately does **not** depend on `corelink-runner`. Liveness arrives
//! through the thin [`BoxProbe`] port; the runner's box/engine adapter
//! (`corelink-runner::recovery::probe_liveness` over `BoxExec`) implements it
//! **at the composition root** — that adapter is the single place the two
//! crates meet, and it is a later WP. This keeps `corelink-fabric` a hermetic,
//! types-only control-plane crate (no docker, no SSH, no engine).
//!
//! ## The two sweeps mirror the runner's proven semantics
//!
//! - **Expiry** (`expire_overdue`) mirrors
//!   `corelink-runner::expiry::{is_expired, enforce_expiry}`: deterministic
//!   from the deadline (`now_ms >= deadline`), an end-of-life the control
//!   plane *initiates*. **Mark-then-kill, fail-closed**: the authoritative
//!   `Expired` mark lands in the ledger FIRST; the hard-kill side-effect
//!   happens at the composition root AFTER, keyed off the returned records.
//!   Consumers read the ledger before any result, so an expired job can never
//!   produce a partial result downstream — even if the kill races or fails,
//!   the lease is already terminally `Expired`.
//! - **Crash surfacing** (`surface_crashes`) mirrors
//!   `corelink-runner::recovery::{detect_and_recover,
//!   LostJob::is_surfaced_not_green}`: an *observed* loss of liveness is
//!   **surfaced, never faked green** — the lease goes to `Crashed`, never to
//!   `Released`, and never silently disappears. Expiry and crash are distinct
//!   states on the wire (contract §1) and distinct sweeps here; they never
//!   share a code path.

use crate::ledger::{LeaseLedger, LeaseRecord};
use corelink_runners_contracts::RunnerState;

/// Port: liveness of the box/VM serving a lease, by opaque `box_ref`.
///
/// The runner's box/engine adapter implements this at composition time
/// (wrapping `corelink-runner::recovery::probe_liveness`); tests use a fake.
/// The fabric never learns what a "box" is — only whether it is alive.
pub trait BoxProbe {
    /// `true` iff the box serving `box_ref` is still alive (job in flight).
    fn is_alive(&self, box_ref: &str) -> bool;
}

/// The lease lifecycle sweeps: expiry + crash surfacing over a [`LeaseLedger`].
///
/// Stateless by design — the ledger is the only authority, so the lifecycle
/// holds no shadow state that could diverge from it across restarts. Both
/// sweeps are idempotent: a lease already in a terminal state is filtered out
/// (it is no longer `Held`), so a second pass returns empty rather than
/// erroring — the legal-transition matrix is consulted by construction, never
/// bypassed and never tripped.
#[derive(Debug, Default)]
pub struct LeaseLifecycle;

impl LeaseLifecycle {
    /// New (stateless) lifecycle.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Expire every `Held` lease past its deadline: `now_ms >=
    /// deadline_ms_of(rec)` (same comparison as
    /// `corelink-runner::expiry::is_expired`) transitions the lease to
    /// [`RunnerState::Expired`] via [`LeaseLedger::transition`] — the legal
    /// matrix, never bypassed. Returns the updated records (the same records
    /// the ledger now holds).
    ///
    /// **Mark-then-kill (fail-closed):** this function only MARKS. The kill
    /// side-effect (`corelink-runner::expiry::hard_kill` + forensic teardown)
    /// happens at the composition root AFTER this returns, driven by the
    /// returned records. Because consumers read the ledger first, an expired
    /// job can never produce a partial result downstream.
    ///
    /// Idempotent: an expired lease leaves `Held`, so the next sweep skips it.
    /// Non-`Held` leases (e.g. already `Released`) are filtered, not errors.
    ///
    /// # Errors
    /// Ledger I/O only (fail-closed: a journal that cannot record the mark
    /// must abort the sweep, never proceed to kills without the mark).
    pub fn expire_overdue<L: LeaseLedger + ?Sized>(
        &mut self,
        ledger: &mut L,
        now_ms: u64,
        deadline_ms_of: impl Fn(&LeaseRecord) -> u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        let mut expired = Vec::new();
        for rec in ledger.held()? {
            if now_ms >= deadline_ms_of(&rec) {
                expired.push(ledger.transition(&rec.lease_id, RunnerState::Expired, now_ms)?);
            }
        }
        Ok(expired)
    }

    /// Surface every `Held` lease whose box is no longer alive: probe says
    /// dead → transition to [`RunnerState::Crashed`] via
    /// [`LeaseLedger::transition`] — the legal matrix, never bypassed.
    /// Returns the updated records (the same records the ledger now holds).
    ///
    /// **Surfaced-not-green:** mirrors
    /// `corelink-runner::recovery::LostJob::is_surfaced_not_green` — a lost
    /// job is recorded `Crashed`, never `Released`, never silently dropped,
    /// never faked green. Cleanup/requeue side-effects happen at the
    /// composition root AFTER the authoritative mark, driven by the returned
    /// records (mark-then-clean, same fail-closed order as expiry).
    ///
    /// Idempotent: a crashed lease leaves `Held`, so the next sweep skips it.
    /// Non-`Held` leases are filtered, not errors.
    ///
    /// # Errors
    /// Ledger I/O only (fail-closed).
    pub fn surface_crashes<L: LeaseLedger + ?Sized>(
        &mut self,
        ledger: &mut L,
        probe: &dyn BoxProbe,
        now_ms: u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        let mut crashed = Vec::new();
        for rec in ledger.held()? {
            if !probe.is_alive(&rec.box_ref) {
                crashed.push(ledger.transition(&rec.lease_id, RunnerState::Crashed, now_ms)?);
            }
        }
        Ok(crashed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{FileLedger, InMemoryLedger, LeaseState};
    use crate::tenant::TenantId;

    fn tenant() -> TenantId {
        TenantId::new("acme").unwrap()
    }

    fn record(lease_id: &str, box_ref: &str) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: tenant(),
            state: LeaseState::Pending,
            box_ref: box_ref.to_string(),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deadline_ms: None,
        }
    }

    /// Put + admit (Pending → Held) — through the matrix, like everything.
    fn put_held(ledger: &mut impl LeaseLedger, lease_id: &str, box_ref: &str) {
        ledger.put(record(lease_id, box_ref)).unwrap();
        ledger
            .transition(lease_id, RunnerState::Held, 2_000)
            .unwrap();
    }

    /// Probe fake: alive iff the box_ref is in the allow-list.
    struct FakeProbe {
        alive: Vec<String>,
    }

    impl BoxProbe for FakeProbe {
        fn is_alive(&self, box_ref: &str) -> bool {
            self.alive.iter().any(|b| b == box_ref)
        }
    }

    #[test]
    fn expiry_marks_expired_no_partial_result() {
        let mut ledger = InMemoryLedger::new();
        put_held(&mut ledger, "lease-exp", "box-1");
        let mut lc = LeaseLifecycle::new();

        // Deadline 5_000, swept at 5_000 (>= — same comparison as
        // runner::expiry::is_expired).
        let marked = lc.expire_overdue(&mut ledger, 5_000, |_| 5_000).unwrap();
        assert_eq!(marked.len(), 1);
        assert_eq!(
            marked[0].state,
            LeaseState::Wire(RunnerState::Expired),
            "the mark is Expired"
        );

        // No partial result: the returned record IS the record the ledger now
        // holds — consumers reading the ledger see Expired before any kill
        // side-effect runs at the composition root.
        let in_ledger = ledger.get("lease-exp").unwrap().unwrap();
        assert_eq!(marked[0], in_ledger, "returned record == ledger record");

        // Idempotent: the lease left Held, a second sweep returns empty.
        let second = lc.expire_overdue(&mut ledger, 6_000, |_| 5_000).unwrap();
        assert!(second.is_empty(), "second expire pass must be empty");
        assert_eq!(
            ledger.get("lease-exp").unwrap().unwrap().state,
            LeaseState::Wire(RunnerState::Expired),
            "still Expired — terminal, untouched"
        );
    }

    #[test]
    fn crash_marks_crashed_no_duplicate_result() {
        let mut ledger = InMemoryLedger::new();
        put_held(&mut ledger, "lease-crash", "box-dead");
        put_held(&mut ledger, "lease-fine", "box-alive");
        let mut lc = LeaseLifecycle::new();
        let probe = FakeProbe {
            alive: vec!["box-alive".to_string()],
        };

        let marked = lc.surface_crashes(&mut ledger, &probe, 7_000).unwrap();
        assert_eq!(marked.len(), 1, "exactly the dead box, exactly once");
        assert_eq!(marked[0].lease_id, "lease-crash");
        assert_eq!(marked[0].state, LeaseState::Wire(RunnerState::Crashed));
        assert_eq!(marked[0], ledger.get("lease-crash").unwrap().unwrap());

        // The alive lease is untouched (no false-positive recovery).
        assert!(ledger.get("lease-fine").unwrap().unwrap().state.is_held());

        // Idempotent: no duplicate Crashed result on a second sweep.
        let second = lc.surface_crashes(&mut ledger, &probe, 8_000).unwrap();
        assert!(second.is_empty(), "second crash pass must be empty");
    }

    #[test]
    fn held_lease_with_dead_box_recovers_surfaced_not_green() {
        let mut ledger = InMemoryLedger::new();
        put_held(&mut ledger, "lease-lost", "box-gone");
        let mut lc = LeaseLifecycle::new();
        let probe = FakeProbe { alive: vec![] };

        let marked = lc.surface_crashes(&mut ledger, &probe, 9_000).unwrap();
        assert_eq!(marked.len(), 1);

        // Surfaced-not-green (mirrors recovery::LostJob::is_surfaced_not_green):
        // the loss is SURFACED as Crashed — never Released (faked green),
        // never silently dropped.
        let state = &ledger.get("lease-lost").unwrap().unwrap().state;
        assert_eq!(state, &LeaseState::Wire(RunnerState::Crashed));
        assert_ne!(
            state,
            &LeaseState::Wire(RunnerState::Released),
            "a lost job is never green"
        );
    }

    #[test]
    fn lifecycle_only_uses_legal_transitions() {
        let mut ledger = InMemoryLedger::new();
        put_held(&mut ledger, "lease-done", "box-1");
        ledger
            .transition("lease-done", RunnerState::Released, 3_000)
            .unwrap();
        let released = ledger.get("lease-done").unwrap().unwrap();
        let mut lc = LeaseLifecycle::new();

        // Sweep with everything overdue AND every box dead: the Released
        // lease is FILTERED (not Held), not an error and not mutated — the
        // matrix is consulted by construction, never tripped.
        let expired = lc.expire_overdue(&mut ledger, u64::MAX, |_| 0).unwrap();
        assert!(expired.is_empty(), "Released lease filtered from expiry");
        let probe = FakeProbe { alive: vec![] };
        let crashed = lc.surface_crashes(&mut ledger, &probe, u64::MAX).unwrap();
        assert!(
            crashed.is_empty(),
            "Released lease filtered from crash sweep"
        );

        assert_eq!(
            ledger.get("lease-done").unwrap().unwrap(),
            released,
            "terminal lease byte-identical after both sweeps — untouched"
        );
    }

    #[test]
    fn ledger_survives_process_restart_with_lifecycle() {
        let path = std::env::temp_dir().join(format!(
            "corelink-fabric-cp1b-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let marked = {
            let mut ledger = FileLedger::open(&path).expect("fresh journal must open");
            put_held(&mut ledger, "lease-durable", "box-1");
            let mut lc = LeaseLifecycle::new();
            let marked = lc.expire_overdue(&mut ledger, 10_000, |_| 5_000).unwrap();
            assert_eq!(marked.len(), 1);
            marked
            // drop = process "restart"
        };

        // Reopen: the Expired mark survived the restart — the kill decision
        // is never lost with the process.
        let reopened = FileLedger::open(&path).expect("journal must replay");
        assert_eq!(
            reopened.get("lease-durable").unwrap().as_ref(),
            Some(&marked[0]),
            "Expired persisted across restart, byte-identical"
        );
        assert!(
            reopened.held().unwrap().is_empty(),
            "no Held leases after replay — the sweep's effect is durable"
        );

        std::fs::remove_file(&path).ok();
    }
}
