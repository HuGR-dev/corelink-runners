//! WP-3 — D-9 per-job CAS PAT mint + revoke client.
//!
//! Mirrors [`crate::runner_broker`] (`RunnerRegistrationBroker` / `MockBroker`)
//! in structure. The D-9 service mints a short-lived, per-job PAT scoped to the
//! CAS/AC for one lease, and revokes it on teardown (idempotent).
//!
//! ## Mint flow (POST /internal/v1/runner/mint)
//!
//! Request header: `x-corelink-internal-auth: <internal-token>`.
//! Body: `{"owner_tenant": "<tenant>", "job_id": "<job_id>", "scope": "read-write"}`.
//! Response: `{"token": "<pat-plaintext>", "pat_id": "<id>", "expires_ms": <u64>}`.
//!
//! ## Revoke flow (POST /internal/v1/runner/revoke)
//!
//! Body: `{"pat_id": "<id>"}`. Idempotent — may be called on Expired + Crashed
//! teardown paths, not only Released (A7b). Failure modes: see [`MintError`].
//!
//! ## Fail-closed law
//!
//! Every failure — transport error, non-2xx, malformed body, missing field —
//! maps to `Err(MintError::…)`. The mint client NEVER returns a partial or empty
//! [`MintedPat`]. Mint failure fails closed (no config-less box — A7).
//!
//! ## TTL bound (A7b)
//!
//! The minted PAT's `expires_ms` MUST NOT exceed the lease deadline:
//! `expires_ms ≤ lease.expiry` (enforced by the D-9 service; the client
//! asserts this in the real impl).
//!
//! ## Secret hygiene
//!
//! [`MintedPat::token`] is a sensitive credential. It is the per-job PAT
//! carried into the box env via [`crate::runner_cas_mint`]'s inject seam — never
//! logged. `Debug` for types carrying `token` redacts its value.

// ── MintedPat ────────────────────────────────────────────────────────────────

/// A successfully minted per-job CAS PAT.
///
/// `token` is the plaintext credential injected into the box env as
/// `CLW_TOKEN`. It is a SENSITIVE short-lived secret: `Debug` is
/// hand-written to REDACT the value so it can never reach a log line.
///
/// `expires_ms` is a unix-millisecond timestamp; it MUST be ≤ the lease
/// deadline (A7b). `pat_id` is the opaque identifier used for revoke.
#[derive(Clone, PartialEq, Eq)]
pub struct MintedPat {
    /// Plaintext per-job PAT (SENSITIVE — redacted in `Debug`).
    pub token: String,
    /// Opaque PAT identifier used by the revoke endpoint.
    pub pat_id: String,
    /// Unix-millisecond expiry; MUST be ≤ the lease deadline (A7b).
    pub expires_ms: u64,
}

/// Redacting `Debug` — the token must never appear in `{:?}` output, logs,
/// panic messages, or structured traces. Mirrors [`crate::runner_broker::JitRunnerConfig`].
impl std::fmt::Debug for MintedPat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintedPat")
            .field("token", &"***REDACTED***")
            .field("pat_id", &self.pat_id)
            .field("expires_ms", &self.expires_ms)
            .finish()
    }
}

// ── MintError ────────────────────────────────────────────────────────────────

/// Every mint/revoke failure mode. No variant carries the PAT token bytes —
/// the redacting `Debug`/`Display` are part of the secret-hygiene contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintError {
    /// The transport could not reach the D-9 service (DNS, TLS, connect, timeout).
    Unreachable,
    /// The D-9 service authoritatively rejected the internal auth (401/403).
    Unauthorized,
    /// A non-2xx status that is neither auth rejection nor reachability.
    BadStatus {
        /// HTTP status code returned.
        status: u16,
    },
    /// A 2xx body that did not carry the expected fields or was unparseable.
    /// Fail-closed: a malformed authoritative response is an error, never an
    /// empty/partial [`MintedPat`].
    BadResponse,
    /// The minted PAT's `expires_ms` exceeds the lease deadline (A7b violation).
    TtlExceedsLease {
        /// The PAT expiry returned by the service (unix ms).
        expires_ms: u64,
        /// The lease deadline it must not exceed (unix ms).
        lease_deadline_ms: u64,
    },
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "D-9 service unreachable"),
            Self::Unauthorized => write!(f, "D-9 service rejected the internal auth (401/403)"),
            Self::BadStatus { status } => write!(f, "D-9 service returned status {status}"),
            Self::BadResponse => write!(f, "D-9 service returned a malformed/incomplete body"),
            Self::TtlExceedsLease {
                expires_ms,
                lease_deadline_ms,
            } => write!(
                f,
                "minted PAT expires_ms {expires_ms} exceeds lease deadline {lease_deadline_ms} \
                 (A7b violation — fail CLOSED)"
            ),
        }
    }
}

impl std::error::Error for MintError {}

// ── CasPatMint trait ─────────────────────────────────────────────────────────

/// The D-9 mint + revoke client trait.
///
/// Mirrors [`crate::runner_broker::RunnerRegistrationBroker`] in structure:
/// `async` via native `impl Future` (no `async_trait`), pinned + boxed for
/// object safety.
///
/// Mint failure fails closed (A7): no box is admitted without a minted PAT.
/// Revoke is idempotent and fires on EVERY terminal path (A7b).
pub trait CasPatMint: Send + Sync {
    /// Mint a per-job CAS PAT for `owner_tenant`/`job_id`.
    ///
    /// POSTs to `/internal/v1/runner/mint` with `x-corelink-internal-auth`.
    /// Returns [`MintedPat`] on success; fails closed on any error.
    ///
    /// `lease_deadline_ms` is the lease expiry (unix ms); the impl asserts
    /// `minted.expires_ms ≤ lease_deadline_ms` (A7b).
    fn mint<'a>(
        &'a self,
        owner_tenant: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    >;

    /// Revoke a previously minted per-job PAT.
    ///
    /// POSTs to `/internal/v1/runner/revoke`. Idempotent — may be called
    /// on Expired + Crashed teardown (A7b), not only Released.
    ///
    /// A revoke failure is logged but does not fail the teardown path (the
    /// PAT is short-lived and self-expires; revoke is defense-in-depth).
    fn revoke<'a>(
        &'a self,
        pat_id: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>,
    >;
}

// ── MockMint ─────────────────────────────────────────────────────────────────

/// A deterministic, no-network mint client for lifecycle tests.
///
/// Mirrors [`crate::runner_broker::MockBroker`]: returns a fixed/derived
/// [`MintedPat`] so the moat suite (WP-1) can test mint/inject/revoke without
/// a real D-9 service. NEVER used in production.
#[derive(Debug, Clone, Default)]
pub struct MockMint;

impl MockMint {
    /// Construct the mock mint client.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// The deterministic PAT token this mock derives for a `(tenant, job_id)`.
    ///
    /// Exposed so tests can assert the exact value without reaching into the
    /// redacted [`MintedPat`].
    #[must_use]
    pub fn derived_token(owner_tenant: &str, job_id: &str) -> String {
        format!("mock-pat::{owner_tenant}::{job_id}")
    }

    /// The deterministic `pat_id` this mock derives for a `(tenant, job_id)`.
    #[must_use]
    pub fn derived_pat_id(owner_tenant: &str, job_id: &str) -> String {
        format!("mock-patid::{owner_tenant}::{job_id}")
    }

    /// A fixed `expires_ms` the mock always returns (year 2100, well beyond
    /// any real lease, so TTL-bound tests must supply a custom mock).
    pub const MOCK_EXPIRES_MS: u64 = 4_102_444_800_000; // 2100-01-01T00:00:00Z
}

impl CasPatMint for MockMint {
    fn mint<'a>(
        &'a self,
        owner_tenant: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        let token = Self::derived_token(owner_tenant, job_id);
        let pat_id = Self::derived_pat_id(owner_tenant, job_id);
        // Clamp to lease_deadline_ms so the mock always satisfies A7b.
        let expires_ms = Self::MOCK_EXPIRES_MS.min(lease_deadline_ms);
        Box::pin(async move {
            Ok(MintedPat {
                token,
                pat_id,
                expires_ms,
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        _pat_id: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>,
    > {
        Box::pin(async move { Ok(()) })
    }
}
