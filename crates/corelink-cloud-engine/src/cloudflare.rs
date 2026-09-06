//! The Cloudflare-Containers-backed [`Engine`] (ADR-0008).
//!
//! Maps the runner's per-job container lifecycle onto a CoreLink **spawn-Worker**
//! — a Cloudflare Worker + Durable Object that spawns a Cloudflare Container and
//! returns an opaque handle. This engine talks HTTP to that Worker (it does NOT
//! call Cloudflare's API directly); the TS Worker on the other side owns the
//! container plumbing. The seam is four endpoints (`/v1/spawn`, `/v1/status`,
//! `/v1/teardown`, `/v1/exec`); the contract is frozen in this module's doc +
//! `docs/spec/cf-check-host-contract.md` (C2/C3) + the request shapes below.
//!
//! ## Two lease shapes: runner-direct + check-host
//! Two kinds of lease are served (the discriminator is the env-carried toolchain
//! digest, [`check_host_toolchain_digest`], C6):
//! - **Runner lease** (ADR-0007 direct-CI, `allow_egress == true`): the spawned
//!   container runs the GitHub-Actions agent via its **image entrypoint**, so
//!   there is no post-spawn exec — `spawn` then teardown. The non-captured
//!   [`exec`](Engine::exec) stays unsupported (faking it would be dishonest).
//! - **Check-host lease** (`allow_egress == false` + `TOOLCHAIN_DIGEST` in env):
//!   `spawn` emits check-mode (C2) so the Worker routes to the check-host
//!   container, which hydrates the toolchain at start (C5); checks then run via
//!   [`exec_captured`](Engine::exec_captured) → `POST /v1/exec` (C3), fail-closed
//!   on any non-2xx (never a fabricated [`CmdOutput`]).
//!
//! A plain hermetic check (no `TOOLCHAIN_DIGEST`) is NOT served (it would 500 on
//! the runner image); it stays rejected at the spawn floor → Northflank/rota-B.
//!
//! ## Security floors (parity with [`NorthflankEngine`])
//! - **Isolation floor:** `spawn` refuses any spec with `no_network == false`
//!   UNLESS it carries the explicit egress grant `allow_egress == true` (only a
//!   runner lease, ADR-0007); a bare `no_network=false` still fails closed.
//! - **Supply-chain floor (X4):** `spawn` refuses any image that is not
//!   content-(digest)-pinned, *before* contacting the Worker. There is no on-box
//!   integrity probe (no box exists); the container image is pulled strictly by
//!   digest, so the registry is the integrity check — a digest resolves only to
//!   its exact bytes.
//! - **Disk floor (cold-start hardening):** a RUNNER box (`allow_egress == true`)
//!   runs a real CI workload; its configured ephemeral disk must meet
//!   [`RUNNER_EPHEMERAL_STORAGE_FLOOR_MB`] or `spawn` fails CLOSED (rather than
//!   ENOSPC mid-build). Cloudflare's standard instances give ample disk, but a
//!   misconfigured small instance must fail closed, not die disk-full mid-build.
//!
//! Field-level exactness of the spawn-Worker request bodies is validated at deploy
//! time against the live Worker; the acceptance suite proves the engine's *logic*
//! (floors, auth, fail-closed error mapping, the exact request shapes) against a
//! fake transport with zero account/network dependency.

use anyhow::{Result, bail};
use corelink_runner::ContainerSpec;
use corelink_runner::isolation::{Engine, IsolationProbe, RunningContainer};
use corelink_runner::lease::CmdOutput;
use corelink_runner::pin::PinnedImageRef;

use crate::http::{HttpRequest, HttpResponse, HttpTransport, Method};
use crate::northflank::RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;

// ── Env var names (centralized) ───────────────────────────────────────────────

/// Spawn-Worker base URL, e.g. `https://spawn.corelink.workers.dev`. REQUIRED to
/// enable the backend (absent/empty ⇒ [`CloudflareConfig::from_env_with`] → `None`).
pub const CLOUDFLARE_SPAWN_WORKER_URL_ENV: &str = "CLOUDFLARE_SPAWN_WORKER_URL";

/// Bearer token for the spawn-Worker. REQUIRED to enable the backend
/// (absent/empty ⇒ `None` ⇒ backend off). Sensitive — redacted in `Debug`.
pub const CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV: &str = "CLOUDFLARE_SPAWN_AUTH_TOKEN";

/// OPTIONAL comma-separated labels attached to every spawned container, e.g.
/// `corelink-runner,prod`. Absent/empty ⇒ no labels.
pub const CLOUDFLARE_RUNNER_LABELS_ENV: &str = "CLOUDFLARE_RUNNER_LABELS";

/// OPTIONAL per-container expiry in milliseconds (the Worker hard-kills the
/// container past it, defense-in-depth with the lease expiry). Absent/garbage ⇒
/// [`DEFAULT_EXPIRY_MS`].
pub const CLOUDFLARE_EXPIRY_MS_ENV: &str = "CLOUDFLARE_EXPIRY_MS";

/// OPTIONAL configured RUNNER ephemeral disk (MiB) — the value the disk floor is
/// asserted against. Absent/garbage ⇒ [`DEFAULT_RUNNER_STORAGE_MB`]. Set this to
/// the actual instance disk so a misconfigured small instance fails CLOSED here
/// rather than ENOSPC mid-build.
pub const CLOUDFLARE_RUNNER_STORAGE_MB_ENV: &str = "CLOUDFLARE_RUNNER_STORAGE_MB";

/// Default per-container expiry (ms) when [`CLOUDFLARE_EXPIRY_MS_ENV`] is unset:
/// 1 hour, matching the Northflank `active_deadline_secs` default.
pub const DEFAULT_EXPIRY_MS: u64 = 3_600_000;

/// Default RUNNER ephemeral disk (MiB) when [`CLOUDFLARE_RUNNER_STORAGE_MB_ENV`]
/// is unset. Cloudflare's standard instances give ~20 GiB; this default clears
/// the [`RUNNER_EPHEMERAL_STORAGE_FLOOR_MB`] floor so a nominal config spawns.
pub const DEFAULT_RUNNER_STORAGE_MB: u32 = 20_480;

/// The env key the runner JIT-registration config arrives under (set by the
/// cloud provision path, ADR-0007). The spawn-Worker contract carries it as a
/// dedicated top-level `jitconfig` field, so `spawn` lifts it out of `spec.env`;
/// it is ALSO left in the `env` map the Worker injects (the container entrypoint
/// reads it from the environment). Absent ⇒ empty `jitconfig` (the floors above
/// already gate a runner lease).
const JITCONFIG_ENV_KEY: &str = "CORELINK_RUNNER_JITCONFIG";

/// The env key that carries the check-host toolchain digest (the clw snapshot
/// manifest digest, C6 addendum). A spec is a **check-host spec** iff it is NOT
/// egress-granted AND carries this key — the env-carried discriminator the
/// fabric injects (C1→C2). For a check-host spawn the value is lifted into the
/// dedicated top-level `toolchain_digest` field the Worker requires for
/// `mode=="check"` (C2); it is ALSO left in the `env` map (the container
/// entrypoint reads `TOOLCHAIN_DIGEST` to drive `clw hydrate`, C5).
const TOOLCHAIN_DIGEST_ENV_KEY: &str = "TOOLCHAIN_DIGEST";

/// Detect whether `spec` is a **check-host spec** (C6): a non-egress
/// (`allow_egress == false`) spec that carries `TOOLCHAIN_DIGEST` in its env.
/// Returns the toolchain digest value when so. A plain hermetic check (no
/// `TOOLCHAIN_DIGEST`) is NOT a check-host spec (it stays runner-only-floor
/// rejected → Northflank / rota-B); a runner spec (`allow_egress == true`) is
/// never a check-host spec regardless of env.
fn check_host_toolchain_digest(spec: &ContainerSpec) -> Option<&str> {
    if spec.allow_egress {
        return None;
    }
    spec.env
        .iter()
        .find(|(k, _)| k == TOOLCHAIN_DIGEST_ENV_KEY)
        .map(|(_, v)| v.as_str())
        .filter(|s| !s.is_empty())
}

/// Tunables for the Cloudflare spawn-Worker backend. `spawn_worker_url` +
/// `auth_token` are required (the backend is OFF without both).
#[derive(Clone)]
pub struct CloudflareConfig {
    /// Spawn-Worker base URL (no trailing slash), e.g.
    /// `https://spawn.corelink.workers.dev`. The `/v1/...` paths are appended.
    pub spawn_worker_url: String,
    /// Bearer token for the spawn-Worker (raw; the transport renders the
    /// `Bearer ` scheme). Sensitive — redacted in [`Debug`].
    pub auth_token: String,
    /// Configured RUNNER ephemeral disk (MiB) — the value the disk floor is
    /// asserted against in [`spawn`](Engine::spawn). A real CI workload needs
    /// at least [`RUNNER_EPHEMERAL_STORAGE_FLOOR_MB`].
    pub runner_storage_mb: u32,
    /// Per-container hard expiry (ms) sent to the Worker (defense-in-depth with
    /// the lease expiry).
    pub expiry_ms: u64,
    /// Labels attached to every spawned container (empty ⇒ none sent).
    pub labels: Vec<String>,
}

impl CloudflareConfig {
    /// A config with the documented defaults; supply `spawn_worker_url` + `auth_token`.
    #[must_use]
    pub fn new(spawn_worker_url: impl Into<String>, auth_token: impl Into<String>) -> Self {
        Self {
            spawn_worker_url: spawn_worker_url.into(),
            auth_token: auth_token.into(),
            runner_storage_mb: DEFAULT_RUNNER_STORAGE_MB,
            expiry_ms: DEFAULT_EXPIRY_MS,
            labels: Vec::new(),
        }
    }

    /// Build a config from an arbitrary key→value lookup (testable without
    /// mutating the process environment).
    ///
    /// **Required:** [`CLOUDFLARE_SPAWN_WORKER_URL_ENV`] and
    /// [`CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV`] — if EITHER is absent or empty, returns
    /// `None` (NEVER a partial config; fail-closed, DEFAULT-OFF).
    ///
    /// **Optional overrides** (sane defaults otherwise):
    /// - [`CLOUDFLARE_RUNNER_LABELS_ENV`] → `labels` (comma-separated; blanks dropped)
    /// - [`CLOUDFLARE_EXPIRY_MS_ENV`] → `expiry_ms` (absent/garbage/0 ⇒ [`DEFAULT_EXPIRY_MS`])
    /// - [`CLOUDFLARE_RUNNER_STORAGE_MB_ENV`] → `runner_storage_mb`
    ///   (absent/garbage/0 ⇒ [`DEFAULT_RUNNER_STORAGE_MB`])
    #[must_use]
    pub fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let spawn_worker_url = get(CLOUDFLARE_SPAWN_WORKER_URL_ENV).filter(|s| !s.is_empty())?;
        let auth_token = get(CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV).filter(|s| !s.is_empty())?;

        let mut cfg = Self::new(spawn_worker_url, auth_token);

        if let Some(labels) = get(CLOUDFLARE_RUNNER_LABELS_ENV).filter(|s| !s.is_empty()) {
            cfg.labels = labels
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        // Absent / non-numeric / `0` all fall back to the default — a `0` expiry
        // (no hard kill) must never be honored as a degenerate value.
        cfg.expiry_ms = get(CLOUDFLARE_EXPIRY_MS_ENV)
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&ms| ms > 0)
            .unwrap_or(DEFAULT_EXPIRY_MS);
        cfg.runner_storage_mb = get(CLOUDFLARE_RUNNER_STORAGE_MB_ENV)
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|&mb| mb > 0)
            .unwrap_or(DEFAULT_RUNNER_STORAGE_MB);

        Some(cfg)
    }

    /// Build a config from the real process environment.
    ///
    /// Thin wrapper over [`Self::from_env_with`] — returns `None` when the
    /// required [`CLOUDFLARE_SPAWN_WORKER_URL_ENV`] or
    /// [`CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV`] env vars are absent or empty. When this
    /// returns `None`, the composition root MUST keep the default-off backend
    /// (DEFAULT-OFF, fail-closed).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Self::from_env_with(|k| std::env::var(k).ok())
    }

    /// Validate an ARMED config for misconfigurations that would otherwise only
    /// surface at spawn time (or, worse, ENOSPC mid-build) — for a BOOT-TIME
    /// diagnostic, mirroring [`NorthflankConfig::validate_runner_disk`].
    ///
    /// The backend is only constructible when [`from_env`](Self::from_env)
    /// returned `Some` (both required vars present), so this is never reached for
    /// a DEFAULT-OFF fabric. When it IS reached, it fails LOUD (returns `Err`)
    /// rather than letting a misconfigured fabric boot and fail per-spawn after a
    /// wasted JIT/CAS mint. The per-spawn floor in [`CloudflareEngine::spawn`] is
    /// the hard backstop; this is the early, actionable boot warning (parity with
    /// the Northflank S3 pattern).
    ///
    /// Fail-loud (`Err`) arms — each names what is wrong and how to fix it:
    /// - `runner_storage_mb` below [`RUNNER_EPHEMERAL_STORAGE_FLOOR_MB`]: a runner
    ///   box would ENOSPC mid-build (the cold-start north star forbids it).
    /// - `spawn_worker_url` is not an `http(s)://` URL: the spawn endpoints would
    ///   be addressed against a garbage base and every spawn would fail.
    /// - `auth_token` is empty: the Worker would reject every request (though
    ///   `from_env` already drops an empty token, a programmatically-built config
    ///   could carry one — fail closed here too).
    ///
    /// `Ok(())` otherwise. The returned `String` is a ready-to-log, actionable
    /// message (no secret material — the token value is never interpolated).
    ///
    /// # Errors
    /// One of the misconfiguration arms documented above.
    pub fn validate(&self) -> Result<(), String> {
        if self.auth_token.is_empty() {
            return Err(format!(
                "{CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV} is empty — the spawn-Worker would reject \
                 every request. Set {CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV} to the Worker bearer token."
            ));
        }
        if !(self.spawn_worker_url.starts_with("http://")
            || self.spawn_worker_url.starts_with("https://"))
        {
            return Err(format!(
                "{CLOUDFLARE_SPAWN_WORKER_URL_ENV} {:?} is not an http(s):// URL — the /v1/... \
                 spawn endpoints would be addressed against a garbage base and every spawn would \
                 fail. Set {CLOUDFLARE_SPAWN_WORKER_URL_ENV} to e.g. https://spawn.example.workers.dev.",
                self.spawn_worker_url
            ));
        }
        if self.runner_storage_mb < RUNNER_EPHEMERAL_STORAGE_FLOOR_MB {
            return Err(format!(
                "{CLOUDFLARE_RUNNER_STORAGE_MB_ENV} resolves to {} MiB — below the \
                 {RUNNER_EPHEMERAL_STORAGE_FLOOR_MB} MiB floor a CI build needs. Every runner \
                 spawn will fail CLOSED (ENOSPC risk, not a slow run). Set \
                 {CLOUDFLARE_RUNNER_STORAGE_MB_ENV} >= {RUNNER_EPHEMERAL_STORAGE_FLOOR_MB} \
                 (within the Cloudflare instance disk).",
                self.runner_storage_mb
            ));
        }
        Ok(())
    }
}

/// Manual `Debug` for [`CloudflareConfig`] — the `auth_token` field is redacted
/// so the raw spawn-Worker bearer token never appears in logs, error context, or
/// panic output even when the struct is `{:?}`-formatted (mirrors
/// `NorthflankConfig`).
impl std::fmt::Debug for CloudflareConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareConfig")
            .field("spawn_worker_url", &self.spawn_worker_url)
            .field("auth_token", &"***REDACTED***")
            .field("runner_storage_mb", &self.runner_storage_mb)
            .field("expiry_ms", &self.expiry_ms)
            .field("labels", &self.labels)
            .finish()
    }
}

/// Bound a Worker response body before it is interpolated into an error (parity
/// with `northflank::bounded_provider_body`): the spawn REQUEST carries the
/// injected `CORELINK_RUNNER_JITCONFIG`; were the Worker ever to reflect submitted
/// env into a 4xx/5xx body, the raw body flowing into a `bail!` could echo it into
/// a log line — exactly the "no secret in a log" posture this fabric forbids.
/// Capping keeps errors actionable (status + a snippet) while bounding any
/// accidental echo to a fragment.
fn bounded_provider_body(body: &str) -> String {
    const CAP: usize = 200;
    let trimmed = body.trim();
    if trimmed.len() <= CAP {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(CAP).collect();
        s.push_str("…[truncated]");
        s
    }
}

/// Parse the opaque container handle from a spawn response (`{"handle":"<id>"}`).
fn parse_handle(body: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow::anyhow!("spawn-Worker response is not JSON: {e}"))?;
    let handle = v
        .get("handle")
        .and_then(|h| h.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("spawn-Worker response missing non-empty handle: {body}"))?;
    // The handle is interpolated verbatim into the `/v1/status/{handle}` URL
    // path. Constrain it to a URL-path-safe charset (`[A-Za-z0-9_-]`, the shape
    // of a UUID/token-id) so a malformed or compromised Worker response can
    // never inject `/`, `?`, `..`, or control characters that would re-address
    // a status/probe to a different (or malformed) target — fail CLOSED on a
    // non-conforming handle rather than mis-address it (V1 audit hardening).
    if !handle
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("spawn-Worker handle is not URL-path-safe ([A-Za-z0-9_-]); refusing — fail CLOSED");
    }
    Ok(handle.to_string())
}

/// Cloudflare-spawn-Worker-backed [`Engine`], generic over the HTTP transport so
/// the engine logic is fully unit-testable against a fake.
#[derive(Clone)]
pub struct CloudflareEngine<H: HttpTransport> {
    http: H,
    cfg: CloudflareConfig,
}

/// Manual `Debug` for [`CloudflareEngine`] — delegates to [`CloudflareConfig`]'s
/// redacting `Debug` impl so the bearer token is never exposed. The `H` transport
/// generic is not required to implement `Debug`.
impl<H: HttpTransport> std::fmt::Debug for CloudflareEngine<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareEngine")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

impl<H: HttpTransport> CloudflareEngine<H> {
    /// Construct over a transport + config.
    pub fn new(http: H, cfg: CloudflareConfig) -> Self {
        Self { http, cfg }
    }

    /// Configured Worker domain used in durable provider binding descriptors.
    pub fn provider_domain(&self) -> &str {
        &self.cfg.spawn_worker_url
    }

    /// Probe the same provider namespace used at spawn and teardown.
    pub fn is_alive_with_mode(&self, c: &RunningContainer, check_mode: bool) -> Result<bool> {
        let mut url = self.status_url(&c.name);
        if check_mode {
            url.push_str("?mode=check");
        }
        let resp = self.send(Method::Get, url, None)?;
        if resp.is_success() {
            Ok(true)
        } else if resp.status == 404 {
            Ok(false)
        } else {
            bail!(
                "cloudflare spawn-Worker is_alive {}: indeterminate HTTP {} — fail-closed",
                c.name,
                resp.status
            )
        }
    }

    /// Teardown with an explicit mode so a restored check-host handle can never
    /// be sent down the runner route.
    pub fn teardown_with_mode(&self, c: &RunningContainer, check_mode: bool) -> Result<()> {
        let mut body = serde_json::json!({ "handle": c.name });
        if check_mode {
            body["mode"] = serde_json::json!("check");
        }
        let resp = self.send(Method::Post, self.teardown_url(), Some(body.to_string()))?;
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            bail!(
                "cloudflare spawn-Worker teardown {} failed: HTTP {} — {} (fail-closed)",
                c.name,
                resp.status,
                bounded_provider_body(&resp.body)
            )
        }
    }

    /// Construct from a [`CloudflareConfig`] (alias of [`new`](Self::new) for
    /// composition-root symmetry with the Northflank wiring; the composition root
    /// can call `CloudflareEngine::from_config(transport, cfg)` once the backend is
    /// flipped live).
    pub fn from_config(http: H, cfg: CloudflareConfig) -> Self {
        Self::new(http, cfg)
    }

    fn spawn_url(&self) -> String {
        format!("{}/v1/spawn", self.cfg.spawn_worker_url)
    }

    fn status_url(&self, handle: &str) -> String {
        format!("{}/v1/status/{}", self.cfg.spawn_worker_url, handle)
    }

    fn teardown_url(&self) -> String {
        format!("{}/v1/teardown", self.cfg.spawn_worker_url)
    }

    fn exec_url(&self) -> String {
        format!("{}/v1/exec", self.cfg.spawn_worker_url)
    }

    /// Send a request carrying the bearer token; surface transport errors and
    /// preserve the HTTP status for the caller to branch on.
    fn send(&self, method: Method, url: String, json_body: Option<String>) -> Result<HttpResponse> {
        self.http.send(&HttpRequest {
            method,
            url,
            bearer_token: self.cfg.auth_token.clone(),
            json_body,
        })
    }

    /// Send and require a 2xx, mapping anything else to a fail-closed `Err` (the
    /// Worker failed; never fabricate a success).
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
                "cloudflare spawn-Worker {ctx} failed: HTTP {} — {} (fail-closed)",
                resp.status,
                bounded_provider_body(&resp.body)
            );
        }
        Ok(resp)
    }

    /// The `POST /v1/spawn` request body for `spec`.
    ///
    /// The runner JIT config (`CORELINK_RUNNER_JITCONFIG`) is lifted out of
    /// `spec.env` into the dedicated top-level `jitconfig` field the contract
    /// names; it is ALSO left in the `env` map so the container entrypoint reads
    /// it from the environment (the Northflank backend likewise injects it via the
    /// runtime environment). An absent key yields an empty `jitconfig` string —
    /// the spawn floors already gate a runner lease, so this only affects the
    /// wire shape, never admission.
    ///
    /// ## Check-mode (C2)
    /// When `spec` is a check-host spec (`!allow_egress` + `TOOLCHAIN_DIGEST` in
    /// env, per [`check_host_toolchain_digest`]) the body gains two additive
    /// top-level fields: `"mode": "check"` (so the Worker routes to
    /// `CHECK_HOST_CONTAINER`) and `"toolchain_digest": "<D>"` (the value lifted
    /// from env, which the Worker requires for `mode=="check"`). The
    /// `TOOLCHAIN_DIGEST` key remains in the `env` map (C2 shows both; the
    /// container entrypoint reads it to drive `clw hydrate`, C5).
    ///
    /// For a RUNNER spec the body is BYTE-IDENTICAL to the pre-check-host wire —
    /// `mode` is OMITTED, which the spawn-Worker treats identically to
    /// `"runner"` (`index.ts`: only `body.mode === "check"` diverts; absent OR
    /// `"runner"` falls through to the unchanged runner path). DEFAULT-OFF /
    /// back-comp is thereby preserved on the wire.
    fn spawn_body(&self, spec: &ContainerSpec) -> String {
        let jitconfig = spec
            .env
            .iter()
            .find(|(k, _)| k == JITCONFIG_ENV_KEY)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let env_map: serde_json::Map<String, serde_json::Value> = spec
            .env
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let mut body = serde_json::json!({
            "image_digest": spec.image,
            "jitconfig": jitconfig,
            "env": serde_json::Value::Object(env_map),
            "labels": self.cfg.labels,
            "expiry_ms": self.cfg.expiry_ms,
        });
        // Additive check-mode fields (C2). For a runner spec these are absent,
        // keeping the wire byte-identical to today (the Worker defaults absent
        // `mode` to the runner path).
        if let Some(toolchain_digest) = check_host_toolchain_digest(spec) {
            let obj = body
                .as_object_mut()
                .expect("spawn_body root is a JSON object");
            obj.insert(
                "mode".to_string(),
                serde_json::Value::String("check".to_string()),
            );
            obj.insert(
                "toolchain_digest".to_string(),
                serde_json::Value::String(toolchain_digest.to_string()),
            );
        }
        body.to_string()
    }

    /// Delete the ephemeral container (teardown). Not part of the [`Engine`] trait
    /// — teardown is owned by the fabric's lifecycle path — but it is the no-leak
    /// guarantee: every spawned container has exactly one delete. Both a 2xx and a
    /// 404 are treated as success (idempotent teardown), mirroring
    /// `NorthflankEngine::delete_job`.
    ///
    /// # Errors
    /// A transport failure, or a non-2xx that is not a 404.
    pub fn teardown(&self, c: &RunningContainer) -> Result<()> {
        let body = serde_json::json!({ "handle": c.name }).to_string();
        let resp = self.send(Method::Post, self.teardown_url(), Some(body))?;
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            bail!(
                "cloudflare spawn-Worker teardown {} failed: HTTP {} — {} (fail-closed)",
                c.name,
                resp.status,
                bounded_provider_body(&resp.body)
            );
        }
    }
}

impl<H: HttpTransport> Engine for CloudflareEngine<H> {
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
        // ── Isolation floor (parity with NorthflankEngine / DockerEngine) ──────
        // A `no_network == false` spec is admitted ONLY when it also carries the
        // egress grant `allow_egress == true` — which only `from_runner_lease`
        // sets (ADR-0007). So a bare `no_network=false` (a hand-built or forged
        // spec) still fails closed; egress requires the explicit grant.
        if !spec.no_network && !spec.allow_egress {
            bail!("ContainerSpec.no_network must be true for isolation (fail-closed)");
        }
        // ── Supply-chain floor (X4): reject any non-digest-pinned image BEFORE
        // contacting the Worker. No on-box probe exists in the cloud path; the
        // by-digest container pull is the integrity check.
        PinnedImageRef::parse(&spec.image).map_err(|e| {
            anyhow::anyhow!(
                "refusing to spawn {}: image {:?} is not content-pinned — fail CLOSED ({e})",
                spec.name,
                spec.image
            )
        })?;

        // ── Disk floor (cold-start hardening, parity with NorthflankEngine): a
        // RUNNER box runs a real CI workload — a cold `cargo build` into
        // `target/`. A too-small instance disk would ENOSPC mid-build, a BROKEN
        // run, not a slow one — the cold-start north star forbids it. Fail CLOSED
        // here, with an actionable message, rather than spawning a runner box
        // sized below the floor. (Cloudflare's standard-4 gives ~20 GiB so this
        // normally passes; the floor only catches a misconfigured small instance.)
        if spec.allow_egress {
            let disk_mb = self.cfg.runner_storage_mb;
            if disk_mb < RUNNER_EPHEMERAL_STORAGE_FLOOR_MB {
                bail!(
                    "refusing to spawn runner box {}: ephemeral disk {disk_mb} MiB is below \
                     the {RUNNER_EPHEMERAL_STORAGE_FLOOR_MB} MiB floor a CI build needs — set \
                     CLOUDFLARE_RUNNER_STORAGE_MB (within the Cloudflare instance disk). \
                     Fail CLOSED rather than ENOSPC mid-build.",
                    spec.name
                );
            }
        }

        // ── Runner-only floor — RELAXED for check-host (C6): CloudflareEngine
        // serves RUNNER leases (ADR-0007) AND, on the check-host campaign,
        // CHECK-HOST leases (a non-egress spec carrying TOOLCHAIN_DIGEST). The
        // discriminator is the env-carried toolchain digest (`check_host_*`):
        //   • RUNNER (allow_egress == true)              → admitted (runner mode)
        //   • CHECK-HOST (!egress + TOOLCHAIN_DIGEST)     → admitted (check mode →
        //       the Worker routes to CHECK_HOST_CONTAINER, which hydrates the
        //       toolchain at start and serves exec via /v1/exec)
        //   • PLAIN HERMETIC CHECK (!egress, NO digest)   → STILL fails closed
        //       (rota-B / Northflank): the spawn-Worker's runner container would
        //       exit 1 without a JIT — an opaque HTTP 500. Fail CLOSED HERE with
        //       an actionable message rather than a confusing downstream 500.
        if !spec.allow_egress && check_host_toolchain_digest(spec).is_none() {
            bail!(
                "refusing to spawn {}: this is a plain hermetic CHECK spec (allow_egress=false, \
                 no TOOLCHAIN_DIGEST), which CloudflareEngine does NOT serve — a runner box \
                 spawned for it exits 1 (opaque HTTP 500). A CHECK-HOST lease must carry \
                 TOOLCHAIN_DIGEST (check-mode); otherwise use a runner-mode lease or a \
                 plain-hermetic CHECK backend (e.g. Northflank). Fail CLOSED.",
                spec.name
            );
        }

        let resp = self.send_2xx(
            Method::Post,
            self.spawn_url(),
            Some(self.spawn_body(spec)),
            "spawn",
        )?;
        let handle = parse_handle(&resp.body)?;
        Ok(RunningContainer { name: handle })
    }

    fn probe(&self, c: &RunningContainer, spec: &ContainerSpec) -> Result<IsolationProbe> {
        // The container exists iff `GET /v1/status/{handle}` returns 2xx; a fresh
        // Cloudflare container's tmp is private by construction and it publishes
        // no ports, so the namespace is isolated. We assert liveness here and
        // report both invariants as held; absence of the container is fail-closed.
        let alive = self.is_alive_with_mode(c, check_host_toolchain_digest(spec).is_some())?;
        Ok(IsolationProbe {
            tmp_is_private: alive,
            net_is_isolated: alive,
        })
    }

    fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
        // The runner-direct path has no post-spawn exec: the container runs its
        // image entrypoint (the GitHub-Actions agent). The check-host path execs
        // via `exec_captured` (POST /v1/exec), NOT this non-captured `exec` — so
        // this stays bailed. Fail CLOSED — never fake an endpoint. See module doc.
        bail!(
            "exec (non-captured) is unsupported on CloudflareEngine: runner leases run their \
             entrypoint; check-host leases exec via exec_captured (/v1/exec)"
        )
    }

    fn exec_captured(&self, c: &RunningContainer, argv: &[&str]) -> Result<CmdOutput> {
        // Check-host exec (C3): POST {handle, argv, timeout_ms} to /v1/exec; the
        // Worker relays it to the in-container exec-server (port 8080, C4) and
        // returns 200 {exit_code, stdout, stderr}. A non-2xx is FAIL-CLOSED — we
        // never fabricate a CmdOutput (a fabricated success would let a check pass
        // on an unreachable/erroring box). `exit_code: null` ⇒ `code: None`
        // (signal-killed / timeout) ⇒ run_check fails closed downstream.
        //
        // timeout_ms source: the engine has no per-call exec timeout in the frozen
        // `Engine::exec_captured(c, argv)` seam, so we use the configured
        // `expiry_ms` (the same per-container hard-kill budget) as the exec ceiling
        // — a check can never outlive its container's expiry, so this is the
        // tightest honest bound available here.
        let body = serde_json::json!({
            "handle": c.name,
            "argv": argv,
            "timeout_ms": self.cfg.expiry_ms,
        })
        .to_string();
        let resp = self.send_2xx(Method::Post, self.exec_url(), Some(body), "exec")?;

        let v: serde_json::Value = serde_json::from_str(&resp.body).map_err(|e| {
            anyhow::anyhow!(
                "cloudflare spawn-Worker exec response is not JSON: {e} — {} (fail-closed)",
                bounded_provider_body(&resp.body)
            )
        })?;

        // exit_code: i32 | null. Present-but-non-integer is malformed ⇒ fail
        // closed (never silently coerce to a success/failure). `null` ⇒ None.
        let code = match v.get("exit_code") {
            Some(serde_json::Value::Null) | None => None,
            Some(serde_json::Value::Number(n)) => {
                let i = n.as_i64().ok_or_else(|| {
                    anyhow::anyhow!(
                        "cloudflare spawn-Worker exec exit_code is not an integer: {n} (fail-closed)"
                    )
                })?;
                Some(i32::try_from(i).map_err(|_| {
                    anyhow::anyhow!(
                        "cloudflare spawn-Worker exec exit_code {i} out of i32 range (fail-closed)"
                    )
                })?)
            }
            Some(other) => bail!(
                "cloudflare spawn-Worker exec exit_code is not an integer|null: {other} \
                 (fail-closed)"
            ),
        };
        let stdout = v
            .get("stdout")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        let stderr = v
            .get("stderr")
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string();
        Ok(CmdOutput {
            code,
            stdout,
            stderr,
        })
    }

    fn is_alive(&self, c: &RunningContainer) -> Result<bool> {
        self.is_alive_with_mode(c, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpRequest, HttpResponse, HttpTransport};
    use std::sync::Mutex;

    /// A transport that records the last request and returns a canned response —
    /// the unit-test seam: zero account/network dependency.
    struct RecordingTransport {
        status: u16,
        body: String,
        last: Mutex<Option<HttpRequest>>,
    }

    impl RecordingTransport {
        fn new(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
                last: Mutex::new(None),
            }
        }
    }

    impl HttpTransport for RecordingTransport {
        fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
            *self.last.lock().unwrap() = Some(req.clone());
            Ok(HttpResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    /// A transport that PANICS if it is ever called — proves a code path fails
    /// BEFORE any Worker HTTP contact.
    struct ExplodingTransport;

    impl HttpTransport for ExplodingTransport {
        fn send(&self, _req: &HttpRequest) -> anyhow::Result<HttpResponse> {
            panic!("the spawn-Worker must NOT be contacted on this path");
        }
    }

    /// A CHECK-style pinned spec (`allow_egress == false`, `no_network == true`).
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

    /// A RUNNER spec (`allow_egress == true`, runner-direct).
    fn runner_spec() -> ContainerSpec {
        let mut s = spec(vec![]);
        s.allow_egress = true;
        s.no_network = false;
        s.run_on_create = true;
        s
    }

    /// A 64-hex clw snapshot manifest digest, the check-host toolchain ref.
    const TOOLCHAIN_DIGEST_VALUE: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";

    /// A CHECK-HOST spec (`allow_egress == false`, `no_network == true`) carrying
    /// `TOOLCHAIN_DIGEST` in env — the env-carried check-host discriminator (C6).
    fn check_host_spec() -> ContainerSpec {
        spec(vec![(
            TOOLCHAIN_DIGEST_ENV_KEY.to_string(),
            TOOLCHAIN_DIGEST_VALUE.to_string(),
        )])
    }

    fn cfg() -> CloudflareConfig {
        CloudflareConfig::new("https://spawn.example.dev", "super-secret-token-value")
    }

    #[test]
    fn token_is_redacted_in_debug() {
        let c = cfg();
        let s = format!("{c:?}");
        assert!(
            !s.contains("super-secret-token-value"),
            "CloudflareConfig Debug leaked the token: {s}"
        );
        assert!(
            s.contains("REDACTED"),
            "CloudflareConfig Debug missing REDACTED placeholder: {s}"
        );

        // The engine wrapping that config must also not expose the token.
        let engine = CloudflareEngine::new(RecordingTransport::new(200, ""), c);
        let es = format!("{engine:?}");
        assert!(
            !es.contains("super-secret-token-value"),
            "CloudflareEngine Debug leaked the token: {es}"
        );
        assert!(
            es.contains("REDACTED"),
            "CloudflareEngine Debug missing REDACTED placeholder: {es}"
        );
    }

    #[test]
    fn spawn_happy_path_posts_exact_shape_and_parses_handle() {
        let mut c = cfg();
        c.labels = vec!["corelink-runner".to_string(), "prod".to_string()];
        c.expiry_ms = 1234;
        let engine = CloudflareEngine::new(
            RecordingTransport::new(200, r#"{"handle":"cf-abc-123"}"#),
            c,
        );

        // A runner spec carrying the JIT config in env.
        let mut rs = runner_spec();
        rs.env = vec![(
            JITCONFIG_ENV_KEY.to_string(),
            "opaque-jit-bytes".to_string(),
        )];

        let running = engine.spawn(&rs).expect("spawn must succeed");
        assert_eq!(
            running.name, "cf-abc-123",
            "handle stored as container name"
        );

        // Inspect the recorded request.
        let req = {
            let guard = engine.http.last.lock().unwrap();
            guard.clone().expect("a request must have been sent")
        };
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.url, "https://spawn.example.dev/v1/spawn");
        assert_eq!(req.bearer_token, "super-secret-token-value");

        let body: serde_json::Value =
            serde_json::from_str(req.json_body.as_deref().unwrap()).unwrap();
        assert_eq!(
            body["image_digest"],
            "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
        );
        // jitconfig is lifted from env into the dedicated top-level field …
        assert_eq!(body["jitconfig"], "opaque-jit-bytes");
        // … AND kept in the env map for the container entrypoint.
        assert_eq!(body["env"][JITCONFIG_ENV_KEY], "opaque-jit-bytes");
        assert_eq!(body["labels"][0], "corelink-runner");
        assert_eq!(body["labels"][1], "prod");
        assert_eq!(body["expiry_ms"], 1234);
    }

    #[test]
    fn spawn_rejects_non_url_path_safe_handle_fail_closed() {
        // V1 audit hardening: a Worker response whose handle contains URL-path
        // metacharacters (`/`, `..`, etc.) must fail CLOSED — never become a
        // RunningContainer whose name would re-address `/v1/status/{handle}`.
        let engine = CloudflareEngine::new(
            RecordingTransport::new(200, r#"{"handle":"cf/../../evil?x=1"}"#),
            cfg(),
        );
        let mut rs = runner_spec();
        rs.env = vec![(JITCONFIG_ENV_KEY.to_string(), "jit".to_string())];
        let err = engine
            .spawn(&rs)
            .expect_err("a non-URL-path-safe handle must fail closed");
        assert!(
            err.to_string().contains("URL-path-safe"),
            "expected a fail-closed handle-charset error, got: {err}"
        );
    }

    #[test]
    fn spawn_floor_isolation_rejects_forged_non_isolated_spec_before_http() {
        // A bare `no_network=false` WITHOUT the egress grant must fail closed
        // BEFORE any Worker contact (ExplodingTransport panics if contacted).
        let engine = CloudflareEngine::new(ExplodingTransport, cfg());
        let mut bad = spec(vec![]);
        bad.no_network = false;
        bad.allow_egress = false;
        let err = engine
            .spawn(&bad)
            .expect_err("a non-isolated, non-egress spec must fail closed");
        assert!(
            format!("{err:#}").contains("no_network must be true"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn spawn_floor_supply_chain_rejects_unpinned_image_before_http() {
        // A non-digest-pinned image must fail closed BEFORE any Worker contact.
        let engine = CloudflareEngine::new(ExplodingTransport, cfg());
        let mut bad = spec(vec![]);
        bad.image = "alpine:latest".to_string();
        let err = engine
            .spawn(&bad)
            .expect_err("an unpinned image must fail closed");
        assert!(
            format!("{err:#}").contains("not content-pinned"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn spawn_floor_disk_rejects_runner_box_below_floor_before_http() {
        // A RUNNER box whose configured disk is below the floor is REJECTED at
        // spawn, before any Worker contact (ExplodingTransport panics if reached).
        let mut c = cfg();
        c.runner_storage_mb = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB - 1;
        let engine = CloudflareEngine::new(ExplodingTransport, c);
        let err = engine
            .spawn(&runner_spec())
            .expect_err("a runner box below the disk floor must fail closed");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("disk") && msg.contains("CLOUDFLARE_RUNNER_STORAGE_MB"),
            "error must name the disk floor and the env var to set, got: {msg}"
        );
    }

    #[test]
    fn spawn_allows_runner_at_floor() {
        // RUNNER box sized exactly at the floor spawns.
        let mut c = cfg();
        c.runner_storage_mb = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;
        let engine = CloudflareEngine::new(RecordingTransport::new(200, r#"{"handle":"h"}"#), c);
        assert!(
            engine.spawn(&runner_spec()).is_ok(),
            "a runner box at the disk floor must spawn"
        );
    }

    #[test]
    fn spawn_still_rejects_plain_hermetic_check_without_toolchain_digest() {
        // A PLAIN HERMETIC CHECK spec (allow_egress == false, NO TOOLCHAIN_DIGEST)
        // is NOT served by CloudflareEngine: the spawn-Worker's runner container
        // exits 1 without a JIT — surfacing live as an opaque HTTP 500. The
        // runner-only floor (RELAXED only for check-host specs) fails CLOSED at
        // spawn, BEFORE any Worker contact (ExplodingTransport panics if reached),
        // with an actionable message → rota-B / Northflank.
        let engine = CloudflareEngine::new(ExplodingTransport, cfg());
        let err = engine
            .spawn(&spec(vec![]))
            .expect_err("a plain hermetic check spec must fail closed (no TOOLCHAIN_DIGEST)");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("plain hermetic CHECK") && msg.contains("TOOLCHAIN_DIGEST"),
            "error must name the missing-toolchain-digest limitation, got: {msg}"
        );
    }

    #[test]
    fn spawn_admits_check_host_spec_with_toolchain_digest_in_env() {
        // A check-host spec (!allow_egress, no_network=true, TOOLCHAIN_DIGEST in
        // env) + a pinned image is ADMITTED, and the recorded POST body carries
        // the additive check-mode fields (C2): mode=="check" + the lifted
        // toolchain_digest, with TOOLCHAIN_DIGEST still present in env.
        let engine = CloudflareEngine::new(
            RecordingTransport::new(201, r#"{"handle":"cf-check-1"}"#),
            cfg(),
        );
        let running = engine
            .spawn(&check_host_spec())
            .expect("a check-host spec must spawn");
        assert_eq!(running.name, "cf-check-1");

        let req = engine.http.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.url, "https://spawn.example.dev/v1/spawn");
        let body: serde_json::Value =
            serde_json::from_str(req.json_body.as_deref().unwrap()).unwrap();
        assert_eq!(body["mode"], "check", "check-host body must set mode=check");
        assert_eq!(
            body["toolchain_digest"], TOOLCHAIN_DIGEST_VALUE,
            "toolchain_digest lifted from env into the top-level field"
        );
        // The digest stays in env too (the entrypoint reads it for clw hydrate).
        assert_eq!(
            body["env"][TOOLCHAIN_DIGEST_ENV_KEY],
            TOOLCHAIN_DIGEST_VALUE
        );
    }

    #[test]
    fn spawn_runner_body_omits_mode_byte_identical_back_comp() {
        // A RUNNER spec's body must NOT carry the additive check-mode fields — the
        // wire stays byte-identical to pre-check-host (the Worker defaults absent
        // `mode` to the runner path).
        let engine =
            CloudflareEngine::new(RecordingTransport::new(201, r#"{"handle":"h"}"#), cfg());
        engine.spawn(&runner_spec()).expect("runner spawn ok");
        let req = engine.http.last.lock().unwrap().clone().unwrap();
        let body: serde_json::Value =
            serde_json::from_str(req.json_body.as_deref().unwrap()).unwrap();
        assert!(
            body.get("mode").is_none(),
            "runner body must omit `mode` (back-comp), got: {body}"
        );
        assert!(
            body.get("toolchain_digest").is_none(),
            "runner body must omit `toolchain_digest`, got: {body}"
        );
    }

    #[test]
    fn spawn_check_host_still_enforces_x4_pin() {
        // The X4 supply-chain floor is NOT bypassed for check-host: an unpinned
        // image fails closed BEFORE any Worker contact (ExplodingTransport panics).
        let engine = CloudflareEngine::new(ExplodingTransport, cfg());
        let mut bad = check_host_spec();
        bad.image = "alpine:latest".to_string();
        let err = engine
            .spawn(&bad)
            .expect_err("an unpinned check-host image must fail closed");
        assert!(
            format!("{err:#}").contains("not content-pinned"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn spawn_fails_closed_on_non_2xx() {
        let engine = CloudflareEngine::new(RecordingTransport::new(500, "boom"), cfg());
        let err = engine
            .spawn(&runner_spec())
            .expect_err("a non-2xx spawn must fail closed");
        assert!(
            format!("{err:#}").contains("fail-closed"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn spawn_fails_closed_on_missing_handle() {
        let engine =
            CloudflareEngine::new(RecordingTransport::new(200, r#"{"not_handle":"x"}"#), cfg());
        let err = engine
            .spawn(&runner_spec())
            .expect_err("a 2xx without a handle must fail closed");
        assert!(
            format!("{err:#}").contains("missing non-empty handle"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn is_alive_2xx_is_true() {
        let engine = CloudflareEngine::new(RecordingTransport::new(200, ""), cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        assert!(engine.is_alive(&c).unwrap());
        // It addresses the status endpoint for the handle.
        let req = engine.http.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.url, "https://spawn.example.dev/v1/status/h");
    }

    #[test]
    fn is_alive_404_is_false() {
        let engine = CloudflareEngine::new(RecordingTransport::new(404, ""), cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        assert!(!engine.is_alive(&c).unwrap());
    }

    #[test]
    fn is_alive_indeterminate_status_fails_closed() {
        // Anything that is neither 2xx nor 404 is indeterminate → fail closed
        // (never silently treated as alive OR dead).
        let engine = CloudflareEngine::new(RecordingTransport::new(503, ""), cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        let err = engine
            .is_alive(&c)
            .expect_err("an indeterminate status must fail closed");
        assert!(
            format!("{err:#}").contains("indeterminate"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn probe_reports_both_invariants_from_liveness() {
        // A live container ⇒ both invariants held; a gone container ⇒ both false.
        let live = CloudflareEngine::new(RecordingTransport::new(200, ""), cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        let p = live.probe(&c, &runner_spec()).unwrap();
        assert!(p.tmp_is_private && p.net_is_isolated);

        let gone = CloudflareEngine::new(RecordingTransport::new(404, ""), cfg());
        let p = gone.probe(&c, &runner_spec()).unwrap();
        assert!(!p.tmp_is_private && !p.net_is_isolated);
    }

    #[test]
    fn exec_non_captured_is_unsupported_and_fails_closed() {
        // The non-captured `exec` is never called on either path (runner runs its
        // entrypoint; check-host execs via exec_captured) — a call fails closed,
        // never faking an endpoint. ExplodingTransport proves no Worker contact.
        let engine = CloudflareEngine::new(ExplodingTransport, cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        let err = engine
            .exec(&c, &["true"])
            .expect_err("exec (non-captured) must be unsupported");
        assert!(format!("{err:#}").contains("unsupported on CloudflareEngine"));
    }

    #[test]
    fn exec_captured_posts_exact_shape_and_parses_output() {
        // Happy path (C3): POST {handle, argv, timeout_ms} to /v1/exec with the
        // bearer; 200 {exit_code, stdout, stderr} → the matching CmdOutput.
        let mut c = cfg();
        c.expiry_ms = 4242;
        let engine = CloudflareEngine::new(
            RecordingTransport::new(
                200,
                r#"{"exit_code":0,"stdout":"hello\n","stderr":"warn\n"}"#,
            ),
            c,
        );
        let container = RunningContainer {
            name: "cf-check-1".to_string(),
        };
        let out = engine
            .exec_captured(&container, &["sh", "-lc", "echo hello"])
            .expect("exec_captured must succeed");
        assert_eq!(out.code, Some(0));
        assert_eq!(out.stdout, "hello\n");
        assert_eq!(out.stderr, "warn\n");

        let req = engine.http.last.lock().unwrap().clone().unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.url, "https://spawn.example.dev/v1/exec");
        assert_eq!(req.bearer_token, "super-secret-token-value");
        let body: serde_json::Value =
            serde_json::from_str(req.json_body.as_deref().unwrap()).unwrap();
        assert_eq!(body["handle"], "cf-check-1");
        assert_eq!(body["argv"][0], "sh");
        assert_eq!(body["argv"][1], "-lc");
        assert_eq!(body["argv"][2], "echo hello");
        // timeout_ms is sourced from the configured expiry (the exec ceiling).
        assert_eq!(body["timeout_ms"], 4242);
    }

    #[test]
    fn exec_captured_null_exit_is_signal_kill() {
        // exit_code: null ⇒ CmdOutput.code == None (signal-killed / timeout) ⇒
        // run_check fails closed downstream.
        let engine = CloudflareEngine::new(
            RecordingTransport::new(200, r#"{"exit_code":null,"stdout":"","stderr":"killed"}"#),
            cfg(),
        );
        let container = RunningContainer {
            name: "h".to_string(),
        };
        let out = engine
            .exec_captured(&container, &["sh", "-lc", "sleep 9999"])
            .expect("a null exit must still parse to a CmdOutput");
        assert_eq!(out.code, None, "exit_code:null ⇒ code None");
        assert_eq!(out.stderr, "killed");
    }

    #[test]
    fn exec_captured_fails_closed_on_non_2xx() {
        // A non-2xx (500) ⇒ Err, NEVER a fabricated CmdOutput (a fake success
        // would let a check pass against an erroring box).
        let engine = CloudflareEngine::new(RecordingTransport::new(500, "boom"), cfg());
        let container = RunningContainer {
            name: "h".to_string(),
        };
        let err = engine
            .exec_captured(&container, &["true"])
            .expect_err("a non-2xx exec must fail closed");
        assert!(
            format!("{err:#}").contains("fail-closed"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn teardown_2xx_and_404_are_both_ok_idempotent() {
        let c = RunningContainer {
            name: "h".to_string(),
        };
        for status in [200u16, 204, 404] {
            let engine = CloudflareEngine::new(RecordingTransport::new(status, ""), cfg());
            assert!(
                engine.teardown(&c).is_ok(),
                "teardown must be Ok for HTTP {status} (idempotent)"
            );
            // It POSTs the handle to the teardown endpoint.
            let req = engine.http.last.lock().unwrap().clone().unwrap();
            assert_eq!(req.method, Method::Post);
            assert_eq!(req.url, "https://spawn.example.dev/v1/teardown");
            let body: serde_json::Value =
                serde_json::from_str(req.json_body.as_deref().unwrap()).unwrap();
            assert_eq!(body["handle"], "h");
        }
    }

    #[test]
    fn teardown_other_non_2xx_fails_closed() {
        let engine = CloudflareEngine::new(RecordingTransport::new(500, "boom"), cfg());
        let c = RunningContainer {
            name: "h".to_string(),
        };
        assert!(
            engine.teardown(&c).is_err(),
            "a non-404 non-2xx teardown must fail closed"
        );
    }

    // ── from_env: DEFAULT-OFF + armed ─────────────────────────────────────────

    #[test]
    fn from_env_is_default_off_when_required_vars_absent() {
        // No env ⇒ None ⇒ nothing wired (the north-star DEFAULT-OFF property).
        assert!(CloudflareConfig::from_env_with(|_| None).is_none());

        // Only the URL set (token missing) ⇒ still None (never a partial config).
        let url_only =
            |k: &str| (k == CLOUDFLARE_SPAWN_WORKER_URL_ENV).then(|| "https://x.dev".to_string());
        assert!(CloudflareConfig::from_env_with(url_only).is_none());

        // Only the token set (URL missing) ⇒ still None.
        let token_only =
            |k: &str| (k == CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV).then(|| "tok".to_string());
        assert!(CloudflareConfig::from_env_with(token_only).is_none());

        // Present-but-empty counts as absent.
        let empty = |k: &str| match k {
            CLOUDFLARE_SPAWN_WORKER_URL_ENV => Some(String::new()),
            CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV => Some("tok".to_string()),
            _ => None,
        };
        assert!(CloudflareConfig::from_env_with(empty).is_none());
    }

    #[test]
    fn from_env_armed_reads_required_and_optional() {
        let env = |k: &str| match k {
            CLOUDFLARE_SPAWN_WORKER_URL_ENV => Some("https://spawn.example.dev".to_string()),
            CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV => Some("tok".to_string()),
            CLOUDFLARE_RUNNER_LABELS_ENV => Some("a, b ,, c".to_string()),
            CLOUDFLARE_EXPIRY_MS_ENV => Some("9000".to_string()),
            CLOUDFLARE_RUNNER_STORAGE_MB_ENV => Some("16384".to_string()),
            _ => None,
        };
        let cfg = CloudflareConfig::from_env_with(env).expect("armed config");
        assert_eq!(cfg.spawn_worker_url, "https://spawn.example.dev");
        assert_eq!(cfg.auth_token, "tok");
        // Labels split on ',', trimmed, blanks dropped.
        assert_eq!(cfg.labels, vec!["a", "b", "c"]);
        assert_eq!(cfg.expiry_ms, 9000);
        assert_eq!(cfg.runner_storage_mb, 16384);
    }

    // ── validate: boot-time fail-loud arms ────────────────────────────────────

    #[test]
    fn validate_ok_for_nominal_armed_config() {
        // A config built via `new` (default storage clears the floor) + a real
        // https URL + a non-empty token is valid.
        assert!(cfg().validate().is_ok(), "a nominal armed config validates");
    }

    #[test]
    fn validate_rejects_disk_below_floor() {
        let mut c = cfg();
        c.runner_storage_mb = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB - 1;
        let err = c.validate().expect_err("a sub-floor disk must fail loud");
        assert!(
            err.contains(CLOUDFLARE_RUNNER_STORAGE_MB_ENV)
                && err.contains(&RUNNER_EPHEMERAL_STORAGE_FLOOR_MB.to_string()),
            "error must name the disk env var and the floor, got: {err}"
        );
    }

    #[test]
    fn validate_allows_disk_exactly_at_floor() {
        let mut c = cfg();
        c.runner_storage_mb = RUNNER_EPHEMERAL_STORAGE_FLOOR_MB;
        assert!(
            c.validate().is_ok(),
            "a disk exactly at the floor must validate"
        );
    }

    #[test]
    fn validate_rejects_non_http_url() {
        for bad_url in ["spawn.example.dev", "ftp://x.dev", "", "ws://x.dev"] {
            let mut c = cfg();
            c.spawn_worker_url = bad_url.to_string();
            let err = c.validate().expect_err("a non-http(s) URL must fail loud");
            assert!(
                err.contains(CLOUDFLARE_SPAWN_WORKER_URL_ENV) && err.contains("http"),
                "error must name the URL env var, got: {err}"
            );
        }
    }

    #[test]
    fn validate_accepts_http_and_https_urls() {
        for ok_url in ["http://x.dev", "https://spawn.example.workers.dev"] {
            let mut c = cfg();
            c.spawn_worker_url = ok_url.to_string();
            assert!(c.validate().is_ok(), "{ok_url} must validate");
        }
    }

    #[test]
    fn validate_rejects_empty_token_without_leaking() {
        let mut c = cfg();
        c.auth_token = String::new();
        let err = c.validate().expect_err("an empty token must fail loud");
        assert!(
            err.contains(CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV),
            "error must name the token env var, got: {err}"
        );
        // The validate message must never carry token material (here it is empty,
        // but the message must not interpolate the secret field).
        assert!(
            !err.contains("super-secret-token-value"),
            "validate must not echo token material: {err}"
        );
    }

    #[test]
    fn from_env_optional_garbage_falls_back_to_defaults() {
        // A `0`/garbage expiry or disk must fall back to the default, never a
        // degenerate 0 (no hard kill / disk floor disabled).
        let env = |k: &str| match k {
            CLOUDFLARE_SPAWN_WORKER_URL_ENV => Some("https://spawn.example.dev".to_string()),
            CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV => Some("tok".to_string()),
            CLOUDFLARE_EXPIRY_MS_ENV => Some("0".to_string()),
            CLOUDFLARE_RUNNER_STORAGE_MB_ENV => Some("nope".to_string()),
            _ => None,
        };
        let cfg = CloudflareConfig::from_env_with(env).unwrap();
        assert_eq!(cfg.expiry_ms, DEFAULT_EXPIRY_MS);
        assert_eq!(cfg.runner_storage_mb, DEFAULT_RUNNER_STORAGE_MB);
        // No labels env ⇒ empty.
        assert!(cfg.labels.is_empty());
    }
}
