//! Lease ledger — the authoritative state machine (CF0 freeze item 4; CP1).
//!
//! Contract §1 (frozen from hugit's side): states are exactly
//! `Pending → Held → (Released | Expired | Crashed)` — five states, no
//! invented intermediates hugit can't observe. The wire type
//! [`RunnerState`] (transcribed, frozen) carries the four observable wire
//! states; `Pending` is the contract's pre-wire admission state (a
//! `RunnerLease` is only ever emitted once `Held`), so the ledger models it
//! as [`LeaseState::Pending`] and REUSES `RunnerState` for the rest —
//! nothing is redefined, and no sixth state is representable.
//!
//! Fail-closed throughout: illegal transitions are errors, unknown leases are
//! errors, a corrupt journal refuses to open, and `put` never silently
//! overwrites.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use corelink_runners_contracts::RunnerState;
use serde::{Deserialize, Serialize};

use crate::tenant::TenantId;

/// Ledger-level lifecycle state: the contract §1 five states.
///
/// `Pending` is the admission state (never on the `RunnerLease` wire — the
/// frozen `RunnerState` deliberately has no such variant); the other four
/// REUSE the transcribed wire type unmodified. Serialized vocabulary is the
/// flat five tokens `"pending" | "held" | "released" | "expired" |
/// "crashed"` (the wire variant is `#[serde(untagged)]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseState {
    /// Contract §1 initial state: lease requested, no box/VM touched yet.
    Pending,
    /// One of the four frozen wire states (`RunnerState`, transcribed —
    /// reused, never redefined).
    #[serde(untagged)]
    Wire(RunnerState),
}

impl LeaseState {
    /// `Held` — the only state that occupies a billable slot.
    pub fn is_held(&self) -> bool {
        matches!(self, LeaseState::Wire(RunnerState::Held))
    }
}

impl From<RunnerState> for LeaseState {
    fn from(value: RunnerState) -> Self {
        LeaseState::Wire(value)
    }
}

/// One lease's authoritative control-plane record (CP1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeaseRecord {
    /// Unique lease identifier (matches `RunnerLease.lease_id` on the wire).
    pub lease_id: String,
    /// Owning tenant (org = tenant, ADR-0002) — the cap/fairness/billing key.
    pub tenant: TenantId,
    /// Current lifecycle state — contract §1 five states only.
    pub state: LeaseState,
    /// Opaque reference to the box/VM serving this lease.
    pub box_ref: String,
    /// Unix epoch ms at record creation.
    pub created_at_ms: u64,
    /// Unix epoch ms at last state change.
    pub updated_at_ms: u64,
    /// Absolute lease-expiry deadline, unix epoch ms (ADR-0004 Decision-1).
    ///
    /// THE durable source of truth for expiry: the reaper dates an overdue
    /// lease purely from this field, so any instance — including one that never
    /// served the acquire — can reap it (fixing the D3-P1 cap-slot leak where
    /// the deadline lived only in the acquiring instance's in-memory map).
    ///
    /// `None` = no deadline = **never-overdue** (the pre-ADR-0004 fail-safe: a
    /// lease we cannot date is never reaped by the deadline path). A state
    /// change NEVER alters this field — [`LeaseLedger::transition`] preserves it
    /// unchanged.
    pub deadline_ms: Option<u64>,
    /// Durable billing-acquire stamp: the epoch-ms of the FIRST `Pending → Held`
    /// transition (the moment a slot is occupied and billing starts). Written
    /// once at that transition (never overwritten, never cleared by a terminal
    /// transition), so the `Acquired → terminal` `slot_seconds` pairing survives
    /// a fabricd restart that drops the billing target's in-memory map (the
    /// revenue-loss fix, #3). Ledger-INTERNAL — NOT on the frozen wire
    /// `RunnerLease`; `None` for a never-Held lease or a pre-fix row.
    /// `#[serde(default)]` so older journaled records deserialize as `None`.
    #[serde(default)]
    pub billing_acquired_at_ms: Option<u64>,
}

/// Legal-transition matrix — contract §1, nothing else:
///
/// | from \ to  | Held | Released | Expired | Crashed |
/// |------------|------|----------|---------|---------|
/// | `Pending`  |  ✔   |    ✘     |    ✘    |    ✘    |
/// | `Held`     |  ✘   |    ✔     |    ✔    |    ✔    |
/// | `Released` |  ✘   |    ✘     |    ✘    |    ✘    |
/// | `Expired`  |  ✘   |    ✘     |    ✘    |    ✘    |
/// | `Crashed`  |  ✘   |    ✘     |    ✘    |    ✘    |
///
/// `Released`/`Expired`/`Crashed` are terminal. Anything outside the four ✔
/// cells is an error (fail-closed).
fn transition_is_legal(from: &LeaseState, to: &RunnerState) -> bool {
    matches!(
        (from, to),
        (LeaseState::Pending, RunnerState::Held)
            | (
                LeaseState::Wire(RunnerState::Held),
                RunnerState::Released | RunnerState::Expired | RunnerState::Crashed,
            )
    )
}

/// Apply a transition to a record, enforcing the contract §1 matrix.
fn apply_transition(
    rec: &mut LeaseRecord,
    to: RunnerState,
    now_ms: u64,
) -> anyhow::Result<LeaseRecord> {
    if !transition_is_legal(&rec.state, &to) {
        anyhow::bail!(
            "illegal lease transition for {}: {:?} -> {to:?} (contract §1 allows only \
             Pending->Held and Held->Released|Expired|Crashed)",
            rec.lease_id,
            rec.state,
        );
    }
    // A state change touches ONLY `state` + `updated_at_ms`; `deadline_ms`
    // (and every other field) is preserved unchanged (ADR-0004 Decision-1: a
    // transition never alters the durable deadline).
    rec.state = LeaseState::Wire(to);
    rec.updated_at_ms = now_ms;
    // Revenue-loss fix (#3): stamp the durable billing-acquire time at the FIRST
    // `Pending → Held` transition (billing starts the instant the slot is
    // occupied). FIRST-held only (`is_none()`), so a terminal transition — which
    // never targets `Held` — leaves it intact for the terminal billing read, and
    // no path can ever move it backwards.
    if matches!(rec.state, LeaseState::Wire(RunnerState::Held))
        && rec.billing_acquired_at_ms.is_none()
    {
        rec.billing_acquired_at_ms = Some(now_ms);
    }
    Ok(rec.clone())
}

/// The authoritative lease state machine (CP1 seam).
///
/// Contract §1: five states only (`Pending → Held → (Released | Expired |
/// Crashed)`), no invented intermediates; transitions outside the matrix on
/// [`transition_is_legal`] are errors. Fail-closed: unknown lease ids and
/// duplicate `put`s are errors, never silent.
///
/// Impls here: [`InMemoryLedger`] (tests/dev) and [`FileLedger`] (the
/// restart-survival oracle). The production Postgres impl is
/// [`crate::pg_ledger::PgLedger`] (WP-3-PGLEDGER) — cross-instance cap-safe,
/// verified by the same conformance suite against a real database.
/// The compute-ceiling gate for an atomic admit (`pricing.md §3`; wave plan §8/§11).
///
/// `None` passed to [`LeaseLedger::try_admit_with_compute`] ⇒ concurrency-only
/// (today's behavior, default-off). `Some` ⇒ the ledger ALSO enforces, in the
/// SAME atomic admit, the vCPU-h ceiling
/// `accrued(tenant, period) + Σ_reserved(pending+held, period) + new_reserved ≤ ceiling`,
/// and records this lease's reservation so the rolling Σ and the terminal accrual
/// stay consistent. The reservation is the **constant** worst case `vcpu × ttl`
/// ([`crate::compute_meter::vcpu_ms`]), already i64-guarded by the caller
/// ([`crate::compute_meter::fits_ledger`]) — never a `now`-dependent remaining.
///
/// The compute state lives **inside the ledger** (Pg columns / InMemory+File
/// side-store), NOT on [`LeaseRecord`] or [`crate::tenant::TenantPlan`]: the
/// wire-adjacent record and the plan caps are unchanged, so this adds no field
/// to any construction site and keeps `RunnerLease` frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputeGate {
    /// Calendar-month `YYYYMM` (UTC) the admit is attributed to — `period_key(created_at)`.
    pub period_key: u32,
    /// The tenant's monthly ceiling in vCPU·ms. `0` ⇒ disabled: the ledger SKIPS
    /// the compute check entirely (it must NEVER compare against `0`, which would
    /// reject-all). A `Some` gate with `ceiling_vcpu_ms == 0` is a no-op gate.
    pub ceiling_vcpu_ms: u64,
    /// The serving box's vCPU count — recorded at admit, the multiplier for the
    /// terminal accrual `vcpu × (terminal − created)`.
    pub box_vcpu_count: u32,
    /// This lease's worst-case reservation `vcpu × ttl` (vCPU·ms), i64-guarded.
    pub new_reserved_vcpu_ms: u64,
}

/// The outcome of an atomic admit attempt ([`LeaseLedger::try_admit_with_compute`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmitOutcome {
    /// Admitted — the `Pending` row is inserted (and, under a `Some` gate, its
    /// reservation recorded) atomically.
    Admitted,
    /// Rejected: the tenant is at its concurrency cap (the existing over-cap 429).
    OverConcurrency,
    /// Rejected: the tenant is at its monthly vCPU-h compute ceiling — a DISTINCT
    /// 429 ("monthly compute ceiling reached; upgrade tier"), never conflated with
    /// the concurrency rejection.
    OverCompute,
}

/// One lease's compute reservation — the ledger-INTERNAL side-store row (NEVER
/// on [`LeaseRecord`], mirroring the `checkpoints` side-map): it carries the
/// constant worst-case `vcpu × ttl` reservation that counts toward the rolling
/// Σ while the lease is Pending/Held, the box vCPU multiplier for the terminal
/// accrual, the period it is attributed to, and the once-only accrual latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct LeaseReservation {
    /// The serving box's vCPU count — the multiplier for the terminal accrual
    /// `vcpu × (terminal − created)`.
    box_vcpu_count: u32,
    /// Calendar-month `YYYYMM` (UTC) this lease's compute is attributed to.
    accrual_period_key: u32,
    /// The constant worst-case `vcpu × ttl` reservation (vCPU·ms) — what counts
    /// toward the rolling Σ while the lease is Pending or Held.
    reserved_vcpu_ms: u64,
    /// `Some(now_ms)` once the terminal accrual has been folded into the accrual
    /// map — the once-only latch (never double-accrue). `None` until then.
    accrued_at_ms: Option<u64>,
}

pub trait LeaseLedger {
    /// Whether this backend enforces the concurrency/vCPU cap SAFELY across
    /// MULTIPLE fabricd instances (shards). Only a shared, atomically-serialized
    /// store qualifies: the [`crate::pg_ledger::PgLedger`] (an advisory-locked
    /// atomic count-and-insert) returns `true`; the per-process in-memory and
    /// single-file backends return the default `false`. The acquire path uses this
    /// to REFUSE admission when `num_shards > 1` on a non-cross-instance ledger,
    /// since otherwise each shard would count only its own leases and a tenant
    /// would get up to N× its paid concurrency (a cap and fairness bypass on
    /// untrusted compute).
    fn is_cross_instance_safe(&self) -> bool {
        false
    }

    /// Durably record a tenant's AUP1 suspension so it survives a shard restart
    /// AND is visible to OTHER instances. Default: no-op — a single-instance /
    /// ephemeral backend keeps suspension only in the fabricd's in-memory cache,
    /// which is correct at N=1. The [`crate::pg_ledger::PgLedger`] overrides it
    /// with a shared table, which is what makes suspend an effective abuse-control
    /// at N>1 (a suspended tenant is blocked on EVERY shard, not just the one that
    /// received the suspend). `&self`: pg writes via its pool; no `&mut` needed.
    fn set_tenant_suspended(&self, _tenant: &str, _suspended: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// Whether `tenant` is DURABLY suspended (the cross-instance source of truth).
    /// Default `false` (no durable store — the in-memory cache is authoritative at
    /// N=1). Pg overrides. Read by the acquire gate ONLY at N>1 (a cache miss),
    /// so it never adds latency to the single-instance hot path.
    fn is_tenant_suspended_durable(&self, _tenant: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    /// Register a new record. Fails if `lease_id` already exists — state is
    /// mutated only through [`LeaseLedger::transition`], never by overwrite.
    ///
    /// `&self` (W-LEDGER-A2): the whole trait is INTERIOR-MUTABLE — every backend
    /// provides its OWN atomicity (pg: the advisory-locked txn; InMemory / File: an
    /// internal `Mutex`), so `AppState.ledger` no longer needs an OUTER process
    /// `Mutex`. That is what keeps the cold close/reaper path off a worker-blocking
    /// `.lock()` held across the (for pg, `block_in_place`) txn.
    fn put(&self, rec: LeaseRecord) -> anyhow::Result<()>;

    /// Look up a record by lease id (`Ok(None)` when absent).
    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>>;

    /// Transition a lease to `to` at `now_ms`, enforcing the contract §1
    /// matrix (see [`transition_is_legal`]); returns the updated record.
    /// Unknown lease or illegal pair → `Err` (fail-closed).
    fn transition(
        &self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord>;

    /// All records for a tenant, ordered by `lease_id` (deterministic).
    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>>;

    /// All currently `Held` records, ordered by `lease_id` (deterministic).
    ///
    /// The enumeration seam for the lifecycle sweeps (CP1b:
    /// [`crate::lifecycle::LeaseLifecycle`]) and the slot-occupancy view
    /// (BIL1: slot occupancy = held leases).
    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>>;

    /// All `Pending` records whose `created_at_ms` is STRICTLY BEFORE
    /// `now_ms.saturating_sub(max_age_ms)` — i.e. leases that have sat in the
    /// pre-provision `Pending` reservation for LONGER than `max_age_ms`,
    /// ordered by `lease_id` (deterministic).
    ///
    /// The comparison is strict (`created_at_ms < cutoff`): a `Pending` whose
    /// age is EXACTLY `max_age_ms` (sitting right at the bound) has not yet sat
    /// LONGER than the bound, so it is NOT returned — only a genuinely
    /// past-bound reservation is reclaimable. This matches the FAIL-SAFE note
    /// below and is consistent across InMemory / File / Pg.
    ///
    /// The enumeration seam for the **stale-Pending sweep**
    /// ([`crate::reaper`]). A `Pending` lease reserves a concurrency slot
    /// BEFORE provisioning; if the instance dies between the `try_admit`
    /// reservation and either the `Held` transition or the rollback `remove`,
    /// the row sits forever counting against the tenant cap — the deadline
    /// reaper only sweeps `Held`, never `Pending`. This method finds the
    /// genuinely-stale ones so the sweep can reclaim the leaked slot.
    ///
    /// FAIL-SAFE: only records strictly older than the bound are returned; a
    /// fresh `Pending` that is legitimately mid-provision is NEVER included
    /// (the bound must be set well past any legitimate provision window).
    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>>;

    /// Atomically admit a `Pending` lease IFF the tenant's active (Pending+Held)
    /// count is strictly under `max_concurrency`. Returns Ok(true) on admit (the
    /// record is inserted as Pending), Ok(false) on over-cap (nothing inserted).
    /// The count and the insert are ONE atomic operation w.r.t. this ledger — the
    /// caller MUST use this instead of a separate count-then-put when admitting,
    /// so the concurrency cap cannot be exceeded by a race.
    ///
    /// Concurrency cap ONLY — the per-instance rate ceiling stays in the caller's
    /// in-memory RateWindow (it is admission bookkeeping, not ledger state).
    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool>;

    /// Atomic admit with an OPTIONAL compute-ceiling gate — the loss-impossible
    /// enforcement seam (`pricing.md §3`; wave plan §8/§11).
    ///
    /// `gate = None` is EXACTLY [`LeaseLedger::try_admit`] (concurrency-only,
    /// default-off, byte-identical). `Some(gate)` additionally enforces the vCPU-h
    /// ceiling IN THE SAME atomic admit — the only place check-and-reserve is
    /// atomic across instances — and records this lease's reservation so the
    /// rolling Σ and the terminal accrual stay consistent.
    ///
    /// The default impl is **fail-closed**: it serves the `None` (concurrency-only)
    /// path verbatim, but RETURNS `Err` for any `Some` gate, so a ledger that has
    /// not implemented compute accounting can NEVER silently admit over a ceiling
    /// (Err ⇒ 503, never a leak). Production ledgers (InMemory/File/Pg) override
    /// it; the acceptance suite forces every override (ceiling-reached → `OverCompute`).
    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        if gate.is_some() {
            anyhow::bail!(
                "compute-ceiling accounting requested but this ledger does not \
                 implement try_admit_with_compute (fail-closed: refusing to admit \
                 without enforcing the ceiling)"
            );
        }
        if self.try_admit(rec, max_concurrency)? {
            Ok(AdmitOutcome::Admitted)
        } else {
            Ok(AdmitOutcome::OverConcurrency)
        }
    }

    /// The durable accrued vCPU·ms for `(tenant, period_key)` — `0` when there is
    /// no accrual (no accounting, or a fresh period). The terminal half of the
    /// ceiling invariant `compute_accrued + Σ_reserved ≤ ceiling`.
    ///
    /// Default `Ok(0)`: a ledger without compute accounting has accrued nothing.
    /// Production ledgers override (InMemory/File side-store; Pg `compute_accrual`).
    fn compute_accrued(&self, _tenant: &TenantId, _period_key: u32) -> anyhow::Result<u64> {
        Ok(0)
    }

    /// Overwrite the lease's durable **envelope checkpoint** — an OPAQUE JSON
    /// blob (ADR-0004 Decision-2; the §13 Item-3 durable-hook SLA).
    ///
    /// The ledger treats the blob as opaque (it NEVER parses it): the
    /// abnormal-reap flush serializes a redacted `IntentMetrics` summary into it
    /// at turn boundaries, and reads it back on a reap where the local hook is
    /// absent. Persisting it on the lease row lets ANY instance — not just the
    /// one that served the acquire — emit the forensic envelope, so it is never
    /// silently dropped.
    ///
    /// Fail-closed: `Err` if the lease does not exist (like every other
    /// mutation — never write a checkpoint for a lease we don't hold). An
    /// idempotent overwrite otherwise (the freshest summary wins).
    fn set_envelope_checkpoint(&self, lease_id: &str, checkpoint_json: &str) -> anyhow::Result<()>;

    /// The lease's durable envelope checkpoint blob, or `None` if the lease has
    /// none (absent lease, or never-written checkpoint). The blob is returned
    /// verbatim — the ledger never parses it (ADR-0004 Decision-2).
    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>>;

    /// Remove a record from the ledger, freeing the cap/occupancy it held.
    /// Returns `Ok(true)` if a record was removed, `Ok(false)` if the lease
    /// was already absent.
    ///
    /// This is the ADMISSION-ROLLBACK seam, NOT a lifecycle transition: it
    /// exists solely so an acquire that RESERVED a `Pending` slot (via
    /// [`LeaseLedger::try_admit`]) but then FAILED to provision can release
    /// that reservation, leaving no trace and freeing the concurrency cap.
    /// The §1 legal matrix forbids `Pending -> terminal`, so a failed
    /// admission cannot be "transitioned away"; removal is the only honest
    /// rollback. It is NOT a way to delete a `Held` lease out from under a
    /// running box — callers must restrict its use to rolling back a
    /// just-reserved `Pending` admission they own.
    fn remove(&self, lease_id: &str) -> anyhow::Result<bool>;

    /// GUARDED admission-rollback: remove the lease ONLY if it is still
    /// `Pending` at delete time, evaluated atomically under the ledger lock.
    /// Returns `Ok(true)` if a `Pending` row was removed, `Ok(false)` if the
    /// lease was absent OR had already left `Pending` (e.g. raced to `Held`).
    ///
    /// This is the state-AWARE counterpart of [`LeaseLedger::remove`] for the
    /// stale-Pending sweep ([`crate::reaper::sweep_stale_pending`]). The sweep
    /// snapshots `Pending` rows, `await`s a teardown, then reclaims — and in
    /// that window a concurrent acquire can complete the provision and
    /// transition the very same lease `Pending → Held`. A state-BLIND `remove`
    /// would then DELETE a live `Held` lease out from under a running box
    /// (over-admit + a leaked box with no ledger record). Conditioning the
    /// delete on `state = Pending` under the lock makes the reclaim a no-op for
    /// any lease that won the race to `Held` — mirroring the reaper's
    /// won-the-race CAS posture (`transition` returns `Err` when the source
    /// state moved).
    ///
    /// Default impl is a check-then-`remove` that is correct ONLY because every
    /// production impl evaluates it while holding the same exclusive lock the
    /// sweep holds; the Pg impl overrides it with a single conditional
    /// `DELETE ... WHERE state = 'pending'` so the guard is atomic in the DB.
    ///
    /// ## Accounting-ON Pending is bare-deleted DELIBERATELY (audit #4, FALSE)
    ///
    /// It is CORRECT — not a leak — that this bare-deletes an accounting-ON
    /// `Pending` (one carrying `reserved_vcpu_ms`/`accrual_period_key`) without
    /// routing through `transition`: a never-Held Pending consumed **zero**
    /// vCPU·ms, and the admit Σ live-sums `reserved_vcpu_ms` over the pending/held
    /// **rows** (see `admit_decision` / the Pg Σ query), so deleting the row frees
    /// its reservation from Σ immediately, with nothing to fold into
    /// `compute_accrual`. `remove`'s refusal above applies ONLY to accounting-ON
    /// **Held** rows (deleting a live box's row would drop a real accrual); its
    /// `state = 'pending'` branch permits exactly this Pending delete. Adding a
    /// `box_vcpu_count IS NULL` guard here would leave accounting-on stale
    /// Pendings **un-swept** — a regression, not a fix. Pinned by
    /// `remove_if_pending_frees_reserved_sigma_headroom_for_accounting_on_pending`.
    ///
    /// `&self` (W-LEDGER-A2): the default's `get`+`remove` is atomic ONLY because
    /// every production impl OVERRIDES it to evaluate both under ONE inner lock
    /// (InMemory / File) or as a single conditional `DELETE` (pg). The default is a
    /// non-atomic fallback for backends that never race.
    fn remove_if_pending(&self, lease_id: &str) -> anyhow::Result<bool> {
        match self.get(lease_id)? {
            Some(rec) if matches!(rec.state, LeaseState::Pending) => self.remove(lease_id),
            // Absent, or no longer Pending (raced to Held / terminal) → no-op.
            _ => Ok(false),
        }
    }
}

/// The ADMIT seam (W-LEDGER-A1) — the acquire hot-path's atomic check-and-reserve,
/// SPLIT OUT of the process-`Mutex`-guarded [`LeaseLedger`] so admission does not
/// serialize behind the cold close/reaper path.
///
/// Both methods take `&self`, NOT `&mut self`: the backing store provides its OWN
/// atomicity for the count-and-reserve — the [`crate::pg_ledger::PgLedger`] via the
/// per-tenant `pg_advisory_xact_lock` (proven cap-exact across INDEPENDENT
/// instances with NO process `Mutex` by
/// `ledger_conformance::cross_instance_concurrent_admit_respects_cap_exactly`), the
/// [`InMemoryLedger`] via an INTERNAL `Arc<Mutex<InMemoryInner>>` it shares with its
/// cold `LeaseLedger` handle. So the OUTER process `Mutex` on `AppState.ledger` adds
/// nothing to cap-safety on the admit path and is dropped for it — while the
/// in-memory count-and-reserve stays atomic (the lock is RELOCATED outer→inner, not
/// removed).
///
/// Semantically identical to [`LeaseLedger::try_admit`] /
/// [`LeaseLedger::try_admit_with_compute`]; only the receiver differs (`&self`) so
/// the handler can call it WITHOUT holding a process `Mutex` across the (for pg,
/// `block_in_place`) blocking admit — the fix that stops a same-tenant acquire burst
/// from parking every tokio worker thread on the std `.lock()`.
pub trait AdmitLedger: Send + Sync {
    /// See [`LeaseLedger::try_admit`] — concurrency-only atomic admit, `&self`.
    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool>;

    /// See [`LeaseLedger::try_admit_with_compute`] — atomic admit with an OPTIONAL
    /// compute-ceiling gate, `&self`. `gate = None` is byte-identical to
    /// [`AdmitLedger::try_admit`] (concurrency-only, default-off).
    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome>;
}

/// In-memory ledger state — dev/test backing store; disqualified for production by
/// `ledger_survives_process_restart` (CP1).
///
/// This is the INNER, un-synchronized state. The public [`InMemoryLedger`] handle
/// wraps it in an `Arc<Mutex<..>>`; [`FileLedger`] embeds it directly as its
/// replay index (single-threaded behind the outer ledger lock).
#[derive(Debug, Default)]
pub(crate) struct InMemoryInner {
    records: HashMap<String, LeaseRecord>,
    /// ADR-0004 Decision-2: the durable envelope-checkpoint blob per lease
    /// (opaque JSON, never parsed by the ledger). A side map keeps the frozen
    /// [`LeaseRecord`] shape — and the journal/wire it round-trips — untouched.
    checkpoints: HashMap<String, String>,
    /// vCPU-h ceiling wave: the per-lease compute reservation, keyed by lease_id
    /// (a side-store mirroring `checkpoints`, NEVER on the frozen [`LeaseRecord`]).
    /// Present only for leases admitted under a `Some` compute gate; absent for
    /// concurrency-only admits (default-off byte-identical to `try_admit`).
    reservations: HashMap<String, LeaseReservation>,
    /// vCPU-h ceiling wave: the durable accrued vCPU·ms per `(tenant, period)`.
    /// The terminal half of the ceiling invariant; folded once per lease at its
    /// terminal transition and read by [`LeaseLedger::compute_accrued`].
    accruals: HashMap<(TenantId, u32), u64>,
}

/// A terminal accrual fold that actually happened — the durable side-effect a
/// [`FileLedger`] must journal so it survives a restart. Returned by
/// [`InMemoryInner::accrue_on_terminal`] when (and only when) a reservation
/// crossed from un-accrued to accrued, so the caller can persist BOTH the
/// updated accrual total and the reservation's `accrued_at_ms` latch.
#[derive(Debug, Clone)]
struct AccrualEvent {
    lease_id: String,
    tenant: TenantId,
    period_key: u32,
    /// The new accrual TOTAL for `(tenant, period_key)` after the fold (already
    /// saturated at `MAX_LEDGER_VCPU_MS`) — journaled absolutely (idempotent on
    /// replay), not as a delta. The reservation's `accrued_at_ms` latch is read
    /// back from the (already-mutated) side-store when the caller journals it.
    new_accrual_total: u64,
}

/// Map the boolean `try_admit` outcome onto an [`AdmitOutcome`] — the
/// default-off path (None / ceiling-0 gate), byte-identical to `try_admit`.
fn concurrency_only_outcome(admitted: bool) -> AdmitOutcome {
    if admitted {
        AdmitOutcome::Admitted
    } else {
        AdmitOutcome::OverConcurrency
    }
}

impl InMemoryInner {
    /// Empty in-memory ledger state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Concurrency-only admit (default-off): byte-identical to `try_admit`,
    /// recording NO compute state.
    fn concurrency_only_admit(
        &mut self,
        rec: LeaseRecord,
        max_concurrency: u32,
    ) -> anyhow::Result<AdmitOutcome> {
        Ok(concurrency_only_outcome(
            self.try_admit(rec, max_concurrency)?,
        ))
    }

    /// The vCPU-h ceiling admit check + reserve, shared by both ledgers' admit
    /// override. Assumes the lease is NOT yet inserted. Returns the [`AdmitOutcome`]
    /// and, on `Admitted`, the [`LeaseReservation`] the caller must record (and,
    /// for `FileLedger`, journal). Inserts NOTHING here — the caller owns the
    /// durable insert + reservation record so File can journal them together.
    ///
    /// `None`/ceiling-0 is NOT routed here (the override short-circuits to the
    /// concurrency-only path); this is the `Some` gate with `ceiling > 0` body.
    fn admit_decision(
        &self,
        rec: &LeaseRecord,
        max_concurrency: u32,
        gate: ComputeGate,
    ) -> (AdmitOutcome, Option<LeaseReservation>) {
        // (a) Σ = reserved_vcpu_ms over THIS tenant's Pending|Held leases whose
        //     reservation is in the same period — read from the side-store.
        let sigma: u64 = self
            .records
            .values()
            .filter(|r| {
                r.tenant == rec.tenant
                    && (matches!(r.state, LeaseState::Pending) || r.state.is_held())
            })
            .filter_map(|r| self.reservations.get(&r.lease_id))
            .filter(|res| res.accrual_period_key == gate.period_key)
            .map(|res| res.reserved_vcpu_ms)
            .fold(0u64, |acc, v| acc.saturating_add(v));
        // (b) the durable accrual for (tenant, period).
        let accrued = self
            .accruals
            .get(&(rec.tenant.clone(), gate.period_key))
            .copied()
            .unwrap_or(0);
        // FIX-B B2 — PRECEDENCE: the COMPUTE ceiling is checked BEFORE the
        // concurrency cap, so a lease that is over BOTH is reported `OverCompute`,
        // never `OverConcurrency`. This ordering is load-bearing: `OverConcurrency`
        // routes the caller to a QUEUE (retry when a slot frees), which would
        // BYPASS the monthly compute wall for an over-ceiling tenant; `OverCompute`
        // is the hard 429 ("upgrade tier") that must win. Both ledgers share this
        // single `admit_decision`, so the precedence is identical by construction.
        //
        // (c) ceiling check — fail-closed: strictly-over the ceiling rejects.
        let projected = accrued
            .saturating_add(sigma)
            .saturating_add(gate.new_reserved_vcpu_ms);
        if projected > gate.ceiling_vcpu_ms {
            return (AdmitOutcome::OverCompute, None);
        }
        // (d) concurrency cap (same definition as `try_admit`) — checked SECOND,
        //     only once the compute ceiling has room (see the precedence note).
        let active = self
            .records
            .values()
            .filter(|r| {
                r.tenant == rec.tenant
                    && (matches!(r.state, LeaseState::Pending) || r.state.is_held())
            })
            .count();
        if (active as u32) >= max_concurrency {
            return (AdmitOutcome::OverConcurrency, None);
        }
        (
            AdmitOutcome::Admitted,
            Some(LeaseReservation {
                box_vcpu_count: gate.box_vcpu_count,
                accrual_period_key: gate.period_key,
                reserved_vcpu_ms: gate.new_reserved_vcpu_ms,
                accrued_at_ms: None,
            }),
        )
    }

    /// `transition` that ALSO returns the terminal [`AccrualEvent`] (if any) so a
    /// durable ledger ([`FileLedger`]) can journal the accrual + latch. The trait
    /// `transition` discards the event; the in-memory fold is identical.
    fn transition_capturing(
        &mut self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<(LeaseRecord, Option<AccrualEvent>)> {
        let rec = self
            .records
            .get_mut(lease_id)
            .ok_or_else(|| anyhow::anyhow!("unknown lease {lease_id}: cannot transition"))?;
        // Snapshot the accrual inputs BEFORE applying (the matrix guard runs
        // inside apply_transition; only a LEGAL transition reaches the fold).
        let tenant = rec.tenant.clone();
        let created_at_ms = rec.created_at_ms;
        let updated = apply_transition(rec, to.clone(), now_ms)?;
        // The §1 matrix only admits `Held -> terminal` here; fold the terminal
        // accrual once (no-op for non-terminal or already-accrued reservations).
        let event = self.accrue_on_terminal(lease_id, &tenant, created_at_ms, to, now_ms);
        Ok((updated, event))
    }

    /// Fold the terminal accrual for `lease_id` IFF `to` is terminal AND a
    /// reservation exists that has not yet accrued (`accrued_at_ms == None`).
    /// Idempotent: a second terminal call (or a racing terminalizer) is a no-op.
    /// Mutates the in-memory accrual map + reservation latch and returns the
    /// [`AccrualEvent`] the caller must journal (File) — `None` if nothing folded.
    fn accrue_on_terminal(
        &mut self,
        lease_id: &str,
        tenant: &TenantId,
        created_at_ms: u64,
        to: RunnerState,
        now_ms: u64,
    ) -> Option<AccrualEvent> {
        let is_terminal = matches!(
            to,
            RunnerState::Released | RunnerState::Expired | RunnerState::Crashed
        );
        if !is_terminal {
            return None;
        }
        let res = self.reservations.get_mut(lease_id)?;
        if res.accrued_at_ms.is_some() {
            // Already accrued — once-only latch (never double-accrue).
            return None;
        }
        // C1 (mirror of PgLedger) — CLAMP THE TERMINAL ACCRUAL TO THE RESERVATION
        // (§8 monotonicity). The loss-impossible proof rests on `actual ≤ reserved`
        // ALWAYS, so that at each terminal `accrued + Σ` is monotone non-increasing
        // (the row leaves Σ shedding `reserved`, and adds back `accrual ≤ reserved`
        // to `accrued`). An overdue-but-unreaped Held lease (now − created > ttl,
        // i.e. it ran past its ttl before the reaper killed it) yields a raw
        // `vcpu × (now − created) > reserved` — which, UNCLAMPED, would make
        // `accrued + Σ` GROW past the sum of reservations and let the gate admit on
        // false headroom → overspend, AND would diverge this in-memory/File accrual
        // from PgLedger's (which DOES clamp at pg_ledger.rs C1) for identical inputs.
        // Clamping to `reserved` (the provider hard-kills at the deadline, so
        // anything past the reserved ttl is provider-bounded noise) restores
        // `actual ≤ reserved` unconditionally on InMemory + File. `reserved_vcpu_ms`
        // is i64-guarded at admit, so the clamped result fits. The clamp can only
        // ever CAP the charge, never raise it — loss-safe.
        let actual =
            crate::compute_meter::vcpu_ms(res.box_vcpu_count, now_ms.saturating_sub(created_at_ms))
                .min(res.reserved_vcpu_ms);
        let period = res.accrual_period_key;
        res.accrued_at_ms = Some(now_ms);
        let entry = self.accruals.entry((tenant.clone(), period)).or_insert(0);
        // Saturate at the i64 ledger bound — the accrual must NEVER wrap.
        *entry = entry
            .saturating_add(actual)
            .min(crate::compute_meter::MAX_LEDGER_VCPU_MS);
        Some(AccrualEvent {
            lease_id: lease_id.to_string(),
            tenant: tenant.clone(),
            period_key: period,
            new_accrual_total: *entry,
        })
    }
}

/// INHERENT `&mut self` core operations (W-LEDGER-A2): `InMemoryInner` is the raw,
/// un-synchronized state — it does NOT implement the (now `&self`, interior-mutable)
/// [`LeaseLedger`] trait. The [`InMemoryLedger`] handle and the [`FileLedger`] both
/// hold `InMemoryInner` behind their OWN `Mutex` and drive these `&mut` methods
/// under that lock; the lock provides the atomicity the trait callers rely on.
impl InMemoryInner {
    /// Trait-default parity: the in-memory backend has no durable suspension store
    /// (suspension lives in the fabricd cache at N=1) — a no-op, like the default.
    fn set_tenant_suspended(&self, _tenant: &str, _suspended: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// Trait-default parity: no durable store ⇒ never durably suspended.
    fn is_tenant_suspended_durable(&self, _tenant: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn put(&mut self, rec: LeaseRecord) -> anyhow::Result<()> {
        if self.records.contains_key(&rec.lease_id) {
            anyhow::bail!(
                "lease {} already exists: put never overwrites",
                rec.lease_id
            );
        }
        self.records.insert(rec.lease_id.clone(), rec);
        Ok(())
    }

    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        Ok(self.records.get(lease_id).cloned())
    }

    fn transition(
        &mut self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        let (updated, _accrual) = self.transition_capturing(lease_id, to, now_ms)?;
        Ok(updated)
    }

    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        let mut out: Vec<LeaseRecord> = self
            .records
            .values()
            .filter(|r| &r.tenant == t)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
        Ok(out)
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        let mut out: Vec<LeaseRecord> = self
            .records
            .values()
            .filter(|r| r.state.is_held())
            .cloned()
            .collect();
        out.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
        Ok(out)
    }

    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        let cutoff = now_ms.saturating_sub(max_age_ms);
        let mut out: Vec<LeaseRecord> = self
            .records
            .values()
            .filter(|r| matches!(r.state, LeaseState::Pending) && r.created_at_ms < cutoff)
            .cloned()
            .collect();
        out.sort_by(|a, b| a.lease_id.cmp(&b.lease_id));
        Ok(out)
    }

    fn try_admit(&mut self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        // Active = Pending OR Held — mirrors CapGate/by_tenant's definition
        // EXACTLY. Counted under the caller's Mutex, so count+put is atomic.
        let active = self
            .records
            .values()
            .filter(|r| {
                r.tenant == rec.tenant
                    && (matches!(r.state, LeaseState::Pending) || r.state.is_held())
            })
            .count();
        if (active as u32) < max_concurrency {
            // `put` keeps the caller-supplied (Pending) state and still errors
            // on a duplicate lease_id (fail-closed).
            self.put(rec)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn try_admit_with_compute(
        &mut self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        // Default-OFF: None gate, or a no-op (ceiling 0) gate ⇒ behave EXACTLY
        // like `try_admit` (byte-identical), recording NO compute state.
        let gate = match gate {
            None => return self.concurrency_only_admit(rec, max_concurrency),
            Some(g) if g.ceiling_vcpu_ms == 0 => {
                return self.concurrency_only_admit(rec, max_concurrency);
            }
            Some(g) => g,
        };
        // Atomic (we hold &mut self): decide first, then apply the insert +
        // reservation record together so nothing is recorded on a rejection.
        let (outcome, reservation) = self.admit_decision(&rec, max_concurrency, gate);
        if let (AdmitOutcome::Admitted, Some(res)) = (outcome, reservation) {
            let lease_id = rec.lease_id.clone();
            // `put` is fail-closed on a duplicate lease_id; only record the
            // reservation once the row is in (no orphan reservation on error).
            self.put(rec)?;
            self.reservations.insert(lease_id, res);
        }
        Ok(outcome)
    }

    fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> anyhow::Result<u64> {
        Ok(self
            .accruals
            .get(&(tenant.clone(), period_key))
            .copied()
            .unwrap_or(0))
    }

    fn set_envelope_checkpoint(
        &mut self,
        lease_id: &str,
        checkpoint_json: &str,
    ) -> anyhow::Result<()> {
        // Fail-closed: the lease must exist (never checkpoint a lease we don't
        // hold). Idempotent overwrite — the freshest summary wins.
        if !self.records.contains_key(lease_id) {
            anyhow::bail!(
                "lease {lease_id} does not exist: cannot set envelope checkpoint (fail-closed)"
            );
        }
        self.checkpoints
            .insert(lease_id.to_string(), checkpoint_json.to_string());
        Ok(())
    }

    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        Ok(self.checkpoints.get(lease_id).cloned())
    }

    fn remove(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        // FIX-B B3 — accounting-on `remove` of a HELD lease is a contract
        // violation: `remove` is the admission-rollback seam for a just-reserved
        // PENDING lease only (see the trait doc). Dropping a Held accounting-on
        // lease here would silently delete its reservation from the rolling Σ
        // WITHOUT folding a terminal accrual ⇒ the tenant's compute is un-billed
        // (undercount). A Held lease must leave via `transition` to a terminal
        // state (which folds the accrual once); fail-closed rather than un-bill.
        // Default-off (no reservation) is unaffected — byte-identical to before.
        if let Some(rec) = self.records.get(lease_id)
            && rec.state.is_held()
            && self.reservations.contains_key(lease_id)
        {
            anyhow::bail!(
                "lease {lease_id} is Held with a compute reservation: `remove` is the \
                 admission-rollback seam for Pending only — a Held accounting-on lease \
                 must terminalize via `transition` so its accrual is folded (fail-closed: \
                 refusing to drop the reservation un-billed)"
            );
        }
        // The checkpoint (if any) goes with the record — a rolled-back lease
        // leaves no checkpoint residue.
        self.checkpoints.remove(lease_id);
        // A rolled-back Pending releases its reservation and accrues NOTHING
        // (the accrual map is untouched — only `transition` to terminal folds).
        self.reservations.remove(lease_id);
        Ok(self.records.remove(lease_id).is_some())
    }

    /// GUARDED admission-rollback (trait-default parity): remove IFF still Pending.
    /// The caller holds the inner lock across this whole get+remove, so it is
    /// atomic — exactly the pre-A2 behaviour under the outer lock.
    fn remove_if_pending(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        match self.get(lease_id)? {
            Some(rec) if matches!(rec.state, LeaseState::Pending) => self.remove(lease_id),
            _ => Ok(false),
        }
    }
}

/// In-memory ledger — dev/test impl (CP1). A cheap-to-clone HANDLE over a shared
/// `Arc<Mutex<InMemoryInner>>`: every clone points at the SAME state.
///
/// W-LEDGER-A1: the state is behind an INTERNAL `Mutex` so the acquire hot path
/// can drive the atomic count-and-reserve through the [`AdmitLedger`] seam (`&self`,
/// NO process `Mutex`) while the cold [`LeaseLedger`] handle (`put`/`transition`/
/// reaper) drives the SAME inner state through the outer `AppState.ledger` `Mutex`.
/// The composition root clones one handle for each side, so admit + cold mutations
/// still serialize on the inner lock — cap atomicity and compute-reservation
/// atomicity are byte-identical to the pre-split single-`Mutex` design, just
/// RELOCATED outer→inner. Nesting order is always outer⊃inner (cold path takes the
/// outer `AppState.ledger` lock, then this inner lock; admit takes only this inner
/// lock) — no path takes inner-then-outer, so there is no lock-order inversion.
#[derive(Debug, Clone, Default)]
pub struct InMemoryLedger {
    inner: Arc<Mutex<InMemoryInner>>,
}

impl InMemoryLedger {
    /// A fresh, empty in-memory ledger handle (its own inner state).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InMemoryInner::new())),
        }
    }

    /// Whether `other` is the SAME ledger — i.e. shares this handle's inner
    /// `Mutex` (a clone, not an independent `new()`). The lock-relocation proof
    /// asserts the admit handle and the cold `LeaseLedger` handle are shared.
    #[doc(hidden)]
    pub fn shares_state_with(&self, other: &InMemoryLedger) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Lock the inner state, mapping a poisoned mutex to a fail-closed `Err` (never
    /// a silent panic on the admit hot path). Every trait method takes this lock
    /// EXACTLY ONCE, so multi-step operations (e.g. `remove_if_pending`'s get+remove)
    /// stay atomic under one guard, identical to the pre-split behaviour.
    fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, InMemoryInner>> {
        self.inner
            .lock()
            .map_err(|_| anyhow::anyhow!("in-memory ledger inner mutex poisoned (fail-closed)"))
    }
}

impl LeaseLedger for InMemoryLedger {
    fn is_cross_instance_safe(&self) -> bool {
        // Per-process state — never cross-instance safe (matches InMemoryInner).
        false
    }

    fn set_tenant_suspended(&self, tenant: &str, suspended: bool) -> anyhow::Result<()> {
        self.lock()?.set_tenant_suspended(tenant, suspended)
    }

    fn is_tenant_suspended_durable(&self, tenant: &str) -> anyhow::Result<bool> {
        self.lock()?.is_tenant_suspended_durable(tenant)
    }

    fn put(&self, rec: LeaseRecord) -> anyhow::Result<()> {
        self.lock()?.put(rec)
    }

    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        self.lock()?.get(lease_id)
    }

    fn transition(
        &self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        self.lock()?.transition(lease_id, to, now_ms)
    }

    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.by_tenant(t)
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.held()
    }

    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.pending_older_than(now_ms, max_age_ms)
    }

    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        self.lock()?.try_admit(rec, max_concurrency)
    }

    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        self.lock()?
            .try_admit_with_compute(rec, max_concurrency, gate)
    }

    fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> anyhow::Result<u64> {
        self.lock()?.compute_accrued(tenant, period_key)
    }

    fn set_envelope_checkpoint(&self, lease_id: &str, checkpoint_json: &str) -> anyhow::Result<()> {
        self.lock()?
            .set_envelope_checkpoint(lease_id, checkpoint_json)
    }

    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        self.lock()?.get_envelope_checkpoint(lease_id)
    }

    fn remove(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.lock()?.remove(lease_id)
    }

    fn remove_if_pending(&self, lease_id: &str) -> anyhow::Result<bool> {
        // Delegate under ONE guard so the inner get+remove stays atomic (the handle
        // must NOT re-enter its own get()/remove(), which would lock twice and race
        // a concurrent admit).
        self.lock()?.remove_if_pending(lease_id)
    }
}

impl AdmitLedger for InMemoryLedger {
    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        // Lock the INNER mutex for the pure-CPU count-and-insert, then release — the
        // lock is NEVER held across an `.await`/`block_on`. The cold `LeaseLedger`
        // handle shares this same inner mutex, so admit + cold put/transition still
        // serialize → cap atomicity is byte-identical, just relocated outer→inner.
        self.lock()?.try_admit(rec, max_concurrency)
    }

    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        // Same inner-lock relocation for the compute Σ read-decide-insert.
        self.lock()?
            .try_admit_with_compute(rec, max_concurrency, gate)
    }
}

/// File-backed ledger: append-only JSONL journal + replay-on-open.
///
/// **The restart-survival oracle** for CP1's
/// `ledger_survives_process_restart`: every `put`/`transition`/`remove`
/// appends one JSON line and **fsyncs it to disk** ([`File::sync_all`]) before
/// returning, so durability holds across power-loss — not merely across a
/// graceful process exit. (`std::fs::File::flush()` is a no-op for a bare
/// `File`, so a flush alone would NOT be durable; see
/// [`FileLedger::append_line`].) `open` replays the journal,
/// last-record-per-lease wins. A torn/un-parseable TRAILING record (a crash
/// mid-append) is tolerated: the committed prefix is recovered and the torn
/// tail truncated; an un-parseable record in the MIDDLE (valid records after
/// it) is real corruption and refuses to open (fail-closed — never a silently
/// truncated state machine). The production Postgres impl is a later WP per
/// ratified decision #3; this impl pins the durability semantics it must match.
#[derive(Debug)]
pub struct FileLedger {
    /// Immutable journal path — backs the `path()` accessor (a `&Path` cannot be
    /// handed out from behind the interior lock).
    path: PathBuf,
    /// W-LEDGER-A2: the mutable state (append handle + replay index) behind an
    /// INTERNAL `Mutex`, so the [`LeaseLedger`] impl is `&self` (interior-mutable)
    /// and `AppState.ledger` needs no OUTER process `Mutex`. Each trait method
    /// takes this lock EXACTLY ONCE, so a multi-step op (e.g. `remove_if_pending`'s
    /// get+remove) stays atomic under one guard — identical to the pre-A2 behaviour
    /// under the outer lock.
    inner: Mutex<FileInner>,
}

/// The mutable core of a [`FileLedger`] — the append handle and the replay index.
/// Held behind [`FileLedger::inner`]'s `Mutex`; its `&mut self` methods run under
/// that lock (the lock provides the atomicity the `&self` trait callers rely on).
#[derive(Debug)]
struct FileInner {
    path: PathBuf,
    file: File,
    index: InMemoryInner,
}

/// One physical line in the [`FileLedger`] journal.
///
/// Historically every line was a bare [`LeaseRecord`]; admission rollback
/// (`remove`) needs a durable "this lease is gone" marker too. The two are
/// distinguished by a `kind` tag (`#[serde(tag = "kind")]`), so a `Record`
/// line still round-trips its full `LeaseRecord` fields and replay can erase
/// a lease that was later tombstoned. A line that is neither shape is corrupt
/// and refuses to open (fail-closed, unchanged).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JournalLine {
    /// A lease record write (put / transition outcome).
    Record(LeaseRecord),
    /// An admission-rollback tombstone: the lease id is removed on replay.
    Tombstone { lease_id: String },
    /// ADR-0004 Decision-2: an envelope-checkpoint write (opaque JSON blob).
    /// Replay overwrites the lease's checkpoint (last write wins); a tombstone
    /// for the same lease erases it.
    Checkpoint {
        lease_id: String,
        checkpoint: String,
    },
    /// vCPU-h ceiling wave: a per-lease compute reservation write (admit under a
    /// `Some` gate, or the terminal `accrued_at_ms` latch update). Replay
    /// overwrites the lease's reservation (last write wins); a tombstone for the
    /// same lease erases it. ADDITIVE — old journals never contain this variant,
    /// new ones load it; absence on replay leaves the side-store empty (default-off).
    Reservation {
        lease_id: String,
        reservation: LeaseReservation,
    },
    /// vCPU-h ceiling wave: the durable accrued vCPU·ms TOTAL for `(tenant,
    /// period_key)` — journaled absolutely (idempotent on replay), so a replay
    /// reconstructs the accrual map by last-write-wins per `(tenant, period)`.
    Accrual {
        tenant: TenantId,
        period_key: u32,
        total: u64,
    },
    /// vCPU-h ceiling wave (FIX-B B1): a COMBINED accounting-on admit — the lease
    /// `Record` AND its [`LeaseReservation`] in ONE physical line, so the single
    /// `writeln!`+`fsync` is atomic by construction. This eliminates the torn
    /// window of the old "Record line, then a SEPARATE Reservation line" pair: a
    /// crash could leave a durable Pending Record with NO reservation, invisible
    /// to the admit Σ → overspend. With one line, replay inserts BOTH or NEITHER.
    /// ADDITIVE — concurrency-only admits still emit a bare `Record` (default-off
    /// byte-identical); only a `Some` accounting gate emits this variant.
    AdmitCommit {
        record: LeaseRecord,
        reservation: LeaseReservation,
    },
    /// vCPU-h ceiling wave (FIX-B B1): a COMBINED terminal transition — the
    /// terminal lease `Record`, plus (when this terminal folded an accrual) the
    /// absolute accrual `total` for `(tenant, period_key)` AND the reservation
    /// carrying the once-only `accrued_at_ms` latch, all in ONE physical line.
    /// The single `writeln!`+`fsync` is atomic: replay can NEVER see a terminal
    /// Record whose accrual/latch was lost (the old "terminal Record, then a
    /// SEPARATE Accrual line" pair could lose the accrual permanently on a crash
    /// between them — the §1 matrix forbids re-transitioning to re-derive it).
    /// `accrual`/`reservation` are `None` for a terminal that folded nothing
    /// (default-off lease, or no reservation). ADDITIVE.
    TerminalTransition {
        record: LeaseRecord,
        accrual: Option<(TenantId, u32, u64)>,
        reservation: Option<LeaseReservation>,
    },
}

impl FileLedger {
    /// Open (or create) the journal at `path` and replay it.
    ///
    /// **Torn trailing record tolerance.** A crash mid-append (or mid-fsync)
    /// can leave a partial/un-parseable line at the very END of the journal. A
    /// torn record that is the LAST non-empty line is recoverable: the prefix
    /// before it is fully committed state, so we replay the prefix, DROP the
    /// torn tail, and physically truncate the journal to the last good record
    /// (so the next append starts on a clean line). A parse error on any line
    /// that is FOLLOWED by a later valid record is real corruption in the
    /// middle of committed history — that still fail-closes (refusing to open),
    /// because silently skipping it would lose committed state.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut index = InMemoryInner::new();
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("cannot read ledger journal {path:?}: {e}"))?;

            // Collect the non-empty journal lines with their byte offsets so we
            // can both (a) tell a TRAILING parse failure from a MIDDLE one and
            // (b) truncate at the last good record if the tail is torn.
            struct PhysLine<'a> {
                lineno: usize,
                byte_start: usize,
                text: &'a str,
            }
            let mut phys: Vec<PhysLine> = Vec::new();
            let mut offset = 0usize;
            for (n, line) in raw.lines().enumerate() {
                let byte_start = offset;
                // `lines()` strips the line terminator; advance the offset past
                // this line and its `\n` (good enough for the LF journal we
                // write — a trailing line with no `\n` is exactly the torn case).
                offset += line.len() + 1;
                if line.trim().is_empty() {
                    continue;
                }
                phys.push(PhysLine {
                    lineno: n + 1,
                    byte_start,
                    text: line,
                });
            }

            // Truncation point: byte offset of the torn trailing record, if any.
            let mut truncate_at: Option<u64> = None;
            for (i, pl) in phys.iter().enumerate() {
                let is_last = i + 1 == phys.len();
                let entry: JournalLine = match serde_json::from_str(pl.text) {
                    Ok(e) => e,
                    Err(_) if is_last => {
                        // Torn TRAILING record: tolerate it — drop the tail and
                        // mark the journal for truncation to the last good line.
                        truncate_at = Some(pl.byte_start as u64);
                        break;
                    }
                    Err(e) => {
                        // Un-parseable record in the MIDDLE (a valid record
                        // follows it) → real corruption, fail-closed.
                        return Err(anyhow::anyhow!(
                            "corrupt ledger journal {path:?} line {}: {e} (un-parseable record \
                             with valid records after it — fail-closed: refusing to open)",
                            pl.lineno,
                        ));
                    }
                };
                // Replay: last write per lease wins (journal is append-only).
                // A tombstone erases the lease (admission rollback).
                match entry {
                    JournalLine::Record(rec) => {
                        index.records.insert(rec.lease_id.clone(), rec);
                    }
                    JournalLine::Tombstone { lease_id } => {
                        index.records.remove(&lease_id);
                        index.checkpoints.remove(&lease_id);
                        // A tombstone (admission rollback) also drops the lease's
                        // reservation — a rolled-back Pending releases it.
                        index.reservations.remove(&lease_id);
                    }
                    JournalLine::Checkpoint {
                        lease_id,
                        checkpoint,
                    } => {
                        // Last checkpoint write per lease wins (append-only).
                        index.checkpoints.insert(lease_id, checkpoint);
                    }
                    JournalLine::Reservation {
                        lease_id,
                        reservation,
                    } => {
                        // Last reservation write per lease wins (append-only) —
                        // a later write carries the terminal `accrued_at_ms` latch.
                        index.reservations.insert(lease_id, reservation);
                    }
                    JournalLine::Accrual {
                        tenant,
                        period_key,
                        total,
                    } => {
                        // Absolute total, last write per (tenant, period) wins.
                        index.accruals.insert((tenant, period_key), total);
                    }
                    JournalLine::AdmitCommit {
                        record,
                        reservation,
                    } => {
                        // FIX-B B1: the combined accounting-on admit — the Record
                        // and its reservation arrived atomically, so replay them
                        // atomically (both or neither; a torn tail dropped both).
                        let lease_id = record.lease_id.clone();
                        index.records.insert(lease_id.clone(), record);
                        index.reservations.insert(lease_id, reservation);
                    }
                    JournalLine::TerminalTransition {
                        record,
                        accrual,
                        reservation,
                    } => {
                        // FIX-B B1: the combined terminal — Record + (folded)
                        // accrual total + latched reservation, all atomic. Replay
                        // applies them together so a terminal Record can NEVER
                        // outlive its accrual/latch.
                        let lease_id = record.lease_id.clone();
                        index.records.insert(lease_id.clone(), record);
                        if let Some((tenant, period_key, total)) = accrual {
                            index.accruals.insert((tenant, period_key), total);
                        }
                        if let Some(res) = reservation {
                            index.reservations.insert(lease_id, res);
                        }
                    }
                }
            }

            // Physically drop the torn tail so the next append begins on a
            // clean line and a second open replays identically.
            if let Some(len) = truncate_at {
                let f = OpenOptions::new().write(true).open(&path).map_err(|e| {
                    anyhow::anyhow!(
                        "cannot open ledger journal {path:?} to \
                         truncate torn tail: {e}"
                    )
                })?;
                f.set_len(len).map_err(|e| {
                    anyhow::anyhow!("cannot truncate torn trailing record in {path:?}: {e}")
                })?;
                f.sync_all()
                    .map_err(|e| anyhow::anyhow!("cannot fsync after truncating {path:?}: {e}"))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| anyhow::anyhow!("cannot open ledger journal {path:?} for append: {e}"))?;
        Ok(Self {
            path: path.clone(),
            inner: Mutex::new(FileInner { path, file, index }),
        })
    }

    /// Journal path this ledger replays from / appends to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lock the interior state, mapping a poisoned mutex to a fail-closed `Err`
    /// (never a silent panic). Every trait method takes this lock EXACTLY ONCE, so
    /// multi-step operations stay atomic under one guard, identical to the pre-A2
    /// behaviour under the outer lock.
    fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, FileInner>> {
        self.inner
            .lock()
            .map_err(|_| anyhow::anyhow!("file ledger inner mutex poisoned (fail-closed)"))
    }
}

impl FileInner {
    /// Append one journal line and **fsync it to disk** before returning.
    ///
    /// Durability holds across power-loss: `std::fs::File::flush()` is a no-op
    /// (a bare `File` has no userspace buffer), so it does NOT guarantee the
    /// bytes reach stable storage — only that they left the process. The
    /// restart-survival oracle this ledger promises requires the bytes to
    /// survive a crash, so after the `writeln!` we call
    /// [`File::sync_all`] to flush both the data and the file metadata to the
    /// underlying device. One fsync per durable mutation is the correct,
    /// deliberate cost of a durability oracle; every `put`/`transition`/
    /// `remove` that returns `Ok` is committed to disk.
    fn append_line(&mut self, entry: &JournalLine) -> anyhow::Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.file, "{line}")
            .map_err(|e| anyhow::anyhow!("cannot append to ledger journal {:?}: {e}", self.path))?;
        // fsync: the bytes must reach stable storage before we return Ok, so a
        // power-loss immediately after a successful mutation still replays it.
        self.file
            .sync_all()
            .map_err(|e| anyhow::anyhow!("cannot fsync ledger journal {:?}: {e}", self.path))?;
        Ok(())
    }

    fn append(&mut self, rec: &LeaseRecord) -> anyhow::Result<()> {
        self.append_line(&JournalLine::Record(rec.clone()))
    }
}

/// The `&mut self` core ops of a [`FileLedger`], run under [`FileLedger::inner`]'s
/// lock. Structurally identical to the pre-A2 `impl LeaseLedger for FileLedger`
/// bodies — only the receiver moved from the (outer-locked) `FileLedger` to the
/// (inner-locked) `FileInner`.
impl FileInner {
    fn put(&mut self, rec: LeaseRecord) -> anyhow::Result<()> {
        if self.index.records.contains_key(&rec.lease_id) {
            anyhow::bail!(
                "lease {} already exists: put never overwrites",
                rec.lease_id
            );
        }
        self.append(&rec)?;
        self.index.records.insert(rec.lease_id.clone(), rec);
        Ok(())
    }

    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        self.index.get(lease_id)
    }

    fn transition(
        &mut self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        // Validate + apply against the in-memory view (folds the terminal accrual
        // there); journal only legal outcomes (the journal never holds an illegal
        // transition). The captured event tells us what compute state to persist.
        let (updated, accrual) = self.index.transition_capturing(lease_id, to, now_ms)?;
        match accrual {
            // FIX-B B1 (TERMINAL): a terminal fold MUST be atomic with its Record.
            // The OLD code wrote the terminal `Record` FIRST and the Accrual/
            // Reservation as SEPARATE appends — a crash between them lost the
            // accrual permanently (the §1 matrix forbids re-transitioning a
            // terminal lease, and replay has no reconcile pass to re-derive it),
            // or left a terminal Record whose latch never landed (a reopen would
            // re-fold). One combined `TerminalTransition` line — written by a
            // single `writeln!`+`fsync` — folds Record + accrual total + latched
            // reservation atomically: replay sees ALL of it or NONE of it (a torn
            // tail drops the whole terminal, leaving the prior Held replayable).
            Some(event) => {
                let reservation = self.index.reservations.get(&event.lease_id).copied();
                self.append_line(&JournalLine::TerminalTransition {
                    record: updated.clone(),
                    accrual: Some((event.tenant, event.period_key, event.new_accrual_total)),
                    reservation,
                })?;
            }
            // No accrual folded (concurrency-only lease, or no reservation) ⇒ a
            // bare `Record` line, byte-identical to the default-off path.
            None => {
                self.append(&updated)?;
            }
        }
        Ok(updated)
    }

    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.index.by_tenant(t)
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.index.held()
    }

    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        self.index.pending_older_than(now_ms, max_age_ms)
    }

    fn try_admit(&mut self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        // Same active definition (Pending+Held) over the replayed index; the
        // admit `put` is durable (journal-append + index-insert) like any other
        // put, and still errors on a duplicate lease_id (fail-closed).
        let active = self
            .index
            .records
            .values()
            .filter(|r| {
                r.tenant == rec.tenant
                    && (matches!(r.state, LeaseState::Pending) || r.state.is_held())
            })
            .count();
        if (active as u32) < max_concurrency {
            self.put(rec)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn try_admit_with_compute(
        &mut self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        // Default-OFF: None / ceiling-0 gate ⇒ byte-identical to `try_admit`,
        // recording NO compute state (no Reservation line, identical journal).
        let gate = match gate {
            None => {
                return Ok(concurrency_only_outcome(
                    self.try_admit(rec, max_concurrency)?,
                ));
            }
            Some(g) if g.ceiling_vcpu_ms == 0 => {
                return Ok(concurrency_only_outcome(
                    self.try_admit(rec, max_concurrency)?,
                ));
            }
            Some(g) => g,
        };
        // Decide over the replayed index (Σ + accrued + cap), then durably apply.
        let (outcome, reservation) = self.index.admit_decision(&rec, max_concurrency, gate);
        if let (AdmitOutcome::Admitted, Some(res)) = (outcome, reservation) {
            // FIX-B B1 (ADMIT): the Record and its reservation MUST land
            // atomically. The OLD code journaled the bare `Record` (via `put`)
            // and the `Reservation` as TWO separate appends — a crash between
            // them left a durable Pending Record with NO reservation, invisible
            // to the admit Σ on reopen ⇒ overspend. One combined `AdmitCommit`
            // line (single `writeln!`+`fsync`) commits both or neither; a torn
            // tail drops the whole admit (no reservation-less Pending escapes Σ).
            // Fail-closed on a duplicate lease_id, exactly like `put`, BEFORE any
            // append (never journal an admit we would have rejected).
            if self.index.records.contains_key(&rec.lease_id) {
                anyhow::bail!(
                    "lease {} already exists: put never overwrites",
                    rec.lease_id
                );
            }
            let lease_id = rec.lease_id.clone();
            self.append_line(&JournalLine::AdmitCommit {
                record: rec.clone(),
                reservation: res,
            })?;
            self.index.records.insert(lease_id.clone(), rec);
            self.index.reservations.insert(lease_id, res);
        }
        Ok(outcome)
    }

    fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> anyhow::Result<u64> {
        self.index.compute_accrued(tenant, period_key)
    }

    fn set_envelope_checkpoint(
        &mut self,
        lease_id: &str,
        checkpoint_json: &str,
    ) -> anyhow::Result<()> {
        // Fail-closed against the replayed index, exactly like InMemory.
        if !self.index.records.contains_key(lease_id) {
            anyhow::bail!(
                "lease {lease_id} does not exist: cannot set envelope checkpoint (fail-closed)"
            );
        }
        // Durable first: append (flushed) before the in-memory index updates, so
        // a crash between the two replays to the same checkpoint state.
        self.append_line(&JournalLine::Checkpoint {
            lease_id: lease_id.to_string(),
            checkpoint: checkpoint_json.to_string(),
        })?;
        self.index
            .checkpoints
            .insert(lease_id.to_string(), checkpoint_json.to_string());
        Ok(())
    }

    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        self.index.get_envelope_checkpoint(lease_id)
    }

    fn remove(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        // Nothing to do (and nothing to journal) if the lease is absent.
        if !self.index.records.contains_key(lease_id) {
            return Ok(false);
        }
        // FIX-B B3 — same fail-closed guard as InMemory: a Held accounting-on
        // lease (with a reservation) must NOT be removed (it would drop its Σ
        // reservation un-billed). Check BEFORE journaling — never write a
        // tombstone for a remove we are about to reject. Default-off unaffected.
        if let Some(rec) = self.index.records.get(lease_id)
            && rec.state.is_held()
            && self.index.reservations.contains_key(lease_id)
        {
            anyhow::bail!(
                "lease {lease_id} is Held with a compute reservation: `remove` is the \
                 admission-rollback seam for Pending only — a Held accounting-on lease \
                 must terminalize via `transition` so its accrual is folded (fail-closed: \
                 refusing to drop the reservation un-billed)"
            );
        }
        // Durable first: append the tombstone (flushed) before the in-memory
        // index forgets the lease, so a crash between the two leaves a journal
        // that replays to the same erased state.
        self.append_line(&JournalLine::Tombstone {
            lease_id: lease_id.to_string(),
        })?;
        self.index.records.remove(lease_id);
        // The tombstone also erases the checkpoint AND the reservation on replay;
        // mirror that in the live index so no residue outlives the lease (a
        // rolled-back Pending releases its reservation, accrues nothing).
        self.index.checkpoints.remove(lease_id);
        self.index.reservations.remove(lease_id);
        Ok(true)
    }

    /// GUARDED admission-rollback: remove IFF still Pending. Run under the single
    /// inner lock the trait method holds, so the get+remove is atomic (identical to
    /// the pre-A2 trait-default under the outer lock). The `remove` journals a
    /// tombstone, so this must be `FileInner`'s own get+remove (not the index's).
    fn remove_if_pending(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        match self.index.get(lease_id)? {
            Some(rec) if matches!(rec.state, LeaseState::Pending) => self.remove(lease_id),
            _ => Ok(false),
        }
    }
}

impl LeaseLedger for FileLedger {
    fn put(&self, rec: LeaseRecord) -> anyhow::Result<()> {
        self.lock()?.put(rec)
    }

    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        self.lock()?.get(lease_id)
    }

    fn transition(
        &self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        self.lock()?.transition(lease_id, to, now_ms)
    }

    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.by_tenant(t)
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.held()
    }

    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        self.lock()?.pending_older_than(now_ms, max_age_ms)
    }

    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        self.lock()?.try_admit(rec, max_concurrency)
    }

    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        self.lock()?
            .try_admit_with_compute(rec, max_concurrency, gate)
    }

    fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> anyhow::Result<u64> {
        self.lock()?.compute_accrued(tenant, period_key)
    }

    fn set_envelope_checkpoint(&self, lease_id: &str, checkpoint_json: &str) -> anyhow::Result<()> {
        self.lock()?
            .set_envelope_checkpoint(lease_id, checkpoint_json)
    }

    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        self.lock()?.get_envelope_checkpoint(lease_id)
    }

    fn remove(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.lock()?.remove(lease_id)
    }

    fn remove_if_pending(&self, lease_id: &str) -> anyhow::Result<bool> {
        // ONE guard so the get+remove stays atomic (never re-enter the handle's own
        // get()/remove(), which would lock twice and race a concurrent admit).
        self.lock()?.remove_if_pending(lease_id)
    }
}

#[cfg(test)]
mod torn_journal_tests {
    // `super::*` already brings `std::io::Write` into scope (module-level
    // `use std::io::Write as _;`), so `writeln!`/`write!` on a `File` resolve.
    use super::*;

    /// Unique per-test journal path (mirrors the conformance idiom — no
    /// tempfile-crate dep in this crate).
    fn temp_journal(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "corelink-fabric-torn-{tag}-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn held_record(lease_id: &str) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: TenantId::new("acme").unwrap(),
            state: LeaseState::Wire(RunnerState::Held),
            box_ref: "box-1".to_string(),
            created_at_ms: 1,
            updated_at_ms: 1,
            deadline_ms: Some(1000),
            billing_acquired_at_ms: None,
        }
    }

    /// One serialized `Record` journal line (without the trailing newline).
    fn record_line(rec: &LeaseRecord) -> String {
        serde_json::to_string(&JournalLine::Record(rec.clone())).unwrap()
    }

    // (a) Valid prefix + torn TRAILING line → opens, recovers the prefix,
    //     drops the torn tail, and physically truncates the journal.
    #[test]
    fn torn_trailing_line_is_tolerated_and_truncated() {
        let path = temp_journal("trailing");
        let a = held_record("lease-a");
        let b = held_record("lease-b");

        // Two good records, then a torn (truncated mid-JSON, no newline) tail.
        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, "{}", record_line(&a)).unwrap();
            writeln!(f, "{}", record_line(&b)).unwrap();
            let torn = &record_line(&a)[..10]; // a partial, un-parseable line
            write!(f, "{torn}").unwrap(); // NB: no newline — the torn case
            f.sync_all().unwrap();
        }

        // Opens (does not fail-closed) and recovers exactly the good prefix.
        let ledger = FileLedger::open(&path).expect("torn trailing tail must be tolerated");
        assert!(
            ledger.get("lease-a").unwrap().is_some(),
            "prefix lease-a recovered"
        );
        assert!(
            ledger.get("lease-b").unwrap().is_some(),
            "prefix lease-b recovered"
        );

        // The torn tail was physically truncated: the on-disk journal is now
        // exactly the two good lines, and a SECOND open replays identically
        // (no torn record left to trip over).
        let on_disk = std::fs::read_to_string(&path).unwrap();
        let good_lines: Vec<&str> = on_disk.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            good_lines.len(),
            2,
            "torn tail truncated to the 2 good lines"
        );
        for l in &good_lines {
            serde_json::from_str::<JournalLine>(l).expect("every surviving line parses");
        }
        let reopened = FileLedger::open(&path).expect("second open after truncation");
        assert!(reopened.get("lease-a").unwrap().is_some());
        assert!(reopened.get("lease-b").unwrap().is_some());

        let _ = std::fs::remove_file(&path);
    }

    // (b) A torn/un-parseable record in the MIDDLE (valid records after it) is
    //     real corruption → still a hard error (committed state must not be
    //     silently lost).
    #[test]
    fn torn_middle_record_still_hard_errors() {
        let path = temp_journal("middle");
        let a = held_record("lease-a");
        let b = held_record("lease-b");

        {
            let mut f = File::create(&path).unwrap();
            writeln!(f, "{}", record_line(&a)).unwrap();
            writeln!(f, "{{ this is not valid json").unwrap(); // corrupt MIDDLE line
            writeln!(f, "{}", record_line(&b)).unwrap(); // a valid record FOLLOWS it
            f.sync_all().unwrap();
        }

        let err = FileLedger::open(&path).expect_err("mid-journal corruption must fail-closed");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("refusing to open"),
            "error must be the fail-closed refusal, got: {msg}"
        );

        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod compute_ceiling_tests {
    //! vCPU-h ceiling acceptance (WP-CD) — run against BOTH `InMemoryLedger` and
    //! `FileLedger`: ceiling-reached ⇒ OverCompute; under-ceiling ⇒ Admitted;
    //! in-flight reservation counts toward Σ; terminal accrual is once-only;
    //! default-off (None / ceiling-0) is byte-identical to `try_admit`; and the
    //! FileLedger reservation + accrual survive a process restart.
    use super::*;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    fn temp_journal(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "corelink-fabric-ceiling-{tag}-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn pending(lease_id: &str, tenant: &TenantId, created_at_ms: u64) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: tenant.clone(),
            state: LeaseState::Pending,
            box_ref: "box-1".to_string(),
            created_at_ms,
            updated_at_ms: created_at_ms,
            deadline_ms: Some(created_at_ms + 60_000),
            billing_acquired_at_ms: None,
        }
    }

    fn gate(period_key: u32, ceiling: u64, vcpu: u32, reserved: u64) -> ComputeGate {
        ComputeGate {
            period_key,
            ceiling_vcpu_ms: ceiling,
            box_vcpu_count: vcpu,
            new_reserved_vcpu_ms: reserved,
        }
    }

    /// Run the same closure against a fresh InMemory and a fresh File ledger so
    /// every invariant is asserted on BOTH impls (the File journal is cleaned up).
    fn on_both(tag: &str, body: impl Fn(&mut dyn LeaseLedger)) {
        let mut mem = InMemoryLedger::new();
        body(&mut mem);

        let path = temp_journal(tag);
        {
            let mut file = FileLedger::open(&path).unwrap();
            body(&mut file);
        }
        let _ = std::fs::remove_file(&path);
    }

    // Under-ceiling ⇒ Admitted; at/over ⇒ OverCompute (distinct from cap).
    #[test]
    fn under_ceiling_admits_over_ceiling_rejects() {
        on_both("ceiling", |led| {
            let t = tid("acme");
            // ceiling 1000; first reservation 600 (accrued 0 + Σ 0 + 600 ≤ 1000) ⇒ Admit.
            let g = gate(202406, 1_000, 2, 600);
            assert_eq!(
                led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // Second reservation 600: accrued 0 + Σ 600 + 600 = 1200 > 1000 ⇒ OverCompute.
            assert_eq!(
                led.try_admit_with_compute(pending("l2", &t, 0), 100, Some(g))
                    .unwrap(),
                AdmitOutcome::OverCompute
            );
            // The rejected lease inserted NOTHING.
            assert!(led.get("l2").unwrap().is_none());
            // A smaller reservation (400) still fits: 600 + 400 = 1000 ≤ 1000 ⇒ Admit.
            let g_small = gate(202406, 1_000, 2, 400);
            assert_eq!(
                led.try_admit_with_compute(pending("l3", &t, 0), 100, Some(g_small))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
        });
    }

    // The in-flight reservation of a still-Held lease counts toward Σ — a 2nd
    // admit that would exceed is rejected while the 1st is Held.
    #[test]
    fn in_flight_reservation_counts_toward_sigma() {
        on_both("inflight", |led| {
            let t = tid("acme");
            let g = gate(202406, 1_000, 2, 700);
            assert_eq!(
                led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // Move it to Held — its reservation STILL counts toward Σ.
            led.transition("l1", RunnerState::Held, 10).unwrap();
            // Σ = 700; 700 + 700 = 1400 > 1000 ⇒ OverCompute while l1 is Held.
            assert_eq!(
                led.try_admit_with_compute(pending("l2", &t, 0), 100, Some(g))
                    .unwrap(),
                AdmitOutcome::OverCompute
            );
        });
    }

    // Terminal accrual is once-only: a second terminal call (here, a no-op
    // re-`transition` is illegal, so we assert the accrual didn't change by a
    // racing/duplicate terminalizer simulated via direct re-fold attempts).
    #[test]
    fn terminal_accrual_is_once_only() {
        on_both("once", |led| {
            let t = tid("acme");
            let g = gate(202406, 1_000_000, 4, 100);
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();
            // Terminalize at now=1000: raw 4 vCPU × 1000 ms = 4000 vCPU·ms, but the
            // reservation is only 100 (this lease ran past its ttl before reaping).
            // C1 CLAMPS the terminal accrual to `reserved` (§8 monotonicity:
            // `actual ≤ reserved` ALWAYS) → 100, NOT the unclamped 4000. This is the
            // SAME value PgLedger's C1 produces for identical inputs.
            led.transition("l1", RunnerState::Released, 1_000).unwrap();
            assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 100);
            // A second terminal transition is ILLEGAL (terminal is absorbing), so
            // it errors and CANNOT double-accrue.
            assert!(led.transition("l1", RunnerState::Crashed, 9_999).is_err());
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                100,
                "accrual is once-only — a 2nd terminalizer never double-accrues"
            );
        });
    }

    // FIX-D (§8 monotonicity, the cross-ledger regression guard) — an OVERDUE
    // lease (terminalized past its ttl: now − created > ttl) accrues the CLAMPED
    // `reserved`, NEVER the larger raw `vcpu × elapsed`. This is the invariant the
    // re-audit found applied to PgLedger ONLY; it now holds on InMemory + File too.
    // `on_both` asserts BOTH backends, and the clamped value (= reserved when
    // overdue) is byte-for-byte the SAME value PgLedger's C1 produces for identical
    // inputs (reserved=100, raw=4×1000=4000) → the §8 invariant `actual ≤ reserved`
    // holds on EVERY backend, so durable accrual can never diverge between the
    // FileLedger restart-oracle and production Pg.
    #[test]
    fn terminal_accrual_clamps_to_reservation_overdue() {
        on_both("overdue", |led| {
            let t = tid("acme");
            // reserved = vcpu(4) × ttl(25 ms) = 100, accounting-on, ceiling generous.
            let g = gate(202406, 1_000_000, 4, 100);
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();
            // Terminalize at now=1000 — WAY past the reserved ttl. Raw would be
            // 4 × 1000 = 4000, but the clamp caps the charge at reserved = 100.
            led.transition("l1", RunnerState::Released, 1_000).unwrap();
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                100,
                "overdue terminal accrues CLAMPED reserved (100), never raw actual (4000)"
            );
        });
    }

    // FIX-D companion — a lease terminalized BEFORE its ttl accrues the REAL
    // (smaller) actual, NOT reserved: the clamp is a ceiling, never a floor, so it
    // only ever caps an overspend and never inflates an under-ttl charge.
    #[test]
    fn within_ttl_accrues_actual() {
        on_both("withinttl", |led| {
            let t = tid("acme");
            // reserved = 4_000 (vcpu 4 × ttl 1000 ms, the worst case).
            let g = gate(202406, 1_000_000, 4, 4_000);
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();
            // Terminalize EARLY at now=250: raw 4 × 250 = 1000 < reserved 4000 ⇒
            // the clamp is a no-op, the REAL 1000 accrues (not the 4000 reservation).
            led.transition("l1", RunnerState::Released, 250).unwrap();
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                1_000,
                "within ttl accrues the real (smaller) actual, never the reservation"
            );
        });
    }

    // A rolled-back Pending (remove) releases its reservation and accrues nothing.
    #[test]
    fn rollback_releases_reservation_accrues_nothing() {
        on_both("rollback", |led| {
            let t = tid("acme");
            let g = gate(202406, 1_000, 2, 600);
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            // Roll the Pending back — its 600 reservation is released.
            assert!(led.remove("l1").unwrap());
            // Σ now 0 again: a fresh 600 + 600-equivalent admit fits twice.
            assert_eq!(
                led.try_admit_with_compute(pending("l2", &t, 0), 100, Some(g))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // Nothing accrued (only terminal transitions accrue).
            assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 0);
        });
    }

    // Default-off: None gate AND a ceiling-0 gate are byte-identical to
    // `try_admit` — same outcomes, and NO compute state recorded.
    #[test]
    fn default_off_is_byte_identical_to_try_admit() {
        on_both("defoff", |led| {
            let t = tid("acme");
            // None gate ⇒ Admitted on a free cap, identical to try_admit.
            assert_eq!(
                led.try_admit_with_compute(pending("n1", &t, 0), 100, None)
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // ceiling-0 (disabled sentinel) gate ⇒ also concurrency-only.
            let g0 = gate(202406, 0, 64, u64::MAX);
            assert_eq!(
                led.try_admit_with_compute(pending("n2", &t, 0), 100, Some(g0))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // NEITHER recorded any compute state (accrual stays 0 even after a
            // terminal transition — no reservation means no fold).
            led.transition("n2", RunnerState::Held, 0).unwrap();
            led.transition("n2", RunnerState::Released, 5_000).unwrap();
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                0,
                "default-off records NO compute state (no reservation ⇒ no accrual)"
            );
            // Concurrency cap still bites identically under a None gate.
            assert_eq!(
                led.try_admit_with_compute(pending("n3", &t, 0), 1, None)
                    .unwrap(),
                AdmitOutcome::OverConcurrency
            );
        });
    }

    // FileLedger ONLY: the in-flight reservation AND the terminal accrual both
    // survive a process restart (reopen replays them from the journal).
    #[test]
    fn file_reservation_and_accrual_survive_restart() {
        let path = temp_journal("restart");
        let t = tid("acme");
        let g = gate(202406, 1_000, 2, 600);

        // Process 1: admit l1 (Held, in-flight 600 reservation) + admit-and-
        // terminalize l2 (1 vCPU held 0→100 ms ⇒ accrues 100 vCPU·ms).
        {
            let led = FileLedger::open(&path).unwrap();
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();

            // reserved=100 ≥ raw actual (1 vCPU × 100 ms = 100) ⇒ within ttl, the C1
            // clamp is a no-op and the REAL 100 vCPU·ms accrues (not clamped down).
            let g2 = gate(202406, 1_000, 1, 100);
            led.try_admit_with_compute(pending("l2", &t, 0), 100, Some(g2))
                .unwrap();
            led.transition("l2", RunnerState::Held, 0).unwrap();
            led.transition("l2", RunnerState::Released, 100).unwrap();
            assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 100);
        }

        // Process 2: reopen — the accrual is reconstructed AND l1's still-Held
        // reservation still counts toward Σ.
        {
            let led = FileLedger::open(&path).unwrap();
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                100,
                "terminal accrual survives restart"
            );
            // l1 is still Held with its 600 reservation; accrued 100 + Σ 600 = 700.
            // A new 400-vCPU·ms admit (100 + 600 + 400 = 1100 > 1000) is rejected ⇒
            // the reservation survived (else 100 + 0 + 400 = 500 ≤ 1000 would admit).
            let led = led;
            let g3 = gate(202406, 1_000, 2, 400);
            assert_eq!(
                led.try_admit_with_compute(pending("l3", &t, 0), 100, Some(g3))
                    .unwrap(),
                AdmitOutcome::OverCompute,
                "in-flight reservation survives restart and still counts toward Σ"
            );
        }

        // Process 3: re-terminalizing is impossible (l2 already terminal), so the
        // restart cannot double-accrue — the latch survived too.
        {
            let led = FileLedger::open(&path).unwrap();
            assert!(led.transition("l2", RunnerState::Crashed, 9_999).is_err());
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                100,
                "the once-only accrual latch survives restart (no double-accrue)"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    // FIX-B B2 — an admit that is over BOTH the compute ceiling AND the
    // concurrency cap must report `OverCompute` (compute WINS), on BOTH ledgers.
    // `OverConcurrency` would route to a queue that bypasses the monthly wall.
    #[test]
    fn over_both_returns_over_compute_not_concurrency() {
        on_both("overboth", |led| {
            let t = tid("acme");
            // Fill the single concurrency slot with a tiny in-flight reservation.
            let g_fill = gate(202406, 1_000, 1, 10);
            assert_eq!(
                led.try_admit_with_compute(pending("l1", &t, 0), 1, Some(g_fill))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // A 2nd admit is over BOTH: cap is full (max_concurrency = 1, l1 active)
            // AND the ceiling is blown (accrued 0 + Σ 10 + 2_000 = 2_010 > 1_000).
            // Compute must WIN ⇒ OverCompute, never OverConcurrency.
            let g_over = gate(202406, 1_000, 2, 2_000);
            assert_eq!(
                led.try_admit_with_compute(pending("l2", &t, 0), 1, Some(g_over))
                    .unwrap(),
                AdmitOutcome::OverCompute,
                "over BOTH ⇒ OverCompute (compute precedes concurrency)"
            );
            assert!(
                led.get("l2").unwrap().is_none(),
                "rejected admit inserts nothing"
            );
        });
    }

    // FIX-B B3 — `remove` of a HELD accounting-on lease (one with a reservation)
    // is a contract violation ⇒ Err, on BOTH ledgers. Dropping it silently would
    // un-bill the reservation (undercount). A Held lease must terminalize.
    #[test]
    fn remove_held_accounting_lease_fails_closed() {
        on_both("removeheld", |led| {
            let t = tid("acme");
            let g = gate(202406, 10_000, 2, 100);
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            // Pending → remove is legal admission rollback (still allowed).
            // Move to Held first, THEN remove must fail-closed.
            led.transition("l1", RunnerState::Held, 0).unwrap();
            assert!(
                led.remove("l1").is_err(),
                "remove of a Held accounting-on lease must fail-closed (un-bill guard)"
            );
            // The lease is untouched: still Held, reservation still in Σ.
            assert!(led.get("l1").unwrap().unwrap().state.is_held());
        });
    }

    // FIX-B B3 — the guard is accounting-ONLY: `remove` of a Held lease with NO
    // reservation (default-off) is still allowed, byte-identical to before.
    #[test]
    fn remove_held_default_off_lease_still_allowed() {
        on_both("removedefoff", |led| {
            let t = tid("acme");
            // No compute gate ⇒ no reservation recorded.
            led.try_admit_with_compute(pending("d1", &t, 0), 100, None)
                .unwrap();
            led.transition("d1", RunnerState::Held, 0).unwrap();
            assert!(
                led.remove("d1").unwrap(),
                "default-off Held remove is unaffected by the B3 guard"
            );
            assert!(led.get("d1").unwrap().is_none());
        });
    }

    // FIX-B B1 (ADMIT, crash-truncation) — admit an accounting-on lease, then
    // simulate a crash by truncating the journal AT the byte boundary of the
    // combined admit line (dropping it as a torn tail). On reopen the invariant
    // holds: NO reservation-less Pending/Held lease that escapes Σ — the whole
    // admit was dropped atomically (record AND reservation gone together).
    #[test]
    fn admit_crash_truncation_never_orphans_a_reservationless_lease() {
        let path = temp_journal("admit-crash");
        let t = tid("acme");
        let g = gate(202406, 10_000, 2, 600);
        {
            let led = FileLedger::open(&path).unwrap();
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
        }
        // The journal is exactly ONE physical line (the combined AdmitCommit).
        // "Crash" = truncate it to zero bytes at that line's boundary.
        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1, "accounting-on admit is ONE combined line");
        // Tear the trailing line: keep the first byte only (un-parseable, no LF).
        let torn = &lines[0][..1];
        std::fs::write(&path, torn).unwrap();

        // Reopen: the torn tail is dropped → the lease is GONE (record AND
        // reservation), never a reservation-less row that escapes Σ.
        let led = FileLedger::open(&path).expect("torn admit tail tolerated");
        assert!(
            led.get("l1").unwrap().is_none(),
            "a half-written admit leaves NO reservation-less lease (dropped atomically)"
        );
        // And Σ is clean: a fresh admit up to the full ceiling fits (no phantom Σ).
        let led = led;
        let g_full = gate(202406, 10_000, 2, 10_000);
        assert_eq!(
            led.try_admit_with_compute(pending("l2", &t, 0), 100, Some(g_full))
                .unwrap(),
            AdmitOutcome::Admitted,
            "no phantom reservation survived the torn admit"
        );

        let _ = std::fs::remove_file(&path);
    }

    // FIX-B B1 (TERMINAL, crash-truncation) — admit→Held→terminal an accounting-on
    // lease, then truncate the journal at the boundary of the combined terminal
    // line (dropping it as a torn tail). On reopen the invariant holds: the lease
    // is back at its last durable state (Held) — a half-written terminal NEVER
    // loses the accrual silently; it simply replays the prior Held, and the
    // terminal can be RE-DRIVEN (legal: Held → terminal), folding the accrual then.
    #[test]
    fn terminal_crash_truncation_never_loses_accrual() {
        let path = temp_journal("term-crash");
        let t = tid("acme");
        let g = gate(202406, 1_000_000, 4, 100);
        {
            let led = FileLedger::open(&path).unwrap();
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();
            led.transition("l1", RunnerState::Released, 1_000).unwrap();
            // reserved=100, raw=4×1000=4000 → C1-clamped to 100 (§8 `actual ≤ reserved`).
            assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 100);
        }
        // Drop the LAST line (the combined TerminalTransition) as a torn tail,
        // leaving the AdmitCommit + the Held Record committed before it.
        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert!(lines.len() >= 2, "admit + Held + terminal lines present");
        // Reassemble all but the last line, then append a torn (partial) copy of
        // the last line with no LF — exactly the crash-mid-terminal-append case.
        let mut rebuilt = String::new();
        for l in &lines[..lines.len() - 1] {
            rebuilt.push_str(l);
            rebuilt.push('\n');
        }
        let last = lines[lines.len() - 1];
        rebuilt.push_str(&last[..last.len() / 2]); // torn, no trailing LF
        std::fs::write(&path, rebuilt).unwrap();

        // Reopen: the torn terminal is dropped → the lease replays at its prior
        // durable state (Held), and the accrual was NOT folded (no half-state).
        let led = FileLedger::open(&path).expect("torn terminal tail tolerated");
        let rec = led
            .get("l1")
            .unwrap()
            .expect("lease survives at prior state");
        assert!(
            rec.state.is_held(),
            "half-written terminal replays prior Held"
        );
        // The accrual is not lost-and-silently-gone: it was simply never folded,
        // and the terminal is re-drivable (Held → terminal is legal), folding it.
        assert_eq!(
            led.compute_accrued(&t, 202406).unwrap(),
            0,
            "no half-folded accrual: the terminal is dropped wholesale, re-drivable"
        );
        led.transition("l1", RunnerState::Released, 1_000).unwrap();
        assert_eq!(
            led.compute_accrued(&t, 202406).unwrap(),
            100,
            "re-driving the dropped terminal folds the (C1-clamped) accrual — never lost"
        );

        let _ = std::fs::remove_file(&path);
    }

    // FIX-B B1 — terminal accrual SURVIVES a clean reopen (admit→Held→terminal→
    // reopen → compute_accrued reflects the charge). The combined TerminalTransition
    // line carries Record + accrual total + latched reservation atomically.
    #[test]
    fn terminal_accrual_survives_clean_reopen() {
        let path = temp_journal("term-clean");
        let t = tid("acme");
        let g = gate(202406, 1_000_000, 4, 100);
        {
            let led = FileLedger::open(&path).unwrap();
            led.try_admit_with_compute(pending("l1", &t, 0), 100, Some(g))
                .unwrap();
            led.transition("l1", RunnerState::Held, 0).unwrap();
            led.transition("l1", RunnerState::Released, 1_000).unwrap();
            // reserved=100, raw=4×1000=4000 → C1-clamped to 100 (§8 `actual ≤ reserved`).
            assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 100);
        }
        // Clean reopen: the accrual is reconstructed from the combined line.
        let led = FileLedger::open(&path).unwrap();
        assert_eq!(
            led.compute_accrued(&t, 202406).unwrap(),
            100,
            "terminal (C1-clamped) accrual survives a clean reopen"
        );
        // And it cannot double-accrue (the latch survived in the same line).
        assert!(led.transition("l1", RunnerState::Crashed, 9_999).is_err());
        assert_eq!(led.compute_accrued(&t, 202406).unwrap(), 100);

        let _ = std::fs::remove_file(&path);
    }

    // ── Track-C C3 (#3, revenue-loss): the durable billing-acquire stamp ──────
    /// The FIRST `Pending → Held` transition stamps `billing_acquired_at_ms`, and
    /// it SURVIVES unchanged through the terminal transition so the terminal
    /// billing read (close/reaper) recovers the acquire time even after a fabricd
    /// restart dropped the billing target's in-memory pairing. A Pending row is
    /// NOT stamped (billing starts when the slot is occupied, not at admission).
    #[test]
    fn held_transition_stamps_durable_billing_acquired_at_ms() {
        on_both("bill-acq", |led| {
            let t = tid("acme");
            led.put(pending("l1", &t, 0)).unwrap();
            // Pending: not yet stamped — billing has not started.
            assert_eq!(
                led.get("l1").unwrap().unwrap().billing_acquired_at_ms,
                None,
                "a Pending row carries no billing-acquire stamp"
            );
            // Pending → Held at t=5_000: billing starts, stamp written.
            led.transition("l1", RunnerState::Held, 5_000).unwrap();
            assert_eq!(
                led.get("l1").unwrap().unwrap().billing_acquired_at_ms,
                Some(5_000),
                "the FIRST Held transition stamps billing_acquired_at_ms = now_ms"
            );
            // Held → Released at t=9_000: the terminal record STILL carries the
            // original stamp (never moved by a later transition), so the terminal
            // billing read yields the correct acquire time.
            let terminal = led.transition("l1", RunnerState::Released, 9_000).unwrap();
            assert_eq!(
                terminal.billing_acquired_at_ms,
                Some(5_000),
                "the durable stamp rides the terminal record (restart-safe billing)"
            );
            assert_eq!(
                led.get("l1").unwrap().unwrap().billing_acquired_at_ms,
                Some(5_000)
            );
        });
    }

    // ── Track-C C3 (#4, DISPOSITIONED-FALSE): remove_if_pending on an
    //    accounting-ON Pending is CORRECT, not a leak ──────────────────────────
    /// PIN: sweeping an accounting-ON **Pending** (with `reserved_vcpu_ms` set)
    /// via `remove_if_pending` FREES its reserved Σ headroom — a subsequent
    /// compute admit that needed exactly that headroom now succeeds. This proves
    /// the audit-handoff #4 "leak" concern is FALSE: a never-Held Pending consumed
    /// ZERO vCPU·ms, `reserved_vcpu_ms` is live-summed into Σ from the pending row,
    /// so deleting the row releases the reservation with nothing to accrue. NO
    /// guard/transition-branch was added (that would leave accounting-on Pendings
    /// un-swept); `remove`'s `state='pending'` branch deliberately allows this.
    #[test]
    fn remove_if_pending_frees_reserved_sigma_headroom_for_accounting_on_pending() {
        on_both("dispose4", |led| {
            let t = tid("acme");
            // Ceiling 1000. Admit an accounting-ON Pending reserving 700 (Σ=700).
            let g1 = gate(202406, 1_000, 2, 700);
            assert_eq!(
                led.try_admit_with_compute(pending("p1", &t, 0), 100, Some(g1))
                    .unwrap(),
                AdmitOutcome::Admitted
            );
            // A second admit reserving 700 would be 700+700=1400 > 1000 → rejected
            // while p1 (never Held) still holds its reservation in Σ.
            let g2 = gate(202406, 1_000, 2, 700);
            assert_eq!(
                led.try_admit_with_compute(pending("p2", &t, 0), 100, Some(g2))
                    .unwrap(),
                AdmitOutcome::OverCompute,
                "p1's reservation occupies Σ while it is Pending"
            );
            // Sweep p1 (still Pending, accounting-ON) via remove_if_pending.
            assert!(
                led.remove_if_pending("p1").unwrap(),
                "an accounting-ON Pending is legally swept (state='pending' branch)"
            );
            // Its reservation is now GONE from Σ (never-Held → zero consumed, zero
            // accrued), so the previously-rejected admit SUCCEEDS.
            let g3 = gate(202406, 1_000, 2, 700);
            assert_eq!(
                led.try_admit_with_compute(pending("p3", &t, 0), 100, Some(g3))
                    .unwrap(),
                AdmitOutcome::Admitted,
                "sweeping the Pending freed its reserved Σ headroom — #4 is FALSE, no leak"
            );
            // And nothing was accrued for the never-Held, swept lease.
            assert_eq!(
                led.compute_accrued(&t, 202406).unwrap(),
                0,
                "a never-Held swept Pending accrues zero (nothing to accrue)"
            );
        });
    }
}

/// W-LEDGER-A1: the in-memory admit-lock-split invariants — the state is now behind
/// an INTERNAL `Arc<Mutex<InMemoryInner>>` a cloned handle shares, and the `&self`
/// [`AdmitLedger`] seam drives the atomic count-and-reserve through that inner lock.
/// These prove the relocation outer→inner preserved (a) cap atomicity and (b)
/// compute-reservation atomicity under real concurrency, and that a clone shares state.
#[cfg(test)]
mod admit_lock_split_tests {
    use super::*;
    use std::sync::Barrier;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).unwrap()
    }

    fn pending(id: &str, tenant: &TenantId) -> LeaseRecord {
        LeaseRecord {
            lease_id: id.to_string(),
            tenant: tenant.clone(),
            state: LeaseState::Pending,
            box_ref: format!("box:{id}"),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deadline_ms: Some(61_000),
            billing_acquired_at_ms: None,
        }
    }

    /// TEST (i)+(iv) — cap-safety UNBROKEN on in-memory. A concurrent same-tenant
    /// burst of N≫cap through the `&self` [`AdmitLedger`] seam on ONE SHARED
    /// `InMemoryLedger` (cloned into every thread — the composition-root shape)
    /// admits EXACTLY `cap`. The atomicity is the INNER mutex; there is no process
    /// `Mutex` here at all, and the count-and-insert still cannot over-admit.
    #[test]
    fn inmemory_admit_seam_concurrent_burst_respects_cap_exactly() {
        const CAP: u32 = 3;
        const N: usize = 64;
        let ledger = InMemoryLedger::new();
        let t = tid("acme");
        let barrier = Arc::new(Barrier::new(N));
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let led = ledger.clone(); // shares the inner Arc<Mutex<..>>
                let barrier = Arc::clone(&barrier);
                let t = t.clone();
                let id = format!("l-{i}");
                std::thread::spawn(move || {
                    barrier.wait();
                    AdmitLedger::try_admit(&led, pending(&id, &t), CAP).unwrap()
                })
            })
            .collect();
        let admitted = handles
            .into_iter()
            .map(|h| h.join().expect("admit thread must not panic"))
            .filter(|ok| *ok)
            .count();
        assert_eq!(
            admitted, CAP as usize,
            "the &self admit seam on a shared in-memory ledger admits EXACTLY the \
             cap (got {admitted}, cap {CAP}); the inner mutex keeps count+insert atomic"
        );
        assert_eq!(
            ledger.by_tenant(&t).unwrap().len(),
            CAP as usize,
            "exactly `cap` rows are held after the burst"
        );
    }

    /// TEST (ii) — compute-ceiling EXACT under concurrency on in-memory. With the
    /// ceiling set so exactly ONE reservation fits (Σ starts empty; two would
    /// overflow), a concurrent same-tenant burst yields EXACTLY one `Admitted`, the
    /// rest `OverCompute` — the read-decide-insert of the compute Σ is atomic under
    /// the relocated inner mutex, never a race that double-reserves.
    #[test]
    fn inmemory_admit_seam_compute_ceiling_exact_under_concurrency() {
        const N: usize = 48;
        let ledger = InMemoryLedger::new();
        let t = tid("acme");
        // ceiling 1000; each reservation 600 ⇒ 1st fits (0+600≤1000), a 2nd would be
        // 600+600=1200>1000. Cap high (100) so CONCURRENCY is never the gate.
        let gate = ComputeGate {
            period_key: 202406,
            ceiling_vcpu_ms: 1_000,
            box_vcpu_count: 2,
            new_reserved_vcpu_ms: 600,
        };
        let barrier = Arc::new(Barrier::new(N));
        let handles: Vec<_> = (0..N)
            .map(|i| {
                let led = ledger.clone();
                let barrier = Arc::clone(&barrier);
                let t = t.clone();
                let id = format!("c-{i}");
                std::thread::spawn(move || {
                    barrier.wait();
                    AdmitLedger::try_admit_with_compute(&led, pending(&id, &t), 100, Some(gate))
                        .unwrap()
                })
            })
            .collect();
        let mut admitted = 0usize;
        let mut over_compute = 0usize;
        for h in handles {
            match h.join().expect("thread must not panic") {
                AdmitOutcome::Admitted => admitted += 1,
                AdmitOutcome::OverCompute => over_compute += 1,
                AdmitOutcome::OverConcurrency => {
                    panic!("cap is 100 — concurrency must never be the gate here")
                }
            }
        }
        assert_eq!(
            admitted, 1,
            "exactly ONE admit fits under the compute ceiling ({admitted} did)"
        );
        assert_eq!(
            over_compute,
            N - 1,
            "every other admit is OverCompute (the Σ read-decide-insert is atomic)"
        );
    }

    /// TEST (v)-inmem — the shared-state invariant. Two handles built by CLONE share
    /// one inner `Mutex` (an independent `new()` does NOT); a reserve committed
    /// through the admit clone is authoritatively visible through the cold clone —
    /// the property the composition root relies on so close/reaper see the reserve.
    #[test]
    fn inmemory_admit_and_cold_clone_share_state() {
        let cold = InMemoryLedger::new();
        let admit = cold.clone();
        assert!(
            cold.shares_state_with(&admit),
            "a clone shares the inner Mutex (same ledger)"
        );
        assert!(
            !cold.shares_state_with(&InMemoryLedger::new()),
            "an independent new() does NOT share state"
        );
        // Reserve through the admit handle (&self, no process Mutex)…
        let t = tid("acme");
        assert!(AdmitLedger::try_admit(&admit, pending("shared-1", &t), 5).unwrap());
        // …and it is visible through the cold LeaseLedger handle.
        assert!(
            cold.get("shared-1").unwrap().is_some(),
            "the admit-committed Pending is visible via the cold ledger handle \
             (shared authoritative state)"
        );
    }
}
