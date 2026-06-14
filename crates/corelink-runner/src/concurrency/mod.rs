// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Concurrency / throughput: run **N jobs in parallel** on a single
//! Hetzner-class box, target ≥8 (WP-C2b item ④).
//!
//! C2a proved *one* job per box (lease → spawn → teardown). C2b loads the box:
//! a [`Scheduler`] drives many per-job containers at once over C2a's frozen
//! [`Engine`](crate::isolation::Engine) seam, fans the spawns out across worker
//! threads, and tears every container down through C2a's
//! [`teardown`](crate::teardown::teardown) — never re-implementing the
//! lifecycle.
//!
//! # Box-sharing (CRITICAL)
//! WP-C5a runs on the **same** box concurrently. Every container this module
//! creates is named under the `hugit-c2b-` prefix (see [`c2b_container_name`]),
//! and the concurrency census ([`Scheduler::running_census`]) is scoped to that
//! prefix only — it never counts or touches containers owned by other WPs.

use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result, bail};

/// Number of census polls taken while the batch is in-flight to capture the
/// peak concurrency. Spinning quickly with no sleep is intentional: real jobs
/// on the box take O(seconds), so 40 polls is ample without adding latency.
const CENSUS_POLL_ITERATIONS: usize = 40;
use corelink_runners_contracts::RunnerLease;

use crate::isolation::{Engine, RunningContainer};
use crate::lease::{BoxExec, ContainerSpec};
use crate::teardown::{ForensicReport, teardown};

/// Prefix under which **all** C2b-owned containers/labels live, so forensic
/// scans and kill-sweeps stay scoped to this WP on the shared box.
pub const C2B_PREFIX: &str = "hugit-c2b-";

/// Derive a C2b-namespaced container name from a lease id.
///
/// Distinct from C2a's `hugit-job-` naming: the `hugit-c2b-` prefix is what
/// lets [`Scheduler::running_census`] and crash sweeps target *only* this WP's
/// containers on the shared box. Docker names must match
/// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`, so non-conforming chars are mapped to `_`.
#[must_use]
pub fn c2b_container_name(lease_id: &str) -> String {
    let mut s = String::with_capacity(lease_id.len() + C2B_PREFIX.len());
    s.push_str(C2B_PREFIX);
    for c in lease_id.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    s
}

/// Build a C2b-namespaced [`ContainerSpec`] from a frozen lease.
///
/// Reuses C2a's [`ContainerSpec::from_lease`] for all validation (isolated
/// net-policy, non-empty ids/tmp_root) then rewrites only the container *name*
/// into the `hugit-c2b-` namespace. The lease is consumed, never modified.
///
/// # Errors
/// Propagates C2a's lease validation errors.
pub fn c2b_spec(lease: &RunnerLease, image: &str) -> Result<ContainerSpec> {
    let mut spec = ContainerSpec::from_lease(lease, image)?;
    spec.name = c2b_container_name(&lease.lease_id);
    Ok(spec)
}

/// Outcome for one job in a concurrent batch.
#[derive(Debug, Clone)]
pub struct JobOutcome {
    /// The C2b-namespaced container name.
    pub name: String,
    /// `true` iff the container spawned and the job command exited 0.
    pub ok: bool,
    /// Exit code of the job command (`None` if killed by signal / never ran).
    pub code: Option<i32>,
    /// Failure detail when `ok` is false.
    pub error: Option<String>,
}

/// A single container whose forensic teardown FAILED during a batch reclaim.
///
/// A failed teardown leaks a box (the container may still be running on the
/// shared host). This record makes that leak VISIBLE and reclaimable — it is
/// never silently dropped (a swallowed teardown error is an invisible leak,
/// which the deadline reaper cannot find because it only knows about leases).
#[derive(Debug, Clone)]
pub struct TeardownFailure {
    /// The C2b-namespaced container name that failed to tear down.
    pub name: String,
    /// The teardown error, rendered (the box may be unreachable, or `docker
    /// rm -f` reported a non-idempotent failure).
    pub error: String,
}

/// Result of running a concurrent batch and tearing it all down.
#[derive(Debug, Clone)]
pub struct BatchReport {
    /// One outcome per submitted lease, in submission order.
    pub outcomes: Vec<JobOutcome>,
    /// Peak number of `hugit-c2b-*` containers observed running at once,
    /// measured by a live census while the batch was in flight.
    pub peak_concurrency: usize,
    /// Forensic re-scan after teardown of every job — must be clean.
    pub residue: ForensicReport,
    /// Per-container teardown FAILURES, collected (never swallowed). One entry
    /// per container whose forensic teardown returned `Err` — each is a leaked
    /// box that must be surfaced so it can be reclaimed. A non-empty vector
    /// means the batch did NOT fully reclaim; callers gate on
    /// [`BatchReport::all_torn_down`].
    pub teardown_failures: Vec<TeardownFailure>,
}

impl BatchReport {
    /// Count of jobs that ran to a clean exit.
    #[must_use]
    pub fn ok_count(&self) -> usize {
        self.outcomes.iter().filter(|o| o.ok).count()
    }

    /// `true` iff every container in the batch tore down cleanly — i.e. no
    /// box was leaked. The complement of a non-empty
    /// [`teardown_failures`](BatchReport::teardown_failures).
    #[must_use]
    pub fn all_torn_down(&self) -> bool {
        self.teardown_failures.is_empty()
    }
}

/// Concurrent per-job scheduler over a [`BoxExec`] + [`Engine`].
///
/// The engine and box are shared (`Arc`) across worker threads; both C2a impls
/// (`SshBox`, `DockerEngine`) are `Clone`/`Send`-friendly, so a thread-per-job
/// fan-out keeps the lifecycle code unchanged.
pub struct Scheduler<B: BoxExec, E: Engine> {
    boxx: Arc<B>,
    engine: Arc<E>,
}

impl<B, E> Scheduler<B, E>
where
    B: BoxExec + Send + Sync + 'static,
    E: Engine + Send + Sync + 'static,
{
    /// Construct over a shared box transport and container engine.
    pub fn new(boxx: B, engine: E) -> Self {
        Self {
            boxx: Arc::new(boxx),
            engine: Arc::new(engine),
        }
    }

    /// Live census of currently-running `hugit-c2b-*` containers on the box.
    ///
    /// Scoped to this WP's prefix only (box-sharing rule): a `docker ps` filter
    /// on `name=hugit-c2b-` never sees C5a's or any other WP's containers.
    ///
    /// # Errors
    /// Fails only if the box is unreachable.
    pub fn running_census(&self) -> Result<usize> {
        census(self.boxx.as_ref())
    }

    /// Spawn `leases.len()` jobs concurrently, run a trivial job command in
    /// each, sample peak concurrency, then tear **every** container down via
    /// C2a's forensic teardown.
    ///
    /// Each job runs `job_argv` inside its container (e.g. `["true"]`). The
    /// batch returns peak observed `hugit-c2b-*` concurrency and a post-teardown
    /// forensic report aggregated across all jobs.
    ///
    /// # Errors
    /// Fails if the box is unreachable for the census; per-job spawn/exec
    /// failures are recorded in [`JobOutcome`], not returned as `Err`.
    pub fn run_batch(
        &self,
        leases: &[(RunnerLease, String)],
        job_argv: &[&str],
    ) -> Result<BatchReport> {
        if leases.is_empty() {
            bail!("run_batch requires at least one lease");
        }
        // Build specs up front so a bad lease fails fast before we touch the box.
        let specs: Vec<ContainerSpec> = leases
            .iter()
            .map(|(l, img)| c2b_spec(l, img))
            .collect::<Result<_>>()
            .context("deriving C2b container specs")?;

        let job_argv: Vec<String> = job_argv.iter().map(|s| (*s).to_string()).collect();

        // Fan out: one thread per job spawns + runs its container.
        let mut handles = Vec::with_capacity(specs.len());
        for spec in &specs {
            let engine = Arc::clone(&self.engine);
            let spec = spec.clone();
            let argv = job_argv.clone();
            handles.push(thread::spawn(move || {
                run_one(engine.as_ref(), &spec, &argv)
            }));
        }

        // While jobs are in flight, sample the live census to capture the peak
        // number of *our* containers running simultaneously.
        let mut peak = 0usize;
        for _ in 0..CENSUS_POLL_ITERATIONS {
            if handles.iter().all(|h| h.is_finished()) {
                break;
            }
            if let Ok(n) = census(self.boxx.as_ref()) {
                peak = peak.max(n);
            }
        }

        let mut outcomes = Vec::with_capacity(handles.len());
        for h in handles {
            outcomes.push(h.join().unwrap_or_else(|_| JobOutcome {
                name: "<panicked>".to_string(),
                ok: false,
                code: None,
                error: Some("worker thread panicked".to_string()),
            }));
        }
        // Final census in case the peak sampling missed the simultaneous window
        // (e.g. all very fast jobs); take the larger reading.
        if let Ok(n) = census(self.boxx.as_ref()) {
            peak = peak.max(n);
        }

        // Teardown every container via C2a's forensic teardown and aggregate.
        //
        // A per-container teardown error MUST NOT be swallowed: a failed
        // teardown leaks a box (the container may still be running on the
        // shared host), and the deadline reaper cannot find it because it
        // reasons over leases, not orphaned containers. So we COLLECT each
        // failure — logging it AND recording it on the report — while still
        // attempting teardown of every remaining container in the batch. The
        // caller gates on `BatchReport::all_torn_down`; a leaked box is now
        // visible and reclaimable, never silently dropped.
        let mut residue = ForensicReport::default();
        let mut teardown_failures = Vec::new();
        for spec in &specs {
            let c = RunningContainer {
                name: spec.name.clone(),
            };
            match teardown(self.boxx.as_ref(), &c) {
                Ok(r) => {
                    residue.containers.extend(r.containers);
                    residue.processes.extend(r.processes);
                    residue.mounts.extend(r.mounts);
                    residue.network.extend(r.network);
                }
                Err(e) => {
                    // Surface, never swallow: log the leak AND record it so the
                    // caller can reclaim it. Continue the loop so one bad
                    // teardown never strands the rest of the batch.
                    let error = format!("{e:#}");
                    eprintln!(
                        "run_batch: teardown FAILED for container {} — box LEAKED, \
                         must be reclaimed: {error}",
                        c.name
                    );
                    teardown_failures.push(TeardownFailure {
                        name: c.name.clone(),
                        error,
                    });
                }
            }
        }

        Ok(BatchReport {
            outcomes,
            peak_concurrency: peak,
            residue,
            teardown_failures,
        })
    }
}

/// Count running `hugit-c2b-*` containers (this WP's prefix only).
fn census<B: BoxExec>(boxx: &B) -> Result<usize> {
    let out = boxx.run(&[
        "docker",
        "ps",
        "--filter",
        &format!("name={C2B_PREFIX}"),
        "--format",
        "{{.Names}}",
    ])?;
    Ok(out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with(C2B_PREFIX))
        .count())
}

/// Spawn one container, run the job command, leave it running (teardown is the
/// batch's job so peak concurrency can be measured first).
fn run_one<E: Engine>(engine: &E, spec: &ContainerSpec, argv: &[String]) -> JobOutcome {
    let container = match engine.spawn(spec) {
        Ok(c) => c,
        Err(e) => {
            return JobOutcome {
                name: spec.name.clone(),
                ok: false,
                code: None,
                error: Some(format!("spawn: {e}")),
            };
        }
    };
    let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();
    let job_argv = if argv_ref.is_empty() {
        vec!["true"]
    } else {
        argv_ref
    };
    match engine.exec(&container, &job_argv) {
        Ok(code) => JobOutcome {
            name: spec.name.clone(),
            ok: code == Some(0),
            code,
            error: (code != Some(0)).then(|| format!("job exit {code:?}")),
        },
        Err(e) => JobOutcome {
            name: spec.name.clone(),
            ok: false,
            code: None,
            error: Some(format!("exec: {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_c2b_namespaced_and_sanitized() {
        assert_eq!(c2b_container_name("lease/x y"), "hugit-c2b-lease_x_y");
        assert!(c2b_container_name("anything").starts_with(C2B_PREFIX));
    }

    #[test]
    fn spec_reuses_c2a_validation() {
        use corelink_runners_contracts::{RunnerLease, RunnerState};
        const PIN: &str =
            "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
        let l = RunnerLease {
            lease_id: "z1".to_string(),
            principal_chain: vec![],
            path_set: vec![],
            expiry: 0,
            net_policy: "none".to_string(),
            tmp_root: "/t".to_string(),
            state: RunnerState::Held,
        };
        let spec = c2b_spec(&l, PIN).unwrap();
        assert_eq!(spec.name, "hugit-c2b-z1");
        assert!(spec.no_network);

        // C2a validation is reused, incl. the supply-chain pin floor.
        let mut bad = l.clone();
        bad.net_policy = "egress".to_string();
        assert!(c2b_spec(&bad, PIN).is_err());
        assert!(
            c2b_spec(&l, "alpine:3.20").is_err(),
            "c2b must reject an unpinned image (inherits the X4 floor)"
        );
    }

    // ── Batch-teardown no-swallow regression (P1) ────────────────────────────
    //
    // A failed per-container teardown in `run_batch` MUST be SURFACED, never
    // swallowed: a swallowed teardown error leaks a box invisibly (the deadline
    // reaper reasons over leases, not orphaned containers, so it can never
    // reclaim it). This harness makes the teardown of ONE specific container
    // fail (its `docker rm -f` errors at the box) and asserts:
    //   1. the failure is reported in `teardown_failures` (not dropped),
    //   2. `all_torn_down()` is false (the leak is visible),
    //   3. EVERY other container is still torn down (one bad item never strands
    //      the rest of the batch).

    use std::sync::Mutex;

    use crate::isolation::{Engine, IsolationProbe, RunningContainer};
    use crate::lease::{BoxExec, CmdOutput, ContainerSpec};

    const TEST_PIN: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

    fn held_lease(id: &str) -> RunnerLease {
        use corelink_runners_contracts::RunnerState;
        RunnerLease {
            lease_id: id.to_string(),
            principal_chain: vec![],
            path_set: vec![],
            expiry: 0,
            net_policy: "none".to_string(),
            tmp_root: "/t".to_string(),
            state: RunnerState::Held,
        }
    }

    /// `BoxExec` that returns `Err` for the `docker rm -f` of one POISONED
    /// container name (forcing `teardown` to return `Err`), and clean/empty
    /// output for every other command (so other teardowns succeed). Records
    /// which container names it was asked to `rm -f`.
    struct PoisonRmBox {
        poison: String,
        rm_targets: Mutex<Vec<String>>,
    }

    impl PoisonRmBox {
        fn new(poison: &str) -> Self {
            Self {
                poison: poison.to_string(),
                rm_targets: Mutex::new(Vec::new()),
            }
        }
        fn rm_targets(&self) -> Vec<String> {
            self.rm_targets.lock().unwrap().clone()
        }
    }

    impl BoxExec for PoisonRmBox {
        fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
            // The teardown destroy step: `docker rm -f <name>`.
            if argv.len() >= 3 && argv[0] == "docker" && argv[1] == "rm" && argv[2] == "-f" {
                let name = argv[3];
                self.rm_targets.lock().unwrap().push(name.to_string());
                if name == self.poison {
                    bail!("box unreachable: docker rm -f {name} failed (simulated)");
                }
            }
            // Everything else (the four forensic scans, census ps) → clean.
            Ok(CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    /// `Engine` whose spawn/exec always succeed (so the batch reaches teardown).
    struct OkEngine;

    impl Engine for OkEngine {
        fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
            Ok(RunningContainer {
                name: spec.name.clone(),
            })
        }
        fn probe(&self, _c: &RunningContainer, _spec: &ContainerSpec) -> Result<IsolationProbe> {
            Ok(IsolationProbe {
                tmp_is_private: true,
                net_is_isolated: true,
            })
        }
        fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
            Ok(Some(0))
        }
        fn exec_captured(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<CmdOutput> {
            Ok(CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
        fn is_alive(&self, _c: &RunningContainer) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn run_batch_surfaces_failed_teardown_and_still_reclaims_the_rest() {
        // Three jobs; the SECOND container's teardown is poisoned to fail.
        let poison = c2b_container_name("leaky");
        let boxx = PoisonRmBox::new(&poison);
        let sched = Scheduler::new(boxx, OkEngine);

        let leases = vec![
            (held_lease("a"), TEST_PIN.to_string()),
            (held_lease("leaky"), TEST_PIN.to_string()),
            (held_lease("c"), TEST_PIN.to_string()),
        ];

        let report = sched
            .run_batch(&leases, &["true"])
            .expect("batch runs; per-item teardown failures are reported, not Err");

        // 1. The failure is SURFACED, not swallowed.
        assert!(
            !report.all_torn_down(),
            "a failed teardown must make all_torn_down() false (leak is visible)"
        );
        assert_eq!(
            report.teardown_failures.len(),
            1,
            "exactly one teardown failure must be reported; got {:?}",
            report.teardown_failures
        );
        assert_eq!(
            report.teardown_failures[0].name, poison,
            "the reported failure must name the leaked container"
        );
        assert!(
            !report.teardown_failures[0].error.is_empty(),
            "the failure must carry the rendered error"
        );

        // 2. Other items are STILL processed — teardown was attempted for all
        //    three containers (one bad item never strands the rest).
        let targets = sched_box_targets(&sched);
        for id in ["a", "leaky", "c"] {
            let name = c2b_container_name(id);
            assert!(
                targets.contains(&name),
                "teardown must be attempted for {name} despite the leaky sibling; \
                 rm targets = {targets:?}"
            );
        }
    }

    /// Helper: read the poisoned box's recorded `rm -f` targets back off the
    /// scheduler (the scheduler owns the box behind an `Arc`).
    fn sched_box_targets(sched: &Scheduler<PoisonRmBox, OkEngine>) -> Vec<String> {
        sched.boxx.rm_targets()
    }
}
