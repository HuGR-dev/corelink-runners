// Transplanted from transferred runner implementation @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Teardown + forensic re-scan: destroy the per-job container and prove the
//! box has **zero residue** afterwards.
//!
//! "Leaves nothing" is verified, not assumed: after destroy we re-scan the box
//! across four surfaces and require every one clean:
//! - **containers** — no container by the job name, running or stopped;
//! - **process table** — no `sleep`/job process tagged to the container;
//! - **mounts** — no tmpfs / overlay mount referencing the job;
//! - **network** — no veth / namespace / iptables artifact for the job.
//!
//! Because v0 spawns with `--rm`, destroy is `docker rm -f` (idempotent), and
//! the kernel reaps namespaces/mounts on container exit; the re-scan is the
//! independent forensic proof, not a courtesy.

use anyhow::{Result, bail};

use crate::isolation::RunningContainer;
use crate::lease::{BoxExec, CmdOutput};

/// Per-surface forensic findings after teardown. Each field is the residue
/// found on that surface; **all empty == clean**.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForensicReport {
    /// Container ids/names still present (running or stopped).
    pub containers: Vec<String>,
    /// Process-table lines still referencing the job.
    pub processes: Vec<String>,
    /// Mount-table lines still referencing the job.
    pub mounts: Vec<String>,
    /// Network artifacts (interfaces/namespaces) still referencing the job.
    pub network: Vec<String>,
    /// Surfaces whose **scan command itself failed** — a non-zero/absent exit
    /// code, or stderr output. A failed scan is fail-CLOSED: empty stdout from
    /// a scan that errored proves nothing, so it can NEVER read as clean. Each
    /// entry is `"<surface>: <reason>"`. (Without this, surfaces 2/3/4 read a
    /// crashed `ps`/`mount`/`ip` as zero residue, masking a real escape.)
    pub scan_failures: Vec<String>,
}

impl ForensicReport {
    /// `true` iff the box has zero residue across all four surfaces **and**
    /// every surface scan ran successfully. A scan that failed (non-zero exit
    /// or stderr) is never a clean verdict — fail-closed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.containers.is_empty()
            && self.processes.is_empty()
            && self.mounts.is_empty()
            && self.network.is_empty()
            && self.scan_failures.is_empty()
    }
}

fn nonempty_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Fail-closed check for a surface whose scan is a plain command expected to
/// exit 0 (the container scan, `docker ps`). A non-zero/absent exit code or any
/// stderr means the scan did not run to completion, so its empty stdout is NOT
/// evidence of a clean box. Mirrors how `teardown`'s destroy step already
/// refuses a non-`ok()` result. Returns `Some(reason)` when the scan failed.
fn scan_failure(surface: &str, out: &CmdOutput) -> Option<String> {
    if !out.ok() {
        return Some(format!("{surface}: scan exited {:?}", out.code));
    }
    let err = out.stderr.trim();
    if !err.is_empty() {
        return Some(format!("{surface}: scan wrote stderr: {err}"));
    }
    None
}

/// Fail-closed check for a `grep`-based surface (process/mount/network scans).
///
/// `grep` exit codes are load-bearing here: **1** is the *clean* case (the
/// pattern matched nothing — zero residue), **0** means residue was found (it
/// lands in stdout), and **≥2** is a real error (e.g. `ps`/`mount`/`ip` is
/// missing or failed). A killed process (`code == None`) or any stderr is also
/// a scan failure. Only exit 0 or 1 are trustworthy verdicts; everything else
/// is fail-closed. Returns `Some(reason)` when the scan failed.
fn grep_scan_failure(surface: &str, out: &CmdOutput) -> Option<String> {
    match out.code {
        Some(0 | 1) => {}
        other => return Some(format!("{surface}: scan exited {other:?}")),
    }
    let err = out.stderr.trim();
    if !err.is_empty() {
        return Some(format!("{surface}: scan wrote stderr: {err}"));
    }
    None
}

/// Destroy the container and forensically re-scan the box.
///
/// Idempotent: safe to call whether or not the container is still alive.
///
/// # Errors
/// Fails only if the box itself is unreachable; a *dirty* box is reported via
/// the returned [`ForensicReport`] (caller asserts [`ForensicReport::is_clean`]).
pub fn teardown<B: BoxExec>(boxx: &B, c: &RunningContainer) -> Result<ForensicReport> {
    let name = &c.name;

    // Destroy (idempotent). `--rm` containers vanish on stop; force-rm covers
    // the stopped/zombie case. Ignore "no such container".
    let rm = boxx.run(&["docker", "rm", "-f", name])?;
    if !rm.ok() && !rm.stderr.contains("No such container") {
        bail!("docker rm -f {name} failed: {}", rm.stderr.trim());
    }

    // Surface 1: containers (any state).
    let containers = boxx.run(&[
        "docker",
        "ps",
        "-a",
        "--filter",
        &format!("name={name}"),
        "--format",
        "{{.Names}}",
    ])?;

    // Surface 2: process table — no process whose cmdline references the job.
    let processes = boxx.run(&[
        "sh",
        "-c",
        &format!("ps -eo args | grep -F {name} | grep -v grep"),
    ])?;

    // Surface 3: mounts — no tmpfs/overlay still referencing the job name.
    let mounts = boxx.run(&["sh", "-c", &format!("mount | grep -F {name}")])?;

    // Surface 4: network — no interface or named netns for the job.
    //
    // Fail-closed by construction: a single grep over the COMBINED output of
    // both `ip` probes, with NO `2>/dev/null` (stderr must reach
    // `grep_scan_failure`) and NO trailing `;` (which would swallow the first
    // probe's exit code, masking a broken `ip`). `set -o pipefail` makes a
    // crashed `ip` propagate as the pipeline's exit code (≥2), and grouping the
    // probes with `{ … ; }` lets a stderr diagnostic from either one surface.
    // The grep then yields the load-bearing verdict (0 = residue, 1 = clean,
    // ≥2 = the network tool failed), routed through `grep_scan_failure` exactly
    // like the process/mount surfaces — so a failed network scan is never read
    // as clean.
    let network = boxx.run(&[
        "sh",
        "-c",
        &format!("set -o pipefail; {{ ip -o link show; ip netns list; }} | grep -F {name}"),
    ])?;

    // Verdict integrity: a surface scan that FAILED (non-zero exit / killed /
    // stderr) is fail-CLOSED — its empty stdout must never read as clean. The
    // container scan expects exit 0; the grep-based scans (process/mount/net)
    // treat exit 1 as "no match == clean" but anything else as a failed scan.
    let scan_failures = [
        scan_failure("containers", &containers),
        grep_scan_failure("processes", &processes),
        grep_scan_failure("mounts", &mounts),
        grep_scan_failure("network", &network),
    ]
    .into_iter()
    .flatten()
    .collect();

    Ok(ForensicReport {
        containers: nonempty_lines(&containers.stdout),
        processes: nonempty_lines(&processes.stdout),
        mounts: nonempty_lines(&mounts.stdout),
        network: nonempty_lines(&network.stdout),
        scan_failures,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn report_clean_when_all_empty() {
        assert!(ForensicReport::default().is_clean());
    }

    #[test]
    fn report_dirty_when_container_remains() {
        let r = ForensicReport {
            containers: vec!["corelink-job-x".to_string()],
            ..Default::default()
        };
        assert!(!r.is_clean());
    }

    #[test]
    fn report_dirty_when_any_scan_failed() {
        let r = ForensicReport {
            scan_failures: vec!["processes: scan exited Some(2)".to_string()],
            ..Default::default()
        };
        assert!(
            !r.is_clean(),
            "a failed scan must never read as clean (fail-closed)"
        );
    }

    /// Classifies which forensic surface a scan `argv` belongs to, so a test
    /// can inject a failure into exactly one stage and leave the rest clean.
    fn surface_of(argv: &[&str]) -> &'static str {
        let joined = argv.join(" ");
        if argv.first() == Some(&"docker") && argv.contains(&"ps") {
            "containers"
        } else if joined.contains("ps -eo args") {
            "processes"
        } else if joined.contains("mount |") {
            "mounts"
        } else if joined.contains("ip -o link") || joined.contains("ip netns") {
            "network"
        } else {
            "destroy" // the `docker rm -f` step
        }
    }

    /// A box that replies CLEAN (exit 0, empty stdout/stderr) for every scan
    /// EXCEPT `fail_surface`, where it returns the supplied failing output —
    /// the per-stage harness for the fail-closed regression tests.
    struct OneFailingSurface {
        fail_surface: &'static str,
        failing: CmdOutput,
        seen: Mutex<Vec<String>>,
    }

    impl OneFailingSurface {
        fn new(fail_surface: &'static str, failing: CmdOutput) -> Self {
            Self {
                fail_surface,
                failing,
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl BoxExec for OneFailingSurface {
        fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
            let surface = surface_of(argv);
            self.seen.lock().unwrap().push(surface.to_string());
            if surface == self.fail_surface {
                return Ok(self.failing.clone());
            }
            Ok(CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn container() -> RunningContainer {
        RunningContainer {
            name: "corelink-job-rescan".to_string(),
        }
    }

    /// Baseline: when every surface scan succeeds with empty output, the box is
    /// clean — proves the new fail-closed logic doesn't reject a real CLEAN.
    /// (Surfaces 2/3/4 are grep-based, so the clean case is exit **1**.)
    #[test]
    fn all_surfaces_clean_is_clean() {
        struct AllClean;
        impl BoxExec for AllClean {
            fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
                let code = if surface_of(argv) == "containers" || surface_of(argv) == "destroy" {
                    Some(0)
                } else {
                    Some(1) // grep "no match" == clean
                };
                Ok(CmdOutput {
                    code,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            }
        }
        let report = teardown(&AllClean, &container()).expect("teardown");
        assert!(report.is_clean(), "empty + successful scans => clean");
        assert!(report.scan_failures.is_empty());
    }

    /// Stage 1 (containers): `docker ps` exiting non-zero is a scan failure,
    /// never CLEAN — empty stdout from a crashed `docker ps` proves nothing.
    #[test]
    fn stage1_containers_nonzero_exit_is_not_clean() {
        let failing = CmdOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
        };
        let boxx = OneFailingSurface::new("containers", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "stage 1 scan failure must be fail-closed"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("containers:")),
            "the container scan failure must be recorded, got {:?}",
            report.scan_failures
        );
    }

    /// Stage 2 (processes): a grep pipeline exiting **2** (e.g. `ps` missing /
    /// errored) is a scan failure, NOT the clean exit-1 "no match" case.
    #[test]
    fn stage2_processes_error_exit_is_not_clean() {
        let failing = CmdOutput {
            code: Some(2),
            stdout: String::new(),
            stderr: String::new(),
        };
        let boxx = OneFailingSurface::new("processes", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "stage 2 scan failure must be fail-closed"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("processes:")),
            "the process scan failure must be recorded, got {:?}",
            report.scan_failures
        );
    }

    /// Stage 2 (processes), stderr variant: a scan that wrote to stderr (the
    /// underlying tool emitted a diagnostic) is a failure even at exit 0/1.
    #[test]
    fn stage2_processes_stderr_is_not_clean() {
        let failing = CmdOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: "ps: command not found\n".to_string(),
        };
        let boxx = OneFailingSurface::new("processes", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "a scan that emitted stderr is fail-closed even on exit 1"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("processes:")),
            "got {:?}",
            report.scan_failures
        );
    }

    /// Stage 3 (mounts): grep pipeline error exit is a scan failure, not CLEAN.
    #[test]
    fn stage3_mounts_error_exit_is_not_clean() {
        let failing = CmdOutput {
            code: Some(2),
            stdout: String::new(),
            stderr: String::new(),
        };
        let boxx = OneFailingSurface::new("mounts", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "stage 3 scan failure must be fail-closed"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("mounts:")),
            "got {:?}",
            report.scan_failures
        );
    }

    /// Stage 4 (network): a killed scan (`code == None`) is a failure, not the
    /// clean case — the `ip` probe never returned a verdict.
    #[test]
    fn stage4_network_killed_scan_is_not_clean() {
        let failing = CmdOutput {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
        };
        let boxx = OneFailingSurface::new("network", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "stage 4 scan failure must be fail-closed"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("network:")),
            "got {:?}",
            report.scan_failures
        );
    }

    /// Stage 4 (network), error-exit variant — the regression for the W2-A
    /// masking bug. The pre-fix command (`ip … 2>/dev/null | grep …; ip … |
    /// grep …`) discarded stderr AND let `;` overwrite the pipeline exit code,
    /// so a broken `ip` (exit ≥2) was read as the clean exit-1 "no match" case,
    /// masking a real network-escape residue. With the fix the network scan
    /// surfaces its non-zero exit through `grep_scan_failure`, exactly like the
    /// process/mount surfaces: a network scan that exits ≥2 is scan-failed /
    /// not-clean, never clean.
    #[test]
    fn stage4_network_error_exit_is_not_clean() {
        let failing = CmdOutput {
            code: Some(2),
            stdout: String::new(),
            stderr: String::new(),
        };
        let boxx = OneFailingSurface::new("network", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(
            !report.is_clean(),
            "a network scan that exited non-zero must be fail-closed, not clean"
        );
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("network:")),
            "the network scan failure must be recorded, got {:?}",
            report.scan_failures
        );
    }

    /// Stage 4 (network), stderr variant.
    #[test]
    fn stage4_network_stderr_is_not_clean() {
        let failing = CmdOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: "ip: not found\n".to_string(),
        };
        let boxx = OneFailingSurface::new("network", failing);
        let report = teardown(&boxx, &container()).expect("teardown");
        assert!(!report.is_clean());
        assert!(
            report
                .scan_failures
                .iter()
                .any(|f| f.starts_with("network:"))
        );
    }
}
