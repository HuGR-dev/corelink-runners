//! CP4 — queued fair admission (ADR-0005), behind a DEFAULT-OFF flag.
//!
//! The live server's acquire path is immediate-or-reject: `try_admit` either
//! reserves a slot atomically or returns `over_cap` (429). The
//! [`corelink_fabric::FairScheduler`] — a complete, tested deficit-round-robin
//! dispatcher — was built (CP3) but NEVER driven by the live server, so
//! `TenantWaitStats::observe_tick` was never called and `GET /v1/metrics/tenant`
//! honestly returned `count:0` for every tenant (the W2-D audit flag).
//!
//! This module wires the scheduler in, selected by [`AdmissionMode`]:
//!
//! - [`AdmissionMode::Reject`] (**default**): the acquire path is byte-for-byte
//!   today's immediate over-cap 429 — ZERO behavior change. This module's queue
//!   path is never reached; `AppState::admission_queue` is `None`.
//! - [`AdmissionMode::Queue`]: an over-cap acquire is ENQUEUED into the
//!   per-tenant scheduler ([`acquire_queued`]) and the HTTP request WAITS
//!   (bounded async) until the background admission loop ([`run_admission_tick`],
//!   spawned by [`spawn_admission_loop`]) dispatches it — or a caller-bounded
//!   timeout elapses (→ 503 fail-closed, never a silent hang).
//!
//! # Why the reservation is authoritative inside the tick
//!
//! [`corelink_fabric::FairScheduler::tick`] is a synchronous pure core, and
//! `LeaseLedger::try_admit` is a synchronous atomic op under the ledger Mutex.
//! So the reservation happens INSIDE the tick's `dispatch` closure (synchronous
//! `try_admit`), and only the async provision/finalize runs AFTER the tick. The
//! cap therefore has exactly one source of truth — `try_admit` — and the
//! scheduler can never over-admit: every dispatched item must win its own
//! atomic reservation. The DB-global advisory-lock count in `try_admit` bounds
//! total admission across ALL instances, so two instances' in-memory queues can
//! never over-admit beyond the tenant cap.
//!
//! # Cross-instance limitation (M1, documented — not solved)
//!
//! The [`FairScheduler`] is per-instance / in-memory: two server instances have
//! independent queues, so global cross-instance *fairness* is not guaranteed at
//! M1 (ADR-0005). The DB-global cap is still enforced (see above), so this is a
//! fairness gap, never an over-admit. A durable cross-instance queue is an M1
//! production item; no durable queue is added here.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use axum::response::Response;
use corelink_fabric::{FairScheduler, LeaseRecord, LeaseState, TenantId, WorkItem};
use corelink_fabric_api::{AcquireRequest, ApiError};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerLease;
use tokio::sync::oneshot;

use crate::app::AppState;
use crate::auth::{BearerPat, error_response};
use crate::handlers::leases::{MintedLease, finalize_admitted_lease};

/// Which admission discipline the acquire path uses when a tenant is over its
/// concurrency cap. From `FABRIC_ADMISSION_MODE` (default [`Reject`]).
///
/// [`Reject`]: AdmissionMode::Reject
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionMode {
    /// Immediate-or-reject (the default + today's behavior): an over-cap
    /// acquire is rejected `over_cap` (429) preventively, before any box/VM.
    Reject,
    /// Queued fair admission (ADR-0005): an over-cap acquire is enqueued into
    /// the per-tenant [`FairScheduler`] and the request waits (bounded) for the
    /// admission loop to dispatch it fairly when a slot frees.
    Queue,
}

/// Default bounded wait for a queued acquire before it 503s (fail-closed). A
/// queued waiter never pins a blocking-pool thread (it `.await`s a oneshot), so
/// this is a generous ceiling, not a thread-occupancy budget. From
/// `FABRIC_ADMISSION_QUEUE_WAIT_MS`.
pub const DEFAULT_QUEUE_WAIT_MS: u64 = 30_000;

/// Default per-tick dispatch budget (global slots) for the admission loop's
/// [`FairScheduler`]. The authoritative cap is always `try_admit` (per-tenant,
/// DB-global), so this is only the MAX items the loop attempts to dispatch per
/// tick — a throughput knob, never a correctness bound. From
/// `FABRIC_ADMISSION_TICK_SLOTS`.
pub const DEFAULT_TICK_SLOTS: u32 = 64;

/// Default admission-loop tick interval. From `FABRIC_ADMISSION_TICK_MS`.
pub const DEFAULT_TICK_MS: u64 = 50;

/// Resolve [`AdmissionMode`] from an environment-variable accessor.
///
/// Reads `FABRIC_ADMISSION_MODE`:
/// - Absent or empty → [`AdmissionMode::Reject`] (the default — today's
///   immediate-or-reject behavior, ZERO change).
/// - `"reject"` → [`AdmissionMode::Reject`].
/// - `"queue"` → [`AdmissionMode::Queue`].
/// - Any other value → `Err` (fail-closed; never a silent default).
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn admission_mode_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<AdmissionMode> {
    match get("FABRIC_ADMISSION_MODE")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => Ok(AdmissionMode::Reject),
        Some(v) => match v.as_str() {
            "reject" => Ok(AdmissionMode::Reject),
            "queue" => Ok(AdmissionMode::Queue),
            other => anyhow::bail!(
                "unknown FABRIC_ADMISSION_MODE {other:?}; expected \"reject\" or \"queue\""
            ),
        },
    }
}

/// Resolve the queued-acquire bounded wait from `FABRIC_ADMISSION_QUEUE_WAIT_MS`.
///
/// Absent/empty → [`DEFAULT_QUEUE_WAIT_MS`]; present → parse as `u64` (0 or
/// unparseable → `Err`, a deployer mistake fails loudly).
pub fn queue_wait_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Duration> {
    let ms = parse_positive_u64(
        &get,
        "FABRIC_ADMISSION_QUEUE_WAIT_MS",
        DEFAULT_QUEUE_WAIT_MS,
    )?;
    Ok(Duration::from_millis(ms))
}

/// Resolve the admission-loop tick interval from `FABRIC_ADMISSION_TICK_MS`.
pub fn tick_interval_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Duration> {
    let ms = parse_positive_u64(&get, "FABRIC_ADMISSION_TICK_MS", DEFAULT_TICK_MS)?;
    Ok(Duration::from_millis(ms))
}

/// Resolve the per-tick dispatch budget from `FABRIC_ADMISSION_TICK_SLOTS`.
pub fn tick_slots_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<u32> {
    let n = parse_positive_u64(
        &get,
        "FABRIC_ADMISSION_TICK_SLOTS",
        DEFAULT_TICK_SLOTS as u64,
    )?;
    Ok(n as u32)
}

/// Parse an optional positive-`u64` env var, falling back to `default` when
/// absent/empty. A present `0` or unparseable value is a hard error.
fn parse_positive_u64(
    get: impl Fn(&str) -> Option<String>,
    key: &str,
    default: u64,
) -> anyhow::Result<u64> {
    match get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => Ok(default),
        Some(v) => {
            let n: u64 = v
                .parse()
                .map_err(|_| anyhow::anyhow!("{key} must be a valid u64 (got {v:?})"))?;
            if n == 0 {
                anyhow::bail!("{key} must be >= 1 (0 is degenerate)");
            }
            Ok(n)
        }
    }
}

/// The deferred context of one queued acquire: everything the admission loop
/// needs to finalize the lease once it wins a `try_admit` reservation, plus the
/// oneshot back to the waiting HTTP request.
struct QueuedAcquire {
    tenant: TenantId,
    pat: BearerPat,
    req: AcquireRequest,
    lease: RunnerLease,
    spec: ContainerSpec,
    /// Built `Pending` record handed to `try_admit` in the dispatch closure.
    pending: LeaseRecord,
    /// Wakes the HTTP waiter with the finalized `Response` (the `AcquireResponse`
    /// on success, a fail-closed body on a post-reserve failure).
    waker: oneshot::Sender<Response>,
}

/// Per-instance queued-admission state (ADR-0005): the [`FairScheduler`] plus
/// the in-flight waiters' deferred contexts, keyed by lease id. Shared by the
/// acquire handlers (enqueue) and the background admission loop (dispatch).
///
/// In-memory and per-instance by design (cross-instance fairness is an M1 item;
/// the DB-global `try_admit` cap still bounds total admission — see module doc).
pub struct AdmissionQueue {
    /// The fair dispatcher. Bounded per-tenant by `MAX_TENANT_QUEUE_DEPTH`.
    scheduler: Mutex<FairScheduler>,
    /// lease_id → the waiter's deferred context. An entry exists exactly while
    /// the acquire is queued (inserted at enqueue, removed at dispatch).
    waiters: Mutex<HashMap<String, QueuedAcquire>>,
}

impl AdmissionQueue {
    /// New queue with a per-tick dispatch budget of `tick_slots` global slots
    /// (the FairScheduler's per-tick budget; the authoritative cap is always
    /// `try_admit`, so this is only a throughput knob, never a correctness one).
    pub fn new(tick_slots: u32) -> Self {
        Self {
            scheduler: Mutex::new(FairScheduler::new(tick_slots.max(1))),
            waiters: Mutex::new(HashMap::new()),
        }
    }

    /// Pending queued acquires for one tenant (test/observability).
    pub fn pending(&self, tenant: &TenantId) -> usize {
        self.scheduler
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending(tenant)
    }

    /// Test-only: flood a tenant's scheduler FIFO to exactly the per-tenant
    /// bound (`MAX_TENANT_QUEUE_DEPTH`) with placeholder WorkItems, so the next
    /// real `acquire_queued` enqueue is over the bound and must SHED. Returns how
    /// many were admitted (== the bound).
    #[cfg(test)]
    fn fill_to_bound(&self, tenant: &TenantId, now_ms: u64) -> usize {
        let mut sched = self.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        let mut admitted = 0usize;
        for i in 0..corelink_fabric::MAX_TENANT_QUEUE_DEPTH {
            let item = WorkItem {
                id: format!("filler-{i}"),
                tenant: tenant.clone(),
                enqueued_at_ms: now_ms,
            };
            if sched.enqueue(item).is_ok() {
                admitted += 1;
            }
        }
        admitted
    }
}

/// Enqueue an over-cap acquire into the per-tenant scheduler and WAIT (bounded
/// async) for the admission loop to dispatch it — the [`AdmissionMode::Queue`]
/// path (ADR-0005).
///
/// Fail-closed at every edge:
/// - No queue configured (defensive; `queue` mode always wires one) → 503.
/// - Over the per-tenant queue bound (`MAX_TENANT_QUEUE_DEPTH`) → 503 SHED,
///   never unbounded growth.
/// - Wait timeout elapses → 503 (the lease was never reserved; nothing leaks).
///
/// On success the loop sends back the SAME `AcquireResponse` the immediate path
/// builds (no wire change). The lease is RESERVED only inside the loop's
/// `try_admit` (the single atomic cap gate), so no slot is consumed unless the
/// waiter is actually dispatched.
pub(crate) async fn acquire_queued(
    state: &AppState,
    tenant: TenantId,
    pat: BearerPat,
    req: AcquireRequest,
    minted: MintedLease,
    now_ms: u64,
) -> Response {
    let MintedLease {
        lease_id,
        lease,
        spec,
    } = minted;
    let Some(queue) = state.admission_queue.as_ref() else {
        // Defensive: queue mode always wires a queue (build_app_and_state); a
        // None here is a composition bug — fail closed, never silently 429.
        return error_response(
            ApiError::FailClosed,
            "queued admission selected but no admission queue is wired; failing closed",
        );
    };

    // Build the Pending record the dispatch loop will hand to try_admit — the
    // SAME shape the immediate path builds (deadline rides the record, ADR-0004).
    let pending = LeaseRecord {
        lease_id: lease_id.clone(),
        tenant: tenant.clone(),
        state: LeaseState::Pending,
        box_ref: format!("box:{lease_id}"),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        deadline_ms: Some(lease.expiry),
    };

    let (waker, wait_rx) = oneshot::channel::<Response>();

    // ── Enqueue under the per-tenant bound (MAX_TENANT_QUEUE_DEPTH). Insert the
    // waiter context FIRST so the dispatch loop can never observe a queued item
    // with no context; if the enqueue is shed, remove it again.
    {
        let queued = QueuedAcquire {
            tenant: tenant.clone(),
            pat,
            req,
            lease,
            spec,
            pending,
            waker,
        };
        queue
            .waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(lease_id.clone(), queued);

        let mut sched = queue.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        let item = WorkItem {
            id: lease_id.clone(),
            tenant: tenant.clone(),
            enqueued_at_ms: now_ms,
        };
        if sched.enqueue(item).is_err() {
            // Over the per-tenant bound: SHED. Drop the waiter context too so
            // nothing is leaked, then 503 (never unbounded queue growth).
            drop(sched);
            queue
                .waiters
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&lease_id);
            return error_response(
                ApiError::FailClosed,
                "admission queue full for tenant: shed (try again shortly)",
            );
        }
    }

    // ── Bounded async wait. The loop fulfills `waker` with the response, or the
    // timeout fires → 503 fail-closed. On timeout, evict the now-orphaned waiter
    // + its queued WorkItem so a late dispatch can never reserve a slot for a
    // request that already gave up.
    match tokio::time::timeout(state.queue_wait_timeout, wait_rx).await {
        Ok(Ok(resp)) => resp,
        // Sender dropped without sending (loop shutdown / internal drop): 503.
        Ok(Err(_)) => error_response(
            ApiError::FailClosed,
            "admission loop dropped the queued acquire; failing closed",
        ),
        Err(_elapsed) => {
            evict_waiter(queue, &lease_id);
            error_response(
                ApiError::FailClosed,
                "queued admission timed out before a slot freed; failing closed",
            )
        }
    }
}

/// Remove a waiter's context AND its queued WorkItem (best-effort) — called when
/// a waiter times out, so a late dispatch cannot reserve a slot for a request
/// that already 503'd. The scheduler's FIFO is drained by id on the next tick if
/// it is mid-flight; here we drop the context so dispatch becomes a no-op.
fn evict_waiter(queue: &AdmissionQueue, lease_id: &str) {
    queue
        .waiters
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(lease_id);
    // The WorkItem may still sit in the scheduler FIFO; the dispatch closure
    // treats a missing waiter context as "already gone" and does not reserve.
}

/// Run ONE admission tick: dispatch queued acquires fairly, reserving each via
/// the authoritative `try_admit`, then finalize the reserved ones (async
/// provision → Held → hook) and wake their waiters. Feeds the per-tenant CP4
/// `wait_stats` from the tick report.
///
/// Returns the number of waiters dispatched (reserved + handed to finalize).
pub async fn run_admission_tick(state: &AppState, now_ms: u64) -> usize {
    let Some(queue) = state.admission_queue.as_ref() else {
        return 0;
    };

    // ── 1. Tick the scheduler. `cap_check` is a cheap under-cap pre-filter;
    // `dispatch` performs the AUTHORITATIVE synchronous try_admit reservation
    // and collects the reserved ids for async finalize after the tick. No
    // MutexGuard is held across an await — the whole tick is synchronous.
    let mut to_finalize: Vec<String> = Vec::new();
    let report = {
        let mut sched = queue.scheduler.lock().unwrap_or_else(|e| e.into_inner());

        let cap_check = |tenant: &TenantId| under_cap(state, tenant);

        let dispatch = |item: &WorkItem| -> bool {
            // Pull the waiter's deferred context. A missing context means the
            // waiter timed out and evicted itself → treat as dispatched (drop
            // the FIFO entry) WITHOUT reserving a slot.
            let pending = {
                let waiters = queue.waiters.lock().unwrap_or_else(|e| e.into_inner());
                match waiters.get(&item.id) {
                    Some(q) => q.pending.clone(),
                    None => return true, // orphaned: consume the FIFO slot, no reserve
                }
            };

            // AUTHORITATIVE atomic reservation — the SAME cap gate as the
            // immediate path. try_admit counts active (Pending+Held) under the
            // ledger lock and inserts iff strictly under cap. This is what makes
            // over-admit impossible even under queue (and cross-instance via the
            // pg advisory lock).
            let plan_cap = tenant_cap(state, &item.tenant);
            let reserved = {
                let mut ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
                ledger.try_admit(pending, plan_cap).unwrap_or(false)
            };
            if reserved {
                to_finalize.push(item.id.clone());
                true
            } else {
                // Still over cap: leave the item queued (FIFO head untouched),
                // park the tenant this tick — retried next tick.
                false
            }
        };

        sched.tick(now_ms, cap_check, dispatch)
    };

    // ── 2. CP4: feed the per-tenant wait stats from this tick's report. This is
    // the W2-D wiring — `GET /v1/metrics/tenant` now lights up with real
    // per-tenant wait counts under queue mode.
    state
        .wait_stats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .observe_tick(&report);

    // ── 3. Finalize each reserved lease (async: provision → Held → hook) and
    // wake its waiter with the response. The reservation already happened in the
    // tick (Pending is in the ledger), so finalize_admitted_lease drives the
    // SAME post-reserve core as the immediate path.
    let mut dispatched = 0usize;
    for lease_id in to_finalize {
        let Some(q) = queue
            .waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&lease_id)
        else {
            // Waiter timed out between reserve and finalize: roll back the
            // reserved Pending so the slot is not leaked, then move on.
            if let Ok(mut ledger) = state.ledger.lock() {
                let _ = ledger.remove(&lease_id);
            }
            continue;
        };

        let minted = MintedLease {
            lease_id,
            lease: q.lease,
            spec: q.spec,
        };
        let resp = finalize_admitted_lease(
            state,
            &state.hook_registry,
            &q.tenant,
            &q.pat,
            minted,
            &q.req,
        )
        .await;

        // Wake the waiter. If the receiver is gone (timed out), the response is
        // dropped; the lease is finalized + Held, and the reaper's deadline
        // sweep reclaims it (it has a durable deadline_ms) — no permanent leak.
        let _ = q.waker.send(resp);
        dispatched += 1;
    }

    dispatched
}

/// The tenant's concurrency cap from the plan source (0 when no plan on file —
/// fail-closed: a tenant with no plan can never be dispatched).
fn tenant_cap(state: &AppState, tenant: &TenantId) -> u32 {
    state.plans.plan_of(tenant).map_or(0, |p| p.max_concurrency)
}

/// Whether the tenant is STRICTLY under its concurrency cap in the ledger right
/// now (active = Pending+Held, the §1 definition — mirrors `CapGate`/`try_admit`).
/// The admission loop's `cap_check` pre-filter; the authoritative gate is still
/// `try_admit` inside `dispatch`.
fn under_cap(state: &AppState, tenant: &TenantId) -> bool {
    let cap = tenant_cap(state, tenant);
    if cap == 0 {
        return false;
    }
    // ACTIVE = Pending OR Held — the EXACT §1 definition `try_admit` counts.
    // Terminal records (Released/Expired/Crashed) stay in the ledger but must
    // NOT count, or a cancelled-then-freed slot would look perpetually full.
    let active = {
        let ledger = state.ledger.lock().unwrap_or_else(|e| e.into_inner());
        ledger
            .by_tenant(tenant)
            .map(|v| {
                v.iter()
                    .filter(|r| matches!(r.state, LeaseState::Pending) || r.state.is_held())
                    .count()
            })
            .unwrap_or(usize::MAX)
    };
    (active as u32) < cap
}

/// Spawn the background admission loop (ADR-0005), mirroring `spawn_reaper`.
///
/// Spawned by the composition root ONLY when `mode == queue`. The returned
/// [`tokio::task::JoinHandle`] runs until aborted; bind it and `.abort()` on
/// graceful shutdown so it does not outlive the process.
pub fn spawn_admission_loop(state: AppState, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let now_ms = state.clock.now_ms();
            let n = run_admission_tick(&state, now_ms).await;
            if n > 0 {
                eprintln!("admission-loop: dispatched {n} queued acquire(s)");
            }
        }
    })
}

// ── Compile-time Send guard ──────────────────────────────────────────────────
//
// If `run_admission_tick` ever holds a `MutexGuard` (or any `!Send` type)
// across the finalize `await`, its future becomes `!Send` and this assertion
// fails at compile time — catching the regression before CI.
#[allow(dead_code)]
const _ASSERT_RUN_ADMISSION_TICK_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: AppState) {
        _assert_send_fut(run_admission_tick(&state, 0));
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── admission_mode_from_env ───────────────────────────────────────────────

    #[test]
    fn admission_mode_absent_is_reject() {
        assert_eq!(
            admission_mode_from_env(|_| None).unwrap(),
            AdmissionMode::Reject,
            "absent env → reject (default, today's behavior)"
        );
        assert_eq!(
            admission_mode_from_env(|k| (k == "FABRIC_ADMISSION_MODE").then(String::new)).unwrap(),
            AdmissionMode::Reject,
            "empty env → reject"
        );
    }

    #[test]
    fn admission_mode_queue() {
        assert_eq!(
            admission_mode_from_env(
                |k| (k == "FABRIC_ADMISSION_MODE").then(|| "queue".to_string())
            )
            .unwrap(),
            AdmissionMode::Queue,
        );
        // Trimmed.
        assert_eq!(
            admission_mode_from_env(
                |k| (k == "FABRIC_ADMISSION_MODE").then(|| "  reject \n".to_string())
            )
            .unwrap(),
            AdmissionMode::Reject,
        );
    }

    #[test]
    fn admission_mode_garbage_is_err() {
        assert!(
            admission_mode_from_env(
                |k| (k == "FABRIC_ADMISSION_MODE").then(|| "queueueue".to_string())
            )
            .is_err(),
            "an unknown value must fail closed (never silently default)"
        );
    }

    #[test]
    fn queue_wait_and_tick_config() {
        assert_eq!(
            queue_wait_from_env(|_| None).unwrap(),
            Duration::from_millis(DEFAULT_QUEUE_WAIT_MS),
        );
        assert!(
            queue_wait_from_env(
                |k| (k == "FABRIC_ADMISSION_QUEUE_WAIT_MS").then(|| "0".to_string())
            )
            .is_err(),
            "0 wait must error"
        );
        assert_eq!(
            tick_slots_from_env(|k| (k == "FABRIC_ADMISSION_TICK_SLOTS").then(|| "8".to_string()))
                .unwrap(),
            8,
        );
        assert_eq!(
            tick_interval_from_env(|k| (k == "FABRIC_ADMISSION_TICK_MS").then(|| "25".to_string()))
                .unwrap(),
            Duration::from_millis(25),
        );
    }
}

// ── Queue-mode integration tests (ADR-0005) ─────────────────────────────────
//
// Deterministic, in-process: a fixed clock, the NoBoxProvisioner default (so
// provision is a no-op → Held immediately), and a MANUALLY-driven admission
// tick (never the timed loop — no sleeps, no flakes). An over-cap acquire is
// spawned as a task (it parks in the queue awaiting dispatch); the test frees a
// slot and calls `run_admission_tick`, then asserts the spawned acquire's HTTP
// response.
#[cfg(test)]
mod queue_tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
    use corelink_fabric_api::{AcquireResponse, paths};
    use tower::ServiceExt;

    use super::*;
    use crate::app::{AppState, Clock, StaticPlans};
    use crate::auth::StaticTokenStore;

    const PINNED: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    /// Count ACTIVE (Pending+Held) leases for a tenant — the cap-relevant set
    /// (`try_admit`'s definition). Terminal records (Released) linger in the
    /// ledger but never count against the cap.
    fn active_count(ledger: &Arc<Mutex<dyn LeaseLedger + Send>>, t: &TenantId) -> usize {
        use corelink_fabric::LeaseState;
        ledger
            .lock()
            .unwrap()
            .by_tenant(t)
            .unwrap()
            .iter()
            .filter(|r| matches!(r.state, LeaseState::Pending) || r.state.is_held())
            .count()
    }

    /// State in QUEUE mode with the given per-tenant cap, a fixed clock, and a
    /// tiny wait timeout (so a never-dispatched waiter 503s fast in the timeout
    /// test). The queue's per-tick budget is generous; `try_admit` is the cap.
    fn queue_state(
        cap: u32,
        now_ms: u64,
        wait: Duration,
    ) -> (AppState, Arc<Mutex<dyn LeaseLedger + Send>>) {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let plans = StaticPlans::new([
            TenantPlan {
                tenant: tid("alpha"),
                max_concurrency: cap,
                rate_ceiling_per_min: 10_000,
            },
            TenantPlan {
                tenant: tid("beta"),
                max_concurrency: cap,
                rate_ceiling_per_min: 10_000,
            },
        ]);
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans),
            Arc::new(FixedClock(now_ms)),
        )
        .with_admission_queue(64, wait);
        (state, ledger)
    }

    fn token_store() -> Arc<StaticTokenStore> {
        Arc::new(StaticTokenStore::new([
            ("pat-alpha".to_string(), tid("alpha")),
            ("pat-beta".to_string(), tid("beta")),
        ]))
    }

    fn acquire_req(pat: &str) -> Request<Body> {
        let body = serde_json::json!({
            "image_digest": PINNED,
            "net_policy": "isolated",
            "tmp_root": "/work/tmp",
            "expiry_ms": 600_000u64,
        });
        Request::builder()
            .method("POST")
            .uri(paths::LEASES)
            .header(header::AUTHORIZATION, format!("Bearer {pat}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    async fn status_of(resp: axum::response::Response) -> StatusCode {
        resp.status()
    }

    async fn lease_id_of(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let acq: AcquireResponse = serde_json::from_slice(&bytes).unwrap();
        acq.lease.lease_id
    }

    /// A freed slot dispatches a queued waiter: with cap=1, the first acquire
    /// fills the slot, a second acquire PARKS in the queue (awaiting dispatch),
    /// cancelling the first frees the slot, and one `run_admission_tick`
    /// dispatches the waiter → it returns 200 with a real lease. No over-admit.
    #[tokio::test]
    async fn freed_slot_dispatches_queued_waiter() {
        let now = 1_000_000u64;
        let (state, ledger) = queue_state(1, now, Duration::from_secs(5));
        let router = crate::app::app(token_store(), state.clone());

        // First acquire fills the only slot.
        let r1 = router
            .clone()
            .oneshot(acquire_req("pat-alpha"))
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::OK, "first acquire fills the slot");
        let first_id = lease_id_of(r1).await;
        assert_eq!(active_count(&ledger, &tid("alpha")), 1);

        // Second acquire is over-cap → it ENQUEUES and parks awaiting dispatch.
        // Spawn it as a task; it will not complete until we tick.
        let router2 = router.clone();
        let waiter =
            tokio::spawn(async move { router2.oneshot(acquire_req("pat-alpha")).await.unwrap() });

        // Let the spawned acquire reach the queue, then assert it is parked.
        for _ in 0..50 {
            if state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha"))
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha")),
            1,
            "the over-cap acquire must be parked in the queue"
        );
        // A tick while the slot is still full dispatches NOBODY (over cap).
        assert_eq!(
            run_admission_tick(&state, now).await,
            0,
            "no dispatch while cap full"
        );
        assert!(!waiter.is_finished(), "waiter still parked while cap full");

        // Free the slot: cancel the first lease.
        let cancel = Request::builder()
            .method("POST")
            .uri(paths::LEASE_CANCEL.replace("{lease_id}", &first_id))
            .header(header::AUTHORIZATION, "Bearer pat-alpha")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(cancel).await.unwrap().status(),
            StatusCode::OK
        );

        // One tick now dispatches the waiter (slot freed).
        assert_eq!(
            run_admission_tick(&state, now).await,
            1,
            "freed slot dispatches the waiter"
        );

        let resp = waiter.await.unwrap();
        assert_eq!(
            status_of(resp).await,
            StatusCode::OK,
            "queued acquire returns 200 after dispatch"
        );
        // No over-admit: exactly one ACTIVE lease for alpha (the dispatched one;
        // the cancelled holder lingers as a terminal Released, never counted).
        assert_eq!(
            active_count(&ledger, &tid("alpha")),
            1,
            "cap=1 holds even under queue — never over-admitted"
        );
    }

    /// Two tenants each over-cap and queued; with both slots freed and a single
    /// tick, BOTH dispatch — fair rotation serves each tenant (neither starves).
    #[tokio::test]
    async fn two_tenants_contend_fair_rotation() {
        let now = 2_000_000u64;
        let (state, _ledger) = queue_state(1, now, Duration::from_secs(5));
        let router = crate::app::app(token_store(), state.clone());

        // Fill both tenants' single slots.
        let a1 = lease_id_of(
            router
                .clone()
                .oneshot(acquire_req("pat-alpha"))
                .await
                .unwrap(),
        )
        .await;
        let b1 = lease_id_of(
            router
                .clone()
                .oneshot(acquire_req("pat-beta"))
                .await
                .unwrap(),
        )
        .await;

        // Queue a second acquire for each tenant.
        let (ra, rb) = (router.clone(), router.clone());
        let wa = tokio::spawn(async move { ra.oneshot(acquire_req("pat-alpha")).await.unwrap() });
        let wb = tokio::spawn(async move { rb.oneshot(acquire_req("pat-beta")).await.unwrap() });
        for _ in 0..100 {
            let q = state.admission_queue.as_ref().unwrap();
            if q.pending(&tid("alpha")) == 1 && q.pending(&tid("beta")) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }

        // Free both slots.
        for (pat, id) in [("pat-alpha", &a1), ("pat-beta", &b1)] {
            let c = Request::builder()
                .method("POST")
                .uri(paths::LEASE_CANCEL.replace("{lease_id}", id))
                .header(header::AUTHORIZATION, format!("Bearer {pat}"))
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(c).await.unwrap().status(),
                StatusCode::OK
            );
        }

        // One tick dispatches BOTH (fair rotation across the two tenants).
        assert_eq!(
            run_admission_tick(&state, now).await,
            2,
            "both tenants dispatched fairly"
        );
        assert_eq!(
            wa.await.unwrap().status(),
            StatusCode::OK,
            "alpha's waiter served"
        );
        assert_eq!(
            wb.await.unwrap().status(),
            StatusCode::OK,
            "beta's waiter served"
        );
    }

    /// A queued waiter that is NEVER dispatched 503s fail-closed when its bounded
    /// wait elapses — never a silent hang, and no slot is reserved.
    #[tokio::test]
    async fn queued_wait_timeout_503() {
        let now = 3_000_000u64;
        // Tiny wait so the timeout fires fast; the slot is never freed.
        let (state, ledger) = queue_state(1, now, Duration::from_millis(40));
        let router = crate::app::app(token_store(), state.clone());

        // Fill the slot; it is held while the queued waiter times out.
        let a1 = router
            .clone()
            .oneshot(acquire_req("pat-alpha"))
            .await
            .unwrap();

        // Over-cap acquire: enqueues, waits 40ms, then 503s (no dispatch ever).
        let resp = router
            .clone()
            .oneshot(acquire_req("pat-alpha"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "a queued acquire that times out must 503 fail-closed"
        );
        // No slot leaked: still exactly one active lease (the holder).
        assert_eq!(
            active_count(&ledger, &tid("alpha")),
            1,
            "the timed-out waiter reserved no slot"
        );
        // The waiter evicted its context, but its orphaned FIFO entry remains
        // while the tenant is over cap (tick cap-skips it, never reaching
        // dispatch). It self-heals the moment a slot frees: dispatch sees no
        // waiter context → drops the FIFO entry WITHOUT reserving anything.
        // Acquire a fresh holder lease so we have one to cancel, freeing a slot
        // — but first the existing holder occupies the only slot, so the orphan
        // is still parked.
        assert_eq!(
            state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha")),
            1,
            "the orphaned FIFO entry persists while the tenant is over cap"
        );
        // Free the slot (cancel the holder) so a tick can drain the orphan.
        let holder_id = lease_id_of(a1).await;
        let c = Request::builder()
            .method("POST")
            .uri(paths::LEASE_CANCEL.replace("{lease_id}", &holder_id))
            .header(header::AUTHORIZATION, "Bearer pat-alpha")
            .body(Body::empty())
            .unwrap();
        router.clone().oneshot(c).await.unwrap();
        // Now the tick reaches the orphan: no waiter context → drop it, reserve
        // NOTHING (the timed-out request must never get a phantom lease).
        let dispatched = run_admission_tick(&state, now).await;
        assert_eq!(
            dispatched, 0,
            "an orphaned (timed-out) FIFO entry reserves nothing"
        );
        assert_eq!(
            state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha")),
            0,
            "the orphaned queue entry is drained once a slot frees, leaving the queue empty"
        );
        // No phantom admission: the only active lease set is now empty (holder
        // cancelled, orphan reserved nothing).
        assert_eq!(
            active_count(&ledger, &tid("alpha")),
            0,
            "the orphan admitted no lease"
        );
    }

    /// The per-tenant queue bound (`MAX_TENANT_QUEUE_DEPTH`) is enforced: an
    /// over-cap acquire that would push the tenant strictly over the bound is
    /// SHED with 503 (never unbounded growth), and reserves no slot.
    #[tokio::test]
    async fn queue_bound_sheds_over_the_cap() {
        let now = 5_000_000u64;
        let (state, ledger) = queue_state(1, now, Duration::from_secs(5));
        let router = crate::app::app(token_store(), state.clone());

        // Fill the slot (so the next acquire is over-cap → routes to the queue).
        let _a1 = router
            .clone()
            .oneshot(acquire_req("pat-alpha"))
            .await
            .unwrap();
        // Flood alpha's FIFO to the per-tenant bound directly.
        let admitted = state
            .admission_queue
            .as_ref()
            .unwrap()
            .fill_to_bound(&tid("alpha"), now);
        assert_eq!(admitted, corelink_fabric::MAX_TENANT_QUEUE_DEPTH);

        // The next over-cap acquire enqueue is over the bound → SHED 503.
        let resp = router.oneshot(acquire_req("pat-alpha")).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "an enqueue over the per-tenant bound must shed 503, never grow unbounded"
        );
        // The queue stayed at the bound; no slot reserved by the shed acquire.
        assert_eq!(
            state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha")),
            corelink_fabric::MAX_TENANT_QUEUE_DEPTH,
            "the queue stays at the bound (shed, not grown)"
        );
        assert_eq!(
            active_count(&ledger, &tid("alpha")),
            1,
            "shed reserves no slot"
        );
    }

    /// `wait_stats` is fed each tick under queue mode → `GET /v1/metrics/tenant`
    /// returns a NON-ZERO count for a tenant whose queued acquire was
    /// dispatched (the W2-D wiring; the immediate `reject` path stays count:0).
    #[tokio::test]
    async fn wait_stats_lit_metrics_nonzero_under_queue() {
        let now = 4_000_000u64;
        let (state, _ledger) = queue_state(1, now, Duration::from_secs(5));
        let router = crate::app::app(token_store(), state.clone());

        let a1 = lease_id_of(
            router
                .clone()
                .oneshot(acquire_req("pat-alpha"))
                .await
                .unwrap(),
        )
        .await;
        let ra = router.clone();
        let waiter =
            tokio::spawn(async move { ra.oneshot(acquire_req("pat-alpha")).await.unwrap() });
        for _ in 0..100 {
            if state
                .admission_queue
                .as_ref()
                .unwrap()
                .pending(&tid("alpha"))
                == 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        // Free the slot and dispatch (now_ms advanced so the wait is > 0).
        let c = Request::builder()
            .method("POST")
            .uri(paths::LEASE_CANCEL.replace("{lease_id}", &a1))
            .header(header::AUTHORIZATION, "Bearer pat-alpha")
            .body(Body::empty())
            .unwrap();
        router.clone().oneshot(c).await.unwrap();
        assert_eq!(run_admission_tick(&state, now + 1_500).await, 1);
        assert_eq!(waiter.await.unwrap().status(), StatusCode::OK);

        // The metrics endpoint now reports a non-zero count for alpha.
        let metrics = Request::builder()
            .method("GET")
            .uri(paths::METRICS_TENANT)
            .header(header::AUTHORIZATION, "Bearer pat-alpha")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(metrics).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            v["count"].as_u64().unwrap() >= 1,
            "wait_stats must be lit under queue mode (count >= 1), got {v}"
        );
    }
}
