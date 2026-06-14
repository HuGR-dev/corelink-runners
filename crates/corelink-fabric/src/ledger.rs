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
/// `ledger_survives_process_restart`: every `put`/`transition` appends one
/// JSON line (flushed before returning); `open` replays the journal,
/// last-record-per-lease wins. A corrupt journal line refuses to open
/// (fail-closed — never a silently truncated state machine). The production
/// Postgres impl is a later WP per ratified decision #3; this impl pins the
/// durability semantics it must match.
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
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut index = InMemoryLedger::new();
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("cannot read ledger journal {path:?}: {e}"))?;
            for (n, line) in raw.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let entry: JournalLine = serde_json::from_str(line).map_err(|e| {
                    anyhow::anyhow!(
                        "corrupt ledger journal {path:?} line {}: {e} (fail-closed: refusing \
                         to open)",
                        n + 1,
                    )
                })?;
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

    fn append_line(&mut self, entry: &JournalLine) -> anyhow::Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.file, "{line}")
            .map_err(|e| anyhow::anyhow!("cannot append to ledger journal {:?}: {e}", self.path))?;
        self.file
            .flush()
            .map_err(|e| anyhow::anyhow!("cannot flush ledger journal {:?}: {e}", self.path))?;
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
