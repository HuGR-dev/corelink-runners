// Transplanted from hugit/crates/hugit-runner @ ead800d83d19bfd7f90bf4241ee27b18b09007f1 (runner-transfer campaign R2, 2026-06-10) — wire-contract seam, no git dep.
//! Lease lifecycle: turn a frozen [`RunnerLease`] into a per-job container
//! spec, and drive commands on the runner box.
//!
//! The [`RunnerLease`] is **consumed, never modified** (it is frozen by
//! WP-00). C2a reads `lease_id`, `tmp_root`, `net_policy`, and `path_set` to
//! build an engine-agnostic [`ContainerSpec`]. Fence path enforcement (ENOENT)
//! is C5a; here the `path_set` only scopes the materialized view, and the
//! lease carries **no raw credentials** by construction (the broker is C5b).

use std::process::Command;

use anyhow::{Context, Result, bail};
use corelink_runners_contracts::RunnerLease;

use crate::namespace::JOB_PREFIX;

/// Network policy semantics understood by the v0 runner.
///
/// C2a's isolation contract requires an **isolated network namespace**. The
/// accepted isolated policy names are `none` · `isolated` · `deny-all` · `""`.
/// (`hermetic` is deliberately NOT an alias — hugit uses `none`, which matches
/// the frozen rule; a second spelling is surface with no gain, 2026-07-07.) Any
/// other policy name is rejected as out of scope for C2a (egress policies are a
/// later, broker-mediated concern).
fn requires_no_network(net_policy: &str) -> bool {
    matches!(net_policy, "none" | "isolated" | "deny-all" | "")
}

/// An engine-agnostic, per-job container spec derived from a [`RunnerLease`].
///
/// Engine-agnostic on purpose: the Firecracker upgrade path (see crate docs)
/// reuses this spec unchanged. It holds no Docker-specific fields beyond the
/// image name.
///
/// `Debug` is HAND-WRITTEN to REDACT every `env` value (audit P2-3): at provision
/// time `env` carries injected per-job CAPABILITIES — the §13.2 ingest credential
/// (check/envelope path) or the `CORELINK_RUNNER_JITCONFIG` GitHub runner
/// registration credential (runner path). The derived `Debug` would render those
/// in full, and this type crosses the `BoxProvisioner` seam — one
/// `tracing::debug!(?spec)` away from writing a live credential to the fabric log,
/// on a fabric whose trust model is "no secret ever in a log". The manual impl
/// prints env KEYS only, mirroring every other secret-holder here (`BearerPat`,
/// `JitRunnerConfig`, `IngestSigner`, `AutoscalerConfig`).
#[derive(Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    /// Stable per-job container name, derived from the lease id. One lease →
    /// one job → one container.
    pub name: String,
    /// Container image to run the job in.
    pub image: String,
    /// In-container path mounted as a private tmpfs (the lease's `tmp_root`).
    pub tmp_root: String,
    /// Whether the container runs with **no** network device (`--network
    /// none`). `true` for every CHECK lease (the hermetic / hugit / §3 exec
    /// path) — the C2a isolation floor. `false` ONLY for a runner lease, and
    /// only in concert with `allow_egress`.
    pub no_network: bool,
    /// Egress permission. `false` everywhere by default; set `true` ONLY by
    /// [`ContainerSpec::from_runner_lease`] (ADR-0007), reachable ONLY from the
    /// trusted runner-acquire path. The engine isolation floor admits a
    /// `no_network == false` spec **iff** `allow_egress == true`, so the egress
    /// decision can never be flipped by a caller-supplied `net_policy` string —
    /// a forged/typo'd policy on a CHECK lease can never leak egress. (ADR-0003
    /// accepts outbound egress on the managed tier; this gates it to runner
    /// leases so the hermetic/check posture is unchanged.)
    pub allow_egress: bool,
    /// Whether the box's command runs immediately at provision (Northflank
    /// `runOnCreate: true`). `false` for CHECK leases (the fabric drives the
    /// command per `/exec`). `true` ONLY for a runner lease, whose image
    /// entrypoint launches the GitHub Actions runner agent autonomously.
    pub run_on_create: bool,
    /// Paths the lease scopes the materialized view to (informational in C2a;
    /// fence enforcement is C5a).
    pub path_set: Vec<String>,
    /// Additional environment variables injected into the box at provision
    /// (`(name, value)` pairs, applied in order). EMPTY by default — the
    /// hermetic on-box path (`--network none` Docker) injects nothing, and
    /// `from_lease` never populates it. The cloud provision path (Northflank)
    /// uses it to deliver the §13.2 envelope ingest URL + the lease credential
    /// so the in-box agent loop can reach the fabric's trajectory turn-feed
    /// endpoint (`CORELINK_ENVELOPE_INGEST_URL`). Never carries box secrets
    /// beyond that brokered credential.
    pub env: Vec<(String, String)>,
}

/// Redacting `Debug` (audit P2-3): every `env` VALUE is replaced with
/// `***REDACTED***` so an injected credential (the §13.2 ingest token or the
/// `CORELINK_RUNNER_JITCONFIG` runner registration) can never reach a log via
/// `{:?}`. Keys are shown (useful for triage; they are not secret).
impl std::fmt::Debug for ContainerSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let env_keys: Vec<(&str, &str)> = self
            .env
            .iter()
            .map(|(k, _)| (k.as_str(), "***REDACTED***"))
            .collect();
        f.debug_struct("ContainerSpec")
            .field("name", &self.name)
            .field("image", &self.image)
            .field("tmp_root", &self.tmp_root)
            .field("no_network", &self.no_network)
            .field("allow_egress", &self.allow_egress)
            .field("run_on_create", &self.run_on_create)
            .field("path_set", &self.path_set)
            .field("env", &env_keys)
            .finish()
    }
}

impl ContainerSpec {
    /// Derive a per-job container spec from a frozen lease.
    ///
    /// # Errors
    /// Fails if the lease id is empty, `tmp_root` is empty or unsafe, the
    /// `net_policy` is not a C2a-supported isolated policy, or `image` is not
    /// content-(digest)-pinned (`repo@sha256:<64-hex>`). The pin requirement is
    /// the supply-chain floor (WP-X4): an unpinned image is rejected here,
    /// before the box is ever touched (fail-closed). Integrity verification of
    /// the pin against the box happens at [`Engine::spawn`](crate::isolation::Engine::spawn)
    /// time, before `docker run`.
    pub fn from_lease(lease: &RunnerLease, image: &str) -> Result<Self> {
        Self::validate_lease_image(lease, image)?;
        // CHECK lease (hermetic / hugit / §3 exec): network-isolated only.
        if !requires_no_network(&lease.net_policy) {
            bail!(
                "net_policy {:?} is not isolated; C2a v0 supports only \
                 network-isolated leases",
                lease.net_policy
            );
        }
        Ok(Self {
            name: container_name(&lease.lease_id),
            image: image.to_string(),
            tmp_root: lease.tmp_root.clone(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: lease.path_set.clone(),
            // No env at spec-build time: the cloud provision path adds the
            // §13.2 envelope ingest vars additively after `from_lease` (the
            // hermetic Docker path stays env-free).
            env: Vec::new(),
        })
    }

    /// Derive a **runner-lease** spec (ADR-0007): an egress-allowed,
    /// GitHub-driven, run-on-create box whose image entrypoint launches the
    /// GitHub Actions runner agent. This is the **only** constructor that sets
    /// `allow_egress = true`; it is reached **only** from the trusted
    /// runner-acquire path, never from a `net_policy` string, so a CHECK lease
    /// can never obtain egress through it (a forged `net_policy` on a check
    /// lease flows through [`from_lease`](Self::from_lease) and is rejected).
    ///
    /// # Errors
    /// Same supply-chain + tmp_root floors as [`from_lease`](Self::from_lease)
    /// (the runner image MUST still be digest-pinned — X4 is not bypassed),
    /// plus a defense-in-depth check that the lease's `net_policy` is the
    /// runner egress policy `"egress-runner"`.
    pub fn from_runner_lease(lease: &RunnerLease, image: &str) -> Result<Self> {
        Self::validate_lease_image(lease, image)?;
        // Defense in depth: a runner lease must carry the explicit egress
        // policy. (The egress DECISION is this constructor being called from the
        // runner-acquire path; this string check is a second, independent gate —
        // never the sole source of the egress grant.)
        if lease.net_policy != "egress-runner" {
            bail!(
                "runner lease requires net_policy=\"egress-runner\", got {:?}",
                lease.net_policy
            );
        }
        Ok(Self {
            name: container_name(&lease.lease_id),
            image: image.to_string(),
            tmp_root: lease.tmp_root.clone(),
            no_network: false,
            allow_egress: true,
            run_on_create: true,
            path_set: lease.path_set.clone(),
            env: Vec::new(),
        })
    }

    /// Derive an **agent-lease** spec (agent-exec, ratified (B) exec-server-drive
    /// with hugit 2026-07-05): an egress-allowed, NON-memoized box that hugit's
    /// OFF-box §13 agent loop drives via `POST /v1/leases/{id}/agent-exec`. Egress
    /// like a runner box, but WITHOUT the runner's GitHub-Actions machinery:
    /// `run_on_create = false` (the box is exec-driven — it waits for agent-exec
    /// commands, exactly like a check-host box, rather than self-launching an
    /// agent on create). Reached **only** from the trusted agent-acquire path,
    /// never from a `net_policy` string, so a CHECK lease can never obtain egress
    /// through it (a forged `net_policy` on a check lease flows through
    /// [`from_lease`](Self::from_lease) and is rejected).
    ///
    /// # Errors
    /// Same supply-chain + tmp_root floors as [`from_lease`](Self::from_lease)
    /// (the agent image MUST still be digest-pinned — X4 is not bypassed), plus a
    /// defense-in-depth check that the lease's `net_policy` is the agent egress
    /// policy `"egress-agent"`.
    pub fn from_agent_lease(lease: &RunnerLease, image: &str) -> Result<Self> {
        Self::validate_lease_image(lease, image)?;
        // Defense in depth: an agent lease must carry the explicit egress policy.
        // (The egress DECISION is this constructor being called from the
        // agent-acquire path; this string check is a second, independent gate —
        // never the sole source of the egress grant.)
        if lease.net_policy != "egress-agent" {
            bail!(
                "agent lease requires net_policy=\"egress-agent\", got {:?}",
                lease.net_policy
            );
        }
        Ok(Self {
            name: container_name(&lease.lease_id),
            image: image.to_string(),
            tmp_root: lease.tmp_root.clone(),
            no_network: false,
            allow_egress: true,
            // Exec-driven, NOT run-on-create: the agent box waits for agent-exec
            // commands (like a check-host box), it does not self-launch anything.
            run_on_create: false,
            path_set: lease.path_set.clone(),
            env: Vec::new(),
        })
    }

    /// Shared spec-build validation (lease id, tmp_root safety, X4 pin).
    fn validate_lease_image(lease: &RunnerLease, image: &str) -> Result<()> {
        if lease.lease_id.trim().is_empty() {
            bail!("RunnerLease.lease_id is empty");
        }
        if lease.tmp_root.trim().is_empty() {
            bail!("RunnerLease.tmp_root is empty");
        }
        // `tmp_root` is interpolated into `--tmpfs <root>:…` and into `sh -c`
        // probe scripts on the box. An unsanitized value (e.g.
        // `"/x' ; touch /pwned ; echo '"`) is a root RCE on the runner box.
        // Restrict to an absolute path over a conservative, shell-inert charset.
        validate_tmp_root(&lease.tmp_root)?;
        // Supply-chain floor: reject any non-content-pinned image at spec-build
        // time so the unpinned/tag path can never reach `docker run`. Holds for
        // runner images too — X4 is never bypassed for runner mode.
        crate::pin::require_pinned(image)?;
        Ok(())
    }
}

/// Validate that `tmp_root` is an absolute path over a shell-inert charset.
///
/// Accepts `^/[A-Za-z0-9._/-]+$` only: a leading `/` then any of
/// alphanumeric, `.`, `_`, `/`, `-`. Every shell metacharacter (space, quote,
/// `;`, `|`, `&`, `$`, backtick, `(`, `)`, newline, …) is excluded, so the
/// value cannot break out of `--tmpfs` or a `sh -c` probe on the box.
///
/// # Errors
/// Fails if `tmp_root` is not absolute or contains a disallowed character.
fn validate_tmp_root(tmp_root: &str) -> Result<()> {
    if !tmp_root.starts_with('/') {
        bail!("RunnerLease.tmp_root {tmp_root:?} must be an absolute path (start with `/`)");
    }
    if tmp_root.len() < 2 {
        bail!("RunnerLease.tmp_root {tmp_root:?} is too short to be a real path");
    }
    if !tmp_root
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
    {
        bail!(
            "RunnerLease.tmp_root {tmp_root:?} contains characters outside \
             ^/[A-Za-z0-9._/-]+$ — refused (shell-injection guard, fail CLOSED)"
        );
    }
    Ok(())
}

/// Sanitize a lease id into a Docker-safe container name. Docker names must
/// match `[a-zA-Z0-9][a-zA-Z0-9_.-]*`.
fn container_name(lease_id: &str) -> String {
    let mut s = String::with_capacity(lease_id.len() + 8);
    s.push_str(JOB_PREFIX);
    for c in lease_id.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    s
}

/// A seam over "run a command on the runner box and capture its output".
///
/// Abstracting the box (rather than hard-coding `ssh`) keeps the lifecycle
/// engine-agnostic and lets the Firecracker upgrade swap the transport. The
/// default implementation, [`SshBox`], drives `ssh` to the live Hetzner box.
pub trait BoxExec {
    /// Run `argv` on the box, returning `(exit_code, stdout, stderr)`.
    ///
    /// `argv` is executed as a single remote shell command (the elements are
    /// shell-quoted and joined). A `None` exit code means the command was
    /// killed by a signal.
    fn run(&self, argv: &[&str]) -> Result<CmdOutput>;

    /// Run `argv` on the box with `stdin` piped to the remote process.
    ///
    /// This is the safe channel for **untrusted bytes** (e.g. a serialized
    /// state payload): the bytes flow over stdin and are never interpolated
    /// into the command string, so no value can break out of the shell. The
    /// default implementation refuses (a transport that cannot stream stdin
    /// must not be handed untrusted payloads).
    ///
    /// # Errors
    /// Fails if the transport cannot stream stdin or the process cannot spawn.
    fn run_with_stdin(&self, _argv: &[&str], _stdin: &[u8]) -> Result<CmdOutput> {
        bail!("this BoxExec transport does not support stdin streaming");
    }
}

/// Captured result of a command run on the box.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    /// Exit status; `None` if the process was killed by a signal.
    pub code: Option<i32>,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

impl CmdOutput {
    /// `true` iff the command exited 0.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// `BoxExec` backed by `ssh` to the live runner box.
///
/// Host is taken from `CORELINK_RUNNER_HOST` (the suite pins
/// `91.99.11.196`). Reads the identity file from `~/.ssh/corelink-runner-01` if
/// present; otherwise relies on the agent / default key. This driver **never**
/// touches the box's ssh/firewall/fail2ban config.
#[derive(Debug, Clone)]
pub struct SshBox {
    /// `user@host` target for ssh.
    pub target: String,
    /// Optional identity file path.
    pub identity: Option<String>,
}

impl SshBox {
    /// Construct from `CORELINK_RUNNER_HOST` (the env the acceptance suite pins),
    /// defaulting the user to `root` and the identity to
    /// `~/.ssh/corelink-runner-01` when that file exists.
    ///
    /// # Errors
    /// Fails if `CORELINK_RUNNER_HOST` is unset/empty.
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("CORELINK_RUNNER_HOST")
            .ok()
            .filter(|h| !h.trim().is_empty())
            .context("CORELINK_RUNNER_HOST is unset; the runner box is required")?;
        let identity = std::env::var("HOME").ok().and_then(|home| {
            let p = format!("{home}/.ssh/corelink-runner-01");
            std::path::Path::new(&p).exists().then_some(p)
        });
        Ok(Self {
            target: format!("root@{host}"),
            identity,
        })
    }
}

/// Path of the pinned `known_hosts` file for runner-box SSH.
///
/// Overridable via `CORELINK_RUNNER_KNOWN_HOSTS`; otherwise `$HOME/.corelink/known_hosts`
/// (falling back to a bare `.corelink/known_hosts` if `HOME` is unset). Paired with
/// `StrictHostKeyChecking=accept-new` this is **trust-on-first-use, pin
/// thereafter**: the first connection records the box's host key, and every
/// later connection is verified against that pin — so a MITM that swaps the host
/// key after first use is refused (unlike `StrictHostKeyChecking=no`, which
/// silently accepts ANY key on EVERY connection and thus pins nothing).
fn known_hosts_path() -> String {
    if let Ok(p) = std::env::var("CORELINK_RUNNER_KNOWN_HOSTS")
        && !p.trim().is_empty()
    {
        return p;
    }
    match std::env::var("HOME") {
        Ok(home) if !home.trim().is_empty() => format!("{home}/.corelink/known_hosts"),
        _ => ".corelink/known_hosts".to_string(),
    }
}

impl BoxExec for SshBox {
    fn run(&self, argv: &[&str]) -> Result<CmdOutput> {
        let remote = shell_join(argv);
        let known_hosts = known_hosts_path();
        let mut cmd = Command::new("ssh");
        if let Some(id) = &self.identity {
            cmd.arg("-i").arg(id);
        }
        // pin-on-first-use: accept-new records the host key on first contact and
        // verifies against the pinned UserKnownHostsFile on every connection
        // thereafter (not the blind-accept of StrictHostKeyChecking=no).
        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg(format!("UserKnownHostsFile={known_hosts}"))
            .arg("-o")
            .arg("ConnectTimeout=15")
            .arg(&self.target)
            .arg(&remote);
        let out = cmd
            .output()
            .with_context(|| format!("failed to spawn ssh to {}", self.target))?;
        Ok(CmdOutput {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn run_with_stdin(&self, argv: &[&str], stdin: &[u8]) -> Result<CmdOutput> {
        use std::io::Write;
        use std::process::Stdio;

        let remote = shell_join(argv);
        let known_hosts = known_hosts_path();
        let mut cmd = Command::new("ssh");
        if let Some(id) = &self.identity {
            cmd.arg("-i").arg(id);
        }
        // pin-on-first-use: accept-new records the host key on first contact and
        // verifies against the pinned UserKnownHostsFile on every connection
        // thereafter (not the blind-accept of StrictHostKeyChecking=no).
        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg(format!("UserKnownHostsFile={known_hosts}"))
            .arg("-o")
            .arg("ConnectTimeout=15")
            .arg(&self.target)
            .arg(&remote)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn ssh to {}", self.target))?;
        // Write the untrusted payload over stdin (never via the command string).
        child
            .stdin
            .take()
            .context("ssh child has no stdin pipe")?
            .write_all(stdin)
            .context("writing payload to ssh stdin")?;
        let out = child
            .wait_with_output()
            .with_context(|| format!("waiting on ssh to {}", self.target))?;
        Ok(CmdOutput {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// POSIX single-quote a command vector into one remote shell string.
///
/// Every argument is **always** single-quoted (no "looks-safe" allowlist
/// passthrough): an allowlist is one missed character away from an injection,
/// so the only safe rule is to quote unconditionally. Embedded single quotes
/// are escaped via the standard `'\''` idiom.
fn shell_join(argv: &[&str]) -> String {
    argv.iter()
        .map(|a| {
            if a.is_empty() {
                "''".to_string()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_runners_contracts::RunnerState;

    /// A content-pinned image reference (the only kind `from_lease` accepts).
    const PIN: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

    fn lease() -> RunnerLease {
        RunnerLease {
            lease_id: "lease/abc 123".to_string(),
            principal_chain: vec!["agent:1".to_string()],
            path_set: vec!["src/".to_string()],
            expiry: 0,
            net_policy: "none".to_string(),
            tmp_root: "/work/tmp".to_string(),
            state: RunnerState::Held,
        }
    }

    #[test]
    fn spec_sanitizes_name_and_forces_no_network() {
        let spec = ContainerSpec::from_lease(&lease(), PIN).unwrap();
        assert_eq!(spec.name, "corelink-job-lease_abc_123");
        assert!(spec.no_network);
        assert_eq!(spec.tmp_root, "/work/tmp");
        assert_eq!(spec.image, PIN);
    }

    #[test]
    fn spec_rejects_non_isolated_policy() {
        let mut l = lease();
        l.net_policy = "egress-allow".to_string();
        assert!(ContainerSpec::from_lease(&l, PIN).is_err());
    }

    /// AUDIT P2-3: `Debug` must REDACT every env value (a provision-time injected
    /// credential — the JIT runner config or §13.2 ingest token — must never
    /// reach a log via `{:?}`), while still showing keys for triage.
    #[test]
    fn debug_redacts_env_values_but_shows_keys() {
        let mut spec = ContainerSpec::from_lease(&lease(), PIN).unwrap();
        spec.env.push((
            "CORELINK_RUNNER_JITCONFIG".to_string(),
            "SUPER-SECRET-jit-registration-token".to_string(),
        ));
        let dbg = format!("{spec:?}");
        assert!(
            !dbg.contains("SUPER-SECRET"),
            "the injected credential value must never appear in Debug, got: {dbg}"
        );
        assert!(
            dbg.contains("***REDACTED***"),
            "env values must be redacted"
        );
        assert!(
            dbg.contains("CORELINK_RUNNER_JITCONFIG"),
            "env keys remain visible for triage"
        );
    }

    // ── ADR-0007 runner-lease egress gate (the security crux) ─────────────────

    /// A CHECK lease NEVER grants egress: `from_lease` always produces the
    /// hermetic posture (no_network=true, allow_egress=false, run_on_create=false).
    #[test]
    fn check_lease_never_grants_egress() {
        let spec = ContainerSpec::from_lease(&lease(), PIN).unwrap();
        assert!(spec.no_network, "check lease must be network-isolated");
        assert!(
            !spec.allow_egress,
            "check lease must NOT carry the egress grant"
        );
        assert!(
            !spec.run_on_create,
            "check lease is fabric-driven, not run-on-create"
        );
    }

    /// RED TEAM: a CHECK lease that smuggles the runner egress policy string is
    /// REJECTED — the egress grant can never be obtained through `from_lease`,
    /// so a forged/typo'd `net_policy` on a hugit/check lease cannot leak egress.
    #[test]
    fn check_lease_forging_egress_policy_is_rejected_not_granted() {
        let mut l = lease();
        l.net_policy = "egress-runner".to_string(); // the runner policy, on a CHECK lease
        // `from_lease` (the check path) does not admit it → no spec, no egress.
        assert!(
            ContainerSpec::from_lease(&l, PIN).is_err(),
            "a check lease must never be buildable with the runner egress policy"
        );
    }

    /// A RUNNER lease grants egress — but ONLY via `from_runner_lease` AND only
    /// with the explicit egress policy. This is the one path that sets
    /// allow_egress=true.
    #[test]
    fn runner_lease_grants_egress_only_via_runner_constructor() {
        let mut l = lease();
        l.net_policy = "egress-runner".to_string();
        let spec = ContainerSpec::from_runner_lease(&l, PIN).unwrap();
        assert!(!spec.no_network, "runner lease has a network device");
        assert!(spec.allow_egress, "runner lease carries the egress grant");
        assert!(spec.run_on_create, "runner box runs its agent at provision");
    }

    /// Defense in depth: `from_runner_lease` itself rejects any lease whose
    /// `net_policy` is not the explicit runner egress policy.
    #[test]
    fn runner_constructor_rejects_a_non_egress_policy() {
        for p in ["none", "isolated", "deny-all", "", "egress-allow"] {
            let mut l = lease();
            l.net_policy = p.to_string();
            assert!(
                ContainerSpec::from_runner_lease(&l, PIN).is_err(),
                "from_runner_lease must require net_policy=egress-runner; {p:?} got through"
            );
        }
    }

    /// An AGENT lease grants egress — but ONLY via `from_agent_lease` AND only
    /// with the explicit `egress-agent` policy. Unlike the runner box, the agent
    /// box is exec-driven (`run_on_create = false`): it waits for /agent-exec
    /// commands rather than self-launching an agent.
    #[test]
    fn agent_lease_grants_egress_only_via_agent_constructor() {
        let mut l = lease();
        l.net_policy = "egress-agent".to_string();
        let spec = ContainerSpec::from_agent_lease(&l, PIN).unwrap();
        assert!(!spec.no_network, "agent lease has a network device");
        assert!(spec.allow_egress, "agent lease carries the egress grant");
        assert!(
            !spec.run_on_create,
            "agent box is exec-driven, not run-on-create"
        );
    }

    /// Defense in depth: `from_agent_lease` rejects any lease whose `net_policy`
    /// is not the explicit agent egress policy — including the runner's policy
    /// (the two egress sentinels never cross constructors).
    #[test]
    fn agent_constructor_rejects_a_non_egress_agent_policy() {
        for p in ["none", "isolated", "egress-runner", "", "egress-agentx"] {
            let mut l = lease();
            l.net_policy = p.to_string();
            assert!(
                ContainerSpec::from_agent_lease(&l, PIN).is_err(),
                "from_agent_lease must require net_policy=egress-agent; {p:?} got through"
            );
        }
    }

    /// A CHECK lease forging the agent egress policy is REJECTED by `from_lease`
    /// — egress can never be obtained through the check path (red-team parity
    /// with the runner sentinel).
    #[test]
    fn check_lease_forging_agent_egress_policy_is_rejected() {
        let mut l = lease();
        l.net_policy = "egress-agent".to_string();
        assert!(
            ContainerSpec::from_lease(&l, PIN).is_err(),
            "a check lease must never be buildable with the agent egress policy"
        );
    }

    /// X4 is NOT bypassed for agent mode: an unpinned agent image is refused at
    /// spec-build, before any box contact.
    #[test]
    fn agent_lease_still_requires_a_pinned_image() {
        let mut l = lease();
        l.net_policy = "egress-agent".to_string();
        assert!(ContainerSpec::from_agent_lease(&l, "alpine:3.20").is_err());
        assert!(ContainerSpec::from_agent_lease(&l, PIN).is_ok());
    }

    /// X4 is NOT bypassed for runner mode: an unpinned runner image is refused
    /// at spec-build, before any box contact.
    #[test]
    fn runner_lease_still_requires_a_pinned_image() {
        let mut l = lease();
        l.net_policy = "egress-runner".to_string();
        assert!(ContainerSpec::from_runner_lease(&l, "alpine:3.20").is_err());
        assert!(ContainerSpec::from_runner_lease(&l, "alpine").is_err());
        assert!(ContainerSpec::from_runner_lease(&l, PIN).is_ok());
    }

    #[test]
    fn spec_rejects_empty_ids() {
        let mut l = lease();
        l.lease_id = "  ".to_string();
        assert!(ContainerSpec::from_lease(&l, PIN).is_err());
    }

    #[test]
    fn spec_rejects_unpinned_image() {
        // The supply-chain floor: a floating tag is refused at spec-build time,
        // before any box contact (WP-X4 on the real spawn surface).
        assert!(ContainerSpec::from_lease(&lease(), "alpine:3.20").is_err());
        assert!(ContainerSpec::from_lease(&lease(), "alpine").is_err());
        let tampered = "alpine@sha256:\
                        0000000000000000000000000000000000000000000000000000000000000000";
        // (tampered digest is *syntactically* pinned; integrity is caught at
        // spawn-time verify, not here — see the spawn-path tests.)
        assert!(ContainerSpec::from_lease(&lease(), tampered).is_ok());
    }

    #[test]
    fn spec_rejects_tmp_root_injection() {
        // tmp_root RCE guard (brutal review R4): a value that escapes `sh -c`
        // must be refused at from_lease before it can reach the box.
        let mut l = lease();
        l.tmp_root = "/x' ; touch /pwned ; echo '".to_string();
        let err = ContainerSpec::from_lease(&l, PIN).unwrap_err().to_string();
        assert!(
            err.contains("tmp_root"),
            "tmp_root injection must be rejected at from_lease; got: {err}"
        );
        // relative path also refused
        l.tmp_root = "relative/tmp".to_string();
        assert!(ContainerSpec::from_lease(&l, PIN).is_err());
        // command substitution refused
        l.tmp_root = "/$(reboot)".to_string();
        assert!(ContainerSpec::from_lease(&l, PIN).is_err());
        // a clean absolute path is accepted
        l.tmp_root = "/corelink/tmp".to_string();
        assert!(ContainerSpec::from_lease(&l, PIN).is_ok());
    }

    #[test]
    fn shell_join_always_quotes() {
        assert_eq!(shell_join(&["echo", "a b"]), "'echo' 'a b'");
        // Even "looks-safe" tokens are quoted — no allowlist passthrough.
        assert_eq!(shell_join(&["ls", "/tmp"]), "'ls' '/tmp'");
        // An injection attempt is fully neutralized by quoting.
        assert_eq!(shell_join(&["echo", "; rm -rf /"]), "'echo' '; rm -rf /'");
    }
}
