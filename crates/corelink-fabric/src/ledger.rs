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

pub trait LeaseLedger {
    /// Register a new record. Fails if `lease_id` already exists — state is
    /// mutated only through [`LeaseLedger::transition`], never by overwrite.
    fn put(&mut self, rec: LeaseRecord) -> anyhow::Result<()>;

    /// Look up a record by lease id (`Ok(None)` when absent).
    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>>;

    /// Transition a lease to `to` at `now_ms`, enforcing the contract §1
    /// matrix (see [`transition_is_legal`]); returns the updated record.
    /// Unknown lease or illegal pair → `Err` (fail-closed).
    fn transition(
        &mut self,
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
    fn try_admit(&mut self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool>;

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
        &mut self,
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
    fn set_envelope_checkpoint(
        &mut self,
        lease_id: &str,
        checkpoint_json: &str,
    ) -> anyhow::Result<()>;

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
    fn remove(&mut self, lease_id: &str) -> anyhow::Result<bool>;

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
    fn remove_if_pending(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        match self.get(lease_id)? {
            Some(rec) if matches!(rec.state, LeaseState::Pending) => self.remove(lease_id),
            // Absent, or no longer Pending (raced to Held / terminal) → no-op.
            _ => Ok(false),
        }
    }
}

/// In-memory ledger — dev/test impl; disqualified for production by
/// `ledger_survives_process_restart` (CP1).
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    records: HashMap<String, LeaseRecord>,
    /// ADR-0004 Decision-2: the durable envelope-checkpoint blob per lease
    /// (opaque JSON, never parsed by the ledger). A side map keeps the frozen
    /// [`LeaseRecord`] shape — and the journal/wire it round-trips — untouched.
    checkpoints: HashMap<String, String>,
}

impl InMemoryLedger {
    /// Empty in-memory ledger.
    pub fn new() -> Self {
        Self::default()
    }
}

impl LeaseLedger for InMemoryLedger {
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
        let rec = self
            .records
            .get_mut(lease_id)
            .ok_or_else(|| anyhow::anyhow!("unknown lease {lease_id}: cannot transition"))?;
        apply_transition(rec, to, now_ms)
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
        // The checkpoint (if any) goes with the record — a rolled-back lease
        // leaves no checkpoint residue.
        self.checkpoints.remove(lease_id);
        Ok(self.records.remove(lease_id).is_some())
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
    path: PathBuf,
    file: File,
    index: InMemoryLedger,
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
        let mut index = InMemoryLedger::new();
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
                    }
                    JournalLine::Checkpoint {
                        lease_id,
                        checkpoint,
                    } => {
                        // Last checkpoint write per lease wins (append-only).
                        index.checkpoints.insert(lease_id, checkpoint);
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
        Ok(Self { path, file, index })
    }

    /// Journal path this ledger replays from / appends to.
    pub fn path(&self) -> &Path {
        &self.path
    }

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

impl LeaseLedger for FileLedger {
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
        // Validate against the in-memory view first; journal only legal
        // outcomes (the journal never contains an illegal transition).
        let updated = self.index.transition(lease_id, to, now_ms)?;
        self.append(&updated)?;
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
        // Durable first: append the tombstone (flushed) before the in-memory
        // index forgets the lease, so a crash between the two leaves a journal
        // that replays to the same erased state.
        self.append_line(&JournalLine::Tombstone {
            lease_id: lease_id.to_string(),
        })?;
        self.index.records.remove(lease_id);
        // The tombstone also erases the checkpoint on replay; mirror that in the
        // live index so no checkpoint residue outlives the lease.
        self.index.checkpoints.remove(lease_id);
        Ok(true)
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
