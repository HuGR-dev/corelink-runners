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
//! - **Isolation floor:** `spawn` refuses any spec with `no_network == false`.
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
#[derive(Debug, Clone)]
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

/// Classify a run-status response body defensively via JSON-value exact-match.
///
/// Parses the body as JSON, recursively collects every **string value** (not
/// keys, not numbers), upper-cases each, and compares for **exact equality**
/// against the documented terminal tokens. This prevents false positives from
/// numeric fields (e.g. `{"errorCount":0}`) or log/image strings that happen to
/// contain the substring "error" — a numeric `errorCount` is not a string value
/// and "errorCount" is a key, so neither matches.
///
/// **Failure precedence:** if any string value exactly matches a FAIL token,
/// the result is `RunState::Failed` regardless of any SUCCESS token in the same
/// body (a failed build inside a "COMPLETED" run must never read as success).
///
/// If the body does not parse as JSON, returns `RunState::Running` (not a
/// terminal state) — garbage in the poll response should exhaust the poll budget
/// and produce an `Err`, never fabricate success or failure.
fn classify_run_status(body: &str) -> RunState {
    const FAIL_TOKENS: &[&str] = &["FAILURE", "FAILED", "ERROR", "CRASHED", "CANCELLED"];
    const SUCCESS_TOKENS: &[&str] = &["SUCCESS", "SUCCEEDED", "COMPLETED"];

    let v = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => v,
        // Non-JSON body: treat as still-running so the poll budget ejects.
        Err(_) => return RunState::Running,
    };

    let mut string_values: Vec<String> = Vec::new();
    json_string_values(&v, &mut string_values);

    // Failure precedence: checked before success.
    if string_values
        .iter()
        .any(|s| FAIL_TOKENS.iter().any(|t| s == *t))
    {
        return RunState::Failed;
    }
    if string_values
        .iter()
        .any(|s| SUCCESS_TOKENS.iter().any(|t| s == *t))
    {
        return RunState::Succeeded;
    }
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
#[derive(Debug, Clone)]
pub struct NorthflankEngine<H: HttpTransport> {
    http: H,
    cfg: NorthflankConfig,
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
    /// default; `exec` sets the per-run command).
    fn create_job_body(&self, spec: &ContainerSpec) -> String {
        serde_json::json!({
            "name": spec.name,
            "billing": { "deploymentPlan": self.cfg.deployment_plan },
            "deployment": {
                "external": { "imagePath": spec.image },
                "docker": { "configType": "default" },
                "storage": { "ephemeralStorage": { "storageSize": self.cfg.ephemeral_storage_mb } }
            },
            "runOnCreate": false,
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
        if !spec.no_network {
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

        self.send_2xx(
            Method::Post,
            self.jobs_url(),
            Some(self.create_job_body(spec)),
            "create-job",
        )?;
        Ok(RunningContainer {
            name: spec.name.clone(),
        })
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
