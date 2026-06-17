//! WP-6 stub — `clw` drive seam: snapshot / run / write-back (A8).
//!
//! The runner invokes `clw snapshot → hydrate → run` on the box.  The `clw`
//! child's exit code passes through transparently (exit-code transparency).
//! A non-zero exit code must NOT be cached (the AC write-back is suppressed).
//! `exit 2` from `clw` itself is distinct from a child's `exit 2`.
//!
//! This is the MINIMAL COMPILING STUB for the moat acceptance suite (WP-1) to
//! test A8 against the `ClwDrive` trait interface.  Real logic is WP-6.
//!
//! ## Stub flags
//! - `ClwDriveOutcome` enum: real cases.
//! - `ClwExitTransparency`: enum distinguishing clw-internal vs child exit.
//! - `ClwDrive` trait: real signature, bodies `unimplemented!()`.
//! - `MockClwDrive`: deterministic test double.

// ── ClwExitTransparency ───────────────────────────────────────────────────────

/// Whether an exit code is the child's or `clw`-internal (A8 invariant).
///
/// `exit 2` from `clw` itself (e.g. bad CLI args, substrate unreachable) is
/// distinct from the child job exiting with code 2.  The runner must surface
/// both correctly to the outer GitHub Actions runtime and to the billing layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClwExitTransparency {
    /// The child process exited with this code (pass-through).
    Child(i32),
    /// `clw` itself exited with this code (internal error, not the child).
    ClwInternal(i32),
}

impl ClwExitTransparency {
    /// `true` iff this is a successful (0) child exit.
    pub fn is_child_success(&self) -> bool {
        matches!(self, Self::Child(0))
    }

    /// `true` iff the run succeeded and the result may be cached (A8).
    /// Non-zero child exit and ANY clw-internal exit are not cached.
    pub fn is_cacheable(&self) -> bool {
        self.is_child_success()
    }
}

// ── ClwDriveOutcome ───────────────────────────────────────────────────────────

/// The outcome of a `clw` snapshot/hydrate/run drive (WP-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClwDriveOutcome {
    /// The child ran and exited (code is transparent — may be non-zero).
    Ran {
        exit: ClwExitTransparency,
        /// Whether the result was written back to the AC (A8: only on exit 0).
        wrote_back: bool,
    },
    /// `clw` itself failed before the child ran (e.g. substrate down).
    ClwFailed {
        clw_exit_code: i32,
        reason: String,
    },
}

impl ClwDriveOutcome {
    /// The child's exit code (if the child ran).
    pub fn child_exit_code(&self) -> Option<i32> {
        match self {
            Self::Ran {
                exit: ClwExitTransparency::Child(code),
                ..
            } => Some(*code),
            _ => None,
        }
    }
}

// ── ClwDrive trait ────────────────────────────────────────────────────────────

/// The `clw` drive seam (WP-6): drive `clw snapshot → hydrate → run` and
/// capture the outcome.
///
/// The real implementation calls `clw` on the box via [`BoxExec`](corelink_runner::lease::BoxExec).
/// The acceptance suite uses `MockClwDrive`.
pub trait ClwDrive: Send + Sync {
    /// Drive `clw` for the given `lease_id`.
    ///
    /// Exit transparency: whatever the child exits with is passed through in
    /// `ClwDriveOutcome::Ran { exit: ClwExitTransparency::Child(code) }`.
    /// `clw`-internal errors surface as
    /// `ClwDriveOutcome::ClwFailed { clw_exit_code: 2, .. }`.
    ///
    /// Write-back: the AC entry is ONLY stored when
    /// `exit: ClwExitTransparency::Child(0)` — a non-zero child exit MUST NOT
    /// be cached (A8 invariant).
    ///
    /// # Errors
    /// Only transport/spawn failures (e.g. `clw` binary missing). A non-zero
    /// exit is `Ok(ClwDriveOutcome::Ran { .. })`, never `Err`.
    fn drive(
        &self,
        lease_id: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<ClwDriveOutcome>> + Send + '_>,
    >;
}

// ── MockClwDrive ──────────────────────────────────────────────────────────────

/// Deterministic `clw` driver for acceptance tests (A8).
///
/// Returns a fixed `ClwDriveOutcome` without spawning any process.
#[derive(Debug, Clone)]
pub struct MockClwDrive {
    pub outcome: ClwDriveOutcome,
}

impl MockClwDrive {
    /// Simulate a successful child run (exit 0, written back to AC).
    pub fn success_with_write_back() -> Self {
        Self {
            outcome: ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(0),
                wrote_back: true,
            },
        }
    }

    /// Simulate a non-zero child exit (exit `code`; NOT written back).
    pub fn child_nonzero(code: i32) -> Self {
        assert_ne!(code, 0, "use success_with_write_back() for exit 0");
        Self {
            outcome: ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(code),
                wrote_back: false,
            },
        }
    }

    /// Simulate a `clw`-internal failure (exit 2 from `clw` itself, not the
    /// child).
    pub fn clw_internal_error() -> Self {
        Self {
            outcome: ClwDriveOutcome::ClwFailed {
                clw_exit_code: 2,
                reason: "clw: substrate unreachable".to_string(),
            },
        }
    }
}

impl ClwDrive for MockClwDrive {
    fn drive(
        &self,
        _lease_id: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<ClwDriveOutcome>> + Send + '_>,
    > {
        let outcome = self.outcome.clone();
        Box::pin(async move { Ok(outcome) })
    }
}
