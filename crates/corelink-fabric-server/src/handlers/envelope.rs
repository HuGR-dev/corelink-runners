//! WP-ENV1 — the authenticated fabric-side transport adapter over the
//! FROZEN §13 capture mechanism (`corelink_runner::envelope::hook`).
//!
//! This module is ONLY transport. The §13.2/§13.3 semantics — bounded
//! in-memory surfaces, drain-releases, per-surface overflow flags that are
//! never silent, the bearer credential seam, no durable backend — live in
//! the mechanism (`CaptureHook`/`Subscriber`) and are consumed here, never
//! reimplemented. The adapter holds NO buffer of its own: a poll drains the
//! mechanism's bounded in-flight surface and releases the entries in the
//! same step, so there is no second queue and no durable spill on the
//! forward path (pinned by `no_durable_write_anywhere_on_forward_path`).
//!
//! **Transport shape (M1, this WP): poll-drain.** A `GET` returns the batch
//! of currently in-flight items and releases them. The contract §13.2 "as
//! they occur" guarantee lives in the mechanism's progressive forwarding
//! (events become drainable the moment they are written); a true streaming
//! transport over the same `Subscriber` seam is an M1 polish item — the
//! poll-drain batch is the smallest honest carrier until then.
//!
//! **Auth (contract §13.2 "authenticated hook point") — two DISTINCT seams,
//! by trust boundary:**
//!
//! - **POLL** (`GET` events/meta — hugit's TRUSTED subscriber) sits behind the
//!   Bearer-PAT layer (WP-API1): the resolved tenant must OWN the lease, and a
//!   valid PAT of another tenant gets `404 not_found`, never 403 (the frozen
//!   no-existence-oracle rule). On a tenant match the handler still goes
//!   through the hook's OWN credential seam ([`CaptureHook::subscribe`]): the
//!   per-hook credential is registered by the composition root at lease
//!   acquire, so the mechanism's gate is exercised on every poll, never
//!   bypassed.
//! - **INGEST** (`POST` turn-feed — the UNTRUSTED in-box agent, contract §4) is
//!   mounted OUTSIDE the Bearer-PAT layer and does NOT use the tenant PAT (the
//!   P0 fix — the box never holds a tenant credential). It authenticates with
//!   the per-lease, write-only, ingest-SCOPED capability token, which the
//!   [`ingest`] handler verifies ITSELF: recompute the expected token for
//!   `{lease_id}` from the dedicated ingest secret and CONSTANT-TIME compare it
//!   against the presented Bearer. Fail-closed — a missing OR wrong/forged/
//!   another-lease's token is `401 unauthorized`, never an accept and never an
//!   existence oracle (a wrong token for a real lease and any token for a
//!   non-lease are byte-identical 401s).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::TenantId;
use corelink_fabric_api::ApiError;
use corelink_runner::envelope::{CaptureHook, PriceCard, TranscriptEvent, TurnMeta, TurnUsage};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::error_response;

/// One registered hook: the owning tenant, the live [`CaptureHook`] handle,
/// and the hook's own subscribe credential (the injected bearer seam of the
/// mechanism — module docs).
struct HookEntry {
    /// Tenant that owns the lease (the 404 boundary).
    tenant: TenantId,
    /// Handle to the lease's capture hook (clones share the surfaces).
    hook: CaptureHook,
    /// The credential the hook's `subscribe` seam expects.
    credential: String,
    /// The fabric-side price card the close path finalizes COGS against
    /// (ENV2). Fabric configuration, never caller-supplied — a forge that
    /// could inject the price could fabricate `cost_usd_micros`.
    price: PriceCard,
}

/// The "no price on file" card: every class free, so a close without a
/// registered card derives an honest `cost_usd_micros` of 0 — a floor,
/// never a fabricated figure.
const ZERO_PRICE: PriceCard = PriceCard {
    input_per_mtok_micros: 0,
    output_per_mtok_micros: 0,
    cache_read_per_mtok_micros: 0,
    cache_write_per_mtok_micros: 0,
};

/// `lease_id` → [`CaptureHook`] handle registry — the composition root
/// registers a lease's hook at lease acquire; the envelope handlers look it
/// up per poll. Lookup is tenant-matched: a miss and a cross-tenant hit are
/// indistinguishable (`None` both ways — no existence oracle).
#[derive(Default)]
pub struct HookRegistry {
    /// Guarded map; poison-recovered like the mechanism's own lock.
    entries: Mutex<HashMap<String, HookEntry>>,
}

impl HookRegistry {
    /// Register `lease_id`'s capture hook for `tenant` (composition root,
    /// at lease acquire). `credential` is the hook's own subscribe bearer
    /// (the mechanism's injected-token seam). Re-registering a lease id
    /// replaces the entry — one lease, one hook.
    pub fn register(
        &self,
        lease_id: impl Into<String>,
        tenant: TenantId,
        hook: CaptureHook,
        credential: impl Into<String>,
    ) {
        self.register_priced(lease_id, tenant, hook, credential, ZERO_PRICE);
    }

    /// [`register`](Self::register), with the fabric-side [`PriceCard`] the
    /// ENV2 close path finalizes `cost_usd_micros` against. The plain
    /// `register` uses the zero card ("no price on file" → cost 0, an honest
    /// floor).
    pub fn register_priced(
        &self,
        lease_id: impl Into<String>,
        tenant: TenantId,
        hook: CaptureHook,
        credential: impl Into<String>,
        price: PriceCard,
    ) {
        self.lock().insert(
            lease_id.into(),
            HookEntry {
                tenant,
                hook,
                credential: credential.into(),
                price,
            },
        );
    }

    /// Drop the lease's entry (composition root, at lease release/close).
    pub fn unregister(&self, lease_id: &str) {
        self.lock().remove(lease_id);
    }

    /// Tenant-matched lookup: the hook handle + its subscribe credential,
    /// or `None` for BOTH "no such lease" and "lease owned by another
    /// tenant" — the caller cannot distinguish them (no existence oracle).
    fn lookup(&self, lease_id: &str, tenant: &TenantId) -> Option<(CaptureHook, String)> {
        let entries = self.lock();
        let entry = entries.get(lease_id)?;
        (entry.tenant == *tenant).then(|| (entry.hook.clone(), entry.credential.clone()))
    }

    /// Tenant-matched close-path lookup (ENV2): the hook handle + the
    /// fabric-side price card. Same no-existence-oracle rule as
    /// [`lookup`](Self::lookup).
    pub(crate) fn close_handle(
        &self,
        lease_id: &str,
        tenant: &TenantId,
    ) -> Option<(CaptureHook, PriceCard)> {
        let entries = self.lock();
        let entry = entries.get(lease_id)?;
        (entry.tenant == *tenant).then(|| (entry.hook.clone(), entry.price))
    }

    /// INGEST lookup by lease id alone (WP-INGEST-SCOPE): the hook handle + the
    /// fabric-side price card. The ingest path authenticates with the per-lease
    /// SCOPED ingest token (NOT a tenant PAT — the box is untrusted and never
    /// holds a tenant secret), so there is no tenant `Extension` to match here:
    /// the token IS the lease binding (it folds `lease_id` into its HMAC
    /// pre-image), and the handler verifies it BEFORE this hook is written.
    /// `None` means "no such lease / no hook" — still no existence oracle (the
    /// caller cannot tell a missing lease from a wrong token; both are rejected
    /// before any side effect).
    fn ingest_handle_scoped(&self, lease_id: &str) -> Option<(CaptureHook, PriceCard)> {
        let entries = self.lock();
        let entry = entries.get(lease_id)?;
        Some((entry.hook.clone(), entry.price))
    }

    /// Trusted (composition-root) close-path lookup by lease id alone —
    /// the abnormal path: the lifecycle sweeps already hold the ledger
    /// record, so there is no tenant boundary to re-prove here. Never
    /// reachable from a wire handler.
    pub(crate) fn close_handle_any(&self, lease_id: &str) -> Option<(CaptureHook, PriceCard)> {
        let entries = self.lock();
        let entry = entries.get(lease_id)?;
        Some((entry.hook.clone(), entry.price))
    }

    /// Lock with poison recovery (a panicking registrant must not DoS the
    /// drain path — same discipline as the mechanism's `Shared::lock`).
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HookEntry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Wire shape of an events poll: the drained raw transcript events, each
/// base64 (standard alphabet) of the byte-identical event bytes.
#[derive(Serialize)]
struct EventsBatch {
    /// Drained raw events, oldest first, base64-encoded bytes.
    events: Vec<String>,
}

/// Wire shape of a metadata poll: the drained per-turn entries.
#[derive(Serialize)]
struct MetaBatch {
    /// Drained `TurnMeta` entries, oldest first.
    meta: Vec<TurnMetaDto>,
}

/// Local wire mirror of the mechanism's [`TurnMeta`] (which deliberately
/// does not derive serde — the mechanism is transport-agnostic; the DTO
/// lives here so the mechanism is never touched).
#[derive(Serialize)]
struct TurnMetaDto {
    /// Monotone per-hook turn index (0-based).
    turn_index: u64,
    /// Milliseconds since the hook opened.
    timestamp_ms: u64,
    /// Tool name, present only for a tool-call event.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    /// Derived token total, present only when the event carried usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<u64>,
}

impl From<TurnMeta> for TurnMetaDto {
    fn from(m: TurnMeta) -> Self {
        Self {
            turn_index: m.turn_index,
            timestamp_ms: m.timestamp_ms,
            tool: m.tool,
            tokens: m.tokens,
        }
    }
}

/// `GET` [`ENVELOPE_EVENTS`](corelink_fabric_api::paths::ENVELOPE_EVENTS):
/// drain-and-release the raw-event surface as `{"events": [base64, …]}`.
pub async fn poll_events(
    Extension(registry): Extension<Arc<HookRegistry>>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
) -> Response {
    match subscribe(&registry, &lease_id, &tenant) {
        Ok(sub) => {
            // Drain what is currently in flight; each pop RELEASES the
            // entry from the mechanism's bounded surface (§13.3 in-flight
            // forwarding only — nothing is retained here or there).
            let mut events = Vec::new();
            while let Some(bytes) = sub.next_event() {
                events.push(BASE64.encode(bytes));
            }
            Json(EventsBatch { events }).into_response()
        }
        Err((err, message)) => error_response(err, message),
    }
}

/// `GET` [`ENVELOPE_META`](corelink_fabric_api::paths::ENVELOPE_META):
/// drain-and-release the per-turn metadata surface as `{"meta": [...]}`.
pub async fn poll_meta(
    Extension(registry): Extension<Arc<HookRegistry>>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
) -> Response {
    match subscribe(&registry, &lease_id, &tenant) {
        Ok(sub) => {
            let mut meta = Vec::new();
            while let Some(m) = sub.next_meta() {
                meta.push(TurnMetaDto::from(m));
            }
            Json(MetaBatch { meta }).into_response()
        }
        Err((err, message)) => error_response(err, message),
    }
}

/// Wire shape of one ingested transcript event (§13.2 turn-feed WRITE side,
/// ENV3). The in-box agent loop POSTs one of these per event, or an array /
/// NDJSON batch of them. `bytes_b64` is the raw transcript bytes (standard
/// base64); they are forwarded VERBATIM into the hook — never scrubbed,
/// inspected, or persisted (§13.3 in-flight forwarding only).
#[derive(Deserialize)]
struct IngestEvent {
    /// Event discriminator: `model_turn` | `tool_call` | `tool_result` |
    /// `prompt`.
    kind: String,
    /// Raw transcript bytes, standard-alphabet base64.
    bytes_b64: String,
    /// Tool name (required for `tool_call` / `tool_result`).
    #[serde(default)]
    tool: Option<String>,
    /// Per-turn usage (only meaningful for `model_turn`; `null`/absent = the
    /// API reported no usage — the collector accumulates nothing, never a
    /// fabricated zero).
    #[serde(default)]
    usage: Option<IngestUsage>,
    /// Model/tool busy span in ms (`model_turn` / `tool_call`); defaults to 0.
    #[serde(default)]
    busy_ms: u64,
}

/// Wire mirror of [`TurnUsage`] — the four §13.1 token classes (no `total`;
/// totals are derived at finalize, never supplied).
#[derive(Deserialize)]
struct IngestUsage {
    /// Input (non-cached) tokens.
    input: u64,
    /// Output tokens.
    output: u64,
    /// Tokens read from prompt cache.
    cache_read: u64,
    /// Tokens written to prompt cache.
    cache_write: u64,
}

/// The POST body: ONE event, or a batch (JSON array) of events. An NDJSON
/// body (one JSON object per line) is normalized to the array form by the
/// handler before deserialization, so all three carriers map here.
#[derive(Deserialize)]
#[serde(untagged)]
enum IngestBody {
    /// A single event.
    One(Box<IngestEvent>),
    /// A batch of events (JSON array).
    Many(Vec<IngestEvent>),
}

impl IngestBody {
    /// Flatten to the ordered list of events.
    fn into_events(self) -> Vec<IngestEvent> {
        match self {
            IngestBody::One(e) => vec![*e],
            IngestBody::Many(v) => v,
        }
    }
}

impl IngestEvent {
    /// Map this wire event to the mechanism's [`TranscriptEvent`], decoding
    /// `bytes_b64`. Returns `Err` on an unknown `kind`, malformed base64, or a
    /// `tool_call`/`tool_result` missing its `tool` — a malformed event is
    /// rejected (400), never silently dropped.
    fn into_transcript_event(self) -> Result<TranscriptEvent, &'static str> {
        let bytes = BASE64
            .decode(self.bytes_b64.as_bytes())
            .map_err(|_| "bytes_b64 is not valid base64")?;
        let usage = self.usage.map(|u| TurnUsage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: u.cache_write,
        });
        match self.kind.as_str() {
            "model_turn" => Ok(TranscriptEvent::ModelTurn {
                bytes,
                usage,
                busy_ms: self.busy_ms,
            }),
            "tool_call" => Ok(TranscriptEvent::ToolCall {
                tool: self.tool.ok_or("tool_call requires `tool`")?,
                bytes,
                busy_ms: self.busy_ms,
            }),
            "tool_result" => Ok(TranscriptEvent::ToolResult {
                tool: self.tool.ok_or("tool_result requires `tool`")?,
                bytes,
            }),
            "prompt" => Ok(TranscriptEvent::SystemPrompt { bytes }),
            _ => Err("unknown event kind"),
        }
    }
}

/// `POST` [`ENVELOPE_INGEST`](corelink_fabric_api::paths::ENVELOPE_INGEST):
/// the §13.2 trajectory turn-feed WRITE side (ENV3). The in-box agent loop
/// forwards its transcript events here; the handler writes each into the
/// lease's capture hook (in-flight forward ONLY — never persisted, §13.3).
///
/// ## Auth: the per-lease SCOPED ingest token — NOT the tenant PAT (P0 fix)
///
/// This route sits OUTSIDE the Bearer-PAT middleware (see [`app_full`]). The
/// box (UNTRUSTED, contract §4, open egress per ADR-0003) holds a per-lease,
/// write-only, ingest-SCOPED capability token — NEVER the tenant PAT. The
/// handler authenticates by recomputing the expected token for `{lease_id}`
/// from the dedicated ingest secret ([`crate::ingest_token::IngestSigner`]) and
/// constant-time comparing it against the presented `Authorization: Bearer`
/// token. Accept iff they match; otherwise reject fail-closed (401 absent /
/// 403 wrong). The token folds `lease_id` into its HMAC pre-image, so lease A's
/// token can never authorize ingest to lease B (cross-lease isolation is
/// intrinsic). An exfiltrated token only lets an attacker POST trajectory to
/// that one (soon-dead) lease's ingest endpoint — no tenant takeover, no other
/// capability.
///
/// The POLL endpoints (`poll_events`/`poll_meta`) are unchanged: they KEEP the
/// tenant-PAT gate (hugit's TRUSTED subscriber polls with the tenant PAT; that
/// path puts nothing on the box). Two credentials, by trust boundary.
///
/// On each ingested `model_turn` (the turn boundary), a NON-destructive
/// snapshot of the collector's current `IntentMetrics` is serialized and
/// written to the lease's durable envelope checkpoint (ADR-0004 Phase 2b) so
/// an abnormal cross-instance reap can still emit the accumulated partial. The
/// checkpoint write is BEST-EFFORT: a failure is logged, never breaks ingest.
///
/// [`app_full`]: crate::app::app_full
pub async fn ingest(
    State(state): State<AppState>,
    Extension(registry): Extension<Arc<HookRegistry>>,
    Path(lease_id): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // ── Auth: the per-lease SCOPED ingest token (NOT the tenant PAT). Extract
    // the presented Bearer, recompute the expected scoped token for {lease_id},
    // and constant-time compare. Fail-closed: a missing OR wrong/forged/another-
    // lease's token is 401 `unauthorized` — never an accept. (The frozen error
    // vocabulary has NO 403 ON PURPOSE — it would leak existence; a bad
    // credential is `Unauthorized`, the same posture the PAT layer uses.) This
    // runs BEFORE the registry lookup, so a wrong token can never probe lease
    // existence — no existence oracle: a wrong token for a real lease and any
    // token for a non-lease are byte-identical 401s.
    let Some(presented) = bearer(&headers) else {
        return error_response(ApiError::Unauthorized, "missing ingest credential");
    };
    if !state
        .ingest_signer
        .verify_ingest_token(&lease_id, presented)
    {
        return error_response(
            ApiError::Unauthorized,
            "ingest credential does not authorize this lease",
        );
    }

    // ── Scope: lease-id hook lookup (the token already proved the lease
    // binding). A miss means the lease has no live hook (closed/reaped/never
    // registered) — fail-closed.
    let Some((hook, price)) = registry.ingest_handle_scoped(&lease_id) else {
        return error_response(ApiError::NotFound, "lease not found");
    };

    // ── Parse: accept ONE object, a JSON array, OR an NDJSON body (one JSON
    // object per line). NDJSON is normalized to the array form first.
    let parsed: Result<IngestBody, _> = serde_json::from_str(&body);
    let events = match parsed {
        Ok(b) => b.into_events(),
        Err(_) => match parse_ndjson(&body) {
            Ok(evs) => evs,
            Err(msg) => return error_response(ApiError::Invalid, msg),
        },
    };
    if events.is_empty() {
        return error_response(ApiError::Invalid, "no events in ingest body");
    }

    // ── Forward each event into the hook, in order. A model_turn additionally
    // triggers the durable checkpoint write (Phase 2b).
    let mut wrote_turn = false;
    for ev in events {
        let te = match ev.into_transcript_event() {
            Ok(te) => te,
            Err(msg) => return error_response(ApiError::Invalid, msg),
        };
        let is_turn = matches!(te, TranscriptEvent::ModelTurn { .. });
        if let Err(e) = hook.write(te) {
            // The hook refused (closed / not-yet-open): the lease is no longer
            // accepting events. Fail-closed rather than silently dropping.
            return error_response(
                ApiError::FailClosed,
                &format!("capture hook refused the event: {e:#}"),
            );
        }
        wrote_turn |= is_turn;
    }

    // ── Phase 2b — per-turn durable checkpoint (ADR-0004 Decision-3a). After
    // the writes, if any model_turn landed, project the collector's CURRENT
    // accumulated metrics (NON-destructive snapshot — the finalize latch is
    // untouched) and persist them to the lease row. Best-effort: a failure is
    // logged, never breaks ingest.
    if wrote_turn {
        checkpoint_turn(&state, &lease_id, &hook, &price);
    }

    StatusCode::OK.into_response()
}

/// Extract the presented token from `Authorization: Bearer <token>`; `None` on
/// a missing header, non-UTF-8 value, wrong scheme, or empty token. (Same
/// extraction shape as the PAT layer's `auth::bearer_token`, but the ingest
/// path's Bearer is the per-lease SCOPED token, never a tenant PAT.)
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    (!token.is_empty()).then_some(token)
}

/// Parse an NDJSON body (one JSON [`IngestEvent`] per non-blank line) to an
/// ordered event list. Used only when the body is neither a single JSON object
/// nor a JSON array.
fn parse_ndjson(body: &str) -> Result<Vec<IngestEvent>, &'static str> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let ev: IngestEvent =
            serde_json::from_str(line).map_err(|_| "malformed NDJSON event line")?;
        out.push(ev);
    }
    if out.is_empty() {
        return Err("ingest body is not a valid event, array, or NDJSON batch");
    }
    Ok(out)
}

/// Write the per-turn durable envelope checkpoint (ADR-0004 Phase 2b).
///
/// Takes a NON-destructive snapshot of the hook's collector (`now` = the
/// fabric clock), serializes the resulting frozen `IntentMetrics` to JSON, and
/// stores it on the lease row via `LeaseLedger::set_envelope_checkpoint`. The
/// blob shape is EXACTLY what the reaper's tier-2 read deserializes back
/// (`IntentMetrics`), so the cross-instance abnormal flush emits the last
/// checkpointed partial. BEST-EFFORT: every failure path here only logs.
fn checkpoint_turn(state: &AppState, lease_id: &str, hook: &CaptureHook, price: &PriceCard) {
    let metrics = hook.snapshot_metrics(std::time::Instant::now(), price);
    let json = match serde_json::to_string(&metrics) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("envelope-ingest: lease {lease_id} checkpoint serialize failed: {e:#}");
            return;
        }
    };
    if let Err(e) = state.ledger.set_envelope_checkpoint(lease_id, &json) {
        // A checkpoint write failure (e.g. the lease already terminalized) must
        // NOT break the in-flight forward — it only costs a forensic refresh.
        eprintln!("envelope-ingest: lease {lease_id} checkpoint write failed: {e:#}");
    }
}

/// Shared gate for both polls: tenant-matched registry lookup (miss and
/// cross-tenant are the SAME 404 — no existence oracle), then the hook's
/// own credential seam. A subscribe refusal on a registered hook means the
/// registry and the hook disagree about the credential — an internal
/// inconsistency, answered fail-closed (503), never open.
fn subscribe(
    registry: &HookRegistry,
    lease_id: &str,
    tenant: &TenantId,
) -> Result<corelink_runner::envelope::Subscriber, (ApiError, &'static str)> {
    let Some((hook, credential)) = registry.lookup(lease_id, tenant) else {
        return Err((ApiError::NotFound, "lease not found"));
    };
    hook.subscribe(&credential).map_err(|_| {
        (
            ApiError::FailClosed,
            "envelope hook refused the registered credential; failing closed",
        )
    })
}
