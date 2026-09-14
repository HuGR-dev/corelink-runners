//! WP-7 stub — AC pre-lease lookup (the memoized-exec short-circuit).
//!
//! Before `try_admit_with_compute` reserves a slot, the acquire path consults
//! the Action Cache: a 200 hit returns the stored `ActionResult` immediately
//! WITHOUT reserving a slot (A3b: 0 slots reserved, 0 vCPU-h accrued).  A
//! 404 miss falls through to the normal slot-reserve + box-spawn path (A4).
//!
//! This is the MINIMAL COMPILING STUB so the moat acceptance suite (WP-1) can
//! test A3b and A4 against the hook interface.  Real logic is WP-7.
//!
//! ## Stub flags (what was added here vs real logic)
//! - `AcPreLeaseOutcome` enum: real cases, no logic.
//! - `AcPreLeaseHook` trait: real signature, bodies `unimplemented!()`.
//! - `NoOpAcHook`: always returns `Miss` (the pass-through default).
//! - `AppState::ac_pre_lease_hook` field: `Arc<dyn AcPreLeaseHook>`, default `NoOpAcHook`.
//!
//! NOTE: `AppState` itself is in `app.rs` and is NOT modified here (adding a
//! field requires editing app.rs — see the companion stub field addition there).
//! This module exports the trait + the no-op + the outcome so tests can use
//! them directly.

use corelink_runner::cas_http::Blake3Key;

// ── AcPreLeaseOutcome ─────────────────────────────────────────────────────────

/// The three-way outcome of a pre-lease AC lookup (WP-7).
///
/// `Hit` ⇒ return the stored `ActionResult`, never reserve a slot (A3b).
/// `Miss` ⇒ fall through to the normal acquire path (A4).
/// `FailClosed` ⇒ the AC could not be consulted; fail-closed (A5 law).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcPreLeaseOutcome {
    /// AC returned 200 — stored `ActionResult` bytes.  No slot reserved.
    Hit(Vec<u8>),
    /// AC returned 404 — cold path, proceed with normal acquire.
    Miss,
    /// AC returned 401/403/5xx or transport error — fail-closed.
    FailClosed(String),
}

// ── AcPreLeaseHook ────────────────────────────────────────────────────────────

/// The AC pre-lease hook seam (WP-7).
///
/// Called at the TOP of `acquire`, BEFORE `try_admit_with_compute` reserves a
/// slot.  A `Hit` short-circuits the entire slot-reserve + box-spawn path (the
/// "never charge twice" invariant — A3b).
///
/// The real implementation (`CasHttpClient::get_ac`) is WP-7.  The moat
/// acceptance suite uses `MockAcHook` (always-hit or always-miss).
pub trait AcPreLeaseHook: Send + Sync {
    /// Consult the Action Cache for `action_digest` on behalf of `tenant`.
    ///
    /// Returns `Hit(result_bytes)` on 200, `Miss` on 404, `FailClosed` on
    /// 401/403/5xx/timeout (A5 law — status-class guard).
    fn lookup(
        &self,
        tenant: &str,
        action_digest: &Blake3Key,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AcPreLeaseOutcome> + Send + '_>>;
}

// ── NoOpAcHook (always Miss — the pass-through default) ──────────────────────

/// The default no-op AC hook: always returns `Miss` so the acquire path
/// behaves exactly as before WP-7 is wired (zero behavior change on the cold
/// path or when no AC endpoint is configured).
#[derive(Debug, Clone, Default)]
pub struct NoOpAcHook;

impl AcPreLeaseHook for NoOpAcHook {
    fn lookup(
        &self,
        _tenant: &str,
        _action_digest: &Blake3Key,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AcPreLeaseOutcome> + Send + '_>> {
        // Always miss: the acquire path falls through to the normal slot-reserve
        // path unchanged.  WP-7 will replace this with a real CAS AC HTTP call.
        Box::pin(async { AcPreLeaseOutcome::Miss })
    }
}

// ── MockAcHook (deterministic for tests) ─────────────────────────────────────

/// A deterministic AC hook for acceptance tests: always returns the configured
/// `outcome` regardless of tenant or digest.
#[derive(Debug, Clone)]
pub struct MockAcHook {
    pub outcome: AcPreLeaseOutcome,
}

impl MockAcHook {
    /// Always-hit: returns `Hit(result_bytes)`.
    pub fn always_hit(result_bytes: Vec<u8>) -> Self {
        Self {
            outcome: AcPreLeaseOutcome::Hit(result_bytes),
        }
    }

    /// Always-miss: same as `NoOpAcHook` but explicit.
    pub fn always_miss() -> Self {
        Self {
            outcome: AcPreLeaseOutcome::Miss,
        }
    }

    /// Always-fail-closed: simulates an unreachable AC.
    pub fn always_fail_closed(reason: impl Into<String>) -> Self {
        Self {
            outcome: AcPreLeaseOutcome::FailClosed(reason.into()),
        }
    }
}

impl AcPreLeaseHook for MockAcHook {
    fn lookup(
        &self,
        _tenant: &str,
        _action_digest: &Blake3Key,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AcPreLeaseOutcome> + Send + '_>> {
        let outcome = self.outcome.clone();
        Box::pin(async move { outcome })
    }
}
