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
//! ## Components
//! - `ClwDriveOutcome` enum: real cases.
//! - `ClwExitTransparency`: enum distinguishing clw-internal vs child exit.
//! - `ClwDrive` trait: real signature.
//! - `MockClwDrive`: deterministic trait-level test double.
//! - `ClwBoxDrive`: the real [`BoxExec`](corelink_runner::lease::BoxExec)-backed
//!   driver (WP-6) — runs `clw snapshot → hydrate → run` on the box.
//! - `MockBoxExec`: a `BoxExec` test double returning programmed `CmdOutput`s.

use corelink_runner::lease::{BoxExec, CmdOutput};

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
    ClwFailed { clw_exit_code: i32, reason: String },
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

// ── ClwRunSpec ────────────────────────────────────────────────────────────────

/// The parameters a [`ClwBoxDrive`] needs beyond the `lease_id` to drive
/// `clw snapshot → hydrate → run` on the box (WP-6).
///
/// At flip-time these are populated from the live lease / job spec; for now the
/// struct just carries them so the drive is constructible (and testable).
#[derive(Debug, Clone)]
pub struct ClwRunSpec {
    /// `--name` for `clw snapshot` / `clw hydrate` (the snapshot identifier).
    pub snapshot_name: String,
    /// The `PATH` argument to `clw snapshot` (what to snapshot).
    pub snapshot_path: String,
    /// The `DEST` argument to `clw hydrate` (where to materialize).
    pub hydrate_dest: String,
    /// The wrapped command + args run under `clw run -- <command…>`.
    pub command: Vec<String>,
}

// ── ClwBoxDrive ───────────────────────────────────────────────────────────────

/// The real [`BoxExec`](corelink_runner::lease::BoxExec)-backed `clw` driver
/// (WP-6).
///
/// Drives the frozen `clw` CLI sequence on the box:
///   1. `clw snapshot <path> --name <name>`
///   2. `clw hydrate <dest> --name <name>`
///   3. `clw run -- <command…>`
///
/// Exit-code interpretation follows the FROZEN `clw` CLI contract (§4):
/// - **snapshot / hydrate** never produce a child verdict — any non-`Some(0)`
///   exit is a `clw`-internal error ⇒ [`ClwDriveOutcome::ClwFailed`].
/// - **run** propagates the wrapped command's exit code, EXCEPT exit `2`, which
///   is reserved for `clw` itself (ALWAYS AND ONLY a `clw`-internal error):
///     - `Some(2)`            ⇒ `ClwFailed { clw_exit_code: 2, .. }`.
///     - `Some(n)`, `n != 2`  ⇒ `Ran { exit: Child(n), wrote_back: n == 0 }`.
///     - `None` (the `clw` PROCESS itself killed by signal at the transport —
///       distinct from a signal-killed CHILD, which `clw` reports as `128+sig`
///       i.e. `Some(n)`) ⇒ `ClwFailed { clw_exit_code: -1, .. }`.
///
/// `wrote_back` is a REPORT, not an action: `clw` owns its own caching, so the
/// drive performs NO AC/CAS PUT. `wrote_back == (run exit == Child(0))`.
///
/// Only a `BoxExec::run` `Err` (spawn/transport failure) propagates as `Err`; a
/// non-zero `clw` OR child exit is always `Ok(outcome)`.
pub struct ClwBoxDrive<B: BoxExec> {
    boxx: B,
    run: ClwRunSpec,
}

impl<B: BoxExec> ClwBoxDrive<B> {
    /// Construct a drive over `boxx` with the given `run` spec.
    pub fn new(boxx: B, run: ClwRunSpec) -> Self {
        Self { boxx, run }
    }

    /// Derive a `ClwFailed` reason from a `clw` step's stderr, falling back to a
    /// step-named default when stderr is empty.
    fn clw_failed(step: &str, out: &CmdOutput) -> ClwDriveOutcome {
        let stderr = out.stderr.trim();
        let reason = if stderr.is_empty() {
            format!("clw {step} failed (exit {:?})", out.code)
        } else {
            stderr.to_string()
        };
        ClwDriveOutcome::ClwFailed {
            clw_exit_code: out.code.unwrap_or(-1),
            reason,
        }
    }

    /// Run the §4 sequence synchronously (the `BoxExec` seam is blocking).
    fn drive_blocking(&self) -> anyhow::Result<ClwDriveOutcome> {
        // 1. clw snapshot <path> --name <name> — snapshot never yields a child
        //    verdict; any non-Some(0) exit is a clw-internal error.
        let snap = self.boxx.run(&[
            "clw",
            "snapshot",
            &self.run.snapshot_path,
            "--name",
            &self.run.snapshot_name,
        ])?;
        if snap.code != Some(0) {
            return Ok(Self::clw_failed("snapshot", &snap));
        }

        // 2. clw hydrate <dest> --name <name> — same: clw-internal on non-zero.
        let hyd = self.boxx.run(&[
            "clw",
            "hydrate",
            &self.run.hydrate_dest,
            "--name",
            &self.run.snapshot_name,
        ])?;
        if hyd.code != Some(0) {
            return Ok(Self::clw_failed("hydrate", &hyd));
        }

        // 3. clw run -- <command…> — interpret per THE RULE.
        let mut argv: Vec<&str> = vec!["clw", "run", "--"];
        argv.extend(self.run.command.iter().map(String::as_str));
        let run = self.boxx.run(&argv)?;
        Ok(match run.code {
            // exit 2 is reserved for clw itself (ALWAYS AND ONLY a clw error).
            Some(2) => {
                let stderr = run.stderr.trim();
                let reason = if stderr.is_empty() {
                    "clw run: internal error (exit 2)".to_string()
                } else {
                    stderr.to_string()
                };
                ClwDriveOutcome::ClwFailed {
                    clw_exit_code: 2,
                    reason,
                }
            }
            // any other code is the child's verdict (cacheable iff 0).
            Some(n) => ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(n),
                wrote_back: n == 0,
            },
            // the clw PROCESS itself was killed by a signal at the transport
            // (distinct from a signal-killed child, which clw reports as 128+sig).
            None => ClwDriveOutcome::ClwFailed {
                clw_exit_code: -1,
                reason: "clw process killed by signal".to_string(),
            },
        })
    }
}

impl<B: BoxExec + Send + Sync> ClwDrive for ClwBoxDrive<B> {
    fn drive(
        &self,
        _lease_id: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<ClwDriveOutcome>> + Send + '_>,
    > {
        Box::pin(async move { self.drive_blocking() })
    }
}

// ── MockBoxExec ───────────────────────────────────────────────────────────────

/// A [`BoxExec`](corelink_runner::lease::BoxExec) test double returning
/// programmed [`CmdOutput`]s, keyed by the `clw` verb (`argv[1]`).
///
/// `snapshot` and `hydrate` succeed (exit 0) by default; `run` returns the
/// test-specified `CmdOutput`. Override `snapshot_out` / `hydrate_out` to drive
/// the snapshot/hydrate failure arms.
///
/// This is a `pub` double (not `#[cfg(test)]`-gated) so the cross-crate
/// `acceptance_moat` integration suite can construct it.
#[derive(Debug, Clone)]
pub struct MockBoxExec {
    /// Programmed output for `clw snapshot …` (default: exit 0).
    pub snapshot_out: CmdOutput,
    /// Programmed output for `clw hydrate …` (default: exit 0).
    pub hydrate_out: CmdOutput,
    /// Programmed output for `clw run -- …` (the verb under test).
    pub run_out: CmdOutput,
}

impl MockBoxExec {
    fn ok_out() -> CmdOutput {
        CmdOutput {
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// A mock whose `snapshot`/`hydrate` succeed and whose `run` returns
    /// `run_out` (the common a8 case).
    pub fn with_run(run_out: CmdOutput) -> Self {
        Self {
            snapshot_out: Self::ok_out(),
            hydrate_out: Self::ok_out(),
            run_out,
        }
    }

    /// A mock whose `run` exits with `code` (and empty stdout/stderr).
    pub fn with_run_code(code: i32) -> Self {
        Self::with_run(CmdOutput {
            code: Some(code),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

impl BoxExec for MockBoxExec {
    fn run(&self, argv: &[&str]) -> anyhow::Result<CmdOutput> {
        // argv[0] == "clw"; argv[1] is the verb.
        match argv.get(1).copied() {
            Some("snapshot") => Ok(self.snapshot_out.clone()),
            Some("hydrate") => Ok(self.hydrate_out.clone()),
            Some("run") => Ok(self.run_out.clone()),
            other => anyhow::bail!("MockBoxExec: unexpected clw verb: {other:?}"),
        }
    }
}

// ── Policy unit tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{ClwDriveOutcome, ClwExitTransparency};

    // ── ClwExitTransparency::is_cacheable ─────────────────────────────────────

    #[test]
    fn child_exit_0_is_cacheable() {
        assert!(ClwExitTransparency::Child(0).is_cacheable());
    }

    #[test]
    fn child_exit_1_is_not_cacheable() {
        assert!(!ClwExitTransparency::Child(1).is_cacheable());
    }

    #[test]
    fn child_exit_2_is_not_cacheable() {
        assert!(!ClwExitTransparency::Child(2).is_cacheable());
    }

    #[test]
    fn clw_internal_exit_0_is_not_cacheable() {
        assert!(!ClwExitTransparency::ClwInternal(0).is_cacheable());
    }

    #[test]
    fn clw_internal_exit_2_is_not_cacheable() {
        assert!(!ClwExitTransparency::ClwInternal(2).is_cacheable());
    }

    // ── ClwExitTransparency::is_child_success ─────────────────────────────────

    #[test]
    fn child_exit_0_is_child_success() {
        assert!(ClwExitTransparency::Child(0).is_child_success());
    }

    #[test]
    fn child_exit_1_is_not_child_success() {
        assert!(!ClwExitTransparency::Child(1).is_child_success());
    }

    #[test]
    fn clw_internal_exit_0_is_not_child_success() {
        assert!(!ClwExitTransparency::ClwInternal(0).is_child_success());
    }

    // ── ClwDriveOutcome::child_exit_code ─────────────────────────────────────

    #[test]
    fn ran_child_7_child_exit_code_is_some_7() {
        let outcome = ClwDriveOutcome::Ran {
            exit: ClwExitTransparency::Child(7),
            wrote_back: true,
        };
        assert_eq!(outcome.child_exit_code(), Some(7));
    }

    #[test]
    fn clw_failed_child_exit_code_is_none() {
        let outcome = ClwDriveOutcome::ClwFailed {
            clw_exit_code: 2,
            reason: "clw: substrate unreachable".to_string(),
        };
        assert_eq!(outcome.child_exit_code(), None);
    }

    // ── ClwBoxDrive (real BoxExec-backed) exit-rule mapping ──────────────────

    use super::{BoxExec, ClwBoxDrive, ClwDrive, ClwRunSpec, CmdOutput, MockBoxExec};

    fn spec() -> ClwRunSpec {
        ClwRunSpec {
            snapshot_name: "snap-1".to_string(),
            snapshot_path: "/work".to_string(),
            hydrate_dest: "/work".to_string(),
            command: vec!["cargo".to_string(), "test".to_string()],
        }
    }

    async fn drive_with(boxx: MockBoxExec) -> ClwDriveOutcome {
        ClwBoxDrive::new(boxx, spec())
            .drive("lease-x")
            .await
            .expect("transport must not error")
    }

    #[tokio::test]
    async fn drive_run_exit_0_is_child_0_wrote_back() {
        let outcome = drive_with(MockBoxExec::with_run_code(0)).await;
        assert_eq!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(0),
                wrote_back: true,
            }
        );
    }

    #[tokio::test]
    async fn drive_run_nonzero_is_child_not_wrote_back() {
        let outcome = drive_with(MockBoxExec::with_run_code(42)).await;
        assert_eq!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(42),
                wrote_back: false,
            }
        );
    }

    #[tokio::test]
    async fn drive_run_exit_2_is_clw_failed_not_child() {
        let outcome = drive_with(MockBoxExec::with_run_code(2)).await;
        assert!(
            matches!(
                outcome,
                ClwDriveOutcome::ClwFailed {
                    clw_exit_code: 2,
                    ..
                }
            ),
            "run exit 2 is reserved for clw itself ⇒ ClwFailed, never Child(2); got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_run_signal_killed_process_is_clw_failed_minus_one() {
        // code == None ⇒ the clw PROCESS itself was killed by a signal at the
        // transport (distinct from a signal-killed CHILD, which clw reports as
        // 128+sig i.e. Some(n)).
        let boxx = MockBoxExec::with_run(CmdOutput {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
        });
        let outcome = drive_with(boxx).await;
        assert!(
            matches!(
                outcome,
                ClwDriveOutcome::ClwFailed {
                    clw_exit_code: -1,
                    ..
                }
            ),
            "a signal-killed clw process ⇒ ClwFailed{{-1}}; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_snapshot_failure_is_clw_failed_never_child() {
        // snapshot exit != 0 ⇒ ClwFailed (snapshot never yields a child verdict).
        let boxx = MockBoxExec {
            snapshot_out: CmdOutput {
                code: Some(2),
                stdout: String::new(),
                stderr: "clw: cannot snapshot /work".to_string(),
            },
            hydrate_out: CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            },
            // run would succeed, but snapshot fails first so run never executes.
            run_out: CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        let outcome = drive_with(boxx).await;
        assert!(
            matches!(
                &outcome,
                ClwDriveOutcome::ClwFailed { clw_exit_code: 2, reason }
                    if reason.contains("cannot snapshot")
            ),
            "snapshot failure ⇒ ClwFailed with stderr-derived reason; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_hydrate_failure_is_clw_failed_never_child() {
        // hydrate exit != 0 ⇒ ClwFailed (after a successful snapshot).
        let boxx = MockBoxExec {
            snapshot_out: CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            },
            hydrate_out: CmdOutput {
                code: Some(2),
                stdout: String::new(),
                stderr: "clw: hydrate target busy".to_string(),
            },
            run_out: CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        let outcome = drive_with(boxx).await;
        assert!(
            matches!(
                &outcome,
                ClwDriveOutcome::ClwFailed { clw_exit_code: 2, reason }
                    if reason.contains("hydrate target busy")
            ),
            "hydrate failure ⇒ ClwFailed with stderr-derived reason; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_transport_err_propagates_as_err() {
        // A BoxExec spawn/transport Err must propagate as Err (NOT Ok(outcome)).
        struct FailingBox;
        impl BoxExec for FailingBox {
            fn run(&self, _argv: &[&str]) -> anyhow::Result<CmdOutput> {
                anyhow::bail!("ssh: connection refused")
            }
        }
        let result = ClwBoxDrive::new(FailingBox, spec()).drive("lease-x").await;
        assert!(
            result.is_err(),
            "a transport Err must propagate as Err, not Ok(outcome)"
        );
    }
}
