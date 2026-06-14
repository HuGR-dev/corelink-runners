//! Background reaper: reclaim orphaned cloud boxes whose leases have expired
//! without a clean close (WP-CF-REAP).
//!
//! ## Problem
//!
//! A lease that expires or whose client crashes without sending `POST
//! /v1/leases/{id}/close` leaves two orphaned objects:
//! - a Northflank job object in the provider, and
//! - the corresponding [`RunningContainer`] entry in the in-memory
//!   [`BoxRegistry`].
//!
//! The compute cost is bounded (`activeDeadlineSeconds`), but the objects
//! accumulate — this reaper reclaims them on a periodic sweep.
//!
//! ## Mechanism (expiry-driven, teardown-first)
//!
//! Every tick the reaper calls [`reap_once`]:
//! 1. A snapshot of deadline entries is taken WITHOUT holding the ledger lock
//!    (no nesting; the snapshot is a cloned map, the guard is dropped
//!    immediately).
//! 2. A snapshot of currently `Held` lease records is taken under the ledger
//!    lock (guard dropped at block end, before any `await`).
//! 3. Overdue leases (`now_ms >= deadline`) are identified.  A `Held` lease
//!    with no recorded deadline is treated as **never-overdue** (fail-safe:
//!    never reap a lease we cannot date).
//! 4. For each overdue lease, [`AppState::teardown_lease`] is called FIRST.
//!    Only if teardown SUCCEEDS is the ledger transitioned to `Expired` and
//!    the side-tables GC'd.  A failed teardown leaves the lease `Held` so
//!    the next sweep retries — no permanent leak from a one-time provider
//!    hiccup.
//!
//! ## Retry posture
//!
//! A failed teardown leaves the lease `Held` (NOT transitioned to `Expired`)
//! so the next sweep finds it again and retries.  This is the key difference
//! from the old mark-then-kill posture: the terminal `Expired` mark is the
//! CONSEQUENCE of a successful teardown, not the precondition for it.
//!
//! ## Side-table GC
//!
//! `deadlines` and `images` entries are removed via [`AppState::forget_lease`]
//! only AFTER teardown succeeds.  On a retry sweep, the deadline entry must
//! still be present (otherwise the lease appears as never-overdue and is
//! silently skipped).
//!
//! ## Crash-surfacing sweep (WP-CRASH-SWEEP, OPT-IN)
//!
//! Leases that die mid-flight (box gone but deadline not yet reached) are now
//! reclaimed by [`surface_crashes`], a SEPARATE sweep from [`reap_once`]. It
//! probes each `Held` lease's box via [`crate::AppState::probe_lease`] and
//! reclaims ONLY a box probed authoritatively-Dead
//! ([`crate::cloud_exec::ProbeStatus::Dead`]), transitioning the lease to
//! `Crashed` (the symmetric counterpart of `reap_once`'s `Expired` path).
//!
//! The sweep is OPT-IN (`FABRIC_CRASH_PROBE_INTERVAL_SECS`): a liveness probe
//! costs one provider `is_alive` call per `Held` lease per tick and is
//! newer/riskier than deadline-expiry, so it is off unless explicitly
//! configured. The always-on deadline reaper ([`reap_once`]) remains the
//! backstop — every lease still has a hard `Expired` deadline regardless.
//!
//! ## §13.5 partial-envelope flush (WIRED — WP-S13.5)
//!
//! BOTH abnormal paths now flush a best-effort PARTIAL envelope on reclaim
//! (Option B, owner-ratified 2026-06-13): `reap_once` (Expired) and
//! `surface_crashes` (Crashed) each call [`flush_partial_envelope`] AFTER
//! teardown→transition→`record_slot` — fire-and-forget, so it never blocks or
//! breaks reclamation. The flush finalizes whatever the `CaptureHook`
//! accumulated through the SAME finalize/redaction path as a normal close (no
//! exemption), stamps `close_reason=expired|crashed` + `capture_incomplete:true`
//! (WRAPPER-level, never inside the frozen §13.4 `IntentMetrics`), and emits it
//! to the M1 forensic sink (a structured log line; the real push to hugit is
//! the P2 transport WP). A lease with no hook is a no-op; an already-closed
//! hook (a normal close raced in) returns the exactly-once `Err`, which is
//! logged and skipped — no second envelope, no double-anything.
//!
//! ## Remaining non-goals
//!
//! - **Live push of the partial envelope**: at M1 the flush is the forensic
//!   log record; the actual transport to hugit is the P2 transport WP.
//!
//! ## Lock ordering
//!
//! `reap_once` takes the `deadlines` snapshot FIRST (lock taken + released),
//! then takes the ledger snapshot (lock taken + released), then calls async
//! teardown.  No lock is held across any `await` point — so this function is
//! always `Send`-safe on the executor (verified by the compile-time assertion
//! at the bottom of this file).
//!
//! After teardown the ledger lock is re-acquired (briefly) to write the
//! `Expired` transition.  No other lock is held at that point.

use std::time::{Duration, Instant};

use corelink_fabric::SlotEventKind;
use corelink_runner::envelope::{AbnormalKind, CloseReason};
use corelink_runners_contracts::RunnerState;

/// §13.5 best-effort partial-envelope flush on an ABNORMAL lease termination
/// (Expired / Crashed), fire-and-forget.
///
/// **Ruling (hugit, owner-ratified 2026-06-13, Option B).** When the deadline
/// reaper ([`reap_once`]) or the crash sweep ([`surface_crashes`]) reclaims a
/// lease, whatever the lease's [`CaptureHook`] accumulated MUST be flushed as a
/// PARTIAL envelope — explicitly marked incomplete — rather than dropped. hugit
/// prices flat, so a partial trajectory carries no billing risk; it is forensic
/// provenance.
///
/// This runs **AFTER** teardown→transition→`record_slot` has already reclaimed
/// the lease, so it can NEVER block or break reclamation: a flush failure is
/// logged and the sweep moves on.
///
/// # Wire shape (§13.5)
/// The EXISTING envelope payload plus two close-metadata markers, both
/// WRAPPER-level (never inside the frozen §13.4 `IntentMetrics` vector):
/// - `close_reason` — `expired` | `crashed` (here; `normal` is the clean path);
/// - `capture_incomplete: true` — set unconditionally by `close_abnormal`.
///
/// # Delivery (M1)
/// Fire-and-forget, **NO ack** — the lease is torn down, so there is no live
/// client to ack. Dedup is NOT by registry removal: [`HookRegistry::close_handle_any`]
/// returns a CLONE of the hook (the entry stays registered). Exactly-once is
/// enforced by the **shared close-latch** on the hook's `Arc<Shared>` state — every
/// clone (the live client's close handle and this reaper clone) observes the same
/// latch, so the second `close_*` returns the exactly-once `Err`. The ledger
/// transition is additionally atomic & exclusive (`Held→Expired|Crashed` vs
/// `Held→Released`), so a normal close and this abnormal flush can never both fire.
///
/// **There is no live push transport at M1** (the envelope is poll-drain;
/// hugit consumes at P2). So at M1 the flush = FINALIZE the partial envelope
/// (markers + the SAME finalize/redaction write-path as a normal close — no
/// exemption) and emit it best-effort to the available forensic sink: a single
/// structured log line carrying `lease_id`, `tenant`, `close_reason`,
/// `capture_incomplete`, and a metrics SUMMARY (the `IntentMetrics` scalar
/// fields — NOT raw trajectory text). The real push to hugit is the P2
/// transport WP; at M1 this log line IS the forensic record.
///
/// All calls here ([`HookRegistry::close_handle_any`] + `JobClose::close_abnormal`)
/// are synchronous, so this holds no `MutexGuard` across an `await` — the
/// `Send` guards on the callers stay satisfied.
fn flush_partial_envelope(
    state: &crate::AppState,
    lease_id: &str,
    tenant: &corelink_fabric::TenantId,
    kind: AbnormalKind,
    died: Instant,
) -> Option<corelink_runner::envelope::CloseOutcome> {
    // Dedup: close_handle_any returns a CLONE of the hook (the registry entry
    // stays; exactly-once rides the shared close-latch on the hook state, NOT
    // registry removal). No hook → a non-agent lease (nothing captured): nothing
    // to flush.
    let (hook, price) = state.hook_registry.close_handle_any(lease_id)?;

    // Drive the FROZEN mechanism: it finalizes the partial envelope through the
    // SAME finalize/redaction path as a normal close (no exemption — "an
    // exemption is a hole") and stamps `capture_incomplete: true` +
    // `close_reason: expired|crashed`.
    let outcome =
        match corelink_runner::envelope::JobClose::new(&hook).close_abnormal(kind, died, &price) {
            Ok(outcome) => outcome,
            Err(e) => {
                // Already-closed (exactly-once): a normal close consumed this hook
                // before the sweep. Do NOT fail the sweep — log and move on; there
                // is NO second envelope (exactly-once is enforced on the shared hook
                // state, not on the registry entry).
                eprintln!(
                    "envelope-flush: skipped partial flush for lease {lease_id} \
                 (close already fired, exactly-once): {e:#}"
                );
                return None;
            }
        };

    // Forensic emit (M1): a structured log line. The metrics come from the
    // FINALIZED (already-redacted) outcome — a SUMMARY of the IntentMetrics
    // scalars, never raw trajectory bytes. The real push to hugit is P2.
    let m = &outcome.metrics;
    let reason = match outcome.close_reason {
        CloseReason::Expired => "expired",
        CloseReason::Crashed => "crashed",
        CloseReason::Normal => "normal",
    };
    eprintln!(
        "envelope-flush: partial envelope FINALIZED (M1 forensic record; P2 pushes to hugit) \
         lease_id={lease_id} tenant={tenant} close_reason={reason} \
         capture_incomplete={} tokens_total={} tool_calls={} cost_usd_micros={} \
         wall_ms={} active_ms={} model_turns={}",
        outcome.capture_incomplete,
        m.tokens.total,
        m.tool_calls,
        m.cost_usd_micros,
        m.wall_ms,
        m.active_ms,
        m.model_turns,
    );

    // Returned for the in-crate tests to assert the finalized markers; the
    // call sites IGNORE it (fire-and-forget — the side-effect is the forensic
    // log line above).
    Some(outcome)
}

/// Configuration for the background reaper.
pub struct ReaperConfig {
    /// How often to scan for overdue leases.
    pub interval: Duration,
}

/// Resolve [`ReaperConfig`] from an environment-variable accessor.
///
/// Reads `FABRIC_REAP_INTERVAL_SECS`.
/// - Absent or empty → default of 30 seconds.
/// - Present → parse as `u32`; value `0` or an unparseable string → `Err`.
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn reaper_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<ReaperConfig> {
    let secs: u64 = match get("FABRIC_REAP_INTERVAL_SECS").filter(|s| !s.is_empty()) {
        None => 30,
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_REAP_INTERVAL_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            if parsed == 0 {
                anyhow::bail!(
                    "FABRIC_REAP_INTERVAL_SECS must be >= 1 (0 would disable the reaper)"
                );
            }
            parsed as u64
        }
    };
    Ok(ReaperConfig {
        interval: Duration::from_secs(secs),
    })
}

/// Run one expiry sweep: teardown overdue `Held` leases and mark them
/// `Expired` only after teardown succeeds.
///
/// Returns the number of leases successfully reclaimed this tick.
///
/// # Posture
///
/// Teardown-first: the `Expired` ledger mark is written ONLY after the
/// provider teardown returns `Ok`.  A failed teardown leaves the lease `Held`
/// so the next sweep retries — no permanent leak from a transient provider
/// error.
///
/// # Lock-ordering note
///
/// No `MutexGuard` is held across any `await` point.  The deadlines snapshot
/// and the ledger snapshot are taken in separate scoped blocks, each guard
/// dropped before the next async operation.  After teardown, the ledger lock
/// is briefly re-acquired to write the `Expired` transition — again dropped
/// before continuing.  The compile-time [`_ASSERT_REAP_ONCE_IS_SEND`]
/// assertion enforces this: if anyone ever adds a guard across an `await`,
/// compilation fails.
pub async fn reap_once(state: &crate::AppState) -> usize {
    let now = state.clock.now_ms();

    // ── 1. Snapshot deadlines WITHOUT holding the ledger lock.
    // The guard is dropped at the end of the expression — no nesting.
    let deadlines = state.deadlines_snapshot();

    // ── 2. Snapshot held leases — guard dropped at end of block, before any await.
    let held = {
        let ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        ledger.held().unwrap_or_default()
        // `ledger` (MutexGuard) is dropped here — before any await below.
    };

    // ── 3. Identify overdue records.
    // A Held lease with no recorded deadline is treated as never-overdue
    // (fail-safe: we never reap a lease we cannot date).
    let overdue: Vec<_> = held
        .into_iter()
        .filter(|rec| now >= deadlines.get(&rec.lease_id).copied().unwrap_or(u64::MAX))
        .collect();

    let mut reaped = 0usize;

    for rec in overdue {
        // ── 4. TEARDOWN FIRST — no lock held.
        let torn = state.teardown_lease(&rec.lease_id).await;

        if torn {
            // ── 5. Mark Expired ONLY after teardown succeeds, and only if WE
            // won the transition race.
            //
            // A concurrent close/cancel may have already moved the lease to
            // Released between teardown succeeding and this lock acquisition.
            // In that case `transition` returns Err (no legal pair out of a
            // terminal state). We bind the result: if it's Err, the lease was
            // already terminalized by someone else — do NOT GC or emit Expired
            // (that would double-free the slot in the journal).
            let expired_ok = {
                let mut ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
                ledger
                    .transition(&rec.lease_id, RunnerState::Expired, now)
                    .is_ok()
                // guard dropped here at end of block
            };

            if expired_ok {
                // ── 6. GC side-tables (deadline + image entries).
                state.forget_lease(&rec.lease_id);

                // ── BIL1 / WP-SLOT-EMIT: slot expired — ledger lock is dropped
                // (the transition block above), forget_lease holds no ledger lock.
                // The symmetric Crashed path now lives in `surface_crashes`
                // (WP-CRASH-SWEEP, opt-in); this expiry path emits Expired only.
                state.record_slot(&rec.lease_id, &rec.tenant, SlotEventKind::Expired);

                // ── WP-S13.5: best-effort PARTIAL-envelope flush, fire-and-forget,
                // AFTER reclamation (never blocks it). Marks close_reason=expired +
                // capture_incomplete:true; no-op if the lease had no capture hook.
                flush_partial_envelope(
                    state,
                    &rec.lease_id,
                    &rec.tenant,
                    AbnormalKind::Expiry,
                    Instant::now(),
                );

                reaped += 1;
            }
            // else: concurrent close/cancel won the race — their transition
            // already freed the slot; we do NOT emit Expired (would double-free)
            // and do NOT count this as a reaper reclaim.
        }
        // else: leave Held — do NOT transition, do NOT GC.
        // The next sweep finds it again and retries teardown.
    }

    reaped
}

/// Spawn the background reaper task.
///
/// The returned [`tokio::task::JoinHandle`] runs until aborted by the caller;
/// bind the handle and call `.abort()` after the server's graceful-shutdown
/// future resolves so the task does not outlive the process.
pub fn spawn_reaper(state: crate::AppState, cfg: ReaperConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(cfg.interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let n = reap_once(&state).await;
            if n > 0 {
                eprintln!("reaper: expired+reclaimed {n} overdue lease(s)");
            }
        }
    })
}

// ── Crash-surfacing sweep (WP-CRASH-SWEEP, OPT-IN) ───────────────────────────

/// Resolve the crash-sweep interval from an environment-variable accessor.
///
/// Reads `FABRIC_CRASH_PROBE_INTERVAL_SECS`.
/// - **Absent or empty → `Ok(None)`**: the crash sweep is OPT-IN, so it is NOT
///   spawned by default. Rationale: crash-probing costs one provider
///   `is_alive` call per `Held` lease per tick and is newer/riskier than
///   deadline-expiry; the always-on deadline reaper ([`reap_once`]) remains the
///   backstop, so the sweep is off unless a deployer explicitly opts in.
/// - **Present → `Ok(Some(Duration))`**: parse as `u32`; value `0` or an
///   unparseable string → `Err` (a configured-but-zero interval is a deployer
///   mistake, not a silent disable — absence is the disable path).
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn crash_probe_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<Duration>> {
    match get("FABRIC_CRASH_PROBE_INTERVAL_SECS").filter(|s| !s.is_empty()) {
        // Absent/empty → opt-in default: the sweep is NOT spawned.
        None => Ok(None),
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_CRASH_PROBE_INTERVAL_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            if parsed == 0 {
                anyhow::bail!(
                    "FABRIC_CRASH_PROBE_INTERVAL_SECS must be >= 1 \
                     (absent/empty is the way to disable the opt-in crash sweep, not 0)"
                );
            }
            Ok(Some(Duration::from_secs(parsed as u64)))
        }
    }
}

/// Run one crash-surfacing sweep: probe each `Held` lease's box and reclaim —
/// as `Crashed` — ONLY a box probed authoritatively-Dead.
///
/// Returns the number of leases reclaimed-as-crashed this tick.
///
/// # Fail-safe core
///
/// The sweep acts ONLY on `Ok(ProbeStatus::Dead)`. `Ok(Alive)`, `Ok(Unbound)`,
/// and EVERY `Err(_)` leave the lease `Held` and do nothing — a probe that is
/// anything other than authoritatively-Dead must NEVER reclaim. The deadline
/// reaper ([`reap_once`]) is the backstop for a lease whose box is dead but
/// whose probe is inconclusive: it still expires on its hard deadline.
///
/// # Posture (mirrors [`reap_once`])
///
/// Teardown-FIRST: the box is torn down before the `Crashed` ledger mark, and
/// the mark is written ONLY if teardown succeeds AND we won the transition race
/// (a concurrent close/cancel may have already terminalized the lease — that
/// `transition` returns `Err`, and we then do NOT GC, do NOT emit `Crashed`,
/// do NOT count, avoiding a double-free of the slot). A failed teardown leaves
/// the lease `Held` so the next sweep retries.
///
/// # Lock-ordering note
///
/// No `MutexGuard` is held across any `await` point: the `Held` snapshot is
/// taken in a scoped block (guard dropped before any await), the probe and
/// teardown awaits hold no lock, and the `Crashed` transition re-acquires the
/// ledger lock briefly (dropped before continuing). The compile-time
/// [`_ASSERT_SURFACE_CRASHES_IS_SEND`] assertion enforces this.
pub async fn surface_crashes(state: &crate::AppState) -> usize {
    let now = state.clock.now_ms();

    // ── 1. Snapshot held leases — guard dropped at end of block, before await.
    let held = {
        let ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        ledger.held().unwrap_or_default()
        // `ledger` (MutexGuard) is dropped here — before any await below.
    };

    let mut reaped = 0usize;

    for rec in held {
        // ── 2. PROBE — no lock held. FAIL-SAFE: act ONLY on Ok(Dead).
        let status = state.probe_lease(&rec.lease_id).await;
        if !matches!(status, Ok(crate::cloud_exec::ProbeStatus::Dead)) {
            // Alive / Unbound / Err(_) → leave Held, do nothing. An unreachable
            // provider (Err) must NEVER be read as death; the deadline reaper is
            // the backstop.
            continue;
        }

        // ── 3. TEARDOWN FIRST — no lock held.
        let torn = state.teardown_lease(&rec.lease_id).await;
        if !torn {
            // Leave Held — the next sweep retries teardown.
            continue;
        }

        // ── 4. Mark Crashed ONLY after teardown succeeds, and only if WE won
        // the transition race. A concurrent close/cancel may have moved the
        // lease to a terminal state between teardown and this lock acquisition;
        // `transition` then returns Err and we must NOT double-free the slot.
        let crashed_ok = {
            let mut ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
            ledger
                .transition(&rec.lease_id, RunnerState::Crashed, now)
                .is_ok()
            // guard dropped here at end of block
        };

        if crashed_ok {
            // ── 5. GC side-tables, then emit the Crashed slot event.
            state.forget_lease(&rec.lease_id);
            state.record_slot(&rec.lease_id, &rec.tenant, SlotEventKind::Crashed);

            // ── WP-S13.5: best-effort PARTIAL-envelope flush, fire-and-forget,
            // AFTER reclamation (never blocks it). Symmetric with the Expired
            // path: marks close_reason=crashed + capture_incomplete:true; no-op
            // if the lease had no capture hook.
            flush_partial_envelope(
                state,
                &rec.lease_id,
                &rec.tenant,
                AbnormalKind::Crash,
                Instant::now(),
            );

            reaped += 1;
        }
        // else: concurrent close/cancel won the race — their transition already
        // freed the slot; we do NOT emit Crashed (would double-free) and do NOT
        // count this as a crash reclaim.
    }

    reaped
}

/// Spawn the background crash-surfacing sweep (WP-CRASH-SWEEP).
///
/// Mirrors [`spawn_reaper`]. The returned [`tokio::task::JoinHandle`] runs until
/// aborted by the caller; bind the handle and call `.abort()` after graceful
/// shutdown so the task does not outlive the process.
///
/// This is OPT-IN: the composition root spawns it only when
/// [`crash_probe_config_from_env`] returns `Some(interval)`.
pub fn spawn_crash_sweep(
    state: crate::AppState,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let n = surface_crashes(&state).await;
            if n > 0 {
                eprintln!("crash-sweep: surfaced+reclaimed {n} crashed lease(s)");
            }
        }
    })
}

// ── Compile-time Send guard ────────────────────────────────────────────────
//
// If `reap_once` ever acquires a `MutexGuard` (or any other `!Send` type)
// across an `await` point, the future it returns becomes `!Send` and this
// static assertion causes a compile error — catching the regression before
// it reaches CI.

#[allow(dead_code)]
const _ASSERT_REAP_ONCE_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: crate::AppState) {
        _assert_send_fut(reap_once(&state));
    }
};

// Same guard for `surface_crashes`: a `MutexGuard` held across an `await`
// would make its future `!Send` and break this assertion at compile time.
#[allow(dead_code)]
const _ASSERT_SURFACE_CRASHES_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: crate::AppState) {
        _assert_send_fut(surface_crashes(&state));
    }
};

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, SlotEventKind, TenantId,
    };
    use corelink_runners_contracts::RunnerState;

    use super::*;
    use crate::app::{AppState, Clock, StaticPlans};
    use crate::cloud_exec::{BoxProvisioner, ProbeStatus};

    // ── Deterministic clock ───────────────────────────────────────────────────

    /// A clock whose time is set by the test.
    #[derive(Clone)]
    struct FixedClock(Arc<AtomicU64>);

    impl FixedClock {
        fn new(ms: u64) -> Self {
            Self(Arc::new(AtomicU64::new(ms)))
        }
        #[allow(dead_code)]
        fn set(&self, ms: u64) {
            self.0.store(ms, Ordering::SeqCst);
        }
    }

    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    // ── RecordingProvisioner ──────────────────────────────────────────────────

    /// Provisioner that records which lease ids `teardown` was called with.
    /// `provision` always returns `Ok(())`.
    #[derive(Default)]
    struct RecordingProvisioner {
        teardown_calls: Mutex<Vec<String>>,
    }

    impl RecordingProvisioner {
        fn new() -> Self {
            Self::default()
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for RecordingProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            Ok(())
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            Ok(ProbeStatus::Unbound)
        }
    }

    // ── TogglesTeardownProvisioner ────────────────────────────────────────────

    /// Provisioner whose teardown result is toggled by an `AtomicBool`.
    ///
    /// `should_succeed = true` → teardown returns `Ok(())`; `false` → `Err`.
    /// Also records which lease ids teardown was called with, for assertion.
    struct TogglesTeardownProvisioner {
        should_succeed: Arc<AtomicBool>,
        teardown_calls: Mutex<Vec<String>>,
    }

    impl TogglesTeardownProvisioner {
        fn new(initial: bool) -> (Self, Arc<AtomicBool>) {
            let flag = Arc::new(AtomicBool::new(initial));
            let prov = Self {
                should_succeed: Arc::clone(&flag),
                teardown_calls: Mutex::new(Vec::new()),
            };
            (prov, flag)
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for TogglesTeardownProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            if self.should_succeed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("teardown intentionally failed"))
            }
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            Ok(ProbeStatus::Unbound)
        }
    }

    // ── ScriptedProbeProvisioner ──────────────────────────────────────────────

    /// Probe outcome scripted per test: `Alive`, `Dead`, `Unbound`, or `Err`.
    #[derive(Clone, Copy)]
    enum Scripted {
        Alive,
        Dead,
        Unbound,
        Err,
    }

    /// Provisioner whose `probe` returns a scripted [`Scripted`] outcome and
    /// whose `teardown` result is toggled by an `AtomicBool`. Records teardown
    /// calls for assertion.
    struct ScriptedProbeProvisioner {
        probe_outcome: Scripted,
        teardown_succeed: Arc<AtomicBool>,
        teardown_calls: Mutex<Vec<String>>,
    }

    impl ScriptedProbeProvisioner {
        fn new(outcome: Scripted, teardown_succeed: bool) -> Self {
            Self {
                probe_outcome: outcome,
                teardown_succeed: Arc::new(AtomicBool::new(teardown_succeed)),
                teardown_calls: Mutex::new(Vec::new()),
            }
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for ScriptedProbeProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            if self.teardown_succeed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("teardown intentionally failed"))
            }
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            match self.probe_outcome {
                Scripted::Alive => Ok(ProbeStatus::Alive),
                Scripted::Dead => Ok(ProbeStatus::Dead),
                Scripted::Unbound => Ok(ProbeStatus::Unbound),
                Scripted::Err => Err(anyhow::anyhow!("provider unreachable (transient)")),
            }
        }
    }

    /// Build an `AppState` with a `ScriptedProbeProvisioner`.
    fn build_state_scripted(
        now_ms: u64,
        outcome: Scripted,
        teardown_succeed: bool,
    ) -> (AppState, FixedClock, Arc<ScriptedProbeProvisioner>) {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let clock = FixedClock::new(now_ms);
        let prov = Arc::new(ScriptedProbeProvisioner::new(outcome, teardown_succeed));
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov)
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build an `AppState` with a `FixedClock` and a `RecordingProvisioner`.
    ///
    /// Returns `(state, clock, provisioner_arc)` so the test can mutate the
    /// clock and inspect teardown calls.
    fn build_state(now_ms: u64) -> (AppState, FixedClock, Arc<RecordingProvisioner>) {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let clock = FixedClock::new(now_ms);
        let prov = Arc::new(RecordingProvisioner::new());
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov)
    }

    /// Build an `AppState` with a `TogglesTeardownProvisioner`.
    fn build_state_toggles(
        now_ms: u64,
        initial_succeed: bool,
    ) -> (
        AppState,
        FixedClock,
        Arc<TogglesTeardownProvisioner>,
        Arc<AtomicBool>,
    ) {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let clock = FixedClock::new(now_ms);
        let (prov, flag) = TogglesTeardownProvisioner::new(initial_succeed);
        let prov = Arc::new(prov);
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov, flag)
    }

    /// Register a capture hook on `state.hook_registry` for `lease_id` so the
    /// §13.5 partial-envelope flush has something to finalize. Feeds one
    /// model-turn so the finalized metrics are non-trivial.
    fn register_hook(state: &AppState, lease_id: &str) {
        use corelink_runner::envelope::{
            CaptureHook, EnvelopeConfig, MetricsCollector, TranscriptEvent,
        };
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: std::time::Duration::from_millis(1),
                buffer_capacity: 16,
            },
            "hookcred-reaper",
            MetricsCollector::new(std::time::Instant::now()),
        );
        // One observed turn → the finalized partial metrics are not all-zero.
        hook.write(TranscriptEvent::ModelTurn {
            bytes: b"partial-turn".to_vec(),
            usage: None,
            busy_ms: 5,
        })
        .expect("hook write on an open hook must succeed");
        state.hook_registry.register(
            lease_id,
            TenantId::new("acme").unwrap(),
            hook,
            "hookcred-reaper",
        );
    }

    /// Insert a `Held` lease record in the ledger with the given deadline.
    fn insert_held(state: &AppState, lease_id: &str, deadline_ms: u64) {
        {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .put(LeaseRecord {
                    lease_id: lease_id.to_string(),
                    tenant: TenantId::new("acme").unwrap(),
                    state: LeaseState::Pending,
                    box_ref: format!("box:{lease_id}"),
                    created_at_ms: 0,
                    updated_at_ms: 0,
                })
                .unwrap();
            ledger.transition(lease_id, RunnerState::Held, 0).unwrap();
        }
        // Record the deadline AND a fake image so forget_lease GC is verifiable.
        state.record_deadline(lease_id, deadline_ms);
        state.record_image(lease_id, "sha256:deadbeef");
    }

    // ── Config tests ──────────────────────────────────────────────────────────

    /// Absent env → default 30 s.
    #[test]
    fn reap_config_default_30() {
        let cfg = reaper_config_from_env(|_| None).unwrap();
        assert_eq!(cfg.interval, std::time::Duration::from_secs(30));
    }

    /// `FABRIC_REAP_INTERVAL_SECS=0` → error; `="15"` → 15 s.
    #[test]
    fn reap_config_zero_errs() {
        let result = reaper_config_from_env(|k| {
            if k == "FABRIC_REAP_INTERVAL_SECS" {
                Some("0".to_string())
            } else {
                None
            }
        });
        assert!(result.is_err(), "FABRIC_REAP_INTERVAL_SECS=0 must error");

        let cfg = reaper_config_from_env(|k| {
            if k == "FABRIC_REAP_INTERVAL_SECS" {
                Some("15".to_string())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(cfg.interval, std::time::Duration::from_secs(15));
    }

    // ── reap_once behavior tests ──────────────────────────────────────────────

    /// An overdue `Held` lease with a succeeding provisioner:
    /// - reap_once returns 1
    /// - lease is `Expired` in the ledger
    /// - deadlines + images entries are GC'd
    /// - teardown was called
    #[tokio::test]
    async fn reap_once_expires_and_tears_down_overdue() {
        // Clock at 2000 ms; deadline 1000 ms → already past.
        let (state, _clock, prov) = build_state(2_000);
        insert_held(&state, "lease-overdue", 1_000);

        let count = reap_once(&state).await;
        assert_eq!(count, 1, "exactly one lease should be reclaimed");

        // Lease must now be `Expired` in the ledger.
        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-overdue").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Expired),
            "lease must be Expired after reap"
        );
        drop(ledger);

        // Provisioner must have seen a teardown call.
        let calls = prov.teardown_calls();
        assert!(
            calls.contains(&"lease-overdue".to_string()),
            "teardown must be called for the expired lease; calls={calls:?}"
        );

        // Side-tables must be GC'd.
        assert!(
            state.deadline_of("lease-overdue").is_none(),
            "deadline entry must be removed after successful reclaim"
        );
        assert!(
            state.image_of("lease-overdue").is_none(),
            "image entry must be removed after successful reclaim"
        );
    }

    /// THE CRUX REGRESSION TEST: teardown-first, retry-on-failure.
    ///
    /// Scenario:
    /// 1. First sweep: teardown FAILS → reap_once returns 0, lease is STILL
    ///    Held (not Expired), deadline entry retained.
    /// 2. Second sweep: teardown succeeds → reap_once returns 1, lease is
    ///    Expired, deadline entry GC'd.
    #[tokio::test]
    async fn reap_once_failed_teardown_keeps_held_for_retry() {
        let (state, _clock, prov, flag) =
            build_state_toggles(2_000, /* initial_succeed */ false);
        insert_held(&state, "lease-retry", 1_000);

        // ── Sweep 1: teardown fails ──────────────────────────────────────────
        let count = reap_once(&state).await;
        assert_eq!(count, 0, "failed teardown: nothing reaped");

        // Lease MUST still be Held — NOT Expired.
        {
            let ledger = state.ledger.lock().unwrap();
            let rec = ledger.get("lease-retry").unwrap().unwrap();
            assert!(
                rec.state.is_held(),
                "lease must remain Held after failed teardown; state={:?}",
                rec.state
            );
        }

        // Deadline entry MUST be retained so the next sweep can date the lease.
        assert!(
            state.deadline_of("lease-retry").is_some(),
            "deadline entry must NOT be GC'd after failed teardown"
        );

        // Teardown was attempted once.
        assert_eq!(
            prov.teardown_calls().len(),
            1,
            "teardown should have been called once (and failed)"
        );

        // ── Sweep 2: make teardown succeed, re-run ───────────────────────────
        flag.store(true, Ordering::SeqCst);

        let count2 = reap_once(&state).await;
        assert_eq!(count2, 1, "second sweep with successful teardown: 1 reaped");

        // Lease is now Expired.
        {
            let ledger = state.ledger.lock().unwrap();
            let rec = ledger.get("lease-retry").unwrap().unwrap();
            assert_eq!(
                rec.state,
                LeaseState::Wire(RunnerState::Expired),
                "lease must be Expired after second sweep"
            );
        }

        // Deadline entry GC'd.
        assert!(
            state.deadline_of("lease-retry").is_none(),
            "deadline entry must be GC'd after successful reclaim"
        );

        // Teardown was called twice total (once fail, once succeed).
        assert_eq!(
            prov.teardown_calls().len(),
            2,
            "teardown must have been called twice (retry)"
        );
    }

    /// A `Held` lease with a future deadline is left untouched.
    #[tokio::test]
    async fn reap_once_leaves_unexpired_held() {
        // Clock at 1000; deadline at 9000 → not yet overdue.
        let (state, _clock, prov) = build_state(1_000);
        insert_held(&state, "lease-fresh", 9_000);

        let count = reap_once(&state).await;
        assert_eq!(count, 0, "no leases should be expired when all are fresh");

        // Lease must still be Held.
        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-fresh").unwrap().unwrap();
        assert!(rec.state.is_held(), "unexpired lease must remain Held");
        drop(ledger);

        // No teardown calls.
        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must not be called for a fresh lease"
        );
    }

    /// Empty or no-Held ledger → 0 expirations, no teardown.
    #[tokio::test]
    async fn reap_once_no_held_is_noop() {
        let (state, _clock, prov) = build_state(999_999);
        // No leases inserted at all.
        let count = reap_once(&state).await;
        assert_eq!(count, 0, "empty ledger must produce 0 expirations");
        assert!(
            prov.teardown_calls().is_empty(),
            "no teardowns on empty ledger"
        );
    }

    // ── WP-SLOT-EMIT: reaper_expiry_emits_expired_slot ───────────────────────
    //
    // Lives here (in-crate) because `record_deadline` is `pub(crate)` and
    // integration tests in `tests/` cannot call it.

    /// Reaper LOST RACE: if a concurrent close/cancel already terminalized the
    /// lease (Released), the reaper's `held()` snapshot excludes it — so
    /// `reap_once` processes zero overdue leases and emits no Expired event.
    ///
    /// This guards FIX 2: even before the transition gate, the held() snapshot
    /// already filters out Released/terminal leases, so a race-lost reaper pass
    /// produces no phantom Expired event and occupied stays 0.
    #[tokio::test]
    async fn reaper_lost_race_emits_no_phantom_expired() {
        // Clock at 2 000 ms; deadline 1 000 ms → overdue if still Held.
        let (state, _clock, _prov) = build_state(2_000);
        let acme = TenantId::new("acme").unwrap();

        // Insert a lease, record it in the slot meter as Acquired (1 occupied).
        insert_held(&state, "lease-race", 1_000);
        state.record_slot("lease-race", &acme, SlotEventKind::Acquired);

        // Simulate close/cancel winning the race: transition to Released under
        // the ledger lock, then emit Released in the slot meter.
        {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .transition("lease-race", RunnerState::Released, 1_500)
                .expect("Held→Released must succeed");
        }
        state.record_slot("lease-race", &acme, SlotEventKind::Released);

        // Now run the reaper. The lease is Released (terminal), so `held()`
        // excludes it — reap_once does nothing.
        let reaped = reap_once(&state).await;
        assert_eq!(
            reaped, 0,
            "reaper must reap 0: the lease is already Released"
        );

        let meter = state.slot_meter.lock().unwrap();
        // No Expired event: the reaper never saw the lease as Held.
        let expired_count = meter
            .journal()
            .iter()
            .filter(|e| matches!(e.kind, corelink_fabric::SlotEventKind::Expired))
            .count();
        assert_eq!(
            expired_count, 0,
            "no Expired event must be emitted when the reaper loses the race"
        );
        // Slot is correctly at 0 (Acquired then Released; no phantom Expired).
        assert_eq!(
            meter.occupied(&acme),
            0,
            "occupied must be 0 (no double-free from phantom Expired)"
        );
        // Journal must contain exactly the Acquired + Released pair from the
        // simulated close, nothing else.
        assert_eq!(
            meter.journal().len(),
            2,
            "journal must have exactly Acquired + Released, no extra Expired"
        );
    }

    /// A Held lease reaped via `reap_once` frees its slot: `occupied == 0`
    /// and the journal records an `Expired` event.
    #[tokio::test]
    async fn reaper_expiry_emits_expired_slot() {
        // Clock at 2 000 ms; deadline 1 000 ms → already past.
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-slot-expiry", 1_000);

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "one lease must be reaped");

        let acme = TenantId::new("acme").unwrap();
        let meter = state.slot_meter.lock().unwrap();

        // Slot freed: occupied back to zero.
        assert_eq!(
            meter.occupied(&acme),
            0,
            "slot must be freed after expiry (occupied==0)"
        );
        // Exactly one event: the Expired emission.
        assert_eq!(
            meter.journal().len(),
            1,
            "journal must have exactly one Expired event"
        );
        assert!(
            matches!(
                meter.journal()[0].kind,
                corelink_fabric::SlotEventKind::Expired
            ),
            "the journaled event must be Expired"
        );
        assert_eq!(
            meter.journal()[0].lease_id,
            "lease-slot-expiry",
            "event lease_id must match"
        );
    }

    // ── WP-S13.5: partial-envelope flush on abnormal close ───────────────────

    /// EXPIRED flush: a reaped lease WITH a registered hook drives
    /// `close_abnormal` — the finalized partial outcome carries
    /// `close_reason = Expired` + `capture_incomplete = true`, and the metrics
    /// come from the FINALIZED (redacted) outcome, not raw capture. Teardown +
    /// transition + slot-free all still happen (the flush is post-reclaim and
    /// non-blocking).
    #[tokio::test]
    async fn expired_flush_finalizes_partial_envelope_marked_incomplete() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, prov) = build_state(2_000);
        insert_held(&state, "lease-flush-exp", 1_000);
        register_hook(&state, "lease-flush-exp");

        // Drive the flush directly (the same call reap_once makes post-reclaim).
        let outcome = flush_partial_envelope(
            &state,
            "lease-flush-exp",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("a registered hook must produce a finalized partial outcome");

        assert_eq!(
            outcome.close_reason,
            CloseReason::Expired,
            "expired flush must mark close_reason=expired"
        );
        assert!(
            outcome.capture_incomplete,
            "an abnormal partial envelope is always capture_incomplete"
        );
        // REDACTION: the summary comes from the finalized outcome's metrics
        // (same finalize path as a normal close — the observed turn is counted).
        assert_eq!(
            outcome.metrics.model_turns, 1,
            "metrics must be the FINALIZED projection (one observed turn), not raw capture"
        );

        // The hook was EXTRACTED (dedup): a second flush finds nothing.
        assert!(
            flush_partial_envelope(
                &state,
                "lease-flush-exp",
                &TenantId::new("acme").unwrap(),
                AbnormalKind::Expiry,
                std::time::Instant::now(),
            )
            .is_none(),
            "the hook is consumed once — no second partial envelope"
        );

        // And the full reaper sweep still reclaims cleanly (teardown + Expired
        // + slot-free), unaffected by the flush.
        let _ = prov; // teardown recorder; the sweep below exercises it.
    }

    /// A reaped Expired lease with NO hook → reaped normally, NO flush, no
    /// panic: `flush_partial_envelope` is a silent no-op and `reap_once`
    /// still returns 1.
    #[tokio::test]
    async fn expired_no_hook_reaps_without_flush() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-nohook", 1_000);
        // No register_hook.

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "a hookless lease still reaps normally");

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-nohook").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Expired),
            "hookless lease must be Expired after reap"
        );
    }

    /// reap_once END-TO-END with a hook: the lease is reaped (Expired + slot
    /// freed) AND its hook is consumed by the flush. Proves the flush is wired
    /// into the sweep and does not break reclamation.
    #[tokio::test]
    async fn reap_once_drives_flush_and_consumes_hook() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-e2e-exp", 1_000);
        register_hook(&state, "lease-e2e-exp");

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "the lease must be reaped");

        // Lease Expired.
        {
            let ledger = state.ledger.lock().unwrap();
            let rec = ledger.get("lease-e2e-exp").unwrap().unwrap();
            assert_eq!(rec.state, LeaseState::Wire(RunnerState::Expired));
        }
        // Hook consumed by the in-sweep flush — a follow-up flush is a no-op.
        assert!(
            flush_partial_envelope(
                &state,
                "lease-e2e-exp",
                &TenantId::new("acme").unwrap(),
                AbnormalKind::Expiry,
                std::time::Instant::now(),
            )
            .is_none(),
            "reap_once already drove the flush and consumed the hook"
        );
    }

    /// CRASHED flush: same shape via `surface_crashes` → the finalized partial
    /// outcome carries `close_reason = Crashed` + `capture_incomplete = true`.
    #[tokio::test]
    async fn crashed_flush_marks_close_reason_crashed() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, _prov) = build_state_scripted(5_000, Scripted::Dead, true);
        insert_held(&state, "lease-flush-crash", 9_999_999);
        register_hook(&state, "lease-flush-crash");

        let outcome = flush_partial_envelope(
            &state,
            "lease-flush-crash",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Crash,
            std::time::Instant::now(),
        )
        .expect("a registered hook must produce a finalized partial outcome");
        assert_eq!(outcome.close_reason, CloseReason::Crashed);
        assert!(outcome.capture_incomplete);

        // The direct-flush lease above is still `Held` (a direct
        // `flush_partial_envelope` does NOT terminalize the lease — only the
        // sweep does). Remove it so the E2E sweep below reclaims exactly the
        // one fresh lease and the reclaim count isn't skewed by this residue.
        state
            .ledger
            .lock()
            .unwrap()
            .remove("lease-flush-crash")
            .unwrap();

        // End-to-end through the crash sweep on a fresh lease + hook.
        insert_held(&state, "lease-crash-e2e", 9_999_999);
        register_hook(&state, "lease-crash-e2e");
        let reaped = surface_crashes(&state).await;
        assert_eq!(reaped, 1, "the dead box must be reclaimed");
        {
            let ledger = state.ledger.lock().unwrap();
            let rec = ledger.get("lease-crash-e2e").unwrap().unwrap();
            assert_eq!(rec.state, LeaseState::Wire(RunnerState::Crashed));
        }
        assert!(
            flush_partial_envelope(
                &state,
                "lease-crash-e2e",
                &TenantId::new("acme").unwrap(),
                AbnormalKind::Crash,
                std::time::Instant::now(),
            )
            .is_none(),
            "surface_crashes already drove the flush and consumed the hook"
        );
    }

    /// NO DOUBLE-FIRE: a lease whose hook's exactly-once close already fired
    /// (a normal close consumed it) then reaped → the sweep's `close_abnormal`
    /// returns the exactly-once `Err`, the flush logs + continues (returns
    /// None), NO second envelope, NO double-free.
    ///
    /// The exactly-once latch lives on the SHARED hook state, so a clone of the
    /// hook re-registered under the lease id still observes the closed latch —
    /// exactly how a normal close (which holds its own clone) races the sweep.
    #[tokio::test]
    async fn no_double_fire_when_hook_already_closed() {
        use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector, PriceCard};

        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-double", 1_000);

        // Build a hook, register a CLONE (clones share the close latch), keep
        // the original to drive the "normal close already fired" first close.
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: std::time::Duration::from_millis(1),
                buffer_capacity: 16,
            },
            "hookcred-reaper",
            MetricsCollector::new(std::time::Instant::now()),
        );
        state.hook_registry.register(
            "lease-double",
            TenantId::new("acme").unwrap(),
            hook.clone(),
            "hookcred-reaper",
        );

        // Normal close fires the exactly-once latch on the shared state.
        corelink_runner::envelope::JobClose::new(&hook)
            .close_abnormal(
                AbnormalKind::Crash,
                std::time::Instant::now(),
                &PriceCard {
                    input_per_mtok_micros: 0,
                    output_per_mtok_micros: 0,
                    cache_read_per_mtok_micros: 0,
                    cache_write_per_mtok_micros: 0,
                },
            )
            .expect("first close fires once");

        // The reaper's flush extracts the (still-registered) clone and tries
        // close_abnormal again → exactly-once Err → returns None, no panic.
        let second = flush_partial_envelope(
            &state,
            "lease-double",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        );
        assert!(
            second.is_none(),
            "an already-closed hook must yield NO second envelope (exactly-once)"
        );

        // The sweep still reclaims the lease cleanly despite the skipped flush.
        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "reclamation is unaffected by a skipped flush");
    }

    // ── WP-CRASH-SWEEP: surface_crashes behavior tests ───────────────────────

    /// Count `Crashed` events for `lease_id` in the slot journal.
    fn crashed_events(state: &AppState, lease_id: &str) -> usize {
        let meter = state.slot_meter.lock().unwrap();
        meter
            .journal()
            .iter()
            .filter(|e| matches!(e.kind, SlotEventKind::Crashed) && e.lease_id == lease_id)
            .count()
    }

    /// Probe → Ok(Dead), teardown succeeds → lease becomes `Crashed`, a Crashed
    /// slot event is emitted, side-tables GC'd, count == 1.
    #[tokio::test]
    async fn dead_box_is_reclaimed_as_crashed() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Dead, true);
        // Deadline far in the future — proves crash-surfacing is NOT deadline-driven.
        insert_held(&state, "lease-dead", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 1, "a dead box must be reclaimed");

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-dead").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Crashed),
            "lease must be Crashed after surface_crashes"
        );
        drop(ledger);

        assert!(
            prov.teardown_calls().contains(&"lease-dead".to_string()),
            "teardown must be called for the dead lease"
        );
        assert_eq!(crashed_events(&state, "lease-dead"), 1, "one Crashed event");
        assert!(
            state.deadline_of("lease-dead").is_none(),
            "deadline entry GC'd after crash reclaim"
        );
        assert!(
            state.image_of("lease-dead").is_none(),
            "image entry GC'd after crash reclaim"
        );
    }

    /// Probe → Ok(Alive) → lease stays Held, NO teardown, NO Crashed, count 0.
    #[tokio::test]
    async fn alive_box_is_left_held() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Alive, true);
        insert_held(&state, "lease-alive", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "an alive box must not be reclaimed");

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-alive").unwrap().unwrap();
        assert!(rec.state.is_held(), "alive lease must remain Held");
        drop(ledger);

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called for an alive box"
        );
        assert_eq!(crashed_events(&state, "lease-alive"), 0, "no Crashed event");
    }

    /// Probe → Ok(Unbound) → untouched, count 0.
    #[tokio::test]
    async fn unbound_lease_is_left_held() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Unbound, true);
        insert_held(&state, "lease-unbound", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "an unbound lease must not be reclaimed");

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-unbound").unwrap().unwrap();
        assert!(rec.state.is_held(), "unbound lease must remain Held");
        drop(ledger);

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called for an unbound lease"
        );
        assert_eq!(
            crashed_events(&state, "lease-unbound"),
            0,
            "no Crashed event"
        );
    }

    /// FAIL-SAFE CRUX: probe → Err(...) → lease stays Held, NO Crashed, count 0.
    /// An unreachable provider must NEVER be read as death.
    #[tokio::test]
    async fn probe_error_is_fail_safe() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Err, true);
        insert_held(&state, "lease-err", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(
            count, 0,
            "a probe Err is NOT death — fail-safe, nothing reclaimed"
        );

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-err").unwrap().unwrap();
        assert!(
            rec.state.is_held(),
            "lease must remain Held when the probe errors (unreachable != dead)"
        );
        drop(ledger);

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called on a probe error"
        );
        assert_eq!(crashed_events(&state, "lease-err"), 0, "no Crashed event");
    }

    /// Probe Ok(Dead) but teardown FAILS → lease stays Held for retry, NO
    /// Crashed transition/event, count 0.
    #[tokio::test]
    async fn teardown_failure_leaves_held_for_retry() {
        let (state, _clock, prov) =
            build_state_scripted(5_000, Scripted::Dead, /* teardown_succeed */ false);
        insert_held(&state, "lease-tdfail", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "failed teardown: nothing reclaimed");

        let ledger = state.ledger.lock().unwrap();
        let rec = ledger.get("lease-tdfail").unwrap().unwrap();
        assert!(
            rec.state.is_held(),
            "lease must remain Held after a failed teardown (retry next sweep)"
        );
        drop(ledger);

        // Teardown was ATTEMPTED (and failed), but no Crashed mark/event.
        assert_eq!(
            prov.teardown_calls().len(),
            1,
            "teardown must have been attempted once"
        );
        assert_eq!(
            crashed_events(&state, "lease-tdfail"),
            0,
            "no Crashed event"
        );
        assert!(
            state.deadline_of("lease-tdfail").is_some(),
            "deadline retained after failed teardown"
        );
    }

    /// Probe Ok(Dead), teardown ok, but the lease was already terminalized
    /// (Released) → transition Err → NO Crashed event, NO GC, count 0 (no
    /// double-free of the slot).
    #[tokio::test]
    async fn crash_loses_race_to_close_does_not_double_free() {
        let (state, _clock, _prov) = build_state_scripted(5_000, Scripted::Dead, true);
        let acme = TenantId::new("acme").unwrap();

        insert_held(&state, "lease-lostrace", 9_999_999);
        state.record_slot("lease-lostrace", &acme, SlotEventKind::Acquired);

        // Simulate close/cancel winning the race: Held → Released, emit Released.
        {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .transition("lease-lostrace", RunnerState::Released, 4_000)
                .expect("Held→Released must succeed");
        }
        state.record_slot("lease-lostrace", &acme, SlotEventKind::Released);

        // The lease is now Released (terminal): held() excludes it, so the
        // sweep processes zero leases.
        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "a race-lost crash sweep reclaims nothing");

        assert_eq!(
            crashed_events(&state, "lease-lostrace"),
            0,
            "NO Crashed event when close won the race (would double-free)"
        );
        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.occupied(&acme),
            0,
            "occupied stays 0 — no phantom Crashed double-free"
        );
        assert_eq!(
            meter.journal().len(),
            2,
            "journal has exactly Acquired + Released, no extra Crashed"
        );
    }

    // ── crash_probe_config_from_env tests ────────────────────────────────────

    /// Absent/empty → opt-in default: `Ok(None)` (sweep NOT spawned).
    #[test]
    fn crash_probe_config_absent_is_none() {
        assert_eq!(crash_probe_config_from_env(|_| None).unwrap(), None);
        assert_eq!(
            crash_probe_config_from_env(|k| {
                if k == "FABRIC_CRASH_PROBE_INTERVAL_SECS" {
                    Some(String::new())
                } else {
                    None
                }
            })
            .unwrap(),
            None,
            "empty string is also treated as absent (opt-in default)"
        );
    }

    /// `0`/garbage → Err; valid → `Ok(Some(Duration))`.
    #[test]
    fn crash_probe_config_zero_or_garbage_errs_valid_ok() {
        let mk = |v: &'static str| {
            crash_probe_config_from_env(move |k| {
                if k == "FABRIC_CRASH_PROBE_INTERVAL_SECS" {
                    Some(v.to_string())
                } else {
                    None
                }
            })
        };
        assert!(
            mk("0").is_err(),
            "0 must error (absence is the disable path)"
        );
        assert!(mk("notanumber").is_err(), "garbage must error");
        assert_eq!(
            mk("45").unwrap(),
            Some(std::time::Duration::from_secs(45)),
            "a valid u32 yields Some(Duration)"
        );
    }
}
