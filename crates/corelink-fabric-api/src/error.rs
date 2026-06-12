//! The FROZEN error vocabulary (CF0 freeze item 3,
//! `docs/plan/m1-decomposition-draft.md` §3: "401/403-vs-404/429/503
//! semantics, fail-closed defaults").
//!
//! Every variant carries a stable machine `code` and an HTTP status; the
//! pairing is pinned by `api_error_vocabulary_is_frozen`. Adding a variant
//! is an API event; changing a status or code is a breaking one.

use serde::{Deserialize, Serialize};

/// The frozen API error vocabulary.
///
/// No `Forbidden`/403 variant exists ON PURPOSE — see [`ApiError::NotFound`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// 401 — missing or invalid Bearer PAT (API1: `missing_pat_is_401`).
    /// The request never reaches tenant resolution.
    Unauthorized,

    /// 404 — the resource does not exist *for this tenant*. This INCLUDES
    /// cross-tenant access: a valid PAT touching another tenant's lease is
    /// 404, NEVER 403 — a 403 would confirm the lease exists and leak
    /// tenancy; there is no existence oracle (API1:
    /// `cross_tenant_pat_cannot_touch_other_lease_404_not_403`).
    NotFound,

    /// 429 — per-tenant concurrency cap or rate ceiling hit. Admission is
    /// PREVENTIVE: rejected at acquire time, before any box/VM is spawned
    /// (contract §6; CP2 "enforced before load", hugit X10⑤).
    OverCap,

    /// 503 — a fail-closed dependency (token store, lease ledger, CAS/AC)
    /// is unreachable. The fabric NEVER fails open: no anonymous admission,
    /// no uncapped admission, no silent cold result — an explicit 503
    /// instead (API1: `token_store_down_fails_closed_503_never_open`;
    /// contract §2 cache-down fail-closed).
    FailClosed,

    /// 400 — the request is structurally valid JSON but semantically
    /// rejected before any box contact: e.g. an unpinned (non-`sha256:`)
    /// image digest (API2:
    /// `acquire_unpinned_image_rejected_400_before_box_contact`).
    Invalid,
}

impl ApiError {
    /// The HTTP status code for this error. Frozen.
    pub fn http_status(&self) -> u16 {
        match self {
            ApiError::Unauthorized => 401,
            ApiError::NotFound => 404,
            ApiError::OverCap => 429,
            ApiError::FailClosed => 503,
            ApiError::Invalid => 400,
        }
    }

    /// The stable machine code for this error — what clients switch on
    /// (HTTP statuses are shared with proxies; codes are ours). Frozen.
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::Unauthorized => "unauthorized",
            ApiError::NotFound => "not_found",
            ApiError::OverCap => "over_cap",
            ApiError::FailClosed => "fail_closed",
            ApiError::Invalid => "invalid",
        }
    }

    /// Build the wire body for this error with a human-readable message.
    /// The `code` is always the frozen machine code, never free text.
    pub fn body(&self, message: impl Into<String>) -> ErrorBody {
        ErrorBody {
            code: self.code().to_string(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.code(), self.http_status())
    }
}

impl std::error::Error for ApiError {}

/// The wire shape of every non-2xx response body.
///
/// `code` is one of the frozen machine codes ([`ApiError::code`]);
/// `message` is human-readable and carries NO contract (clients must never
/// parse it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// Frozen machine code (e.g. `"not_found"`, `"fail_closed"`).
    pub code: String,

    /// Human-readable detail. Informational only; no machine contract.
    pub message: String,
}
