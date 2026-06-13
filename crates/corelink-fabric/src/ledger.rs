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
/// restart-survival oracle). The production Postgres impl is a **later WP**
/// per ratified decision #3 (`docs/plan/m1-decomposition-draft.md` §4).
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
}

/// In-memory ledger — dev/test impl; disqualified for production by
/// `ledger_survives_process_restart` (CP1).
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    records: HashMap<String, LeaseRecord>,
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
                let rec: LeaseRecord = serde_json::from_str(line).map_err(|e| {
                    anyhow::anyhow!(
                        "corrupt ledger journal {path:?} line {}: {e} (fail-closed: refusing \
                         to open)",
                        n + 1,
                    )
                })?;
                // Replay: last record per lease wins (journal is append-only).
                index.records.insert(rec.lease_id.clone(), rec);
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

    fn append(&mut self, rec: &LeaseRecord) -> anyhow::Result<()> {
        let line = serde_json::to_string(rec)?;
        writeln!(self.file, "{line}")
            .map_err(|e| anyhow::anyhow!("cannot append to ledger journal {:?}: {e}", self.path))?;
        self.file
            .flush()
            .map_err(|e| anyhow::anyhow!("cannot flush ledger journal {:?}: {e}", self.path))?;
        Ok(())
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
}
