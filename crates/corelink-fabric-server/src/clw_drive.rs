//! WP-6 stub — `clw` drive seam: snapshot / run / write-back (A8).
//!
//! The runner invokes `clw snapshot → hydrate → run` on the box.  The `clw`
//! child's exit code passes through transparently (exit-code transparency).
//! A non-zero exit code must NOT be cached (the AC write-back is suppressed).
//! The wrapper and child may both return `125`; the execution receipt distinguishes
//! those outcomes because the numeric value alone is ambiguous.
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

/// The reserved `clw`-internal exit code, frozen by the `clw` v0.1.1 CLI
/// contract (`clw-releases` `CLI-CONTRACT.md` / `CLI-SURFACE-EXIT-FREEZE.md`):
/// a `clw`-internal failure where the child NEVER ran exits **`125`**. A child
/// may also exit 125; the driver distinguishes those outcomes with the receipt,
/// not the numeric value alone.
pub const CLW_INTERNAL_EXIT_CODE: i32 = 125;

// ── ClwExitTransparency ───────────────────────────────────────────────────────

/// Whether an exit code is the child's or `clw`-internal (A8 invariant).
///
/// The wrapper and child may both produce numeric exit 125. The driver reports a
/// child verdict only when the receipt proves the child started; absent or
/// ambiguous receipt state is a wrapper failure.
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
    /// `ClwDriveOutcome::ClwFailed { clw_exit_code: 125, .. }`.
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

    /// Simulate a `clw`-internal failure (the reserved exit `125` from `clw`
    /// itself, not the child).
    pub fn clw_internal_error() -> Self {
        Self {
            outcome: ClwDriveOutcome::ClwFailed {
                clw_exit_code: CLW_INTERNAL_EXIT_CODE,
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
/// Exit interpretation follows the additive `clw` execution-receipt contract:
/// - **snapshot / hydrate** never produce a child verdict — any non-`Some(0)`
///   exit is a `clw`-internal error ⇒ [`ClwDriveOutcome::ClwFailed`].
/// - **run** distinguishes the wrapped verdict from internal failure using the receipt:
///     - `Some(125)` + `EXECUTED` ⇒ `Ran { exit: Child(125), wrote_back: false }`.
///     - `Some(125)` + `NOT_STARTED` ⇒ `ClwFailed { clw_exit_code: 125, .. }`.
///     - `Some(125)` + unknown state ⇒ `ClwFailed` (fail closed).
///     - `Some(n)`, `n != 125` ⇒ `Ran { exit: Child(n), wrote_back: n == 0 }`.
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

        // Keep execution proof out of child stdout/stderr. A separate BoxExec call
        // reads the private receipt after `clw run` returns, so child text cannot
        // forge a status line.
        let state_dir_out = self
            .boxx
            .run(&["mktemp", "-d", "/tmp/corelink-run-state.XXXXXX"])?;
        let state_dir = state_dir_out.stdout.trim();
        let valid_state_dir = state_dir_out.code == Some(0)
            && state_dir.starts_with("/tmp/corelink-run-state.")
            && state_dir
                .strip_prefix("/tmp/corelink-run-state.")
                .is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                });
        let state_path = valid_state_dir.then(|| format!("{state_dir}/state"));
        let state_env = state_path
            .as_ref()
            .map(|path| format!("CLW_RUN_STATE_FILE={path}"));
        let mut argv: Vec<&str> = Vec::new();
        if let Some(value) = state_env.as_deref() {
            argv.extend(["env", value]);
        }
        argv.extend(["clw", "run", "--"]);
        argv.extend(self.run.command.iter().map(String::as_str));
        let run_result = self.boxx.run(&argv);
        let receipt = state_path.as_deref().and_then(|path| {
            let out = self.boxx.run(&["cat", path]).ok()?;
            (out.code == Some(0)).then(|| out.stdout)
        });
        if let Some(path) = state_path.as_deref() {
            let _ = self.boxx.run(&["rm", "-f", "--", path]);
            let _ = self.boxx.run(&["rmdir", "--", state_dir]);
        }
        let run = run_result?;
        Ok(match run.code {
            // A receipt is the only discriminator when child and wrapper both
            // return 125. Unknown/malformed states fail closed.
            Some(CLW_INTERNAL_EXIT_CODE) if receipt.as_deref() == Some("EXECUTED\n") => {
                ClwDriveOutcome::Ran {
                    exit: ClwExitTransparency::Child(CLW_INTERNAL_EXIT_CODE),
                    wrote_back: false,
                }
            }
            Some(CLW_INTERNAL_EXIT_CODE) => {
                let stderr = run.stderr.trim();
                let reason = if stderr.is_empty() {
                    format!("clw run: internal or unknown execution state (exit {CLW_INTERNAL_EXIT_CODE})")
                } else {
                    stderr.to_string()
                };
                ClwDriveOutcome::ClwFailed {
                    clw_exit_code: CLW_INTERNAL_EXIT_CODE,
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

impl<B: BoxExec + Clone + Send + Sync + 'static> ClwDrive for ClwBoxDrive<B> {
    /// Drive `clw` for `lease_id` from ANY async context.
    ///
    /// `drive_blocking` runs the `clw` sequence over the SYNCHRONOUS [`BoxExec`]
    /// seam (a snapshot → hydrate → run round-trip). Awaiting it inline would pin
    /// a tokio ASYNC worker for the whole round-trip, so a burst of concurrent
    /// leases starves the executor. We offload the blocking call onto
    /// [`tokio::task::spawn_blocking`] — mirroring [`mint_jit_offloaded`]
    /// (`app.rs`). The spawned closure must be `'static + Send`, so it cannot
    /// borrow `&self`: we move OWNED copies of `boxx` (`B: Clone`) and `run`
    /// (`ClwRunSpec: Clone`) in, reconstruct a throwaway [`ClwBoxDrive`], and
    /// call `drive_blocking()` on it — keeping that the single source of truth.
    ///
    /// Fail-closed on join error: a [`tokio::task::JoinError`] (the blocking task
    /// panicked or was cancelled) maps to the SAME clw-internal outcome
    /// `drive_blocking` produces for a clw error — [`ClwDriveOutcome::ClwFailed`]
    /// (exit-transparency = clw error, NOT a child verdict, NOT cacheable). A
    /// `JoinError` is NEVER surfaced as a child success or a cacheable result.
    ///
    /// [`mint_jit_offloaded`]: crate::app::AppState::mint_jit_offloaded
    fn drive(
        &self,
        _lease_id: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<ClwDriveOutcome>> + Send + '_>,
    > {
        let boxx = self.boxx.clone();
        let run = self.run.clone();
        Box::pin(async move {
            match tokio::task::spawn_blocking(move || ClwBoxDrive::new(boxx, run).drive_blocking())
                .await
            {
                // The blocking task ran to completion — surface its result
                // verbatim (the frozen §4 exit-code semantics are unchanged).
                Ok(result) => result,
                // The blocking task panicked or was cancelled: fail closed to a
                // clw-internal failure (NOT a child verdict, NOT cacheable),
                // exactly as drive_blocking reports a clw error.
                Err(join_err) => Ok(ClwDriveOutcome::ClwFailed {
                    clw_exit_code: -1,
                    reason: format!("clw drive task did not complete: {join_err}"),
                }),
            }
        })
    }
}

// ── MockBoxExec ───────────────────────────────────────────────────────────────

/// A [`BoxExec`](corelink_runner::lease::BoxExec) test double returning
/// programmed [`CmdOutput`]s for the drive commands and receipt operations.
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
    /// Contents returned by the separate receipt read. Defaults to a state
    /// consistent with a pre-execution 125 or an executed child otherwise.
    pub run_state_out: CmdOutput,
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
        let state = if run_out.code == Some(CLW_INTERNAL_EXIT_CODE) {
            "NOT_STARTED\n"
        } else {
            "EXECUTED\n"
        };
        Self {
            snapshot_out: Self::ok_out(),
            hydrate_out: Self::ok_out(),
            run_out,
            run_state_out: CmdOutput {
                code: Some(0),
                stdout: state.to_string(),
                stderr: String::new(),
            },
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

    /// A mock `clw run` result paired with an explicit execution receipt.
    pub fn with_run_code_and_state(code: i32, state: &str) -> Self {
        let mut mock = Self::with_run_code(code);
        mock.run_state_out.stdout = state.to_string();
        mock
    }
}

impl BoxExec for MockBoxExec {
    fn run(&self, argv: &[&str]) -> anyhow::Result<CmdOutput> {
        if argv.first().copied() == Some("mktemp") {
            return Ok(CmdOutput {
                code: Some(0),
                stdout: "/tmp/corelink-run-state.test\n".to_string(),
                stderr: String::new(),
            });
        }
        if argv.first().copied() == Some("cat") {
            return Ok(self.run_state_out.clone());
        }
        if matches!(argv.first().copied(), Some("rm" | "rmdir")) {
            return Ok(Self::ok_out());
        }
        let verb = if argv.first().copied() == Some("clw") {
            argv.get(1).copied()
        } else {
            argv.windows(2)
                .find(|pair| pair[0] == "clw")
                .map(|pair| pair[1])
        };
        match verb {
            Some("snapshot") => Ok(self.snapshot_out.clone()),
            Some("hydrate") => Ok(self.hydrate_out.clone()),
            Some("run") => Ok(self.run_out.clone()),
            other => anyhow::bail!("MockBoxExec: unexpected command: {other:?}"),
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
    fn clw_internal_exit_125_is_not_cacheable() {
        assert!(!ClwExitTransparency::ClwInternal(125).is_cacheable());
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
            clw_exit_code: 125,
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
    async fn drive_offloaded_child_0_wrote_back_on_spawn_blocking() {
        // WP-8c: `drive` offloads `drive_blocking` onto `tokio::task::spawn_blocking`
        // so it is safe in any async context. Exercise that wrapper directly
        // (multi-thread runtime) and assert the offloaded path still yields the
        // child-0 / wrote_back outcome — i.e. moving WHERE the work runs did not
        // change WHAT it returns.
        let outcome = ClwBoxDrive::new(MockBoxExec::with_run_code(0), spec())
            .drive("lease-offload")
            .await
            .expect("transport must not error");
        assert_eq!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(0),
                wrote_back: true,
            },
            "the spawn_blocking-offloaded drive must still report child 0 + wrote_back"
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
    async fn drive_run_exit_125_is_clw_failed_not_child() {
        // The mock's default state for 125 is NOT_STARTED: positive proof that
        // the wrapper did not dispatch the child.
        let outcome = drive_with(MockBoxExec::with_run_code(125)).await;
        assert!(
            matches!(
                outcome,
                ClwDriveOutcome::ClwFailed {
                    clw_exit_code: 125,
                    ..
                }
            ),
            "run exit 125 with NOT_STARTED is a wrapper failure; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_run_exit_2_is_child_not_clw_failed() {
        // REGRESSION GUARD for the 2026-06-19 contract correction: under clw
        // v0.1.1, a child exit 2 is an ORDINARY child verdict (Child(2),
        // non-zero ⇒ not cached), NOT a clw-internal failure. (Pre-v0.1.1 we
        // wrongly treated 2 as the sentinel.)
        let outcome = drive_with(MockBoxExec::with_run_code(2)).await;
        assert_eq!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(2),
                wrote_back: false,
            },
            "child exit 2 must be Child(2) (not cached), never ClwFailed; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_run_child_exit_125_uses_executed_receipt() {
        let outcome = drive_with(MockBoxExec::with_run_code_and_state(125, "EXECUTED\n")).await;
        assert_eq!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(125),
                wrote_back: false,
            }
        );
    }

    #[tokio::test]
    async fn drive_run_125_without_preexecution_proof_fails_closed() {
        let outcome = drive_with(MockBoxExec::with_run_code_and_state(125, "DISPATCHING\n")).await;
        assert!(matches!(
            outcome,
            ClwDriveOutcome::ClwFailed { clw_exit_code: 125, .. }
        ));
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
                code: Some(125),
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
            run_state_out: CmdOutput {
                code: Some(0),
                stdout: "EXECUTED\n".to_string(),
                stderr: String::new(),
            },
        };
        let outcome = drive_with(boxx).await;
        assert!(
            matches!(
                &outcome,
                ClwDriveOutcome::ClwFailed { clw_exit_code: 125, reason }
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
                code: Some(125),
                stdout: String::new(),
                stderr: "clw: hydrate target busy".to_string(),
            },
            run_out: CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            },
            run_state_out: CmdOutput {
                code: Some(0),
                stdout: "EXECUTED\n".to_string(),
                stderr: String::new(),
            },
        };
        let outcome = drive_with(boxx).await;
        assert!(
            matches!(
                &outcome,
                ClwDriveOutcome::ClwFailed { clw_exit_code: 125, reason }
                    if reason.contains("hydrate target busy")
            ),
            "hydrate failure ⇒ ClwFailed with stderr-derived reason; got {outcome:?}"
        );
    }

    #[tokio::test]
    async fn drive_transport_err_propagates_as_err() {
        // A BoxExec spawn/transport Err must propagate as Err (NOT Ok(outcome)).
        #[derive(Clone)]
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
