// Transplanted from hugit/crates/hugit-fence/src/broker/redteam.rs @ 69e28e5 (runner-transfer campaign WP-R4, 2026-06-10) — wire-contract seam, no git dep; removed hugit-side by WP-R4②.
//! Active escape red-team harness (WP-C5b item **⑤**).
//!
//! This is not a passive assertion that the fence *should* hold — it
//! **genuinely attempts** six escapes against a live per-job container on the
//! Hetzner box and proves each is contained:
//!
//! | vector            | attack                                   | containment |
//! |-------------------|------------------------------------------|-------------|
//! | [`Traversal`]     | `..`-climb from the workspace to a REAL host secret planted outside the container | the host file is outside the mount namespace → the planted sentinel never appears in-container |
//! | [`SymlinkEscape`] | `ln -s <real host secret>` then read the link | the link target really exists on the host but is outside the container's view → the read yields no sentinel |
//! | [`OutOfFence`]    | overwrite a REAL host secret (absolute + `..`-climb) from inside the container | the container cannot write across the mount namespace → the host file is byte-unchanged afterwards |
//! | [`ForkBomb`]      | classic `:(){ :|:& };:` fork bomb         | `--pids-limit` caps the process count; the box is never starved |
//! | [`DiskFill`]      | `dd` 1 GiB into the writable workdir      | the `--tmpfs size=` cap stops the write; the box disk is never filled |
//! | [`FenceMaterializedEscape`] | materialize a real `FenceManifest`, then read an out-of-fence path **in the same container** | the fence (`classify()` + sparse materialize) — NOT the Docker namespace — is the boundary: the out-of-fence file is ENOENT because it was never materialized |
//!
//! The traversal/symlink/out-of-fence vectors each attack a **real** target
//! planted by [`RedTeamHarness::ensure_host_secret`] (a host file carrying
//! [`HOST_SECRET_SENTINEL`]) — so a broken boundary genuinely leaks or corrupts
//! it and the vector reports an ESCAPE. They are no longer vacuous (pointing at
//! a non-existent target or comparing two never-shared paths).
//!
//! The first five lean (correctly) on the Docker mount/pid/tmpfs namespace; the
//! **sixth is the one where the fence itself is the only control** — it would
//! escape if `classify()` were a no-op constant `Inside`, so it is the vector
//! that holds the fence honest.
//!
//! [`Traversal`]: AttackVector::Traversal
//! [`SymlinkEscape`]: AttackVector::SymlinkEscape
//! [`OutOfFence`]: AttackVector::OutOfFence
//! [`ForkBomb`]: AttackVector::ForkBomb
//! [`DiskFill`]: AttackVector::DiskFill
//! [`FenceMaterializedEscape`]: AttackVector::FenceMaterializedEscape
//!
//! **Containment is bounded and self-cleaning.** Resource attacks are capped by
//! the container's own cgroup limits ([`ContainerLimits`]) — never the box's —
//! and every container is `hugit-c5b-*`-namespaced and force-removed at the end
//! so box residue is **0**. The fork bomb and disk fill cannot reach another
//! lease or starve a sibling because the caps are per-container.
//!
//! The harness uses **held** commands (a backgrounded `sleep`) so an attack
//! overlaps in time with the containment observation — we measure the live
//! cap, not a process that already exited.

use anyhow::{Context, Result, bail};

use crate::isolation::RunningContainer;
use crate::lease::BoxExec;
use corelink_runners_contracts::FenceManifest;

use crate::enforce::probe_outside_enoent;
use crate::materialize::{CandidateEntry, materialize_sparse};

/// The container's own resource caps. These are the cgroup limits the red-team
/// attacks run against — bounding starvation to the container, never the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerLimits {
    /// Hard cap on the number of PIDs (the fork-bomb bound).
    pub pids_limit: u32,
    /// Hard cap, in MiB, on the writable tmpfs workdir (the disk-fill bound).
    pub disk_mib: u32,
    /// Hard cap, in MiB, on memory.
    pub mem_mib: u32,
}

impl Default for ContainerLimits {
    /// Conservative v0 caps: 24 PIDs, 16 MiB disk, 64 MiB memory. Small enough
    /// that the attacks bite quickly, large enough for a real job to run.
    fn default() -> Self {
        Self {
            pids_limit: 24,
            disk_mib: 16,
            mem_mib: 64,
        }
    }
}

/// One escape vector the harness attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackVector {
    /// `..`-traversal read of a host file outside the fence.
    Traversal,
    /// Create a symlink to a host secret and try to read through it.
    SymlinkEscape,
    /// Write into a path belonging to a *different* lease's workspace.
    OutOfFence,
    /// Classic shell fork bomb (`:(){ :|:& };:`).
    ForkBomb,
    /// Fill the writable workdir to exhaust disk.
    DiskFill,
    /// **The fence is the control.** Materialize a *real* [`FenceManifest`]
    /// into the container (only the in-fence `path_set`), then — inside the
    /// **same** container — attack an out-of-fence path. Containment requires
    /// the in-fence file to be present *and* the out-of-fence path to be ENOENT.
    /// This is the only vector where the Docker namespace/cgroup is **not** the
    /// boundary; the boundary is `classify()` + sparse materialization. If
    /// `classify()` ever returned a constant `Inside`, the out-of-fence file
    /// would be materialized and this vector would report an ESCAPE.
    FenceMaterializedEscape,
}

impl AttackVector {
    /// All six vectors, in attack-matrix order.
    #[must_use]
    pub fn all() -> [AttackVector; 6] {
        [
            AttackVector::Traversal,
            AttackVector::SymlinkEscape,
            AttackVector::OutOfFence,
            AttackVector::ForkBomb,
            AttackVector::DiskFill,
            AttackVector::FenceMaterializedEscape,
        ]
    }

    /// A stable slug for evidence/logging.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            AttackVector::Traversal => "traversal",
            AttackVector::SymlinkEscape => "symlink_escape",
            AttackVector::OutOfFence => "out_of_fence",
            AttackVector::ForkBomb => "fork_bomb",
            AttackVector::DiskFill => "disk_fill",
            AttackVector::FenceMaterializedEscape => "fence_materialized_escape",
        }
    }
}

/// Whether a single attack was contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedTeamOutcome {
    /// The attack was contained — it could not escape the fence or starve the
    /// box.
    Contained,
    /// The attack escaped — a containment FAILURE (the test must reject this).
    Escaped,
}

/// The result of one attack: the vector, whether it was contained, and a
/// secret-free observation string for the evidence bundle.
#[derive(Debug, Clone)]
pub struct ContainmentReport {
    /// Which vector was attempted.
    pub vector: AttackVector,
    /// Contained vs escaped.
    pub outcome: RedTeamOutcome,
    /// Human-readable, secret-free evidence (counts / tokens observed).
    pub evidence: String,
}

impl ContainmentReport {
    /// `true` iff this attack was contained.
    #[must_use]
    pub fn is_contained(&self) -> bool {
        self.outcome == RedTeamOutcome::Contained
    }
}

/// The red-team harness, bound to a box transport. All containers it spawns are
/// `hugit-c5b-*`-namespaced and torn down by [`RedTeamHarness::teardown_all`].
pub struct RedTeamHarness<'b, B: BoxExec> {
    boxx: &'b B,
    image: String,
    limits: ContainerLimits,
    /// Names spawned by this harness, for scoped teardown.
    spawned: Vec<String>,
    /// Absolute path of a REAL secret planted on the box **host** filesystem,
    /// outside every container's mount namespace. The traversal/symlink/
    /// out-of-fence vectors genuinely attempt to read or write it; containment
    /// means the planted [`HOST_SECRET_SENTINEL`] content never leaks into a
    /// container and the host file is never mutated by a container. Lazily
    /// created on first use and force-removed by `teardown_all`.
    host_secret: Option<String>,
}

/// The mandatory namespace prefix for every red-team container.
pub const REDTEAM_PREFIX: &str = "hugit-c5b-";
/// In-container writable workspace root (a size-capped tmpfs).
pub const WORKDIR: &str = "/hugit-c5b-ws";
/// Unique sentinel content written into the planted host secret. If this exact
/// string ever appears INSIDE a container, the mount-namespace boundary leaked
/// (an ESCAPE). It is the marker the traversal/symlink vectors search for.
pub const HOST_SECRET_SENTINEL: &str = "HUGIT_C5B_HOST_SECRET_LEAKED_ed1f9c2a";

impl<'b, B: BoxExec> RedTeamHarness<'b, B> {
    /// Construct a harness over `boxx`, attacking `image` under `limits`.
    pub fn new(boxx: &'b B, image: impl Into<String>, limits: ContainerLimits) -> Self {
        Self {
            boxx,
            image: image.into(),
            limits,
            spawned: Vec::new(),
            host_secret: None,
        }
    }

    /// Plant (once) a REAL secret file on the box **host** filesystem, outside
    /// every container, and return its absolute path. The traversal/symlink/
    /// out-of-fence vectors attack THIS real target — so a broken boundary would
    /// genuinely leak [`HOST_SECRET_SENTINEL`] (or let a container overwrite the
    /// file), which is what the containment assertions detect. Idempotent.
    fn ensure_host_secret(&mut self) -> Result<String> {
        if let Some(p) = &self.host_secret {
            return Ok(p.clone());
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = format!("/tmp/{REDTEAM_PREFIX}host-secret-{nonce}");
        // Write the sentinel on the HOST (not in any container). This is the
        // real out-of-fence target every cross-boundary vector now attacks.
        let out = self.boxx.run(&[
            "sh",
            "-c",
            &format!("printf %s '{HOST_SECRET_SENTINEL}' > '{path}'; chmod 600 '{path}'; echo OK"),
        ])?;
        if !(out.ok() && out.stdout.contains("OK")) {
            bail!(
                "failed to plant host secret at {path}: {}",
                out.stderr.trim()
            );
        }
        self.host_secret = Some(path.clone());
        Ok(path)
    }

    /// A unique, prefix-namespaced container name.
    fn fresh_name(&self, slug: &str) -> String {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{REDTEAM_PREFIX}{slug}-{nonce}")
    }

    /// Spawn one `hugit-c5b-*` container: no network, capped PIDs, capped memory,
    /// and a size-capped tmpfs as the writable workdir. The held `sleep` keeps
    /// it alive so attacks overlap the observation window.
    fn spawn(&mut self, slug: &str) -> Result<RunningContainer> {
        let name = self.fresh_name(slug);
        let pids = self.limits.pids_limit.to_string();
        let mem = format!("{}m", self.limits.mem_mib);
        let tmpfs = format!("{WORKDIR}:rw,size={}m", self.limits.disk_mib);
        let out = self.boxx.run(&[
            "docker",
            "run",
            "-d",
            "--rm",
            "--name",
            &name,
            "--network",
            "none",
            "--pids-limit",
            &pids,
            "--memory",
            &mem,
            "--tmpfs",
            &tmpfs,
            "--label",
            "hugit.wp=c5b",
            &self.image,
            // held command: overlaps the attack with the observation.
            "sleep",
            "300",
        ])?;
        if !out.ok() {
            bail!("spawn {name} failed: {}", out.stderr.trim());
        }
        self.spawned.push(name.clone());
        Ok(RunningContainer { name })
    }

    /// Run a script inside `c` and return the captured output.
    fn exec(&self, c: &RunningContainer, script: &str) -> Result<String> {
        let out = self
            .boxx
            .run(&["docker", "exec", &c.name, "sh", "-c", script])
            .with_context(|| format!("exec in {}", c.name))?;
        Ok(out.stdout)
    }

    /// Run every attack vector and return one [`ContainmentReport`] each.
    ///
    /// # Errors
    /// Fails only if the box is unreachable. A *contained* attack is a normal
    /// `Ok` report; an escape is reported as `RedTeamOutcome::Escaped` (the
    /// caller asserts containment).
    pub fn run_all(&mut self) -> Result<Vec<ContainmentReport>> {
        let mut reports = Vec::with_capacity(AttackVector::all().len());
        for v in AttackVector::all() {
            reports.push(self.run_one(v)?);
        }
        Ok(reports)
    }

    /// Attempt a single attack and judge containment.
    ///
    /// # Errors
    /// Fails only if the box is unreachable.
    pub fn run_one(&mut self, vector: AttackVector) -> Result<ContainmentReport> {
        match vector {
            AttackVector::Traversal => self.attack_traversal(),
            AttackVector::SymlinkEscape => self.attack_symlink(),
            AttackVector::OutOfFence => self.attack_out_of_fence(),
            AttackVector::ForkBomb => self.attack_fork_bomb(),
            AttackVector::DiskFill => self.attack_disk_fill(),
            AttackVector::FenceMaterializedEscape => self.attack_fence_materialized_escape(),
        }
    }

    /// **Traversal:** from the fenced workdir, try to `..`-climb to a REAL host
    /// secret. A genuine target: [`ensure_host_secret`] plants a file with
    /// [`HOST_SECRET_SENTINEL`] on the box host, OUTSIDE every container mount
    /// namespace. The attack `cd`s into WORKDIR and reads the secret's absolute
    /// path through a long `../` chain (which resolves to `/` then down). If the
    /// container shared the host filesystem, the sentinel would print → BREACH.
    /// Containment = the sentinel never appears (the host file is unreachable
    /// across the mount-namespace boundary). NOTE: this can only pass because
    /// the boundary holds — if it were removed, the real planted secret leaks
    /// and this vector reports ESCAPE.
    fn attack_traversal(&mut self) -> Result<ContainmentReport> {
        let host_secret = self.ensure_host_secret()?;
        let c = self.spawn("traversal")?;
        // `../`×12 from WORKDIR saturates at `/`; appending the host secret's
        // absolute tail probes whether the host fs bled into the container.
        let tail = host_secret.trim_start_matches('/');
        let script = format!(
            "cd {WORKDIR} 2>/dev/null || true; \
             out=$(cat ../../../../../../../../../../../../{tail} 2>/dev/null); \
             case \"$out\" in \
               *{HOST_SECRET_SENTINEL}*) printf BREACH ;; \
               *) printf CONTAINED_ENOENT ;; \
             esac"
        );
        let out = self.exec(&c, &script)?;
        let contained = out.contains("CONTAINED_ENOENT") && !out.contains("BREACH");
        Ok(ContainmentReport {
            vector: AttackVector::Traversal,
            outcome: outcome(contained),
            evidence: format!("traversal observed: {} (real host target)", out.trim()),
        })
    }

    /// **Symlink escape:** create a symlink to a REAL host secret and try to
    /// read through it. The target is the genuine [`HOST_SECRET_SENTINEL`] file
    /// planted on the box host by [`ensure_host_secret`] — it really exists, so
    /// a working symlink would yield its content. Containment = the link target
    /// is outside the container's mount namespace, so reading through it yields
    /// NO sentinel (the host secret never crosses). The target EXISTS on the
    /// host, so this is not a vacuous dangling-link pass: only the boundary
    /// makes the read empty.
    fn attack_symlink(&mut self) -> Result<ContainmentReport> {
        let host_secret = self.ensure_host_secret()?;
        let c = self.spawn("symlink")?;
        // Symlink the in-workdir link at the REAL absolute host path. If the
        // container shared the host fs, reading the link returns the sentinel.
        let script = format!(
            "ln -sf '{host_secret}' {WORKDIR}/link 2>/dev/null; \
             content=$(cat {WORKDIR}/link 2>/dev/null); \
             case \"$content\" in \
               *{HOST_SECRET_SENTINEL}*) printf BREACH ;; \
               *) printf CONTAINED_SYMLINK_DEAD ;; \
             esac"
        );
        let out = self.exec(&c, &script)?;
        let contained = out.contains("CONTAINED_SYMLINK_DEAD") && !out.contains("BREACH");
        Ok(ContainmentReport {
            vector: AttackVector::SymlinkEscape,
            outcome: outcome(contained),
            evidence: format!("symlink observed: {} (real host target)", out.trim()),
        })
    }

    /// **Out-of-fence write:** the attacker container tries to MUTATE a REAL
    /// out-of-fence target — the host secret planted by [`ensure_host_secret`],
    /// which lives outside its mount namespace. The attack overwrites the link's
    /// absolute path with attacker bytes; containment = the host file is
    /// UNCHANGED afterwards (the container could not write across the boundary).
    /// This genuinely attempts a cross-boundary write to a file that really
    /// exists — a broken boundary would corrupt it, which the post-check detects.
    fn attack_out_of_fence(&mut self) -> Result<ContainmentReport> {
        let host_secret = self.ensure_host_secret()?;
        let attacker = self.spawn("attacker")?;
        let tamper = "OUT-OF-FENCE-TAMPER-ed1f9c2a";
        // Attacker attempts to overwrite the REAL host secret via its absolute
        // path (and via a `..`-climb), then truncate it — every cross-boundary
        // write must fail. We do not assert the in-container rc (a write to an
        // unreachable path may "succeed" into a private overlay); the authority
        // is the HOST-side post-check below.
        let _ = self.exec(
            &attacker,
            &format!(
                "printf %s '{tamper}' > '{host_secret}' 2>/dev/null; \
                 printf %s '{tamper}' > ../../../../../../../../../../../..{host_secret} 2>/dev/null; \
                 echo done"
            ),
        )?;
        // HOST-side authority: read the real file back. It must still hold the
        // original sentinel and NOT the attacker's tamper bytes.
        let after = self
            .boxx
            .run(&["sh", "-c", &format!("cat '{host_secret}' 2>/dev/null")])?;
        let body = after.stdout;
        let unchanged = body.contains(HOST_SECRET_SENTINEL) && !body.contains(tamper);
        Ok(ContainmentReport {
            vector: AttackVector::OutOfFence,
            outcome: outcome(unchanged),
            evidence: format!(
                "host secret intact={unchanged} (sentinel present & tamper absent — \
                 a cross-boundary write would have replaced it)"
            ),
        })
    }

    /// **Fork bomb:** launch a classic fork bomb (held, so it saturates and
    /// stays) and observe from the **host** that the container's live PID count
    /// is bounded by `pids_limit` — the box is never starved.
    fn attack_fork_bomb(&mut self) -> Result<ContainmentReport> {
        let c = self.spawn("forkbomb")?;
        let cap = u64::from(self.limits.pids_limit);

        // Launch a SATURATING spawner: tightly fork long-lived (`sleep`) children
        // so `pids.current` climbs and HOLDS at the cap (a classic `:(){...}`
        // bomb under non-interactive `sh -c` does not reliably replicate, and its
        // transient children evade `docker top`). Each child holds a PID, so the
        // cgroup quickly refuses further forks (fork → EAGAIN) — recorded in
        // `pids.events: max`.
        let _ = self.boxx.run(&[
            "docker",
            "exec",
            "-d",
            &c.name,
            "sh",
            "-c",
            "while :; do sleep 30 & done",
        ]);

        // Observe from the HOST via the container's pids cgroup — NOT `docker
        // exec` (a saturated container cannot even fork the observer) and NOT
        // `docker top` (misses transient children). cgroup v2 exposes the exact
        // live count (`pids.current`), the cap (`pids.max`), and a refusal
        // counter (`pids.events: max N`). Resolve the container id, then sample
        // the peak `pids.current` and the refusal count over a short window.
        let id = self
            .boxx
            .run(&["docker", "inspect", "-f", "{{.Id}}", &c.name])
            .with_context(|| format!("resolving container id for {}", c.name))?;
        let id = id.stdout.trim().to_string();
        // cgroup v2 path on a systemd host; fall back to the cgroupfs driver path.
        let scope = format!("/sys/fs/cgroup/system.slice/docker-{id}.scope");
        let alt = format!("/sys/fs/cgroup/docker/{id}");
        let sample = self
            .boxx
            .run(&[
                "sh",
                "-c",
                &format!(
                    "d={scope}; [ -d \"$d\" ] || d={alt}; \
                     peak=0; refused=0; capmax=0; \
                     for _ in 1 2 3 4 5; do \
                       cur=$(cat \"$d/pids.current\" 2>/dev/null); \
                       [ -n \"$cur\" ] && [ \"$cur\" -gt \"$peak\" ] && peak=$cur; \
                       m=$(cat \"$d/pids.max\" 2>/dev/null); [ -n \"$m\" ] && capmax=$m; \
                       r=$(awk '/^max /{{print $2}}' \"$d/pids.events\" 2>/dev/null); \
                       [ -n \"$r\" ] && [ \"$r\" -gt \"$refused\" ] && refused=$r; \
                       sleep 1; \
                     done; \
                     printf 'peak=%s refused=%s capmax=%s' \"$peak\" \"$refused\" \"$capmax\""
                ),
            ])
            .with_context(|| format!("reading pids cgroup for {}", c.name))?;
        let kv = |k: &str| -> u64 {
            sample
                .stdout
                .split_whitespace()
                .find_map(|t| t.strip_prefix(&format!("{k}=")))
                .and_then(|v| v.parse().ok())
                .unwrap_or(u64::MAX)
        };
        let peak = kv("peak");
        let refused = kv("refused");
        // Two conditions, BOTH required:
        //  (a) the cap BIT: the spawner saturated the cgroup — either the kernel
        //      recorded ≥1 refused fork (`pids.events: max`) OR the peak reached
        //      the neighbourhood of the cap. A fizzled bomb proves nothing.
        //  (b) the cap HELD: the live count never exceeded the cap (the box pid
        //      space is untouched).
        let saturation_floor = cap / 2;
        let cap_bit = refused >= 1 || (peak != u64::MAX && peak >= saturation_floor);
        let cap_held = peak <= cap;
        let contained = cap_bit && cap_held;
        Ok(ContainmentReport {
            vector: AttackVector::ForkBomb,
            outcome: outcome(contained),
            evidence: format!(
                "pids.current peak={peak} cap={cap} refused-forks={refused} \
                 (cap_bit={cap_bit} @floor={saturation_floor}, cap_held={cap_held})"
            ),
        })
    }

    /// **Disk fill:** `dd` far more than the workdir cap into the size-capped
    /// tmpfs and confirm the bytes-on-disk never exceed the cap — the box disk
    /// is never filled.
    fn attack_disk_fill(&mut self) -> Result<ContainmentReport> {
        let c = self.spawn("diskfill")?;
        let cap = self.limits.disk_mib;
        // Attempt to write 16x the cap; the tmpfs size limit must truncate it.
        // Capture dd's exit status so we can prove the write actually hit the
        // limit (ENOSPC), not that it silently fit. `dd` returns non-zero when
        // it cannot write the requested count to a full filesystem.
        let want = cap * 16;
        let script = format!(
            "dd if=/dev/zero of={WORKDIR}/fill bs=1M count={want} 2>err.$$; rc=$?; \
             enospc=0; grep -qi 'No space left' err.$$ 2>/dev/null && enospc=1; \
             used=$(du -m {WORKDIR}/fill 2>/dev/null | cut -f1); \
             rm -f err.$$; \
             printf 'used=%sMB rc=%s enospc=%s' \"${{used:-0}}\" \"$rc\" \"$enospc\""
        );
        let out = self.exec(&c, &script)?;
        let parse_kv = |k: &str| -> Option<&str> {
            out.split_whitespace()
                .find_map(|tok| tok.strip_prefix(&format!("{k}=")))
        };
        let used: u32 = parse_kv("used")
            .and_then(|s| s.strip_suffix("MB"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(u32::MAX);
        let rc: i32 = parse_kv("rc").and_then(|s| s.parse().ok()).unwrap_or(-1);
        let enospc: u32 = parse_kv("enospc").and_then(|s| s.parse().ok()).unwrap_or(0);
        // Two conditions, BOTH required:
        //  (a) the cap BIT: the write hit the tmpfs limit — `dd` failed (rc!=0)
        //      and/or ENOSPC was observed. A write that simply "fit" proves
        //      nothing (it never reached the cap).
        //  (b) the cap HELD: bytes-on-disk never exceeded the cap (+1 MiB slack
        //      for filesystem accounting). The box disk is never filled.
        let cap_bit = rc != 0 || enospc == 1;
        let cap_held = used <= cap + 1;
        let contained = cap_bit && cap_held;
        Ok(ContainmentReport {
            vector: AttackVector::DiskFill,
            outcome: outcome(contained),
            evidence: format!(
                "disk used={used}MB cap={cap}MB requested={want}MB \
                 (cap_bit={cap_bit} rc={rc}/enospc={enospc}, cap_held={cap_held})"
            ),
        })
    }

    /// **Fence-materialized escape — the fence is the only control here.**
    ///
    /// Unlike the other vectors (which lean on the Docker mount/pid/tmpfs
    /// namespace), this materializes a *real* [`FenceManifest`] into a single
    /// container via the production `materialize_sparse` path, then — inside the
    /// **same** container — proves:
    /// 1. the in-fence file (`src/in.txt`) **is** present and readable, and
    /// 2. the out-of-fence files (`secret.env`, a traversal escape) are
    ///    **ENOENT** (never materialized), checked with [`probe_outside_enoent`].
    ///
    /// Containment ⇔ both hold. The load-bearing property: if `classify()` (the
    /// fence core) were replaced by a constant `Inside`, `select_in_fence` would
    /// materialize `secret.env` too — the out-of-fence probe would observe it
    /// PRESENT — and this vector would report **Escaped**. The acceptance oracle
    /// therefore catches a no-op classifier on this vector alone.
    fn attack_fence_materialized_escape(&mut self) -> Result<ContainmentReport> {
        let c = self.spawn("fence-mat")?;

        // A REAL fence: only `src/` is in the path_set. The attacker offers an
        // out-of-fence secret as a candidate; the fence must drop it.
        let manifest = FenceManifest {
            path_set: vec!["src/".to_string()],
            deny_default: true,
            materialized: vec![],
        };
        let candidates = vec![
            CandidateEntry::new("src/in.txt", b"in-fence-content".to_vec()),
            CandidateEntry::new("secret.env", b"OUT-OF-FENCE-TOKEN".to_vec()),
        ];

        let filled = materialize_sparse(self.boxx, &c, WORKDIR, &manifest, &candidates)
            .map_err(|e| anyhow::anyhow!("materialize for fence-escape vector: {e}"))?;

        // The materialized record must contain ONLY the in-fence file. (If the
        // classifier were constant-Inside, secret.env would appear here.)
        let mat: Vec<&str> = filled
            .materialized
            .iter()
            .map(|m| m.path.as_str())
            .collect();
        let only_in_fence = mat == ["src/in.txt"];

        // In-container truth: in-fence present, out-of-fence ENOENT.
        let in_present = !probe_outside_enoent(self.boxx, &c, WORKDIR, "src/in.txt")?.enoent;
        let secret_absent = probe_outside_enoent(self.boxx, &c, WORKDIR, "secret.env")?.enoent;
        let traversal_absent =
            probe_outside_enoent(self.boxx, &c, WORKDIR, "src/../secret.env")?.enoent;

        let contained = only_in_fence && in_present && secret_absent && traversal_absent;
        Ok(ContainmentReport {
            vector: AttackVector::FenceMaterializedEscape,
            outcome: outcome(contained),
            evidence: format!(
                "materialized={mat:?} in_present={in_present} \
                 secret_absent={secret_absent} traversal_absent={traversal_absent} \
                 (escape iff out-of-fence file was materialized → classify() broke)"
            ),
        })
    }

    /// Force-remove **only** the `hugit-c5b-*` containers this harness spawned,
    /// and re-scan to confirm box residue for the prefix is **0**.
    ///
    /// # Errors
    /// Fails only if the box is unreachable.
    pub fn teardown_all(&mut self) -> Result<RedTeamResidue> {
        for name in &self.spawned {
            debug_assert!(name.starts_with(REDTEAM_PREFIX));
            let _ = self.boxx.run(&["docker", "rm", "-f", name]);
        }
        self.spawned.clear();
        // Remove the planted host secret (prefix-scoped, host-side). Best-effort:
        // the forensic residue scan below is container-scoped; the host secret
        // lives under /tmp/hugit-c5b-* and is cleaned here so the box is left
        // exactly as found.
        if let Some(path) = self.host_secret.take() {
            debug_assert!(
                path.starts_with(&format!("/tmp/{REDTEAM_PREFIX}")),
                "host secret must be prefix-scoped before removal"
            );
            let _ = self.boxx.run(&["rm", "-f", &path]);
        }
        // Re-scan: no hugit-c5b-* container may remain (running or stopped).
        let scan = self.boxx.run(&[
            "docker",
            "ps",
            "-aq",
            "--filter",
            &format!("name={REDTEAM_PREFIX}"),
            "--format",
            "{{.Names}}",
        ])?;
        let remaining: Vec<String> = scan
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToString::to_string)
            .collect();
        Ok(RedTeamResidue { remaining })
    }
}

/// Box residue after a red-team teardown, scoped to the `hugit-c5b-*` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedTeamResidue {
    /// Any `hugit-c5b-*` containers still present (must be empty).
    pub remaining: Vec<String>,
}

impl RedTeamResidue {
    /// `true` iff zero `hugit-c5b-*` residue remains on the box.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.remaining.is_empty()
    }
}

fn outcome(contained: bool) -> RedTeamOutcome {
    if contained {
        RedTeamOutcome::Contained
    } else {
        RedTeamOutcome::Escaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lease::CmdOutput;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    /// A hermetic, in-memory fake of the box filesystem — **no network, no
    /// Docker, no box**. It interprets the *exact* shell scripts the production
    /// fence emits:
    ///   - `place_file` (via `materialize_sparse`): `mkdir -p '<dir>' && printf
    ///     %s '<b64>' | base64 -d > '<full>'` — we record `<full>` (and its
    ///     ancestor dirs) as present.
    ///   - `probe_outside_enoent`: `if test ! -e '<p>'; then printf ABSENT;
    ///     elif test -d '<p>'; then printf PRESENT_DIR; else printf
    ///     PRESENT_FILE; fi` — we answer from the recorded set.
    ///
    /// Because the test drives the REAL `materialize_sparse` (which routes every
    /// candidate through `select_in_fence` → `is_admitted` → `classify`) and the
    /// REAL `probe_outside_enoent`, the only thing deciding whether the
    /// out-of-fence file gets "written" is `classify`. If `classify` were
    /// replaced by a constant `Inside`, `select_in_fence` would admit
    /// `secret.env`, this fake would record it as present, and the probe would
    /// observe PRESENT_FILE → the assertion that it is ENOENT goes RED. The test
    /// is therefore LOAD-BEARING on the fence core, in the BARE `cargo test`
    /// gate, with no box.
    struct FakeFsBox {
        /// Absolute in-container paths that "exist" (files and their dirs).
        files: RefCell<BTreeSet<String>>,
        dirs: RefCell<BTreeSet<String>>,
    }

    impl FakeFsBox {
        fn new() -> Self {
            Self {
                files: RefCell::new(BTreeSet::new()),
                dirs: RefCell::new(BTreeSet::new()),
            }
        }

        /// Extract the i-th single-quoted token from a `sh -c` script. The fence
        /// scripts single-quote every interpolated path via `shell_quote`.
        fn quoted(script: &str) -> Vec<String> {
            let mut out = Vec::new();
            let bytes = script.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    let start = i + 1;
                    let mut j = start;
                    while j < bytes.len() && bytes[j] != b'\'' {
                        j += 1;
                    }
                    out.push(script[start..j].to_string());
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            out
        }

        fn add_dirs_for(&self, full: &str) {
            // Record every ancestor directory of `full` as present (excluding
            // the file itself).
            let parts: Vec<&str> = full.trim_start_matches('/').split('/').collect();
            let mut cur = String::new();
            for p in &parts[..parts.len().saturating_sub(1)] {
                cur.push('/');
                cur.push_str(p);
                self.dirs.borrow_mut().insert(cur.clone());
            }
        }
    }

    impl BoxExec for FakeFsBox {
        fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
            // The fence always invokes `docker exec <name> sh -c <script>`.
            let script = argv.last().copied().unwrap_or("");
            let toks = Self::quoted(script);

            // A `place_file` write: contains "base64 -d >" and the redirect
            // target is the LAST quoted token (the full path).
            if script.contains("base64 -d >") {
                if let Some(full) = toks.last() {
                    self.add_dirs_for(full);
                    self.files.borrow_mut().insert(full.clone());
                }
                return Ok(CmdOutput {
                    code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }

            // A `probe_outside_enoent` probe: `test ! -e '<p>' ...`. The probed
            // path is the (single, repeated) quoted token.
            if script.contains("test ! -e") {
                let p = toks.first().cloned().unwrap_or_default();
                let observed = if self.files.borrow().contains(&p) {
                    "PRESENT_FILE"
                } else if self.dirs.borrow().contains(&p) {
                    "PRESENT_DIR"
                } else {
                    "ABSENT"
                };
                return Ok(CmdOutput {
                    code: Some(0),
                    stdout: observed.to_string(),
                    stderr: String::new(),
                });
            }

            // Any other command (none expected) succeeds with no output.
            Ok(CmdOutput {
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    /// HERMETIC fence-materialized-escape — the box-gated `item_5` vector's
    /// property, proven in the BARE `cargo test` gate (no box, no network).
    ///
    /// Drives the REAL `materialize_sparse` + `probe_outside_enoent` against a
    /// fake in-memory FS. The fence (`classify` + `select_in_fence`) is the only
    /// control: the in-fence file is materialized (PRESENT) and the out-of-fence
    /// candidate is dropped (ENOENT). FAIL-not-skip; LOAD-BEARING: a
    /// constant-`Inside` classifier would admit `secret.env`, the fake would
    /// record it present, and `secret_absent` would be false → RED.
    #[test]
    fn fence_materialized_escape_is_contained_hermetically_no_box() {
        let boxx = FakeFsBox::new();
        let c = RunningContainer {
            name: "hermetic-fence".to_string(),
        };

        // A REAL fence: only `src/` is in the path_set; `secret.env` is offered
        // as an out-of-fence candidate the fence must drop.
        let manifest = FenceManifest {
            path_set: vec!["src/".to_string()],
            deny_default: true,
            materialized: vec![],
        };
        let candidates = vec![
            CandidateEntry::new("src/in.txt", b"in-fence-content".to_vec()),
            CandidateEntry::new("secret.env", b"OUT-OF-FENCE-TOKEN".to_vec()),
        ];

        let filled = materialize_sparse(&boxx, &c, WORKDIR, &manifest, &candidates)
            .expect("hermetic materialize must succeed");

        // The materialized record must contain ONLY the in-fence file.
        let mat: Vec<&str> = filled
            .materialized
            .iter()
            .map(|m| m.path.as_str())
            .collect();
        assert_eq!(
            mat,
            ["src/in.txt"],
            "fence must materialize ONLY the in-fence file; got {mat:?}"
        );

        // In-container truth via the REAL probe: in-fence present, out-of-fence
        // ENOENT, traversal ENOENT.
        let in_present = !probe_outside_enoent(&boxx, &c, WORKDIR, "src/in.txt")
            .expect("probe in-fence")
            .enoent;
        let secret_absent = probe_outside_enoent(&boxx, &c, WORKDIR, "secret.env")
            .expect("probe out-of-fence")
            .enoent;
        let traversal_absent = probe_outside_enoent(&boxx, &c, WORKDIR, "src/../secret.env")
            .expect("probe traversal")
            .enoent;

        assert!(
            in_present,
            "the in-fence file must be materialized (present)"
        );
        assert!(
            secret_absent,
            "the out-of-fence file MUST be ENOENT (never materialized) — \
             this is the property a no-op classifier would break"
        );
        assert!(traversal_absent, "a traversal escape must be ENOENT");
    }

    /// Guards the guard: prove the hermetic test above would actually CATCH a
    /// broken (constant-`Inside`) classifier. We simulate that failure directly
    /// — admitting the out-of-fence candidate — and assert the fake records it
    /// as PRESENT (i.e. `secret_absent` would be false). This pins that the
    /// hermetic oracle is load-bearing, not a tautology.
    #[test]
    fn hermetic_oracle_would_go_red_if_out_of_fence_were_materialized() {
        let boxx = FakeFsBox::new();
        let c = RunningContainer {
            name: "hermetic-fence-red".to_string(),
        };
        // Simulate a broken classifier by placing the out-of-fence file the same
        // way `place_file` would (the exact script shape).
        let full = format!("{WORKDIR}/secret.env");
        let dir = full.rsplit_once('/').map_or("/", |(d, _)| d);
        let script = format!("mkdir -p '{dir}' && printf %s 'eA==' | base64 -d > '{full}'");
        boxx.run(&["docker", "exec", &c.name, "sh", "-c", &script])
            .unwrap();
        let secret_absent = probe_outside_enoent(&boxx, &c, WORKDIR, "secret.env")
            .unwrap()
            .enoent;
        assert!(
            !secret_absent,
            "if the out-of-fence file IS materialized, the probe must observe it \
             present — proving the hermetic oracle goes RED under a broken fence"
        );
    }

    #[test]
    fn all_vectors_distinct() {
        let all = AttackVector::all();
        assert_eq!(all.len(), 6);
        let slugs: std::collections::BTreeSet<_> = all.iter().map(|v| v.slug()).collect();
        assert_eq!(slugs.len(), 6, "vectors must be distinct");
        // The fence-as-the-control vector must be present.
        assert!(all.contains(&AttackVector::FenceMaterializedEscape));
    }

    #[test]
    fn default_limits_are_conservative() {
        let l = ContainerLimits::default();
        assert!(l.pids_limit > 0 && l.pids_limit <= 64);
        assert!(l.disk_mib > 0 && l.disk_mib <= 64);
    }

    #[test]
    fn residue_zero_when_empty() {
        assert!(RedTeamResidue { remaining: vec![] }.is_zero());
        assert!(
            !RedTeamResidue {
                remaining: vec!["hugit-c5b-x".to_string()]
            }
            .is_zero()
        );
    }

    #[test]
    fn containment_report_judges_outcome() {
        let r = ContainmentReport {
            vector: AttackVector::ForkBomb,
            outcome: RedTeamOutcome::Contained,
            evidence: "x".to_string(),
        };
        assert!(r.is_contained());
    }
}
