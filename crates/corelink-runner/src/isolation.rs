// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Per-job isolation: spawn one container with a **private tmp** and an
//! **isolated network namespace**, run a job, and probe that the isolation
//! holds.
//!
//! v0 uses Docker on the runner box: `--tmpfs <tmp_root>` gives each job its
//! own tmpfs (invisible to and unshared with other jobs / the host), and
//! `--network none` gives each job an isolated network namespace with no
//! reachable interface. Both probes ([`IsolationProbe`]) are run inside the
//! live container against the live box.
//!
//! The Firecracker upgrade path (crate docs) implements the same [`Engine`]
//! against microVMs; the [`IsolationProbe`] contract is engine-independent.

use anyhow::{Context, Result, bail};

use crate::lease::{BoxExec, ContainerSpec};
// Re-exported so Engine consumers get the full trait surface (including the
// `exec_captured` return type) from one coherent import path.
pub use crate::lease::CmdOutput;

/// Maximum size of the per-job private tmpfs mounted at `tmp_root`.
///
/// 64 MiB is intentionally conservative: job scratch space is ephemeral and
/// the box's RAM is shared across concurrent jobs. Increase only with a
/// concurrent-job capacity analysis.
const TMPFS_SIZE: &str = "64m";

/// Duration the idle-container heartbeat `sleep` runs inside a just-spawned
/// container. `docker run … sleep 3600` keeps the container alive while the
/// orchestrator calls `docker exec` to run job steps. 3600 s (1 h) is a hard
/// ceiling; expiry hard-kill (C2b) terminates it earlier via `docker kill`.
const IDLE_SLEEP_SECS: &str = "3600";

/// Result of probing a running container's isolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationProbe {
    /// `tmp_root` is mounted as a tmpfs distinct from the host filesystem.
    pub tmp_is_private: bool,
    /// The network namespace has no usable interface beyond loopback
    /// (`--network none`): no `eth*`, and outbound DNS/connect is impossible.
    pub net_is_isolated: bool,
}

impl IsolationProbe {
    /// `true` iff both tmp and network are isolated.
    #[must_use]
    pub fn fully_isolated(&self) -> bool {
        self.tmp_is_private && self.net_is_isolated
    }
}

/// A running per-job container handle.
#[derive(Debug, Clone)]
pub struct RunningContainer {
    /// Container name (== [`ContainerSpec::name`]).
    pub name: String,
}

/// The container engine seam. v0 has one implementation, [`DockerEngine`];
/// the Firecracker upgrade adds another against the same contract.
pub trait Engine {
    /// Spawn the per-job container detached, idling, per `spec`.
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer>;

    /// Probe isolation of a running container against `spec`.
    fn probe(&self, c: &RunningContainer, spec: &ContainerSpec) -> Result<IsolationProbe>;

    /// Run one job command inside the container, returning its exit code.
    fn exec(&self, c: &RunningContainer, argv: &[&str]) -> Result<Option<i32>>;

    /// Run one job command inside the container, capturing stdout/stderr bytes
    /// and the exit code. The captured-output half of the seam (Engine v2,
    /// CF0 freeze): the CheckResult path derives canonical bytes + content
    /// digest from this, and a Firecracker engine must implement it without
    /// any docker-exec analogue.
    fn exec_captured(&self, c: &RunningContainer, argv: &[&str]) -> Result<CmdOutput>;

    /// `true` iff the container `c` is still live on the box.
    ///
    /// Used by the dedup spawner to avoid handing back a cached handle for a
    /// container that has since exited/been reaped (a dead-container corpse).
    ///
    /// # Errors
    /// Fails only if the box is unreachable.
    fn is_alive(&self, c: &RunningContainer) -> Result<bool>;
}

/// Docker-backed [`Engine`], driving the box via a [`BoxExec`].
#[derive(Debug, Clone)]
pub struct DockerEngine<B: BoxExec> {
    /// The box the containers run on.
    pub boxx: B,
}

impl<B: BoxExec> DockerEngine<B> {
    /// Construct over a box transport.
    pub fn new(boxx: B) -> Self {
        Self { boxx }
    }
}

impl<B: BoxExec> Engine for DockerEngine<B> {
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
        if !spec.no_network {
            bail!("ContainerSpec.no_network must be true for C2a isolation");
        }
        // ── Supply-chain gate (WP-X4) — verify BEFORE docker run ──────────────
        // The real spawn surface enforces the pin itself, so no caller can
        // reach `docker run` with an unpinned or tampered image. The ordering is
        // load-bearing: (1) re-parse the pin (no box), (2) integrity-verify the
        // digest against the box (still no container), and ONLY then (3) run.
        // A spec built directly (bypassing `from_lease`) is caught here too.
        let pinned = crate::pin::PinnedImageRef::parse(&spec.image).with_context(|| {
            format!(
                "refusing to spawn {}: image {:?} is not content-pinned — fail CLOSED",
                spec.name, spec.image
            )
        })?;
        pinned.verify_on_box(&self.boxx).with_context(|| {
            format!(
                "refusing to spawn {}: image {:?} failed integrity verification \
                 against the box — fail CLOSED (no container spawned)",
                spec.name, spec.image
            )
        })?;

        let tmpfs = format!("{}:rw,size={TMPFS_SIZE}", spec.tmp_root);
        let argv = vec![
            "docker",
            "run",
            "-d",
            "--rm",
            "--name",
            &spec.name,
            "--network",
            "none",
            "--tmpfs",
            &tmpfs,
            "--label",
            "hugit.job=1",
            &spec.image,
            "sleep",
            IDLE_SLEEP_SECS,
        ];
        let out = self.boxx.run(&argv)?;
        if !out.ok() {
            bail!("docker run failed: {}", out.stderr.trim());
        }
        Ok(RunningContainer {
            name: spec.name.clone(),
        })
    }

    fn probe(&self, c: &RunningContainer, spec: &ContainerSpec) -> Result<IsolationProbe> {
        // tmp privacy: the mounted tmp_root must be a tmpfs, and a file written
        // there must not appear on the host filesystem.
        let marker = format!("hugit-isolation-{}", c.name);
        let write = self.boxx.run(&[
            "docker",
            "exec",
            &c.name,
            "sh",
            "-c",
            &format!(
                "mount | grep -q 'on {root} type tmpfs' && echo {m} > {root}/{m} && echo OK",
                root = spec.tmp_root,
                m = marker,
            ),
        ])?;
        let tmp_is_tmpfs = write.ok() && write.stdout.contains("OK");
        // Host must not see the in-container tmpfs marker anywhere on disk.
        let host_leak = self.boxx.run(&[
            "sh",
            "-c",
            &format!("find / -name {marker} 2>/dev/null | head -1"),
        ])?;
        let tmp_is_private = tmp_is_tmpfs && host_leak.stdout.trim().is_empty();

        // net isolation: no non-loopback interface, and an outbound connect
        // must fail (no route / no DNS) under `--network none`.
        let no_eth = self.boxx.run(&[
            "docker",
            "exec",
            &c.name,
            "sh",
            "-c",
            // Count non-loopback links. `grep -vc` exits 1 on a zero count, so
            // `printf` the result unconditionally to avoid a spurious fallback.
            "n=$(ip -o link show 2>/dev/null | grep -vc ' lo:'); printf '%s' \"$n\"",
        ])?;
        // Any non-loopback interface => not isolated. Parse defensively: a
        // non-numeric/garbled reading is treated as "interfaces present".
        let eth_count: i64 = no_eth.stdout.trim().parse().unwrap_or(i64::MAX);
        let connect = self.boxx.run(&[
            "docker",
            "exec",
            &c.name,
            "sh",
            "-c",
            // Any successful outbound connect would print REACHED; isolation
            // means this fails (timeout / unreachable).
            "timeout 4 sh -c 'echo > /dev/tcp/1.1.1.1/53 && echo REACHED' 2>/dev/null || echo BLOCKED",
        ])?;
        let net_is_isolated = eth_count == 0
            && connect.stdout.contains("BLOCKED")
            && !connect.stdout.contains("REACHED");

        Ok(IsolationProbe {
            tmp_is_private,
            net_is_isolated,
        })
    }

    fn exec(&self, c: &RunningContainer, argv: &[&str]) -> Result<Option<i32>> {
        let mut full = vec!["docker", "exec", &c.name];
        full.extend_from_slice(argv);
        let out = self
            .boxx
            .run(&full)
            .with_context(|| format!("docker exec in {}", c.name))?;
        Ok(out.code)
    }

    fn exec_captured(&self, c: &RunningContainer, argv: &[&str]) -> Result<CmdOutput> {
        // Same docker-exec transport path as `exec`, but the whole captured
        // output is handed back verbatim: stdout and stderr separately, exit
        // code as-is. No trimming, no scrubbing — the CheckResult path derives
        // canonical bytes + a content digest from exactly what the job wrote.
        let mut full = vec!["docker", "exec", &c.name];
        full.extend_from_slice(argv);
        self.boxx
            .run(&full)
            .with_context(|| format!("docker exec (captured) in {}", c.name))
    }

    fn is_alive(&self, c: &RunningContainer) -> Result<bool> {
        // `docker ps` (running only) filtered to the exact name. A dead/reaped
        // container does not appear, so the cached handle is not reused.
        let out = self
            .boxx
            .run(&[
                "docker",
                "ps",
                "--filter",
                &format!("name={}", c.name),
                "--format",
                "{{.Names}}",
            ])
            .with_context(|| format!("liveness probe for {}", c.name))?;
        Ok(out.stdout.lines().any(|l| l.trim() == c.name))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Hermetic loopback box (the FakeBox house pattern, cf.
    /// `tests/hermetic_supply_chain.rs`): records every argv and replays one
    /// scripted [`CmdOutput`] verbatim. No box, no docker, no network.
    #[derive(Clone)]
    struct FakeBox {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        reply: CmdOutput,
    }

    impl FakeBox {
        fn replying(reply: CmdOutput) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                reply,
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl BoxExec for FakeBox {
        fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
            self.calls
                .lock()
                .unwrap()
                .push(argv.iter().map(ToString::to_string).collect());
            Ok(self.reply.clone())
        }
    }

    fn container() -> RunningContainer {
        RunningContainer {
            name: "hugit-job-cf0b".to_string(),
        }
    }

    #[test]
    fn exec_captured_returns_bytes_and_exit() {
        let boxx = FakeBox::replying(CmdOutput {
            code: Some(7),
            stdout: "out-bytes\n".to_string(),
            stderr: "err-bytes\n".to_string(),
        });
        let engine = DockerEngine::new(boxx.clone());
        let out = engine
            .exec_captured(&container(), &["sh", "-c", "exit 7"])
            .expect("exec_captured");
        assert_eq!(out.code, Some(7), "exit code must pass through as-is");
        assert_eq!(out.stdout, "out-bytes\n");
        assert_eq!(out.stderr, "err-bytes\n");
        // The transport is the engine's own docker-exec plumbing (same path
        // as `exec`): `docker exec <name> <argv…>`, nothing else.
        assert_eq!(
            boxx.calls(),
            vec![vec![
                "docker".to_string(),
                "exec".to_string(),
                "hugit-job-cf0b".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "exit 7".to_string(),
            ]],
        );
    }

    #[test]
    fn exec_captured_bytes_are_byte_faithful() {
        // Binary-unsafe content: NUL, BEL, ESC/ANSI, CRLF, tabs, and
        // leading/trailing whitespace + trailing newlines. Everything must come
        // back verbatim — no trimming, no scrubbing, no normalization.
        let hostile = "\u{0}\u{7}\u{1b}[31m  spaced  \r\n\ttab\u{0} trailing \n\n";
        let boxx = FakeBox::replying(CmdOutput {
            code: Some(0),
            stdout: hostile.to_string(),
            stderr: hostile.to_string(),
        });
        let engine = DockerEngine::new(boxx);
        let out = engine
            .exec_captured(&container(), &["cat", "/hugit/tmp/blob"])
            .expect("exec_captured");
        assert_eq!(out.stdout, hostile, "stdout must be byte-faithful");
        assert_eq!(out.stderr, hostile, "stderr must be byte-faithful");
        assert_eq!(out.code, Some(0));
    }

    #[test]
    fn exec_captured_separates_stdout_stderr() {
        let boxx = FakeBox::replying(CmdOutput {
            code: Some(3),
            stdout: "ONLY-ON-STDOUT".to_string(),
            stderr: "ONLY-ON-STDERR".to_string(),
        });
        let engine = DockerEngine::new(boxx);
        let out = engine
            .exec_captured(&container(), &["sh", "-c", "true"])
            .expect("exec_captured");
        assert_eq!(out.stdout, "ONLY-ON-STDOUT");
        assert_eq!(out.stderr, "ONLY-ON-STDERR");
        assert!(
            !out.stdout.contains("STDERR"),
            "stderr must never bleed into stdout"
        );
        assert!(
            !out.stderr.contains("STDOUT"),
            "stdout must never bleed into stderr"
        );
        assert_eq!(out.code, Some(3));
    }
}
