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
//! **Auth (contract §13.2 "authenticated hook point"):** both routes sit
//! behind the Bearer-PAT layer (WP-API1), and the resolved tenant must own
//! the lease — a valid PAT of another tenant gets `404 not_found`, never
//! 403 (the frozen no-existence-oracle rule). On a tenant match the handler
//! still goes through the hook's OWN credential seam
//! ([`CaptureHook::subscribe`]): the per-hook credential is registered by
//! the composition root at lease acquire, so the mechanism's gate is
//! exercised on every poll, never bypassed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::TenantId;
use corelink_fabric_api::ApiError;
use corelink_runner::envelope::{CaptureHook, TurnMeta};
use serde::Serialize;

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
}

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
        self.lock().insert(
            lease_id.into(),
            HookEntry {
                tenant,
                hook,
                credential: credential.into(),
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
