//! corelink-fabric — control-plane core types (CF0 freeze items 4 + 5).
//!
//! The shared vocabulary the M1 control-plane work-packages build against
//! (`docs/plan/m1-decomposition-draft.md` §3):
//!
//! - **Freeze item 4 — ledger + tenant + slot types**: [`tenant::TenantId`] /
//!   [`tenant::TenantPlan`] (CP2/BIL2), [`ledger::LeaseRecord`] +
//!   [`ledger::LeaseLedger`] (CP1, the authoritative lease state machine —
//!   contract §1 five states only, fail-closed) with [`ledger::InMemoryLedger`]
//!   and the append-only-JSONL [`ledger::FileLedger`] (the restart-survival
//!   oracle; the Postgres impl is a later WP per ratified decision #3).
//! - **Freeze item 5 — meter event schema**: [`meter::SlotOccupancyEvent`] /
//!   [`meter::SlotEventKind`] / [`meter::CogsCounters`] (BIL1 emits, CP1/ENV2
//!   produce; the SLOT is the billable unit, never minutes).
//! - **CP1 lifecycle wiring**: [`lifecycle::LeaseLifecycle`] (mark-then-kill
//!   expiry + surfaced-not-green crash sweeps, legal matrix only) over the
//!   thin [`lifecycle::BoxProbe`] port — the runner adapts at the composition
//!   root, so this crate stays hermetic (no runner dep).
//!
//! Scope discipline: **no scheduler, no HTTP here.** This crate is the typed
//! seam; CP3 (scheduler) and API1+ (HTTP surface) live in their own crates and
//! consume these types.
//!
//! Wire-contract law: lease lifecycle states REUSE the frozen, transcribed
//! `corelink_runners_contracts::RunnerState` — this crate never redefines a
//! contract type.

pub mod billing;
pub mod caps;
pub mod ledger;
pub mod lifecycle;
pub mod meter;
pub mod tenant;

pub use billing::SlotMeter;
pub use caps::{CapDecision, CapGate, RateWindow};
pub use ledger::{FileLedger, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState};
pub use lifecycle::{BoxProbe, LeaseLifecycle};
pub use meter::{CogsCounters, SlotEventKind, SlotOccupancyEvent};
pub use tenant::{TenantId, TenantPlan};
