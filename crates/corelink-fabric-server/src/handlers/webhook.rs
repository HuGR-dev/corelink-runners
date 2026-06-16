//! Stage-B autoscaler — GitHub `workflow_job` webhook → ephemeral runner lease
//! (ADR-0007 Stage B).
//!
//! ## What this is
//!
//! Stage A wired the runner-lease lifecycle (broker · image · net-policy ·
//! provision→wait→teardown) and proved a job runs on a freshly-provisioned
//! ephemeral box — but provisioning was driven MANUALLY (an out-of-band
//! `acquire(runner)` per job). Stage B closes the loop: GitHub itself tells us
//! when a job is queued, and the fabric provisions exactly one ephemeral runner
//! for it. That is what lets a normal `push`/PR CI run land on the cloud fleet
//! with no human in the loop — the property that retires the builder Mac.
//!
//! ```text
//!  GitHub: job queued ─(workflow_job webhook)─▶ this handler
//!     │                                            │ verify HMAC, match label
//!     │                                            ▼
//!     │                              acquire(runner=Repo{owner,repo}, labels)
//!     │                                            │  (the SAME audited core
//!     │                                            │   leases::acquire drives)
//!     ▼                                            ▼
//!  job assigned ◀──── JIT runner registers ──── ephemeral box provisioned
//!     │
//!  job completed ─(workflow_job webhook)─▶ this handler ─▶ cancel(lease)
//!                                                          (free the slot now)
//! ```
//!
//! ## Security posture (default-off, HMAC-gated, no new authority)
//!
//! - **Default-off:** no `FABRIC_AUTOSCALER_WEBHOOK_SECRET` ⇒ the route is not
//!   even mounted (404). Mirrors the occupancy/admin internal-route pattern.
//! - **Authenticated by HMAC, not a PAT:** GitHub signs every delivery with the
//!   App webhook secret (`X-Hub-Signature-256: sha256=<hex>`). We recompute the
//!   HMAC-SHA256 over the RAW body and compare in constant time
//!   ([`crate::ingest_token`]'s RFC-4231-pinned primitive). A bad/absent
//!   signature ⇒ 401, before any parse or provision.
//! - **No privilege escalation:** the handler does NOT bypass admission. It
//!   acquires as a CONFIGURED tenant PAT through the very same audited
//!   [`leases::acquire`] path (cap gate · rate ceiling · atomic reserve · mint ·
//!   provision · fail-closed rollback). The autoscaler can do nothing a holder
//!   of that PAT could not already do at `POST /v1/leases`.
//! - **Scoped by label (and optional repo allowlist):** a job is served only if
//!   it carries one of the operator's managed labels — so jobs targeting
//!   `ubuntu-latest`, the persistent `corelink-builder`, or any other pool are
//!   ignored. The repo the runner registers against comes from the
//!   HMAC-authenticated body; an optional owner/repo allowlist is a cheap
//!   defense-in-depth bound on top.
//!
//! ## Dependency
//!
//! Provisioning a runner lease requires the Stage-A registration broker
//! (`FABRIC_GITHUB_APP_*`) to be wired; without it `leases::acquire` fails
//! closed (`400`) and this handler logs the deferral and acks the webhook (a
//! non-2xx would only make GitHub redeliver into the same closed door).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, http::header::HeaderName};
use corelink_fabric::TenantId;
use corelink_fabric_api::{AcquireRequest, AcquireResponse, RunnerSpec, RunnerTargetDto};
use serde::Deserialize;

use crate::app::AppState;
use crate::auth::{BearerPat, TokenStore};
use crate::handlers::envelope::HookRegistry;
use crate::handlers::leases;

/// GitHub's per-delivery HMAC-SHA256 signature header (hex, `sha256=` prefixed).
const SIGNATURE_HEADER: &str = "x-hub-signature-256";
/// GitHub's event-type header (`workflow_job`, `ping`, …).
const EVENT_HEADER: &str = "x-github-event";

/// The default managed label the fabric serves when `FABRIC_AUTOSCALER_LABELS`
/// is unset. Deliberately NOT `corelink-builder` (the persistent self-hosted
/// runner's label) so the autoscaler never races the always-on builder.
const DEFAULT_MANAGED_LABEL: &str = "corelink";
/// Default runner tmp root (a runner box's work dir; the value is advisory —
/// the runner agent uses its own `_work` folder, this is the lease tmp_root the
/// isolation gate validates).
const DEFAULT_TMP_ROOT: &str = "/tmp/runner";
/// Default lease TTL: 1h. Generous enough for any single CI job; the reaper is
/// the backstop if a `completed` webhook is ever missed.
const DEFAULT_EXPIRY_MS: u64 = 3_600_000;
/// Default bound on the job→lease tracking map (dedup + cancel). Far above any
/// realistic in-flight CI fan-out; oldest entries are evicted (never silently).
const DEFAULT_MAX_TRACKED_JOBS: usize = 4096;
/// Cap on the acquire-response body we drain to read the lease id (the body is a
/// tiny `AcquireResponse`; this only bounds a pathological backend).
const ACQUIRE_BODY_LIMIT: usize = 64 * 1024;

// ── Config ──────────────────────────────────────────────────────────────────

/// Resolved autoscaler configuration. Carries the tenant PAT it acquires as —
/// so `Debug` is intentionally NOT derived (the PAT must never reach a log).
pub struct AutoscalerConfig {
    /// The tenant PAT the autoscaler acquires runner leases as. Resolved to a
    /// tenant via the same [`TokenStore`] the auth layer uses; that tenant must
    /// have a plan (cap ≥ 1) or every acquire fails over-cap. For dogfood this
    /// is simply the bootstrap `FABRIC_PAT`.
    pub pat: String,
    /// The digest-pinned runner image the lease provisions (X4 floor). An
    /// unpinned reference is rejected by `ContainerSpec::from_runner_lease`.
    pub image_digest: String,
    /// Labels the fabric serves. A queued job is provisioned only if its labels
    /// intersect this set (so non-CoreLink pools are ignored).
    pub managed_labels: Vec<String>,
    /// Lease tmp root passed through to the isolation gate.
    pub tmp_root: String,
    /// Lease TTL in ms (the reaper backstop if a `completed` event is missed).
    pub expiry_ms: u64,
    /// Optional owner/repo allowlist (lowercased). `None` ⇒ serve any repo the
    /// HMAC authenticates; `Some` ⇒ only these `(owner, repo)` pairs.
    pub repo_allowlist: Option<Vec<(String, String)>>,
    /// Bound on the job→lease tracking map.
    pub max_tracked_jobs: usize,
}

impl std::fmt::Debug for AutoscalerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoscalerConfig")
            .field("pat", &"***REDACTED***")
            .field("image_digest", &self.image_digest)
            .field("managed_labels", &self.managed_labels)
            .field("tmp_root", &self.tmp_root)
            .field("expiry_ms", &self.expiry_ms)
            .field("repo_allowlist", &self.repo_allowlist)
            .field("max_tracked_jobs", &self.max_tracked_jobs)
            .finish()
    }
}

// ── Handler state ─────────────────────────────────────────────────────────────

/// State for the webhook handler — injected via axum [`State`] on a separate
/// router branch (OUTSIDE the Bearer-PAT layer; GitHub authenticates by HMAC).
#[derive(Clone)]
pub struct WebhookHandlerState {
    /// The App webhook HMAC secret. `None` ⇒ the route 404s (default-off). The
    /// composition root treats a blank env var as `None`.
    pub secret: Option<Arc<str>>,
    /// The fabric app state — the audited acquire/cancel core this handler
    /// drives. The SAME `AppState` the `/v1` handlers use (shared ledger, broker,
    /// provisioner, hook registry).
    pub app: AppState,
    /// Token store to resolve the configured autoscaler PAT → tenant.
    pub store: Arc<dyn TokenStore + Send + Sync>,
    /// The shared §13 hook registry the acquire path registers a CaptureHook in
    /// (the SAME instance the HTTP poll endpoints read — wired in the
    /// composition root).
    pub registry: Arc<HookRegistry>,
    /// Provisioning configuration.
    pub cfg: Arc<AutoscalerConfig>,
    /// `workflow_job.id` → lease id. Bounded; serves two jobs at once: dedup of
    /// redelivered `queued` events, and `completed`→cancel of the right lease.
    pub jobs: Arc<Mutex<JobLeaseMap>>,
}

/// A bounded `job_id → lease_id` map with FIFO eviction. A placeholder (empty
/// lease id) claims a job at the START of provisioning so a concurrently
/// redelivered `queued` cannot double-provision; the real lease id is filled in
/// on success, or the claim is dropped on failure.
#[derive(Debug)]
pub struct JobLeaseMap {
    map: HashMap<u64, String>,
    order: VecDeque<u64>,
    cap: usize,
}

impl JobLeaseMap {
    /// A map bounded to `cap` entries (coerced to ≥ 1).
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    /// Claim `job_id` if not already present (insert an empty placeholder).
    /// Returns `true` if the claim is FRESH (caller should provision), `false`
    /// if the job was already seen (dedup — caller does nothing). Eviction of
    /// the oldest entry is logged, never silent.
    fn claim(&mut self, job_id: u64) -> bool {
        if self.map.contains_key(&job_id) {
            return false;
        }
        while self.order.len() >= self.cap {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                    eprintln!(
                        "autoscaler: job-tracking map at cap {}, evicted oldest tracked job {old} \
                         (its lease, if any, falls back to deadline-reaper teardown)",
                        self.cap
                    );
                }
                None => break,
            }
        }
        self.order.push_back(job_id);
        self.map.insert(job_id, String::new());
        true
    }

    /// Fill in the lease id for a job whose claim is still held.
    fn record_lease(&mut self, job_id: u64, lease_id: String) {
        if self.map.contains_key(&job_id) {
            self.map.insert(job_id, lease_id);
        }
    }

    /// Drop a claim (provision failed) so a later redelivery could retry.
    fn drop_claim(&mut self, job_id: u64) {
        if self.map.remove(&job_id).is_some() {
            self.order.retain(|&x| x != job_id);
        }
    }

    /// Remove `job_id` and return its lease id if one was recorded (non-empty).
    /// An empty placeholder (provision in-flight/failed) yields `None`.
    fn take(&mut self, job_id: u64) -> Option<String> {
        let lease = self.map.remove(&job_id)?;
        self.order.retain(|&x| x != job_id);
        if lease.is_empty() { None } else { Some(lease) }
    }
}

// ── Webhook payload (only the fields we read; GitHub sends many more) ─────────

#[derive(Debug, Deserialize)]
struct WorkflowJobEvent {
    action: String,
    workflow_job: WorkflowJob,
    repository: Repository,
}

#[derive(Debug, Deserialize)]
struct WorkflowJob {
    id: u64,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
    owner: RepoOwner,
}

#[derive(Debug, Deserialize)]
struct RepoOwner {
    login: String,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /webhooks/github` — the autoscaler webhook receiver.
///
/// `Bytes` is the last extractor so we get the RAW body (the HMAC is computed
/// over exactly those bytes, before any JSON parse).
pub async fn github_webhook(
    State(state): State<WebhookHandlerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Default-off: no secret configured ⇒ the route is invisible (404).
    let Some(secret) = state.secret.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Authenticate the delivery by HMAC over the raw body. Absent/bad ⇒ 401,
    // before any parse or side effect.
    if !signature_valid(
        secret.as_bytes(),
        &body,
        header_str(&headers, SIGNATURE_HEADER),
    ) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Dispatch on the event type. `ping` (sent when the webhook is created) and
    // any non-`workflow_job` event are acknowledged with 200 and ignored.
    match header_str(&headers, EVENT_HEADER) {
        Some("workflow_job") => {}
        Some("ping") => return ack("pong"),
        _ => return ack("ignored: not a workflow_job event"),
    }

    let event: WorkflowJobEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(_) => {
            // The HMAC verified, so this is genuinely from GitHub — a parse
            // failure means the payload shape drifted. Surface it (400), do not
            // silently swallow.
            return (StatusCode::BAD_REQUEST, "unparseable workflow_job payload").into_response();
        }
    };

    match event.action.as_str() {
        "queued" => handle_queued(&state, event).await,
        "completed" => handle_completed(&state, event).await,
        // in_progress and any future action: nothing to do.
        other => ack(&format!("ignored: workflow_job action={other}")),
    }
}

/// Handle `workflow_job.queued`: provision one ephemeral runner if the job is
/// for us (managed label + allowlist) and not already claimed.
async fn handle_queued(state: &WebhookHandlerState, event: WorkflowJobEvent) -> Response {
    let job_id = event.workflow_job.id;
    let owner = event.repository.owner.login;
    let repo = event.repository.name;
    let labels = event.workflow_job.labels;

    // Label gate: only serve jobs that target one of our managed labels.
    if !labels.iter().any(|l| state.cfg.managed_labels.contains(l)) {
        return ack("ignored: no managed label on the job");
    }

    // Optional repo allowlist (defense-in-depth on top of the HMAC).
    if let Some(allow) = &state.cfg.repo_allowlist {
        let key = (owner.to_ascii_lowercase(), repo.to_ascii_lowercase());
        if !allow.contains(&key) {
            eprintln!(
                "autoscaler: job {job_id} for {owner}/{repo} is not in the repo allowlist — ignored"
            );
            return ack("ignored: repo not in allowlist");
        }
    }

    // Claim the job FIRST (under the lock, no await) so a concurrently
    // redelivered `queued` for the same job cannot double-provision.
    if !claim_job(state, job_id) {
        return ack("deduped: job already claimed");
    }

    eprintln!("autoscaler: provisioning runner for queued job {job_id} ({owner}/{repo})");
    match provision_runner(state, &owner, &repo, labels).await {
        Some(lease_id) => {
            record_lease(state, job_id, lease_id.clone());
            eprintln!("autoscaler: job {job_id} → runner lease {lease_id}");
            ack(&format!("provisioned lease {lease_id}"))
        }
        None => {
            // Provision failed (over-cap, broker off, transient). Drop the claim
            // so a redelivery could retry. Ack 200 — a non-2xx would only make
            // GitHub redeliver into the same closed door.
            drop_claim(state, job_id);
            ack("deferred: could not provision (see server log)")
        }
    }
}

/// Handle `workflow_job.completed`: cancel the lease we provisioned for it, if
/// any, so the concurrency slot + box are reclaimed immediately (rather than at
/// the lease deadline).
async fn handle_completed(state: &WebhookHandlerState, event: WorkflowJobEvent) -> Response {
    let job_id = event.workflow_job.id;
    let Some(lease_id) = take_lease(state, job_id) else {
        // Not one of ours (or already reclaimed) — nothing to do.
        return ack("ignored: no tracked lease for this job");
    };
    eprintln!("autoscaler: job {job_id} completed → cancelling lease {lease_id}");
    cancel_lease(state, lease_id).await;
    ack("cancelled the job's lease")
}

// ── Provision / cancel via the audited leases core ────────────────────────────

/// Acquire a runner lease for `owner/repo` with `labels`, returning the new
/// lease id on success or `None` on any failure. Drives the SAME audited
/// [`leases::acquire`] path the `/v1` surface uses — no admission bypass.
async fn provision_runner(
    state: &WebhookHandlerState,
    owner: &str,
    repo: &str,
    labels: Vec<String>,
) -> Option<String> {
    let tenant = resolve_tenant(state).await?;

    let req = AcquireRequest {
        image_digest: state.cfg.image_digest.clone(),
        // Ignored for runner mode — `leases::acquire` FORCES "egress-runner"
        // server-side. Sent for shape only.
        net_policy: "egress-runner".to_string(),
        tmp_root: state.cfg.tmp_root.clone(),
        expiry_ms: state.cfg.expiry_ms,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: owner.to_string(),
                repo: repo.to_string(),
            },
            labels,
        }),
    };

    let resp = leases::acquire(
        State(state.app.clone()),
        Extension(tenant),
        Extension(Arc::clone(&state.registry)),
        Extension(BearerPat(state.cfg.pat.clone())),
        Json(req),
    )
    .await;

    let status = resp.status();
    if status != StatusCode::OK {
        eprintln!("autoscaler: acquire(runner) returned {status} for {owner}/{repo}");
        return None;
    }

    // Drain + parse the lease id out of the AcquireResponse.
    let bytes = match axum::body::to_bytes(resp.into_body(), ACQUIRE_BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => {
            eprintln!("autoscaler: could not read acquire response body");
            return None;
        }
    };
    match serde_json::from_slice::<AcquireResponse>(&bytes) {
        Ok(acq) => Some(acq.lease.lease_id),
        Err(_) => {
            eprintln!("autoscaler: could not parse acquire response");
            None
        }
    }
}

/// Cancel `lease_id` through the audited [`leases::cancel`] path (frees the slot
/// and tears down the box). Best-effort: a failure is logged and the deadline
/// reaper is the backstop.
async fn cancel_lease(state: &WebhookHandlerState, lease_id: String) {
    let Some(tenant) = resolve_tenant(state).await else {
        eprintln!("autoscaler: cannot resolve tenant to cancel lease {lease_id}");
        return;
    };
    let resp = leases::cancel(
        State(state.app.clone()),
        Extension(tenant),
        Path(lease_id.clone()),
    )
    .await;
    eprintln!("autoscaler: cancel lease {lease_id} → {}", resp.status());
}

/// Resolve the configured autoscaler PAT to its tenant, off the blocking pool
/// (the production `CoreLinkTokenStore` does a synchronous introspect round-trip
/// — never pin an async worker, mirroring `auth::require_tenant`). Any failure
/// (unknown PAT, store unreachable, task panic) ⇒ `None`, fail-closed.
async fn resolve_tenant(state: &WebhookHandlerState) -> Option<TenantId> {
    let store = Arc::clone(&state.store);
    let pat = state.cfg.pat.clone();
    match tokio::task::spawn_blocking(move || store.tenant_of(&pat)).await {
        Ok(Ok(Some(t))) => Some(t),
        Ok(Ok(None)) => {
            eprintln!("autoscaler: configured PAT is not a known tenant — cannot provision");
            None
        }
        Ok(Err(_)) => {
            eprintln!("autoscaler: token store unreachable resolving the autoscaler PAT");
            None
        }
        Err(_) => {
            eprintln!("autoscaler: PAT-resolution task panicked");
            None
        }
    }
}

// ── jobs-map helpers (each takes the lock for a short critical section only) ──

fn claim_job(state: &WebhookHandlerState, job_id: u64) -> bool {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .claim(job_id)
}

fn record_lease(state: &WebhookHandlerState, job_id: u64, lease_id: String) {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .record_lease(job_id, lease_id);
}

fn drop_claim(state: &WebhookHandlerState, job_id: u64) {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .drop_claim(job_id);
}

fn take_lease(state: &WebhookHandlerState, job_id: u64) -> Option<String> {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take(job_id)
}

// ── Signature verification + small helpers ────────────────────────────────────

/// Validate the GitHub `X-Hub-Signature-256` header against the HMAC-SHA256 of
/// the raw body under `secret`, in constant time. Absent header, wrong scheme,
/// or any mismatch ⇒ `false`.
fn signature_valid(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(sig) = header else {
        return false;
    };
    let Some(hex) = sig.strip_prefix("sha256=") else {
        return false;
    };
    let expected = hex_lower(&crate::ingest_token::hmac_sha256(secret, body));
    crate::ingest_token::constant_time_eq(expected.as_bytes(), hex.as_bytes())
}

/// Lowercase-hex encode (GitHub's signature digest form). No `hex` crate.
fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Case-insensitive header lookup as a `&str` (header names are ASCII).
fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let hn = HeaderName::from_bytes(name.as_bytes()).ok()?;
    headers.get(&hn)?.to_str().ok()
}

/// A 200 acknowledgement with a short JSON body (`{"ok":true,"detail":…}`).
fn ack(detail: &str) -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "detail": detail })),
    )
        .into_response()
}

// ── Composition-root wiring (FABRIC_AUTOSCALER_* → config) ─────────────────────

/// Env var names the autoscaler reads. Centralized so the docs, the reader, and
/// the tests cannot drift.
pub mod env {
    /// The GitHub App webhook HMAC secret. REQUIRED to enable the autoscaler
    /// (absent ⇒ the route is not mounted; the feature is simply off).
    pub const WEBHOOK_SECRET: &str = "FABRIC_AUTOSCALER_WEBHOOK_SECRET";
    /// The tenant PAT the autoscaler acquires runner leases as. REQUIRED.
    pub const PAT: &str = "FABRIC_AUTOSCALER_PAT";
    /// The digest-pinned runner image. REQUIRED.
    pub const RUNNER_IMAGE: &str = "FABRIC_AUTOSCALER_RUNNER_IMAGE";
    /// CSV of managed labels (default `corelink`). A job is served only if its
    /// labels intersect this set.
    pub const LABELS: &str = "FABRIC_AUTOSCALER_LABELS";
    /// Lease tmp root (default `/tmp/runner`).
    pub const TMP_ROOT: &str = "FABRIC_AUTOSCALER_TMP_ROOT";
    /// Lease TTL in ms (default 3_600_000 = 1h).
    pub const EXPIRY_MS: &str = "FABRIC_AUTOSCALER_EXPIRY_MS";
    /// Optional CSV owner/repo allowlist (`owner/repo,owner2/repo2`). Unset ⇒
    /// serve any repo the HMAC authenticates.
    pub const REPO_ALLOWLIST: &str = "FABRIC_AUTOSCALER_REPO_ALLOWLIST";
    /// Bound on the job→lease tracking map (default 4096).
    pub const MAX_TRACKED_JOBS: &str = "FABRIC_AUTOSCALER_MAX_TRACKED_JOBS";
}

/// Build the autoscaler `(secret, config)` from the environment, or `None`.
///
/// **Default-off contract** (mirrors `runner_broker_from_env`):
/// - [`env::WEBHOOK_SECRET`] ABSENT ⇒ `None`, silently (the autoscaler is simply
///   not configured; the route is not mounted).
/// - Secret PRESENT but a required companion ([`env::PAT`] / [`env::RUNNER_IMAGE`])
///   missing ⇒ `None` with a redacted stderr diagnostic (the operator clearly
///   intended the autoscaler, so a misconfiguration must be loud).
#[must_use]
pub fn autoscaler_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> Option<(Arc<str>, AutoscalerConfig)> {
    let nonempty = |k: &str| {
        get(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    // Secret absent ⇒ off (no diagnostic; this is the default).
    let secret = nonempty(env::WEBHOOK_SECRET)?;

    // From here the operator intends the autoscaler: a missing companion is a
    // LOUD (secret-free) misconfiguration that disables it, never silence.
    let Some(pat) = nonempty(env::PAT) else {
        eprintln!(
            "autoscaler: {} is set but {} is missing — autoscaler DISABLED",
            env::WEBHOOK_SECRET,
            env::PAT
        );
        return None;
    };
    let Some(image_digest) = nonempty(env::RUNNER_IMAGE) else {
        eprintln!(
            "autoscaler: {} is set but {} is missing — autoscaler DISABLED",
            env::WEBHOOK_SECRET,
            env::RUNNER_IMAGE
        );
        return None;
    };

    let managed_labels = match nonempty(env::LABELS) {
        Some(csv) => {
            let v: Vec<String> = csv
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if v.is_empty() {
                vec![DEFAULT_MANAGED_LABEL.to_string()]
            } else {
                v
            }
        }
        None => vec![DEFAULT_MANAGED_LABEL.to_string()],
    };

    let tmp_root = nonempty(env::TMP_ROOT).unwrap_or_else(|| DEFAULT_TMP_ROOT.to_string());
    let expiry_ms = nonempty(env::EXPIRY_MS)
        .and_then(|s| s.parse().ok())
        .filter(|&ms: &u64| ms > 0)
        .unwrap_or(DEFAULT_EXPIRY_MS);
    let max_tracked_jobs = nonempty(env::MAX_TRACKED_JOBS)
        .and_then(|s| s.parse().ok())
        .filter(|&n: &usize| n > 0)
        .unwrap_or(DEFAULT_MAX_TRACKED_JOBS);

    let repo_allowlist = nonempty(env::REPO_ALLOWLIST).map(|csv| {
        csv.split(',')
            .filter_map(|pair| {
                let pair = pair.trim();
                let (owner, repo) = pair.split_once('/')?;
                let owner = owner.trim().to_ascii_lowercase();
                let repo = repo.trim().to_ascii_lowercase();
                (!owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
            })
            .collect::<Vec<_>>()
    });

    let cfg = AutoscalerConfig {
        pat,
        image_digest,
        managed_labels,
        tmp_root,
        expiry_ms,
        repo_allowlist,
        max_tracked_jobs,
    };
    Some((Arc::from(secret), cfg))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::routing::post;
    use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
    use corelink_runner::lease::ContainerSpec;
    use tower::ServiceExt as _;

    use super::*;
    use crate::app::{AppState, Clock, StaticPlans};
    use crate::auth::StaticTokenStore;
    use crate::cloud_exec::BoxProvisioner;
    use crate::runner_broker::MockBroker;

    const PINNED: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
    const SECRET: &str = "test-webhook-secret";
    const PAT: &str = "pat-dogfood";

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }

    /// A provisioner that succeeds and records provision/teardown calls.
    #[derive(Default)]
    struct RecordingProvisioner {
        provisioned: Arc<Mutex<Vec<String>>>,
        torn_down: Arc<Mutex<Vec<String>>>,
    }
    impl BoxProvisioner for RecordingProvisioner {
        fn provision(&self, lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            self.provisioned.lock().unwrap().push(lease_id.to_string());
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.torn_down.lock().unwrap().push(lease_id.to_string());
            Ok(())
        }
    }

    fn tenant() -> TenantId {
        TenantId::new("dogfood").unwrap()
    }

    /// Shared call-log (lease ids) a recording provisioner appends to.
    type CallLog = Arc<Mutex<Vec<String>>>;

    /// Build a webhook state with runner mode wired (MockBroker), a recording
    /// provisioner, a plan with `cap`, and the given secret/labels/allowlist.
    /// Returns the state plus the (provisioned, torn-down) call logs.
    fn webhook_state(
        secret: Option<&str>,
        cap: u32,
        labels: Vec<String>,
        allowlist: Option<Vec<(String, String)>>,
    ) -> (WebhookHandlerState, CallLog, CallLog) {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let prov = RecordingProvisioner::default();
        let provisioned = Arc::clone(&prov.provisioned);
        let torn_down = Arc::clone(&prov.torn_down);
        let mut app = AppState::new(
            Arc::clone(&ledger),
            Arc::new(StaticPlans::new([TenantPlan {
                tenant: tenant(),
                max_concurrency: cap,
                rate_ceiling_per_min: 1000,
            }])),
            Arc::new(FixedClock(1_717_000_000_000)),
        )
        .with_runner_broker(Arc::new(MockBroker::new()));
        app.provisioner = Arc::new(prov);

        let store = Arc::new(StaticTokenStore::new([(PAT.to_string(), tenant())]));
        let cfg = AutoscalerConfig {
            pat: PAT.to_string(),
            image_digest: PINNED.to_string(),
            managed_labels: labels,
            tmp_root: "/tmp/runner".to_string(),
            expiry_ms: 60_000,
            repo_allowlist: allowlist,
            max_tracked_jobs: 4096,
        };
        let state = WebhookHandlerState {
            secret: secret.map(Arc::from),
            app,
            store,
            registry: Arc::new(HookRegistry::default()),
            cfg: Arc::new(cfg),
            jobs: Arc::new(Mutex::new(JobLeaseMap::new(4096))),
        };
        (state, provisioned, torn_down)
    }

    fn router(state: WebhookHandlerState) -> Router {
        Router::new()
            .route("/webhooks/github", post(github_webhook))
            .with_state(state)
    }

    fn sign(secret: &str, body: &str) -> String {
        format!(
            "sha256={}",
            hex_lower(&crate::ingest_token::hmac_sha256(
                secret.as_bytes(),
                body.as_bytes()
            ))
        )
    }

    fn webhook_request(event: &str, body: &str, sig: Option<&str>) -> Request<Body> {
        let mut b = Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header(header::CONTENT_TYPE, "application/json")
            .header(EVENT_HEADER, event);
        if let Some(s) = sig {
            b = b.header(SIGNATURE_HEADER, s);
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    fn queued_body(job_id: u64, labels: &[&str], owner: &str, repo: &str) -> String {
        let labels: Vec<String> = labels.iter().map(|l| format!("\"{l}\"")).collect();
        format!(
            r#"{{"action":"queued","workflow_job":{{"id":{job_id},"labels":[{}]}},"repository":{{"name":"{repo}","owner":{{"login":"{owner}"}}}}}}"#,
            labels.join(",")
        )
    }

    fn completed_body(job_id: u64, owner: &str, repo: &str) -> String {
        format!(
            r#"{{"action":"completed","workflow_job":{{"id":{job_id},"labels":["corelink-dogfood"]}},"repository":{{"name":"{repo}","owner":{{"login":"{owner}"}}}}}}"#
        )
    }

    // ── feature-off / auth ────────────────────────────────────────────────────

    /// No secret configured ⇒ the route 404s (default-off, invisible).
    #[tokio::test]
    async fn no_secret_configured_is_404() {
        let (state, _, _) = webhook_state(None, 5, vec!["corelink-dogfood".into()], None);
        let body = queued_body(1, &["corelink-dogfood"], "humangr-labs", "corelink-runners");
        let resp = router(state)
            .oneshot(webhook_request(
                "workflow_job",
                &body,
                Some(&sign(SECRET, &body)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// A wrong/absent signature ⇒ 401 before any side effect.
    #[tokio::test]
    async fn bad_signature_is_401() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = queued_body(1, &["corelink-dogfood"], "humangr-labs", "corelink-runners");

        // Missing header.
        let r1 = router(state.clone())
            .oneshot(webhook_request("workflow_job", &body, None))
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::UNAUTHORIZED);

        // Wrong signature.
        let r2 = router(state)
            .oneshot(webhook_request(
                "workflow_job",
                &body,
                Some("sha256=deadbeef"),
            ))
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::UNAUTHORIZED);

        assert!(
            provisioned.lock().unwrap().is_empty(),
            "no box may be provisioned on an unauthenticated webhook"
        );
    }

    /// A `ping` event with a valid signature ⇒ 200 (and no provision).
    #[tokio::test]
    async fn ping_is_acked() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = r#"{"zen":"Keep it simple."}"#;
        let resp = router(state)
            .oneshot(webhook_request("ping", body, Some(&sign(SECRET, body))))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(provisioned.lock().unwrap().is_empty());
    }

    // ── queued → provision ────────────────────────────────────────────────────

    /// A queued job with a managed label provisions exactly one runner lease and
    /// tracks it (job→lease).
    #[tokio::test]
    async fn queued_managed_label_provisions_one_runner() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = queued_body(
            42,
            &["corelink-dogfood"],
            "humangr-labs",
            "corelink-runners",
        );
        let resp = router(state.clone())
            .oneshot(webhook_request(
                "workflow_job",
                &body,
                Some(&sign(SECRET, &body)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        assert_eq!(
            provisioned.lock().unwrap().len(),
            1,
            "exactly one runner box must be provisioned for the queued job"
        );
        // The job is tracked with a real (non-empty) lease id.
        let tracked = state.jobs.lock().unwrap().map.get(&42).cloned();
        assert!(
            matches!(tracked, Some(ref l) if !l.is_empty()),
            "the job must be tracked with its lease id, got {tracked:?}"
        );
    }

    /// A queued job whose labels do NOT intersect the managed set is ignored
    /// (no provision) — e.g. the persistent builder's job, or ubuntu-latest.
    #[tokio::test]
    async fn queued_unmanaged_label_is_ignored() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        for labels in [&["corelink-builder"][..], &["ubuntu-latest"][..]] {
            let body = queued_body(7, labels, "humangr-labs", "corelink-runners");
            let resp = router(state.clone())
                .oneshot(webhook_request(
                    "workflow_job",
                    &body,
                    Some(&sign(SECRET, &body)),
                ))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "ignored jobs still ack 200");
        }
        assert!(
            provisioned.lock().unwrap().is_empty(),
            "no box may be provisioned for an unmanaged label"
        );
    }

    /// A redelivered `queued` for the SAME job id provisions only once (dedup).
    #[tokio::test]
    async fn redelivered_queued_provisions_once() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = queued_body(
            99,
            &["corelink-dogfood"],
            "humangr-labs",
            "corelink-runners",
        );
        let sig = sign(SECRET, &body);

        for _ in 0..3 {
            let resp = router(state.clone())
                .oneshot(webhook_request("workflow_job", &body, Some(&sig)))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
        assert_eq!(
            provisioned.lock().unwrap().len(),
            1,
            "three deliveries of one queued job must provision exactly one box"
        );
    }

    /// The repo allowlist rejects a job for a repo not on the list (no provision).
    #[tokio::test]
    async fn repo_allowlist_rejects_foreign_repo() {
        let allow = Some(vec![("humangr-labs".into(), "corelink-runners".into())]);
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], allow);
        let body = queued_body(5, &["corelink-dogfood"], "someone-else", "their-repo");
        let resp = router(state)
            .oneshot(webhook_request(
                "workflow_job",
                &body,
                Some(&sign(SECRET, &body)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            provisioned.lock().unwrap().is_empty(),
            "a repo not in the allowlist must not be provisioned"
        );
    }

    // ── completed → cancel ────────────────────────────────────────────────────

    /// queued provisions, then completed cancels the SAME lease (box torn down,
    /// job no longer tracked).
    #[tokio::test]
    async fn completed_cancels_the_jobs_lease() {
        let (state, provisioned, torn_down) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);

        let qbody = queued_body(
            123,
            &["corelink-dogfood"],
            "humangr-labs",
            "corelink-runners",
        );
        let r = router(state.clone())
            .oneshot(webhook_request(
                "workflow_job",
                &qbody,
                Some(&sign(SECRET, &qbody)),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let lease_id = state.jobs.lock().unwrap().map.get(&123).cloned().unwrap();
        assert!(!lease_id.is_empty());
        assert_eq!(provisioned.lock().unwrap().len(), 1);

        let cbody = completed_body(123, "humangr-labs", "corelink-runners");
        let r = router(state.clone())
            .oneshot(webhook_request(
                "workflow_job",
                &cbody,
                Some(&sign(SECRET, &cbody)),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        // The lease was cancelled (box torn down) and the job untracked.
        assert!(
            torn_down.lock().unwrap().contains(&lease_id),
            "completed must tear down the job's runner box"
        );
        assert!(
            !state.jobs.lock().unwrap().map.contains_key(&123),
            "the job must be untracked after completed"
        );
    }

    /// A `completed` for an untracked job is a harmless no-op (200, no teardown).
    #[tokio::test]
    async fn completed_untracked_job_is_noop() {
        let (state, _, torn_down) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = completed_body(404, "humangr-labs", "corelink-runners");
        let resp = router(state)
            .oneshot(webhook_request(
                "workflow_job",
                &body,
                Some(&sign(SECRET, &body)),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(torn_down.lock().unwrap().is_empty());
    }

    // ── JobLeaseMap unit + env parsing ────────────────────────────────────────

    /// The tracking map is bounded: at cap, the oldest entry is evicted.
    #[test]
    fn job_lease_map_evicts_oldest_at_cap() {
        let mut m = JobLeaseMap::new(2);
        assert!(m.claim(1));
        m.record_lease(1, "lease-1".into());
        assert!(m.claim(2));
        m.record_lease(2, "lease-2".into());
        assert!(m.claim(3)); // evicts job 1
        m.record_lease(3, "lease-3".into());

        assert!(m.take(1).is_none(), "oldest job 1 was evicted");
        assert_eq!(m.take(2).as_deref(), Some("lease-2"));
        assert_eq!(m.take(3).as_deref(), Some("lease-3"));
    }

    /// claim/drop_claim round-trips; a dropped claim is re-claimable.
    #[test]
    fn job_lease_map_claim_drop_reclaim() {
        let mut m = JobLeaseMap::new(8);
        assert!(m.claim(1), "fresh claim");
        assert!(!m.claim(1), "second claim is a dedup");
        m.drop_claim(1);
        assert!(m.claim(1), "after drop the job is re-claimable");
        // An in-flight (placeholder-only) claim yields no lease on take.
        assert!(m.take(1).is_none());
    }

    /// Default-off: no secret ⇒ `None`.
    #[test]
    fn from_env_absent_secret_is_off() {
        assert!(autoscaler_config_from_env(|_| None).is_none());
    }

    /// Secret present but PAT/image missing ⇒ disabled (loud), `None`.
    #[test]
    fn from_env_partial_config_is_off() {
        let get = |k: &str| (k == env::WEBHOOK_SECRET).then(|| "s".to_string());
        assert!(autoscaler_config_from_env(get).is_none());
    }

    /// Full config parses with defaults applied and an allowlist lowercased.
    #[test]
    fn from_env_full_config_parses() {
        let get = |k: &str| match k {
            x if x == env::WEBHOOK_SECRET => Some("sekret".to_string()),
            x if x == env::PAT => Some("pat-x".to_string()),
            x if x == env::RUNNER_IMAGE => Some(PINNED.to_string()),
            x if x == env::LABELS => Some("corelink-dogfood, corelink".to_string()),
            x if x == env::REPO_ALLOWLIST => Some("HumanGR-Labs/CoreLink-Runners".to_string()),
            _ => None,
        };
        let (secret, cfg) = autoscaler_config_from_env(get).expect("full config wires");
        assert_eq!(&*secret, "sekret");
        assert_eq!(cfg.managed_labels, vec!["corelink-dogfood", "corelink"]);
        assert_eq!(cfg.tmp_root, "/tmp/runner", "tmp_root default");
        assert_eq!(cfg.expiry_ms, DEFAULT_EXPIRY_MS, "expiry default");
        assert_eq!(
            cfg.repo_allowlist,
            Some(vec![("humangr-labs".into(), "corelink-runners".into())]),
            "allowlist is lowercased owner/repo pairs"
        );
    }

    /// The config Debug never leaks the PAT.
    #[test]
    fn config_debug_redacts_pat() {
        let cfg = AutoscalerConfig {
            pat: "super-secret-pat".to_string(),
            image_digest: PINNED.to_string(),
            managed_labels: vec!["corelink".into()],
            tmp_root: "/tmp/runner".into(),
            expiry_ms: 1,
            repo_allowlist: None,
            max_tracked_jobs: 1,
        };
        let dbg = format!("{cfg:?}");
        assert!(dbg.contains("***REDACTED***"));
        assert!(
            !dbg.contains("super-secret-pat"),
            "the PAT must not leak via Debug"
        );
    }
}
