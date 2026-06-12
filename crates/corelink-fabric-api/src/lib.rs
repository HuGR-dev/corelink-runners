//! Frozen API vocabulary for the M1 fabric (CF0 freeze item 3). Types only:
//! DTOs, endpoint paths, error vocabulary. The HTTP server lives elsewhere;
//! everything here is wire shape.
//!
//! This crate is the vocabulary that API1..API4, CP2 and ENV1 all build
//! against (`docs/plan/m1-decomposition-draft.md` §3, freeze item 3:
//! "API DTOs + endpoint paths + error vocabulary (401/403-vs-404/429/503
//! semantics, fail-closed defaults) — API1..4, CP2, ENV1 all cite it").
//!
//! Deliberately dependency-light: `serde` for the wire shapes plus the
//! in-workspace `corelink-runners-contracts` crate for the frozen wire types
//! (`RunnerLease`, `RunnerState`, `CheckDef`, `CheckResult`). No server, no
//! axum, no tokio.

pub mod dto;
pub mod error;
pub mod paths;

pub use dto::{
    AcquireRequest, AcquireResponse, AttestationKeyResponse, CancelResponse, CloseRequest,
    CloseResponse, ExecRequest, ExecResponse, StatusResponse, TriggerRequest, TriggerResponse,
};
pub use error::{ApiError, ErrorBody};
