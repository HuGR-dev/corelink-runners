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
/// GitHub's unique per-delivery GUID header — the replay-guard key (audit P1-3).
const DELIVERY_HEADER: &str = "x-github-delivery";

/// The default managed label the fabric serves when `FABRIC_AUTOSCALER_LABELS`
/// is unset. Deliberately NOT `corelink-builder` (the persistent self-hosted
/// runner's label) so the autoscaler never races the always-on builder.
const DEFAULT_MANAGED_LABEL: &str = "corelink";
/// Default runner tmp root (a runner box's work dir; the value is advisory —
/// the runner agent uses its own `_work` folder, this is the lease tmp_root the
/// isolation gate validates).
const DEFAULT_TMP_ROOT: &str = "/tmp/runner";
/// Default lease TTL: 45 min. This is the LEAK-WINDOW backstop — if a `completed`
/// webhook is ever missed (e.g. the in-memory job→lease map is reset by a
/// redeploy, audit P0-1), an orphaned box/slot is reclaimed by the deadline
/// reaper after at most this long. Kept tight (operators set
/// `FABRIC_AUTOSCALER_EXPIRY_MS` to just above their CI ceiling) so a redeploy
/// during sustained CI cannot pin the tenant cap for an hour.
const DEFAULT_EXPIRY_MS: u64 = 2_700_000;
/// Default bound on the job→lease tracking map (dedup + cancel). Far above any
/// realistic in-flight CI fan-out; terminal tombstones are evicted first, and a
/// live binding is only ever evicted with a LOUD warning (audit P2-2).
const DEFAULT_MAX_TRACKED_JOBS: usize = 4096;
/// Default bound on the seen-delivery replay-guard set (audit P1-3). One entry
/// per processed `X-GitHub-Delivery` GUID; oldest evicted FIFO.
pub const DEFAULT_MAX_TRACKED_DELIVERIES: usize = 8192;
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
    /// `workflow_job.id` → tracking state. Bounded; serves dedup of redelivered
    /// `queued`, the `completed`→cancel binding, AND the completed-vs-provision
    /// race tombstone (audit P1-1/P1-2/P2-2).
    pub jobs: Arc<Mutex<JobLeaseMap>>,
    /// Bounded replay-guard over `X-GitHub-Delivery` GUIDs (audit P1-3): a
    /// captured/duplicated delivery that already verified once is dropped, so a
    /// replayed `completed` cannot tear down a live job and a replayed `queued`
    /// cannot burn slots.
    pub seen_deliveries: Arc<Mutex<SeenDeliveries>>,
}

/// Per-job tracking state — a small state machine that makes the
/// `completed`-vs-provision interleave safe (audit P1-1/P1-2).
#[derive(Debug, Clone, PartialEq, Eq)]
enum JobState {
    /// Claim placed; the provision (3-leg mint + box) is in flight, no lease id yet.
    Provisioning,
    /// A `completed` arrived WHILE provisioning — cancel the lease the moment
    /// provision hands it back (so the box never lives to its deadline).
    CancelRequested,
    /// Lease recorded, box live; a `completed` cancels it.
    Held(String),
    /// Terminal tombstone (completed / cancelled / provision-failed). Kept so a
    /// REDELIVERED or reordered `queued` for a finished job is deduped, never
    /// re-provisioned (audit P1-2).
    Done,
}

/// What [`JobLeaseMap::claim`] decided.
enum ClaimOutcome {
    /// Fresh job — caller should provision.
    Fresh,
    /// Already tracked (in-flight, held, or terminal) — caller does nothing.
    AlreadyTracked,
}

/// What [`JobLeaseMap::record_lease`] decided after a successful provision.
enum RecordOutcome {
    /// Track the live lease normally.
    Track,
    /// A `completed` raced the provision (or the claim was evicted) — the caller
    /// must cancel this just-provisioned lease NOW.
    CancelNow(String),
}

/// What [`JobLeaseMap::take`] decided on a `completed`.
enum TakeOutcome {
    /// Cancel this live lease now.
    CancelNow(String),
    /// Nothing live to cancel (in-flight → tombstoned for the provision path to
    /// cancel; or already terminal; or completed-before-queued).
    Noted,
}

/// A bounded `job_id → `[`JobState`] map with terminal-first eviction. The state
/// machine closes the completed-vs-provision race and the redelivery
/// double-provision hole the audit found (P1-1/P1-2); eviction prefers terminal
/// tombstones and only ever drops a LIVE binding with a loud warning (P2-2).
#[derive(Debug)]
pub struct JobLeaseMap {
    map: HashMap<u64, JobState>,
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

    /// Make room for one new entry: evict the OLDEST terminal (`Done`) tombstone
    /// if any exists; only if every entry is non-terminal (genuinely that many
    /// concurrent in-flight jobs) do we evict the oldest live binding — and then
    /// LOUDLY (audit P2-2: a silently-evicted live binding leaks its box).
    fn evict_one(&mut self) {
        if self.map.len() < self.cap {
            return;
        }
        // Prefer the oldest terminal tombstone.
        if let Some(pos) = self
            .order
            .iter()
            .position(|id| matches!(self.map.get(id), Some(JobState::Done)))
        {
            if let Some(id) = self.order.remove(pos) {
                self.map.remove(&id);
            }
            return;
        }
        // No tombstone to reclaim: every tracked job is in-flight/held. Evict the
        // oldest, but never silently — name the lease so ops can reconcile; the
        // deadline reaper is the backstop.
        if let Some(id) = self.order.pop_front() {
            let st = self.map.remove(&id);
            eprintln!(
                "autoscaler: job-tracking map saturated at cap {} with NO terminal entries; \
                 evicted LIVE job {id} state={st:?} — its box now relies on deadline-reaper \
                 teardown. Raise FABRIC_AUTOSCALER_MAX_TRACKED_JOBS.",
                self.cap
            );
        }
    }

    /// Claim `job_id` for provisioning iff it is not already tracked in ANY state
    /// (in-flight, held, or terminal tombstone). A tombstone deduplicates a
    /// redelivered/reordered `queued` for a finished job (audit P1-2).
    fn claim(&mut self, job_id: u64) -> ClaimOutcome {
        if self.map.contains_key(&job_id) {
            return ClaimOutcome::AlreadyTracked;
        }
        self.evict_one();
        self.order.push_back(job_id);
        self.map.insert(job_id, JobState::Provisioning);
        ClaimOutcome::Fresh
    }

    /// Record the provisioned lease. If a `completed` raced in
    /// (`CancelRequested`), or the claim was evicted mid-provision, the lease is
    /// orphaned → tell the caller to cancel it NOW (audit P1-1).
    fn record_lease(&mut self, job_id: u64, lease_id: String) -> RecordOutcome {
        match self.map.get(&job_id) {
            Some(JobState::Provisioning) => {
                self.map.insert(job_id, JobState::Held(lease_id));
                RecordOutcome::Track
            }
            Some(JobState::CancelRequested) => {
                // completed arrived during provisioning — tombstone + cancel now.
                self.map.insert(job_id, JobState::Done);
                RecordOutcome::CancelNow(lease_id)
            }
            // Evicted under saturation (no entry) — the binding is gone; cancel
            // the orphan rather than leak it to the deadline. Do NOT re-insert.
            None => RecordOutcome::CancelNow(lease_id),
            // Defensive: a Held/Done here means a double-record; keep the lease
            // tracked, do not cancel (no known live duplicate to reclaim).
            Some(_) => RecordOutcome::Track,
        }
    }

    /// A provision attempt failed: tombstone the job so a redelivery does not
    /// re-provision (GitHub never re-emits `queued` for the same job, so there is
    /// nothing to retry — and a tombstone prevents an at-least-once duplicate
    /// from spawning a second box).
    fn provision_failed(&mut self, job_id: u64) {
        if self.map.contains_key(&job_id) {
            self.map.insert(job_id, JobState::Done);
        }
    }

    /// Handle a `completed` for `job_id`.
    fn take(&mut self, job_id: u64) -> TakeOutcome {
        match self.map.get(&job_id).cloned() {
            Some(JobState::Held(lease_id)) => {
                self.map.insert(job_id, JobState::Done);
                TakeOutcome::CancelNow(lease_id)
            }
            // Provision still in flight → ask the provision path to cancel on record.
            Some(JobState::Provisioning) => {
                self.map.insert(job_id, JobState::CancelRequested);
                TakeOutcome::Noted
            }
            // Already requested / already terminal → idempotent no-op (handles a
            // replayed or duplicate `completed`).
            Some(JobState::CancelRequested) | Some(JobState::Done) => TakeOutcome::Noted,
            // completed before any queued (or after eviction): tombstone so a
            // later `queued` for this finished job is deduped, not provisioned.
            None => {
                self.evict_one();
                self.order.push_back(job_id);
                self.map.insert(job_id, JobState::Done);
                TakeOutcome::Noted
            }
        }
    }

    /// Test helper: the current state of `job_id`, if tracked.
    #[cfg(test)]
    fn state_of(&self, job_id: u64) -> Option<JobState> {
        self.map.get(&job_id).cloned()
    }
}

/// A bounded FIFO set of seen `X-GitHub-Delivery` GUIDs — the replay guard
/// (audit P1-3). A delivery whose GUID is already present is a replay/duplicate
/// and is dropped before any side effect.
#[derive(Debug)]
pub struct SeenDeliveries {
    set: std::collections::HashSet<String>,
    order: VecDeque<String>,
    cap: usize,
}

impl SeenDeliveries {
    /// A guard bounded to `cap` GUIDs (coerced to ≥ 1).
    pub fn new(cap: usize) -> Self {
        Self {
            set: std::collections::HashSet::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    /// Returns `true` if this GUID is NEW (and records it); `false` if it was
    /// already seen (a replay/duplicate to drop).
    fn check_and_record(&mut self, guid: &str) -> bool {
        if self.set.contains(guid) {
            return false;
        }
        while self.order.len() >= self.cap {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            } else {
                break;
            }
        }
        self.order.push_back(guid.to_string());
        self.set.insert(guid.to_string());
        true
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

    // Replay guard (audit P1-3): GitHub deliveries are intentionally replayable
    // and at-least-once. A GUID we have already processed is a replay/duplicate —
    // drop it (ack 200) BEFORE any provision/cancel side effect, so a captured
    // `completed` cannot tear down a live job and a captured `queued` cannot burn
    // slots. (Absent header — never in practice from GitHub — skips the guard.)
    if let Some(guid) = header_str(&headers, DELIVERY_HEADER) {
        let fresh = state
            .seen_deliveries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .check_and_record(guid);
        if !fresh {
            return ack("ignored: duplicate delivery (replay guard)");
        }
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

    // ── Label SUBSET gate (audit P0-2 — label hijack) ──
    // Serve a job ONLY if it is non-empty AND EVERY one of its labels is managed.
    // The previous intersection ("any managed label") let an attacker author
    // `runs-on: [corelink-dogfood, corelink-builder]` — the job passed the gate
    // and its FULL label set was forwarded verbatim into the JIT mint, so the
    // provisioned ephemeral runner advertised `corelink-builder` too and could be
    // assigned a privileged builder job. With a subset gate, a job carrying ANY
    // non-managed label is refused here, so the labels we forward to the mint
    // below are provably all-managed — the attacker can no longer inject a
    // foreign pool's label, and the runner we mint can only ever match the very
    // job that requested it.
    if labels.is_empty() || !labels.iter().all(|l| state.cfg.managed_labels.contains(l)) {
        return ack("ignored: job labels are not all managed");
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
    // redelivered `queued` for the same job cannot double-provision, and so a
    // tombstoned/finished job is not re-provisioned.
    match claim_job(state, job_id) {
        ClaimOutcome::AlreadyTracked => return ack("deduped: job already tracked"),
        ClaimOutcome::Fresh => {}
    }

    eprintln!("autoscaler: provisioning runner for queued job {job_id} ({owner}/{repo})");
    // `labels` is now provably all-managed — safe to forward verbatim (the runner
    // must advertise exactly the job's labels for GitHub to assign it).
    match provision_runner(state, &owner, &repo, labels).await {
        Some(lease_id) => match record_lease(state, job_id, lease_id.clone()) {
            RecordOutcome::Track => {
                eprintln!("autoscaler: job {job_id} → runner lease {lease_id}");
                ack(&format!("provisioned lease {lease_id}"))
            }
            // A `completed` raced this provision (or the claim was evicted under
            // saturation): the lease is orphaned — cancel it now, not at the
            // deadline (audit P1-1).
            RecordOutcome::CancelNow(lid) => {
                eprintln!(
                    "autoscaler: job {job_id} completed during provision → cancelling fresh lease {lid}"
                );
                cancel_lease(state, lid).await;
                ack("provisioned then immediately cancelled (completed raced provision)")
            }
        },
        None => {
            // Provision failed (over-cap, broker off, transient). Tombstone the
            // job (do NOT drop it): GitHub never re-emits `queued` for the same
            // job, so there is nothing to retry, and a tombstone stops an
            // at-least-once duplicate delivery from spawning a second box. Ack
            // 200 — a non-2xx would only make GitHub redeliver into the same door.
            provision_failed(state, job_id);
            ack("deferred: could not provision (see server log)")
        }
    }
}

/// Handle `workflow_job.completed`: cancel the lease we provisioned for it, if
/// any, so the concurrency slot + box are reclaimed immediately (rather than at
/// the lease deadline).
async fn handle_completed(state: &WebhookHandlerState, event: WorkflowJobEvent) -> Response {
    let job_id = event.workflow_job.id;
    match take_lease(state, job_id) {
        TakeOutcome::CancelNow(lease_id) => {
            eprintln!("autoscaler: job {job_id} completed → cancelling lease {lease_id}");
            cancel_lease(state, lease_id).await;
            ack("cancelled the job's lease")
        }
        // In-flight (now tombstoned cancel-requested → the provision path cancels
        // it), already terminal, or completed-before-queued. Nothing live here.
        TakeOutcome::Noted => ack("noted: no live lease to cancel for this job"),
    }
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
        toolchain_digest: None,
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

fn claim_job(state: &WebhookHandlerState, job_id: u64) -> ClaimOutcome {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .claim(job_id)
}

fn record_lease(state: &WebhookHandlerState, job_id: u64, lease_id: String) -> RecordOutcome {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .record_lease(job_id, lease_id)
}

fn provision_failed(state: &WebhookHandlerState, job_id: u64) {
    state
        .jobs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .provision_failed(job_id);
}

fn take_lease(state: &WebhookHandlerState, job_id: u64) -> TakeOutcome {
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
    /// CSV of managed labels (default `corelink`). A job is served only if ALL
    /// of its labels are in this set (subset gate — audit P0-2).
    pub const LABELS: &str = "FABRIC_AUTOSCALER_LABELS";
    /// Lease tmp root (default `/tmp/runner`).
    pub const TMP_ROOT: &str = "FABRIC_AUTOSCALER_TMP_ROOT";
    /// Lease TTL in ms (default 2_700_000 = 45 min — the orphan leak-window
    /// backstop; set just above your CI ceiling).
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
            seen_deliveries: Arc::new(Mutex::new(SeenDeliveries::new(8192))),
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
        // The job is tracked Held with a real lease id.
        let tracked = state.jobs.lock().unwrap().state_of(42);
        assert!(
            matches!(tracked, Some(JobState::Held(ref l)) if !l.is_empty()),
            "the job must be tracked Held with its lease id, got {tracked:?}"
        );
    }

    /// AUDIT P0-2 (label hijack): a job carrying ANY non-managed label — even
    /// alongside a managed one — is REFUSED, so an attacker can never inject a
    /// privileged pool's label (`corelink-builder`) into the minted runner.
    #[tokio::test]
    async fn mixed_managed_and_foreign_label_is_refused() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        // The attack: managed label present (passes an intersection gate) PLUS the
        // privileged builder label injected.
        let body = queued_body(
            7,
            &["corelink-dogfood", "corelink-builder"],
            "humangr-labs",
            "corelink-runners",
        );
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
            "a job with a non-managed label must NOT be provisioned (no label hijack)"
        );
    }

    /// A queued job whose labels are not ALL managed is ignored (no provision) —
    /// e.g. the persistent builder's job, or ubuntu-latest.
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

    /// AUDIT P1-3 (HTTP wiring): two deliveries with the SAME `X-GitHub-Delivery`
    /// GUID — a replay — provision exactly once; the second is dropped before any
    /// side effect.
    #[tokio::test]
    async fn replayed_delivery_guid_is_dropped() {
        let (state, provisioned, _) =
            webhook_state(Some(SECRET), 5, vec!["corelink-dogfood".into()], None);
        let body = queued_body(
            55,
            &["corelink-dogfood"],
            "humangr-labs",
            "corelink-runners",
        );
        let sig = sign(SECRET, &body);
        let with_guid = || {
            Request::builder()
                .method("POST")
                .uri("/webhooks/github")
                .header(header::CONTENT_TYPE, "application/json")
                .header(EVENT_HEADER, "workflow_job")
                .header(SIGNATURE_HEADER, &sig)
                .header(DELIVERY_HEADER, "delivery-guid-fixed")
                .body(Body::from(body.clone()))
                .unwrap()
        };
        for _ in 0..3 {
            let resp = router(state.clone()).oneshot(with_guid()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
        assert_eq!(
            provisioned.lock().unwrap().len(),
            1,
            "three replays of one delivery GUID must provision exactly once"
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
        let lease_id = match state.jobs.lock().unwrap().state_of(123) {
            Some(JobState::Held(l)) => l,
            other => panic!("job must be Held after queued, got {other:?}"),
        };
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

        // The lease was cancelled (box torn down) and the job is now a terminal
        // tombstone (Done) — NOT untracked — so a redelivered queued is deduped.
        assert!(
            torn_down.lock().unwrap().contains(&lease_id),
            "completed must tear down the job's runner box"
        );
        assert_eq!(
            state.jobs.lock().unwrap().state_of(123),
            Some(JobState::Done),
            "completed must tombstone the job (dedup a redelivered queued)"
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

    // ── JobLeaseMap state-machine units (audit P1-1/P1-2/P2-2) ────────────────

    fn is_fresh(o: ClaimOutcome) -> bool {
        matches!(o, ClaimOutcome::Fresh)
    }
    fn cancel_now(o: RecordOutcome) -> Option<String> {
        match o {
            RecordOutcome::CancelNow(l) => Some(l),
            RecordOutcome::Track => None,
        }
    }
    fn take_cancel(o: TakeOutcome) -> Option<String> {
        match o {
            TakeOutcome::CancelNow(l) => Some(l),
            TakeOutcome::Noted => None,
        }
    }

    /// AUDIT P1-1: a `completed` arriving WHILE the provision is in flight must
    /// not silently drop the lease — it tombstones to `CancelRequested`, and when
    /// the provision records the lease, the map says "cancel it now".
    #[test]
    fn completed_during_provision_cancels_on_record() {
        let mut m = JobLeaseMap::new(8);
        assert!(is_fresh(m.claim(1)));
        // completed races in before record_lease.
        assert!(
            take_cancel(m.take(1)).is_none(),
            "in-flight take cancels nothing yet"
        );
        assert_eq!(m.state_of(1), Some(JobState::CancelRequested));
        // provision finishes → the lease must be cancelled now, not leaked.
        assert_eq!(
            cancel_now(m.record_lease(1, "lease-1".into())).as_deref(),
            Some("lease-1"),
            "the raced lease must be cancelled on record (no leaked box)"
        );
        assert_eq!(m.state_of(1), Some(JobState::Done));
    }

    /// AUDIT P1-2: after `completed`, a redelivered/reordered `queued` for the
    /// same job is DEDUPED (tombstone), never re-provisioned.
    #[test]
    fn completed_then_requeue_is_deduped() {
        let mut m = JobLeaseMap::new(8);
        assert!(is_fresh(m.claim(1)));
        assert!(cancel_now(m.record_lease(1, "lease-1".into())).is_none());
        // completed cancels the live lease and tombstones.
        assert_eq!(take_cancel(m.take(1)).as_deref(), Some("lease-1"));
        assert_eq!(m.state_of(1), Some(JobState::Done));
        // a re-delivered queued must NOT re-provision.
        assert!(
            !is_fresh(m.claim(1)),
            "a queued for an already-completed job must be deduped, not re-provisioned"
        );
    }

    /// A double `completed` (replay/duplicate) is idempotent — the second cancels
    /// nothing.
    #[test]
    fn double_completed_is_idempotent() {
        let mut m = JobLeaseMap::new(8);
        m.claim(1);
        m.record_lease(1, "lease-1".into());
        assert_eq!(take_cancel(m.take(1)).as_deref(), Some("lease-1"));
        assert!(
            take_cancel(m.take(1)).is_none(),
            "second completed cancels nothing"
        );
    }

    /// A failed provision tombstones the job (no re-provision on a duplicate
    /// delivery).
    #[test]
    fn provision_failure_tombstones() {
        let mut m = JobLeaseMap::new(8);
        assert!(is_fresh(m.claim(1)));
        m.provision_failed(1);
        assert_eq!(m.state_of(1), Some(JobState::Done));
        assert!(
            !is_fresh(m.claim(1)),
            "a failed job is not re-provisioned by a redelivery"
        );
    }

    /// AUDIT P2-2: eviction reclaims terminal tombstones FIRST and never silently
    /// drops a live (`Held`) binding while a tombstone exists to reclaim.
    #[test]
    fn eviction_prefers_terminal_tombstones() {
        let mut m = JobLeaseMap::new(2);
        // job 1: live (Held). job 2: terminal (Done).
        m.claim(1);
        m.record_lease(1, "lease-1".into());
        m.claim(2);
        m.record_lease(2, "lease-2".into());
        take_cancel(m.take(2)); // job 2 → Done tombstone
        // Insert job 3 at cap: the Done tombstone (job 2) is evicted, the LIVE
        // job 1 survives.
        m.claim(3);
        assert!(
            matches!(m.state_of(1), Some(JobState::Held(_))),
            "the live binding must survive eviction"
        );
        assert_eq!(
            m.state_of(2),
            None,
            "the terminal tombstone is evicted first"
        );
        assert!(matches!(m.state_of(3), Some(JobState::Provisioning)));
    }

    /// A `completed` for a never-seen job tombstones it so a later `queued` for
    /// that (already-finished) job is deduped.
    #[test]
    fn completed_before_queued_tombstones() {
        let mut m = JobLeaseMap::new(8);
        assert!(take_cancel(m.take(99)).is_none());
        assert_eq!(m.state_of(99), Some(JobState::Done));
        assert!(
            !is_fresh(m.claim(99)),
            "queued after a prior completed is deduped"
        );
    }

    /// AUDIT P1-3: the delivery replay guard records a GUID once and drops a
    /// repeat.
    #[test]
    fn seen_deliveries_drops_replays() {
        let mut s = SeenDeliveries::new(4);
        assert!(s.check_and_record("guid-A"), "first sight is fresh");
        assert!(
            !s.check_and_record("guid-A"),
            "a replay of the same GUID is dropped"
        );
        assert!(s.check_and_record("guid-B"));
        // FIFO bound: overflow evicts the oldest, which then reads as fresh again
        // (acceptable — the bound is a memory cap, not a forever-set).
        for i in 0..4 {
            s.check_and_record(&format!("g{i}"));
        }
        assert!(
            s.check_and_record("guid-A"),
            "evicted-then-reseen GUID is fresh"
        );
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
