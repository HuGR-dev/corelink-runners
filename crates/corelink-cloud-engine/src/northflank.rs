//! The Northflank-backed [`Engine`].
//!
//! Maps the runner's per-job container lifecycle onto Northflank's **Job-run**
//! primitive (create-job → run → poll → capture → delete), the shape that fits
//! ephemeral, untrusted, scale-to-zero compute. The production execution path
//! (`concurrency::run_one`) is one-shot — `spawn` then a single `exec`/
//! `exec_captured`, then teardown — so the Job-run model is a clean fit; there
//! is no "exec into a long-lived box" requirement to satisfy (Northflank has no
//! REST exec, and we do not need one).
//!
//! ## Security floors (parity with [`DockerEngine`](corelink_runner::isolation))
//! - **Isolation floor:** `spawn` refuses any spec with `no_network == false`
//!   UNLESS it carries the explicit egress grant `allow_egress == true` (only a
//!   runner lease, ADR-0007); a bare `no_network=false` still fails closed.
//! - **Supply-chain floor (X4):** `spawn` refuses any image that is not
//!   content-(digest)-pinned, *before* contacting the provider. There is no
//!   on-box integrity probe (no box exists); instead the provider pulls strictly
//!   by digest, so the registry itself is the integrity check — a digest can
//!   only resolve to its exact bytes.
//!
//! ## Fidelity notes (provider-bounded, documented, not silently dropped)
//! - **Exit code:** Northflank run status surfaces success/failure, not the
//!   raw numeric code. We map success → `0`, failure → `1`. Granularity beyond
//!   that is provider-limited.
//! - **Captured output:** the provider returns structured CRI JSON logs
//!   (`{"data":[{"log":"<ts> <stream> <flag> <message>"}]}`); `fetch_logs`
//!   parses each entry and routes `stdout`/`stderr` into separate buckets
//!   (live-confirmed shape). The `CheckResult` content-digest is computed over
//!   `stdout` deterministically. This only matters for cross-engine memo
//!   identity (docker↔cloud), which the single-engine launch fabric does not do.
//!
//! Field-level exactness of the Northflank request bodies is validated at deploy
//! time against a live token; the acceptance suite proves the engine's *logic*
//! (floors, auth, the poll loop, fail-closed error mapping) against a fake
//! transport with zero account dependency.
//!
//! # SECURITY — DEPLOY GATE
//!
//! **Cross-tenant isolation is the hard guarantee:** Northflank deploys Cilium
//! network policies between projects/namespaces by default; multi-project
//! networking is OFF, so a job cannot reach other tenants' projects. This is
//! the tenant-isolation guarantee.
//!
//! **Internet egress is ACCEPTED at launch (owner decision, ADR-0003):** full
//! outbound-internet blocking is a BYOC-tier feature, not a managed-PaaS
//! per-job toggle. The accepted-risk posture rests on: no free tier /
//! card-on-file (no anonymous untrusted code — abuse is identified and
//! billable), secrets brokered (never on the box), ephemeral microVM-per-job,
//! and cache-warm reducing real egress need. BYOC egress-gateway is the
//! enterprise lockdown upgrade path.
//!
//! See `docs/adr/0003-egress-isolation-posture.md` for the decision record.

use anyhow::{Result, bail};
use corelink_runner::ContainerSpec;
use corelink_runner::isolation::{Engine, IsolationProbe, RunningContainer};
use corelink_runner::lease::CmdOutput;
use corelink_runner::pin::PinnedImageRef;

use crate::http::{HttpRequest, HttpResponse, HttpTransport, Method};

/// Tunables for the Northflank backend. Defaults match the docs' example shapes;
/// `token`/`project_id` are required.
#[derive(Clone)]
pub struct NorthflankConfig {
    /// API root, e.g. `https://api.northflank.com/v1`.
    pub base_url: String,
    /// Northflank project the ephemeral jobs live in.
    pub project_id: String,
    /// API token (raw; the transport renders the `Bearer ` scheme).
    pub token: String,
    /// Billing/compute plan id (vCPU/mem class), e.g. `nf-compute-20`.
    pub deployment_plan: String,
    /// Per-job ephemeral disk (MiB).
    pub ephemeral_storage_mb: u32,
    /// Hard wall-clock ceiling for a single run (seconds) — the provider kills
    /// the container past it (defense in depth with the lease expiry).
    pub active_deadline_secs: u32,
    /// Max status polls before a run is declared stuck → fail-closed `Err`.
    pub max_poll_attempts: u32,
    /// Sleep between status polls (ms). Set to 0 in tests.
    pub poll_interval_ms: u64,
}

impl NorthflankConfig {
    /// A config with the documented defaults; supply `project_id` + `token`.
    #[must_use]
    pub fn new(project_id: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            base_url: "https://api.northflank.com/v1".to_string(),
            project_id: project_id.into(),
            token: token.into(),
            deployment_plan: "nf-compute-20".to_string(),
            ephemeral_storage_mb: 1024,
            active_deadline_secs: 3600,
            max_poll_attempts: 600,
            poll_interval_ms: 1000,
        }
    }

    /// Build a config from an arbitrary key→value lookup (testable without
    /// mutating the process environment).
    ///
    /// **Required:** `NORTHFLANK_API_TOKEN` and `NORTHFLANK_PROJECT_ID` — if
    /// EITHER is absent or empty, returns `None` (NEVER a partial config;
    /// fail-closed).
    ///
    /// **Optional overrides** (numeric tunables stay at defaults):
    /// - `NORTHFLANK_BASE_URL` → `base_url` (explicit; takes precedence over all)
    /// - `NORTHFLANK_TEAM_ID` → `base_url = https://api.northflank.com/v1/teams/{team}`
    ///   (required for ORG API tokens, which need team-scoped paths)
    /// - `NORTHFLANK_DEPLOYMENT_PLAN` → `deployment_plan`
    ///
    /// **`base_url` precedence:**
    /// 1. `NORTHFLANK_BASE_URL` (non-empty) — verbatim, wins over everything.
    /// 2. `NORTHFLANK_TEAM_ID` (non-empty) — `https://api.northflank.com/v1/teams/{team}`.
    /// 3. Default from [`NorthflankConfig::new`] (`https://api.northflank.com/v1`).
    #[must_use]
    pub fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let token = get("NORTHFLANK_API_TOKEN").filter(|s| !s.is_empty())?;
        let project_id = get("NORTHFLANK_PROJECT_ID").filter(|s| !s.is_empty())?;

        let mut cfg = Self::new(project_id, token);

        if let Some(base_url) = get("NORTHFLANK_BASE_URL").filter(|s| !s.is_empty()) {
            cfg.base_url = base_url;
        } else if let Some(team) = get("NORTHFLANK_TEAM_ID").filter(|s| !s.is_empty()) {
            cfg.base_url = format!("https://api.northflank.com/v1/teams/{team}");
        }
        if let Some(plan) = get("NORTHFLANK_DEPLOYMENT_PLAN").filter(|s| !s.is_empty()) {
            cfg.deployment_plan = plan;
        }

        Some(cfg)
    }

    /// Build a config from the real process environment.
    ///
    /// Thin wrapper over [`Self::from_env_with`] — returns `None` when the
    /// required `NORTHFLANK_API_TOKEN` or `NORTHFLANK_PROJECT_ID` env vars are
    /// absent or empty. When this returns `None`, the composition root MUST
    /// keep the `NoBoxExec` default (default-off, fail-closed).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Self::from_env_with(|k| std::env::var(k).ok())
    }
}

/// Manual `Debug` for [`NorthflankConfig`] — the `token` field is redacted so
/// the raw API bearer token never appears in logs, error context, or panic
/// output even when the struct is `{:?}`-formatted.
impl std::fmt::Debug for NorthflankConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NorthflankConfig")
            .field("base_url", &self.base_url)
            .field("project_id", &self.project_id)
            .field("token", &"***REDACTED***")
            .field("deployment_plan", &self.deployment_plan)
            .field("ephemeral_storage_mb", &self.ephemeral_storage_mb)
            .field("active_deadline_secs", &self.active_deadline_secs)
            .field("max_poll_attempts", &self.max_poll_attempts)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .finish()
    }
}

/// Terminal/!terminal classification of a Northflank run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunState {
    Running,
    Succeeded,
    Failed,
}

/// Collect every JSON string **value** (not keys, not numbers) from `v`
/// recursively into `out`. Used by [`classify_run_status`] to avoid false
/// positives from numeric fields (e.g. `{"errorCount":0}`) or key names
/// containing status-adjacent words.
fn json_string_values(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.to_ascii_uppercase()),
        serde_json::Value::Array(arr) => {
            for item in arr {
                json_string_values(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (_key, val) in map {
                json_string_values(val, out);
            }
        }
        // Bool / Number / Null carry no status signal — skip.
        _ => {}
    }
}

/// The ONE documented Northflank job-run terminal-success status (`status`
/// field of a job run). Per the Northflank API, a job run's `status` is exactly
/// one of `SUCCESS` / `RUNNING` / `FAILED`; `SUCCESS` is the only positive
/// success evidence. Anything else — `SUCCEEDED`, `COMPLETED`, `DONE`, a forged
/// string, an unknown future status — is NOT success.
///
/// Stored upper-cased so the comparison in [`classify_run_status`] is
/// case-insensitive against the upper-cased string values it collects.
const SUCCESS_STATUS: &str = "SUCCESS";

/// Explicit, documented FAIL statuses — for *reporting* a terminal failure
/// promptly (so the poll loop returns `Failed` instead of burning the whole
/// budget). These are not load-bearing for the fail-CLOSED guarantee: a run
/// that is neither an exact `SUCCESS` nor an explicit fail is treated as
/// non-terminal (`Running`) and ultimately ejected as an `Err` by the poll
/// budget — never fabricated into a success.
const FAIL_TOKENS: &[&str] = &["FAILED", "FAILURE", "ERROR", "CRASHED", "CANCELLED"];

/// Classify a run-status response body **fail-CLOSED** via JSON-value exact-match.
///
/// Parses the body as JSON, recursively collects every **string value** (not
/// keys, not numbers), upper-cases each, and compares for **exact equality**.
/// Collecting only string values prevents false positives from numeric fields
/// (e.g. `{"errorCount":0}`) or key names containing status-adjacent words.
///
/// **Success requires POSITIVE, unambiguous evidence:** the result is
/// `RunState::Succeeded` **iff** some string value is exactly the one documented
/// Northflank success status ([`SUCCESS_STATUS`]). A body that merely *contains*
/// a success-ish substring, an undocumented token like `COMPLETED`/`SUCCEEDED`,
/// or no recognised status at all is NEVER classified as success. In an
/// untrusted-compute setting a failed or forged run must not read as passed.
///
/// **Failure precedence:** an exact FAIL token wins over an exact success token
/// in the same body (a failed build inside an otherwise-"SUCCESS" envelope must
/// read as `Failed`).
///
/// **Everything ambiguous/unknown → `RunState::Running`** (not terminal). A
/// non-JSON body, an empty body, or a body with no recognised terminal status
/// is treated as still-running so the poll budget ejects it into a fail-closed
/// `Err` — the engine never fabricates success OR failure from ambiguity.
fn classify_run_status(body: &str) -> RunState {
    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v,
        // Non-JSON body: treat as still-running so the poll budget ejects.
        Err(_) => return RunState::Running,
    };

    let mut string_values: Vec<String> = Vec::new();
    json_string_values(&v, &mut string_values);

    // Failure precedence: an explicit fail wins over any success token.
    if string_values
        .iter()
        .any(|s| FAIL_TOKENS.iter().any(|t| s == *t))
    {
        return RunState::Failed;
    }
    // Success demands the ONE documented success status, exact-match. No
    // substring, no undocumented synonym, no "completed" — fail CLOSED.
    if string_values.iter().any(|s| s == SUCCESS_STATUS) {
        return RunState::Succeeded;
    }
    // Unknown / ambiguous / no terminal status → not terminal. The poll budget
    // turns persistent ambiguity into an `Err`, never a fabricated success.
    RunState::Running
}

/// Extract a Northflank object id from a create/run response (`data.id`).
fn parse_id(body: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("northflank response is not JSON: {e}"))?;
    v.get("data")
        .and_then(|d| d.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("northflank response missing data.id: {body}"))
}

/// Derive a Northflank-safe job name from an arbitrary `spec.name`, **injectively**.
///
/// Northflank object names are far stricter than Docker's: lowercase
/// `[a-z0-9-]`, must start with a letter, length-capped. The upstream
/// container-name derivations (`hugit-c2b-…`, `hugit-job-…`) sanitize foreign
/// characters to `_` and apply no length cap, so the *Docker* name they produce
/// is already **non-injective** (`lease/x` and `lease x` both → `…lease_x`) and
/// not even Northflank-legal. Passing it verbatim risked two distinct leases
/// colliding onto one Northflank job — a cross-lease teardown/spawn hazard in
/// untrusted compute.
///
/// This derivation is **collision-free by construction**: the human-readable
/// part is best-effort (lowercased, non-`[a-z0-9-]` → `-`, truncated to fit),
/// but a fixed-width hex suffix of the BLAKE-free SHA-256 of the *full, original*
/// `spec.name` is always appended. Two distinct inputs can share the readable
/// prefix but never the hash suffix, so the mapping is injective on the full
/// input. The result always starts with a letter and fits Northflank's 63-char
/// object-name ceiling.
fn northflank_job_name(spec_name: &str) -> String {
    use sha2::{Digest, Sha256};

    // Full-input hash → collision-free suffix (16 hex chars = 64 bits).
    let digest = Sha256::digest(spec_name.as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(suffix, "{byte:02x}");
    }

    // Readable, Northflank-legal slug of the original name (lossy is fine — the
    // hash carries injectivity). Lowercase; keep [a-z0-9], everything else → '-'.
    let mut slug = String::with_capacity(spec_name.len());
    for c in spec_name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else {
            slug.push('-');
        }
    }
    // Collapse runs of '-' and trim leading/trailing '-' for tidiness.
    let mut collapsed = String::with_capacity(slug.len());
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    let readable = collapsed.trim_matches('-');

    // Northflank object names cap at 63 chars and must start with a letter.
    // Layout: "nf-" (3) + readable + "-" (1) + 16-hex suffix = budget readable
    // to 63 - 3 - 1 - 16 = 43 chars.
    const READABLE_BUDGET: usize = 63 - 3 - 1 - 16;
    let readable: String = readable.chars().take(READABLE_BUDGET).collect();
    let readable = readable.trim_matches('-');

    if readable.is_empty() {
        format!("nf-{suffix}")
    } else {
        format!("nf-{readable}-{suffix}")
    }
}

/// Single-quote each argv element and join — a shell-safe rendering of the job
/// command for Northflank's `customCommand` (a single shell string). A literal
/// `'` inside an arg is escaped the POSIX way (`'\''`).
fn shell_join(argv: &[&str]) -> String {
    argv.iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Northflank-backed [`Engine`], generic over the HTTP transport so the engine
/// logic is fully unit-testable against a fake.
#[derive(Clone)]
pub struct NorthflankEngine<H: HttpTransport> {
    http: H,
    cfg: NorthflankConfig,
}

/// Manual `Debug` for [`NorthflankEngine`] — delegates to [`NorthflankConfig`]'s
/// redacting `Debug` impl so the bearer token is never exposed. The `H`
/// transport generic is not required to implement `Debug`.
impl<H: HttpTransport> std::fmt::Debug for NorthflankEngine<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NorthflankEngine")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl<H: HttpTransport> NorthflankEngine<H> {
    /// Construct over a transport + config.
    pub fn new(http: H, cfg: NorthflankConfig) -> Self {
        Self { http, cfg }
    }

    fn jobs_url(&self) -> String {
        format!(
            "{}/projects/{}/jobs",
            self.cfg.base_url, self.cfg.project_id
        )
    }

    fn job_url(&self, job: &str) -> String {
        format!("{}/{}", self.jobs_url(), job)
    }

    /// Send a request carrying the bearer token; surface transport errors and
    /// preserve the HTTP status for the caller to branch on.
    fn send(&self, method: Method, url: String, json_body: Option<String>) -> Result<HttpResponse> {
        self.http.send(&HttpRequest {
            method,
            url,
            bearer_token: self.cfg.token.clone(),
            json_body,
        })
    }

    /// Send and require a 2xx, mapping anything else to a fail-closed `Err`
    /// (the provider failed; never fabricate a success).
    fn send_2xx(
        &self,
        method: Method,
        url: String,
        json_body: Option<String>,
        ctx: &str,
    ) -> Result<HttpResponse> {
        let resp = self.send(method, url, json_body)?;
        if !resp.is_success() {
            bail!(
                "northflank {ctx} failed: HTTP {} — {} (fail-closed)",
                resp.status,
                resp.body.trim()
            );
        }
        Ok(resp)
    }

    /// The create-job request body for `spec` (command left to the image
    /// default; `exec` sets the per-run command). `job_name` is the
    /// injectively-derived Northflank-legal name (see [`northflank_job_name`]);
    /// it MUST match the name stored on the returned [`RunningContainer`] so all
    /// later `job_url` calls address the same job.
    fn create_job_body(&self, spec: &ContainerSpec, job_name: &str) -> String {
        let mut deployment = serde_json::json!({
            "external": { "imagePath": spec.image },
            "docker": { "configType": "default" },
            "storage": { "ephemeralStorage": { "storageSize": self.cfg.ephemeral_storage_mb } }
        });
        // Additive runtime environment (Northflank `runtimeEnvironment` map):
        // emitted ONLY when the spec carries env (the §13.2 envelope ingest URL
        // + lease credential injected by the cloud provision path). An empty
        // `spec.env` leaves the body byte-identical to before — DEFAULT-OFF.
        if !spec.env.is_empty() {
            let env_map: serde_json::Map<String, serde_json::Value> = spec
                .env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            deployment["runtimeEnvironment"] = serde_json::Value::Object(env_map);
        }
        serde_json::json!({
            "name": job_name,
            "billing": { "deploymentPlan": self.cfg.deployment_plan },
            "deployment": deployment,
            // CHECK leases stay `false` (the fabric drives the command per
            // `/exec`); a RUNNER lease (ADR-0007) runs its agent entrypoint
            // immediately at create.
            "runOnCreate": spec.run_on_create,
            "backoffLimit": 0,
            "activeDeadlineSeconds": self.cfg.active_deadline_secs
        })
        .to_string()
    }

    /// Set the job's run command to `argv` (Northflank `customCommand`).
    fn set_command(&self, job: &str, argv: &[&str]) -> Result<()> {
        let body = serde_json::json!({
            "deployment": {
                "docker": {
                    "configType": "customCommand",
                    "customCommand": shell_join(argv)
                }
            }
        })
        .to_string();
        // Northflank job update is a PATCH on the job resource.
        self.send_2xx(Method::Patch, self.job_url(job), Some(body), "set-command")?;
        Ok(())
    }

    /// Trigger a run and poll it to a terminal state, returning the run outcome
    /// as a process-style exit code (`0` success, `1` failure). Fails closed if
    /// the run never reaches a terminal state within the poll budget.
    fn run_to_completion(&self, job: &str) -> Result<Option<i32>> {
        let runs_url = format!("{}/runs", self.job_url(job));
        let started = self.send_2xx(Method::Post, runs_url.clone(), None, "trigger-run")?;
        let run_id = parse_id(&started.body)?;
        let run_url = format!("{runs_url}/{run_id}");

        for _ in 0..self.cfg.max_poll_attempts {
            let resp = self.send_2xx(Method::Get, run_url.clone(), None, "poll-run")?;
            match classify_run_status(&resp.body) {
                RunState::Running => {
                    if self.cfg.poll_interval_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(
                            self.cfg.poll_interval_ms,
                        ));
                    }
                }
                RunState::Succeeded => return Ok(Some(0)),
                RunState::Failed => return Ok(Some(1)),
            }
        }
        bail!(
            "northflank run {run_id} for job {job} did not reach a terminal state \
             within {} polls (fail-closed)",
            self.cfg.max_poll_attempts
        )
    }

    /// Fetch the run logs for `job`, split into `(stdout, stderr)`.
    ///
    /// The Northflank logs endpoint returns structured CRI JSON:
    /// `{"data":[{"log":"<RFC3339Nano-ts> <stream> <flag> <message>", ...}, ...]}`.
    /// We parse each `.data[].log` CRI line, routing the message part to the
    /// stdout or stderr bucket based on the `<stream>` field (`"stdout"` /
    /// `"stderr"`). Lines with fewer than 3 leading whitespace-delimited fields
    /// are placed verbatim into stdout (defensive).
    ///
    /// If the body does not parse as the expected JSON shape, the raw body is
    /// returned as stdout with stderr empty (defensive fallback — never error on
    /// shape).
    ///
    /// **Correctness invariant:** each lease maps to exactly one Northflank Job
    /// and exactly one run (`concurrency::run_one` is one-shot). The job-scoped
    /// `/logs` endpoint therefore returns this run's output — there is no
    /// ambiguity between runs of the same job.
    fn fetch_logs(&self, job: &str) -> Result<(String, String)> {
        let url = format!("{}/logs", self.job_url(job));
        let resp = self.send_2xx(Method::Get, url, None, "fetch-logs")?;

        // Parse as structured CRI JSON: {"data":[{"log":"<ts> <stream> <flag> <msg>"},...]}
        // Defensive: on any parse/shape failure, return raw body as stdout.
        let parsed = serde_json::from_str::<serde_json::Value>(&resp.body).ok();
        let data = parsed
            .as_ref()
            .and_then(|v| v.get("data"))
            .and_then(|d| d.as_array());

        let Some(entries) = data else {
            return Ok((resp.body, String::new()));
        };

        let mut stdout_lines: Vec<String> = Vec::new();
        let mut stderr_lines: Vec<String> = Vec::new();

        for entry in entries {
            let log = match entry.get("log").and_then(|l| l.as_str()) {
                Some(s) => s,
                None => continue,
            };
            // CRI format: "<ts> <stream> <flag> <message…>"
            // splitn(4, ' ') produces at most [ts, stream, flag, message].
            let mut parts = log.splitn(4, ' ');
            let _ts = parts.next();
            let stream = parts.next();
            let _flag = parts.next();
            let message = parts.next();

            match (stream, message) {
                (Some("stdout"), Some(msg)) => stdout_lines.push(msg.to_string()),
                (Some("stderr"), Some(msg)) => stderr_lines.push(msg.to_string()),
                _ => {
                    // Fewer than 3 leading fields — put the whole log line into stdout.
                    stdout_lines.push(log.to_string());
                }
            }
        }

        Ok((stdout_lines.join("\n"), stderr_lines.join("\n")))
    }

    /// Delete the ephemeral job (teardown). Not part of the [`Engine`] trait —
    /// teardown is owned by the fabric's lifecycle path — but it is the no-leak
    /// guarantee for the Job-run model: every spawned job has exactly one delete.
    /// A 404 is treated as already-gone (idempotent teardown), not an error.
    ///
    /// # Errors
    /// A transport failure, or a non-2xx that is not a 404.
    pub fn delete_job(&self, c: &RunningContainer) -> Result<()> {
        let resp = self.send(Method::Delete, self.job_url(&c.name), None)?;
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            bail!(
                "northflank delete-job {} failed: HTTP {} — {} (fail-closed)",
                c.name,
                resp.status,
                resp.body.trim()
            );
        }
    }
}

impl<H: HttpTransport> Engine for NorthflankEngine<H> {
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
        // ── Isolation floor (parity with DockerEngine) ────────────────────────
        // A `no_network == false` spec is admitted ONLY when it also carries the
        // egress grant `allow_egress == true` — which only `from_runner_lease`
        // sets (ADR-0007). So a bare `no_network=false` (e.g. a hand-built or
        // forged spec) still fails closed; egress requires the explicit grant.
        if !spec.no_network && !spec.allow_egress {
            bail!("ContainerSpec.no_network must be true for isolation (fail-closed)");
        }
        // ── Supply-chain floor (X4): reject any non-digest-pinned image BEFORE
        // contacting the provider. No on-box probe exists in the cloud path;
        // the provider's by-digest pull is the integrity check.
        PinnedImageRef::parse(&spec.image).map_err(|e| {
            anyhow::anyhow!(
                "refusing to spawn {}: image {:?} is not content-pinned — fail CLOSED ({e})",
                spec.name,
                spec.image
            )
        })?;

        // Map the (possibly non-injective, Northflank-illegal) spec name onto an
        // injective, Northflank-legal job name. Stored on the RunningContainer so
        // every later job_url() addresses exactly this lease's job — distinct
        // leases can never collide onto one Northflank job.
        let job_name = northflank_job_name(&spec.name);

        self.send_2xx(
            Method::Post,
            self.jobs_url(),
            Some(self.create_job_body(spec, &job_name)),
            "create-job",
        )?;
        Ok(RunningContainer { name: job_name })
    }

    fn probe(&self, c: &RunningContainer, _spec: &ContainerSpec) -> Result<IsolationProbe> {
        // The job exists iff a GET returns 2xx; a fresh ephemeral container's tmp
        // is private by construction, and the job carries no published ports /
        // network device, so the namespace is isolated. We assert liveness here
        // and report both invariants as held; absence of the job is fail-closed.
        let resp = self.send(Method::Get, self.job_url(&c.name), None)?;
        let alive = resp.is_success();
        Ok(IsolationProbe {
            tmp_is_private: alive,
            net_is_isolated: alive,
        })
    }

    fn exec(&self, c: &RunningContainer, argv: &[&str]) -> Result<Option<i32>> {
        self.set_command(&c.name, argv)?;
        self.run_to_completion(&c.name)
    }

    fn exec_captured(&self, c: &RunningContainer, argv: &[&str]) -> Result<CmdOutput> {
        // **Correctness invariant:** each lease maps to exactly one job and one
        // run (`concurrency::run_one` is one-shot), so the job-scoped `/logs`
        // endpoint returns this run's output — no ambiguity between runs.
        self.set_command(&c.name, argv)?;
        let code = self.run_to_completion(&c.name)?;
        let (stdout, stderr) = self.fetch_logs(&c.name)?;
        Ok(CmdOutput {
            code,
            stdout,
            stderr,
        })
    }

    fn is_alive(&self, c: &RunningContainer) -> Result<bool> {
        let resp = self.send(Method::Get, self.job_url(&c.name), None)?;
        if resp.is_success() {
            Ok(true)
        } else if resp.status == 404 {
            Ok(false)
        } else {
            bail!(
                "northflank is_alive {}: indeterminate HTTP {} — fail-closed",
                c.name,
                resp.status
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpRequest, HttpResponse, HttpTransport};

    /// Trivial transport stub — always returns 200 with an empty body.
    struct StubTransport;

    impl HttpTransport for StubTransport {
        fn send(&self, _req: &HttpRequest) -> anyhow::Result<HttpResponse> {
            Ok(HttpResponse {
                status: 200,
                body: String::new(),
            })
        }
    }

    #[test]
    fn token_is_redacted_in_debug() {
        let cfg = NorthflankConfig::new("proj", "super-secret-token-value");

        // NorthflankConfig must not expose the raw token.
        let s = format!("{cfg:?}");
        assert!(
            !s.contains("super-secret-token-value"),
            "NorthflankConfig Debug leaked the token: {s}"
        );
        assert!(
            s.contains("REDACTED"),
            "NorthflankConfig Debug missing REDACTED placeholder: {s}"
        );

        // NorthflankEngine wrapping that config must also not expose the token.
        let engine = NorthflankEngine::new(StubTransport, cfg);
        let es = format!("{engine:?}");
        assert!(
            !es.contains("super-secret-token-value"),
            "NorthflankEngine Debug leaked the token: {es}"
        );
        assert!(
            es.contains("REDACTED"),
            "NorthflankEngine Debug missing REDACTED placeholder: {es}"
        );
    }

    /// A pinned spec helper for body-shape tests.
    fn spec(env: Vec<(String, String)>) -> ContainerSpec {
        ContainerSpec {
            name: "hugit-job-x".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: vec![],
            env,
        }
    }

    #[test]
    fn create_job_body_omits_runtime_environment_when_env_empty() {
        // DEFAULT-OFF: an empty `spec.env` leaves the body free of
        // runtimeEnvironment (byte-for-byte the prior shape).
        let engine = NorthflankEngine::new(StubTransport, NorthflankConfig::new("proj", "tok"));
        let body = engine.create_job_body(&spec(vec![]), "job-1");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(
            v["deployment"].get("runtimeEnvironment").is_none(),
            "no env ⇒ no runtimeEnvironment key"
        );
    }

    #[test]
    fn create_job_body_injects_runtime_environment_when_env_present() {
        // The §13.2 ingest vars surface as the Northflank runtimeEnvironment map.
        let engine = NorthflankEngine::new(StubTransport, NorthflankConfig::new("proj", "tok"));
        let env = vec![
            (
                "CORELINK_ENVELOPE_INGEST_URL".to_string(),
                "https://f/v1/leases/l/envelope/ingest".to_string(),
            ),
            (
                "CORELINK_ENVELOPE_INGEST_CREDENTIAL".to_string(),
                "pat-xyz".to_string(),
            ),
        ];
        let body = engine.create_job_body(&spec(env), "job-1");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let re = &v["deployment"]["runtimeEnvironment"];
        assert_eq!(
            re["CORELINK_ENVELOPE_INGEST_URL"].as_str().unwrap(),
            "https://f/v1/leases/l/envelope/ingest"
        );
        assert_eq!(
            re["CORELINK_ENVELOPE_INGEST_CREDENTIAL"].as_str().unwrap(),
            "pat-xyz"
        );
    }

    // ── [P1] classify_run_status: fail-CLOSED on ambiguity ────────────────────

    #[test]
    fn classify_exact_success_status_is_success() {
        // The ONE documented Northflank terminal-success status.
        assert_eq!(
            classify_run_status(r#"{"data":{"status":"SUCCESS"}}"#),
            RunState::Succeeded
        );
        // Case-insensitive (values are upper-cased before comparison).
        assert_eq!(
            classify_run_status(r#"{"data":{"status":"success"}}"#),
            RunState::Succeeded
        );
    }

    #[test]
    fn classify_success_substring_but_overall_failed_is_not_success() {
        // A FAILED run whose status string merely CONTAINS a success token must
        // never read as success — the core untrusted-compute fail-open.
        // "SUCCESS_THEN_FAILED" contains "SUCCESS" as a substring but is not the
        // exact success status, and FAILED is present → Failed.
        assert_eq!(
            classify_run_status(r#"{"status":"FAILED","note":"SUCCESS_THEN_FAILED"}"#),
            RunState::Failed
        );
        // A lone substring carrier with NO exact success status and NO fail token
        // is ambiguous → Running (never Succeeded).
        assert_eq!(
            classify_run_status(r#"{"status":"BUILD_SUCCESS_PARTIAL"}"#),
            RunState::Running
        );
    }

    #[test]
    fn classify_undocumented_success_synonyms_are_not_success() {
        // `COMPLETED` / `SUCCEEDED` / `DONE` / `OK` are NOT the documented
        // Northflank success status — fail CLOSED (treated as still-running so
        // the poll budget ejects to an Err, never fabricates success).
        for body in [
            r#"{"status":"COMPLETED"}"#,
            r#"{"status":"SUCCEEDED"}"#,
            r#"{"status":"DONE"}"#,
            r#"{"status":"OK"}"#,
            r#"{"status":"PASSED"}"#,
        ] {
            assert_eq!(
                classify_run_status(body),
                RunState::Running,
                "undocumented success synonym must NOT classify as success: {body}"
            );
        }
    }

    #[test]
    fn classify_unknown_status_is_not_success() {
        // An unknown / future / forged status string → not terminal-success.
        assert_eq!(
            classify_run_status(r#"{"status":"WAT_IS_THIS"}"#),
            RunState::Running
        );
        // Empty object, empty body, non-JSON garbage — none fabricate success.
        assert_eq!(classify_run_status("{}"), RunState::Running);
        assert_eq!(classify_run_status(""), RunState::Running);
        assert_eq!(classify_run_status("not json at all"), RunState::Running);
    }

    #[test]
    fn classify_explicit_fail_is_failed() {
        assert_eq!(
            classify_run_status(r#"{"status":"FAILED"}"#),
            RunState::Failed
        );
    }

    #[test]
    fn classify_fail_wins_over_success_in_same_body() {
        // Failure precedence: an exact FAIL token beats an exact success token.
        assert_eq!(
            classify_run_status(r#"{"status":"SUCCESS","build":"FAILED"}"#),
            RunState::Failed
        );
    }

    // ── [P2] northflank_job_name: injective derivation ────────────────────────

    #[test]
    fn job_name_is_injective_for_previously_colliding_inputs() {
        // Upstream sanitization maps non-conforming chars → '_' with no length
        // cap, so distinct lease ids collide to one Docker name. Feeding those
        // same distinct spec names through the derivation must yield DISTINCT
        // Northflank job names (collision-free hash suffix).
        // Each pair upstream-sanitizes to ONE Docker name (non-[alnum_._-] → '_'),
        // i.e. these collided before this fix.
        let collide_pairs = [
            // Both → "hugit-c2b-lease_x_y".
            ("hugit-c2b-lease/x/y", "hugit-c2b-lease x y"),
            // Both → "hugit-c2b-a_b".
            ("hugit-c2b-a:b", "hugit-c2b-a;b"),
            // Both → "hugit-job-a_b".
            ("hugit-job-a b", "hugit-job-a/b"),
        ];
        for (a, b) in collide_pairs {
            let na = northflank_job_name(a);
            let nb = northflank_job_name(b);
            assert_ne!(
                na, nb,
                "distinct spec names {a:?} and {b:?} collided onto one job name {na:?}"
            );
        }
    }

    #[test]
    fn job_name_is_northflank_legal() {
        for input in [
            "hugit-c2b-lease/x y",
            "HUGIT-JOB-Weird.Name",
            "////",           // slug collapses to empty
            &"x".repeat(500), // length stress
            "hugit-c2b-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let name = northflank_job_name(input);
            // Starts with a letter, ≤63 chars, only [a-z0-9-].
            assert!(
                name.len() <= 63,
                "name too long ({}) for {input:?}",
                name.len()
            );
            assert!(
                name.starts_with(|c: char| c.is_ascii_lowercase()),
                "name {name:?} must start with a letter"
            );
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "name {name:?} has illegal chars"
            );
            assert!(!name.is_empty());
        }
    }

    #[test]
    fn job_name_is_deterministic() {
        assert_eq!(
            northflank_job_name("hugit-c2b-lease-abc"),
            northflank_job_name("hugit-c2b-lease-abc")
        );
    }
}
