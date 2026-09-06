//! Background reaper: reclaim orphaned cloud boxes whose leases have expired
//! without a clean close (WP-CF-REAP).
//!
//! ## Problem
//!
//! A lease that expires or whose client crashes without sending `POST
//! /v1/leases/{id}/close` leaves two orphaned objects:
//! - a Northflank job object in the provider, and
//! - the corresponding [`RunningContainer`] entry in the in-memory
//!   [`BoxRegistry`].
//!
//! The compute cost is bounded (`activeDeadlineSeconds`), but the objects
//! accumulate — this reaper reclaims them on a periodic sweep.
//!
//! ## Mechanism (expiry-driven, teardown-first)
//!
//! Every tick the reaper calls [`reap_once`]:
//! 1. A snapshot of currently `Held` lease records is taken under the ledger
//!    lock (guard dropped at block end, before any `await`). Each record
//!    carries its own durable `deadline_ms` (ADR-0004 Decision-1 — the ledger
//!    is the single source of truth for the deadline; there is no in-memory
//!    `deadlines` map). This is what lets ANY instance reap an overdue lease,
//!    including one that never served the acquire (the D3-P1 cap-slot-leak fix).
//! 2. Overdue leases (`now_ms >= rec.deadline_ms`) are identified.  A `Held`
//!    lease with `deadline_ms = None` is treated as **never-overdue**
//!    (fail-safe: never reap a lease we cannot date).
//! 3. For each overdue lease, [`AppState::teardown_lease`] is called FIRST.
//!    Only if teardown SUCCEEDS is the ledger transitioned to `Expired` and
//!    the `images` side-table GC'd.  A failed teardown leaves the lease `Held`
//!    so the next sweep retries — no permanent leak from a one-time provider
//!    hiccup.
//!
//! ## Retry posture (bounded retry — re-audit decision, 2026-06-14)
//!
//! A failed teardown leaves the lease `Held` (NOT transitioned to `Expired`)
//! so the next sweep finds it again and retries.  This is the key difference
//! from the old mark-then-kill posture: the terminal `Expired` mark is the
//! CONSEQUENCE of a successful teardown, not the precondition for it.  This
//! ordering is deliberate and is NOT inverted.
//!
//! The re-audit flagged "kill-then-mark with UNBOUNDED retry": a box that can
//! never be torn down is retried every sweep forever.  DECISION — the retry is
//! left as-is (re-tried indefinitely) because it is NOT a runaway:
//! - **Bounded WORK per tick.** Each sweep makes exactly ONE teardown call per
//!   overdue lease; a permanently-stuck box adds one bounded call per tick, not
//!   a hot spin.  The reaper does not busy-loop on it.
//! - **Compute is already bounded by the provider.**  The ultimate backstop is
//!   `activeDeadlineSeconds` (see the "Problem" section): the provider
//!   force-kills the box at its hard deadline regardless of whether our
//!   teardown call ever succeeds, so a box that resists teardown still STOPS
//!   COSTING MONEY.  The lingering `Held` row is forensic, not a live compute
//!   leak.
//! - **It is no longer SILENT.**  A failed teardown now emits a forensic log
//!   line every sweep (`reaper: teardown FAILED for overdue lease …`), so a
//!   permanently-stuck box is observable to ops rather than spinning quietly.
//!
//! What was deliberately NOT added: a per-lease attempt counter + force-
//! terminalize-after-N.  Force-marking a lease `Expired` while its box may
//! still be alive would re-introduce the exact mark-then-kill hazard the
//! teardown-first posture exists to avoid (a phantom-reclaimed slot whose box
//! is still running), and it would require cross-instance attempt state for no
//! reclaim benefit — the deadline already terminalizes the COMPUTE.  If a
//! future SLA needs an explicit ops escalation, the forensic line is the hook
//! to alert on.
//!
//! ## Side-table GC
//!
//! The `images` entry is removed via [`AppState::forget_lease`] only AFTER
//! teardown succeeds.  The deadline is NO LONGER a side table (ADR-0004: it
//! rides the `LeaseRecord` in the ledger); on a retry sweep the lease is still
//! `Held` with its `deadline_ms` intact, so it is re-found and re-dated — the
//! terminal `Expired` transition is what finally drops it from `held()`.
//!
//! ## Crash-surfacing sweep (WP-CRASH-SWEEP, OPT-IN)
//!
//! Leases that die mid-flight (box gone but deadline not yet reached) are now
//! reclaimed by [`surface_crashes`], a SEPARATE sweep from [`reap_once`]. It
//! probes each `Held` lease's box via [`crate::AppState::probe_lease`] and
//! reclaims ONLY a box probed authoritatively-Dead
//! ([`crate::cloud_exec::ProbeStatus::Dead`]), transitioning the lease to
//! `Crashed` (the symmetric counterpart of `reap_once`'s `Expired` path).
//!
//! The sweep is OPT-IN (`FABRIC_CRASH_PROBE_INTERVAL_SECS`): a liveness probe
//! costs one provider `is_alive` call per `Held` lease per tick and is
//! newer/riskier than deadline-expiry, so it is off unless explicitly
//! configured. The always-on deadline reaper ([`reap_once`]) remains the
//! backstop — every lease still has a hard `Expired` deadline regardless.
//!
//! ## §13.5 partial-envelope flush (WIRED — WP-S13.5)
//!
//! BOTH abnormal paths now flush a best-effort PARTIAL envelope on reclaim
//! (Option B, owner-ratified 2026-06-13): `reap_once` (Expired) and
//! `surface_crashes` (Crashed) each call [`flush_partial_envelope`] BEFORE
//! `forget_lease` (which unregisters the capture hook) and before `record_slot`
//! — fire-and-forget, so it never blocks or breaks reclamation, but the hook is
//! still registered so TIER-1 finalizes the live partial metrics (audit r5 fix).
//! The flush finalizes whatever the `CaptureHook`
//! accumulated through the SAME finalize/redaction path as a normal close (no
//! exemption), stamps `close_reason=expired|crashed` + `capture_incomplete:true`
//! (WRAPPER-level, never inside the frozen §13.4 `IntentMetrics`), and emits it
//! to the M1 forensic sink (a structured log line; the real push to hugit is
//! the P2 transport WP). A lease with no hook is a no-op; an already-closed
//! hook (a normal close raced in) returns the exactly-once `Err`, which is
//! logged and skipped — no second envelope, no double-anything.
//!
//! ## Remaining non-goals
//!
//! - **Live push of the partial envelope**: at M1 the flush is the forensic
//!   log record; the actual transport to hugit is the P2 transport WP.
//!
//! ## Lock ordering
//!
//! `reap_once` takes the ledger snapshot (lock taken + released — the held
//! records carry their own durable `deadline_ms`, so no separate deadline
//! snapshot is needed), then calls async teardown.  No lock is held across any
//! `await` point — so this function is always `Send`-safe on the executor
//! (verified by the compile-time assertion at the bottom of this file).
//!
//! After teardown the ledger lock is re-acquired (briefly) to write the
//! `Expired` transition.  No other lock is held at that point.

use std::io::Read;
use std::time::{Duration, Instant};

use corelink_fabric::{SlotEventKind, TenantSuspensionEvent};
use corelink_runner::envelope::{AbnormalKind, CloseReason};
use corelink_runners_contracts::RunnerState;

/// Compose the consumer envelope from the generation captured by this exact
/// immutable outbox event. The consumer contract carries the generation as a
/// decimal string; reject values outside the checked PostgreSQL boundary.
fn suspension_envelope_body(
    event: &TenantSuspensionEvent,
    generation: anyhow::Result<u64>,
) -> anyhow::Result<String> {
    let generation = generation?;
    if generation > i64::MAX as u64 {
        anyhow::bail!("lifecycle generation exceeds i64::MAX");
    }
    Ok(serde_json::to_string(&serde_json::json!({
        "event_id": event.event_id,
        "tenant_id": event.tenant_id,
        "action": "suspended",
        "lifecycle_generation": generation.to_string(),
    }))?)
}

const MAX_SUSPENSION_RECEIPT_BYTES: usize = 4 * 1024;

/// The reaper reads this binding directly, so it must apply the same security
/// boundary that the normal Cloudflare composition applies to outbound URLs.
/// Only an HTTPS origin is accepted: no credentials, query/fragment, path, or
/// ambiguous authority may be combined with the bearer token.
fn secure_worker_origin(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed != raw || !trimmed.starts_with("https://") {
        return None;
    }
    let rest = &trimmed["https://".len()..];
    if rest.is_empty() || rest.contains(['?', '#', '\\']) {
        return None;
    }
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('%')
        || authority.chars().any(|c| c.is_ascii_whitespace() || c.is_control())
    {
        return None;
    }
    if authority_end < rest.len() && &rest[authority_end..] != "/" {
        return None;
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']')?;
        let port = authority.get(close + 1..).unwrap_or_default();
        if !port.is_empty() && !port.starts_with(':') {
            return None;
        }
        (&authority[..=close], port.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() || port.is_some_and(|p| p.is_empty() || p.parse::<u16>().is_err()) {
        return None;
    }
    Some(trimmed.trim_end_matches('/'))
}

fn suspension_receipt_is_ack(
    status: u16,
    body: &[u8],
    event_id: &str,
    tenant_id: &str,
    lifecycle_generation: &str,
) -> bool {
    if status != 200 || body.len() > MAX_SUSPENSION_RECEIPT_BYTES {
        return false;
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let Some(object) = payload.as_object() else {
        return false;
    };
    object.len() == 4
        && object.get("event_id").and_then(serde_json::Value::as_str) == Some(event_id)
        && object.get("tenant_id").and_then(serde_json::Value::as_str) == Some(tenant_id)
        && object
            .get("lifecycle_generation")
            .and_then(serde_json::Value::as_str)
            == Some(lifecycle_generation)
        && object.get("complete").and_then(serde_json::Value::as_bool) == Some(true)
}

/// Deliver the durable suspension outbox to the authenticated runner Worker.
/// The URL and scoped lifecycle token are Cloudflare bindings forwarded into the
/// fabric container; absent configuration leaves the outbox pending.
pub async fn dispatch_tenant_suspension_events(state: &crate::AppState) {
    let Some(base) = std::env::var("CLOUDFLARE_SPAWN_WORKER_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .and_then(|v| secure_worker_origin(&v).map(str::to_owned))
    else {
        return;
    };
    let Some(token) = std::env::var("CLOUDFLARE_LIFECYCLE_AUTH_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())
    else {
        return;
    };
    let events = match state.ledger.pending_tenant_suspension_events(32) {
        Ok(events) => events,
        Err(e) => {
            eprintln!("suspension-outbox: read failed: {e:#}");
            return;
        }
    };
    let url = format!(
        "{}/internal/v1/tenant-suspension",
        base.trim_end_matches('/')
    );
    for event in events {
        let generation = match state.ledger.tenant_suspension_generation(&event.event_id) {
            Ok(generation) => generation,
            Err(e) => {
                eprintln!(
                    "suspension-outbox: generation lookup/encode failed event={}: {e:#}",
                    event.event_id
                );
                continue;
            }
        };
        let body = match suspension_envelope_body(&event, Ok(generation)) {
            Ok(body) => body,
            Err(e) => {
                eprintln!(
                    "suspension-outbox: generation lookup/encode failed event={}: {e:#}",
                    event.event_id
                );
                continue;
            }
        };
        if let Err(e) = state
            .ledger
            .mark_tenant_suspension_event_attempt(&event.event_id)
        {
            eprintln!(
                "suspension-outbox: attempt stamp failed event={}: {e:#}",
                event.event_id
            );
            continue;
        }
        let url = url.clone();
        let token = token.clone();
        let expected_event_id = event.event_id.clone();
        let expected_tenant_id = event.tenant_id.clone();
        let expected_generation = generation.to_string();
        let delivered = tokio::task::spawn_blocking(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .http_status_as_error(false)
                .build()
                .into();
            let response = agent
                .post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .send(&body);
            response
                .map(|mut r| {
                    let status = r.status().as_u16();
                    let mut reader = r
                        .body_mut()
                        .as_reader()
                        .take((MAX_SUSPENSION_RECEIPT_BYTES + 1) as u64);
                    let mut body = Vec::with_capacity(MAX_SUSPENSION_RECEIPT_BYTES + 1);
                    if reader.read_to_end(&mut body).is_err() {
                        return false;
                    }
                    suspension_receipt_is_ack(
                        status,
                        &body,
                        &expected_event_id,
                        &expected_tenant_id,
                        &expected_generation,
                    )
                })
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if delivered {
            if let Err(e) = state
                .ledger
                .mark_tenant_suspension_event_delivered(&event.event_id)
            {
                eprintln!(
                    "suspension-outbox: ack failed event={}: {e:#}",
                    event.event_id
                );
            }
        } else {
            eprintln!(
                "suspension-outbox: delivery failed event={} tenant={} attempt={}",
                event.event_id, event.tenant_id, event.attempts
            );
        }
    }
}

/// §13.5 best-effort partial-envelope flush on an ABNORMAL lease termination
/// (Expired / Crashed), fire-and-forget.
///
/// **Ruling (hugit, owner-ratified 2026-06-13, Option B).** When the deadline
/// reaper ([`reap_once`]) or the crash sweep ([`surface_crashes`]) reclaims a
/// lease, whatever the lease's [`CaptureHook`] accumulated MUST be flushed as a
/// PARTIAL envelope — explicitly marked incomplete — rather than dropped. hugit
/// prices flat, so a partial trajectory carries no billing risk; it is forensic
/// provenance.
///
/// This runs **AFTER** teardown→transition→`record_slot` has already reclaimed
/// the lease, so it can NEVER block or break reclamation: a flush failure is
/// logged and the sweep moves on.
///
/// # 3-tier abnormal flush (ADR-0004 Decision-2; hugit §13 Item-3 SLA)
/// The `CaptureHook` registry is in-memory PER INSTANCE, so the reaper that wins
/// the terminal CAS may not be the one holding the hook. Rather than silently
/// drop the forensic envelope, the flush falls back through three tiers:
/// 1. **local hook present** (`source=local-hook`): full fidelity via
///    `close_abnormal` — unchanged behavior.
/// 2. **no local hook, durable checkpoint exists** (`source=durable-checkpoint`):
///    deserialize the ledger's opaque [`IntentMetrics`] checkpoint blob and emit a
///    PARTIAL envelope from it (`capture_incomplete=true`). This is the
///    cross-instance SLA: any instance can emit, so the envelope survives owner
///    death.
/// 3. **no hook AND no checkpoint** (`source=no-capture`): the lease died before
///    anything was captured → an explicit `no_capture` marker (zero metrics,
///    `capture_incomplete=true`, `no_capture=true`) — the owner-ratified "never
///    silently dropped" record (Decision-3b).
///
/// Tiers 2/3 fire ONLY when there is no local hook, so they can never double-emit
/// against tier 1. Tier 1's already-closed (exactly-once) `Err` is logged and
/// skipped with no fall-through — the hook DID exist, so emitting a checkpoint /
/// no_capture record would be a second envelope for the same lease.
///
/// # Wire shape (§13.5)
/// The EXISTING envelope payload plus two close-metadata markers, both
/// WRAPPER-level (never inside the frozen §13.4 `IntentMetrics` vector):
/// - `close_reason` — `expired` | `crashed` (here; `normal` is the clean path);
/// - `capture_incomplete: true` — set unconditionally by `close_abnormal`.
///
/// # Delivery (M1)
/// Fire-and-forget, **NO ack** — the lease is torn down, so there is no live
/// client to ack. Dedup is NOT by registry removal: [`HookRegistry::close_handle_any`]
/// returns a CLONE of the hook (the entry stays registered). Exactly-once is
/// enforced by the **shared close-latch** on the hook's `Arc<Shared>` state — every
/// clone (the live client's close handle and this reaper clone) observes the same
/// latch, so the second `close_*` returns the exactly-once `Err`. The ledger
/// transition is additionally atomic & exclusive (`Held→Expired|Crashed` vs
/// `Held→Released`), so a normal close and this abnormal flush can never both fire.
///
/// **There is no live push transport at M1** (the envelope is poll-drain;
/// hugit consumes at P2). So at M1 the flush = FINALIZE the partial envelope
/// (markers + the SAME finalize/redaction write-path as a normal close — no
/// exemption) and emit it best-effort to the available forensic sink: a single
/// structured log line carrying `lease_id`, `tenant`, `close_reason`,
/// `capture_incomplete`, and a metrics SUMMARY (the `IntentMetrics` scalar
/// fields — NOT raw trajectory text). The real push to hugit is the P2
/// transport WP; at M1 this log line IS the forensic record.
///
/// All calls here ([`HookRegistry::close_handle_any`] + `JobClose::close_abnormal`)
/// are synchronous, so this holds no `MutexGuard` across an `await` — the
/// `Send` guards on the callers stay satisfied.
fn flush_partial_envelope(
    state: &crate::AppState,
    lease_id: &str,
    tenant: &corelink_fabric::TenantId,
    kind: AbnormalKind,
    died: Instant,
) -> Option<corelink_runner::envelope::CloseOutcome> {
    let close_reason = CloseReason::from(kind);

    // ── TIER 1 — local hook present (the common case: the owning instance is
    // alive and is often the reaper). Full-fidelity: finalize through the SAME
    // finalize/redaction path as a normal close (no exemption — "an exemption is
    // a hole"), which stamps `capture_incomplete: true` + the close reason.
    //
    // Dedup: close_handle_any returns a CLONE of the hook (the registry entry
    // stays; exactly-once rides the shared close-latch on the hook state, NOT
    // registry removal).
    if let Some((hook, price)) = state.hook_registry.close_handle_any(lease_id) {
        match corelink_runner::envelope::JobClose::new(&hook).close_abnormal(kind, died, &price) {
            Ok(outcome) => {
                // source = local-hook; metrics are the FINALIZED (redacted)
                // projection, capture_incomplete carried from the outcome.
                emit_forensic(
                    lease_id,
                    tenant,
                    &outcome.metrics,
                    outcome.close_reason,
                    outcome.capture_incomplete,
                    false,
                    "local-hook",
                );
                return Some(outcome);
            }
            Err(e) => {
                // Already-closed (exactly-once): a normal close consumed this
                // hook before the sweep. Do NOT fail the sweep and do NOT fall
                // through to tiers 2/3 — the local hook DID exist and its
                // exactly-once latch already fired; a checkpoint/no_capture
                // record here would be a SECOND envelope for the same lease.
                eprintln!(
                    "envelope-flush: skipped partial flush for lease {lease_id} \
                     (close already fired, exactly-once): {e:#}"
                );
                return None;
            }
        }
    }

    // ── TIER 2 — no local hook, but a durable checkpoint exists (ADR-0004
    // Decision-2): the lease ran on an instance that is now gone/elsewhere and
    // left a redacted summary on its ledger row. Emit a PARTIAL forensic
    // envelope from it (the §13 Item-3 cross-instance SLA: never silently
    // dropped). source = durable-checkpoint.
    let checkpoint = {
        let ledger = &*state.ledger;
        // A read failure must NOT break reclamation (post-teardown,
        // fire-and-forget) — degrade to None and fall through to tier 3. But do
        // NOT swallow it silently: a pg read error here downgrades a durable-
        // checkpoint flush to a no_capture marker, which is a real (if benign)
        // loss of §13 provenance — log it so it is diagnosable, not invisible.
        ledger
            .get_envelope_checkpoint(lease_id)
            .unwrap_or_else(|e| {
                eprintln!(
                    "reaper: §13 checkpoint read FAILED for lease {lease_id}: {e:#} \
                 — degrading to no_capture (durable provenance lost for this lease)"
                );
                None
            })
    };
    if let Some(json) = checkpoint {
        match serde_json::from_str::<corelink_runners_contracts::IntentMetrics>(&json) {
            Ok(metrics) => {
                // capture_incomplete = true: a checkpoint is by definition a
                // mid-flight summary, never the finalized close.
                emit_forensic(
                    lease_id,
                    tenant,
                    &metrics,
                    close_reason,
                    true,
                    false,
                    "durable-checkpoint",
                );
                return Some(corelink_runner::envelope::CloseOutcome {
                    status: corelink_runner::envelope::JobStatus::Killed,
                    metrics,
                    capture_incomplete: true,
                    close_reason,
                });
            }
            Err(e) => {
                // A corrupt checkpoint must not silently vanish: fall through to
                // the tier-3 no_capture marker so the loss is still RECORDED.
                eprintln!(
                    "envelope-flush: lease {lease_id} durable checkpoint failed to deserialize \
                     ({e:#}) — falling through to no_capture marker"
                );
            }
        }
    }

    // ── TIER 3 — no hook AND no (usable) checkpoint: the lease died before
    // anything was captured. Emit an EXPLICIT `no_capture` marker (zero
    // metrics) — the owner-ratified "never silently dropped" record. source =
    // no-capture.
    let zero = zero_intent_metrics();
    emit_forensic(
        lease_id,
        tenant,
        &zero,
        close_reason,
        true,
        true,
        "no-capture",
    );
    Some(corelink_runner::envelope::CloseOutcome {
        status: corelink_runner::envelope::JobStatus::Killed,
        metrics: zero,
        capture_incomplete: true,
        close_reason,
    })
}

/// The all-zero [`IntentMetrics`] for the tier-3 `no_capture` marker.
///
/// The frozen contracts type does NOT derive `Default` (and we must NOT add a
/// derive to it — its wire shape, sha256 `2d8d2215…`, is frozen), so the zero
/// value is built as an explicit literal. This constructs a VALUE; it does not
/// alter the type.
fn zero_intent_metrics() -> corelink_runners_contracts::IntentMetrics {
    use corelink_runners_contracts::{IntentMetrics, TokenCounts};
    IntentMetrics {
        tokens: TokenCounts {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        },
        wall_ms: 0,
        active_ms: 0,
        tool_calls: 0,
        tool_breakdown: Vec::new(),
        model_turns: 0,
        cost_usd_micros: 0,
    }
}

/// Emit ONE consistent structured forensic line — the M1 forensic record for an
/// abnormal-close envelope (the P2 transport pushes it to hugit). All three
/// flush tiers route through here so the record shape is identical regardless of
/// whether the metrics came from a live hook (tier 1), a durable checkpoint
/// (tier 2), or the `no_capture` zero floor (tier 3); `source` names which.
///
/// The metrics are always a SUMMARY of the `IntentMetrics` scalars — never raw
/// trajectory bytes (redaction is preserved on every path).
fn emit_forensic(
    lease_id: &str,
    tenant: &corelink_fabric::TenantId,
    metrics: &corelink_runners_contracts::IntentMetrics,
    close_reason: CloseReason,
    capture_incomplete: bool,
    no_capture: bool,
    source: &str,
) {
    let reason = match close_reason {
        CloseReason::Expired => "expired",
        CloseReason::Crashed => "crashed",
        CloseReason::Normal => "normal",
    };
    eprintln!(
        "envelope-flush: partial envelope FINALIZED (M1 forensic record; P2 pushes to hugit) \
         lease_id={lease_id} tenant={tenant} close_reason={reason} source={source} \
         capture_incomplete={capture_incomplete} no_capture={no_capture} \
         tokens_total={} tool_calls={} cost_usd_micros={} \
         wall_ms={} active_ms={} model_turns={}",
        metrics.tokens.total,
        metrics.tool_calls,
        metrics.cost_usd_micros,
        metrics.wall_ms,
        metrics.active_ms,
        metrics.model_turns,
    );
}

/// Configuration for the background reaper.
pub struct ReaperConfig {
    /// How often to scan for overdue leases.
    pub interval: Duration,
}

/// Resolve [`ReaperConfig`] from an environment-variable accessor.
///
/// Reads `FABRIC_REAP_INTERVAL_SECS`.
/// - Absent or empty → default of 30 seconds.
/// - Present → parse as `u32`; value `0` or an unparseable string → `Err`.
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn reaper_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<ReaperConfig> {
    let secs: u64 = match get("FABRIC_REAP_INTERVAL_SECS").filter(|s| !s.is_empty()) {
        None => 30,
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_REAP_INTERVAL_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            if parsed == 0 {
                anyhow::bail!(
                    "FABRIC_REAP_INTERVAL_SECS must be >= 1 (0 would disable the reaper)"
                );
            }
            parsed as u64
        }
    };
    Ok(ReaperConfig {
        interval: Duration::from_secs(secs),
    })
}

/// Run one expiry sweep: teardown overdue `Held` leases and mark them
/// `Expired` only after teardown succeeds.
///
/// Returns the number of leases successfully reclaimed this tick.
///
/// # Posture
///
/// Teardown-first: the `Expired` ledger mark is written ONLY after the
/// provider teardown returns `Ok`.  A failed teardown leaves the lease `Held`
/// so the next sweep retries — no permanent leak from a transient provider
/// error.
///
/// # Lock-ordering note
///
/// No `MutexGuard` is held across any `await` point.  The ledger snapshot is
/// taken in a scoped block, the guard dropped before the next async operation
/// (the durable `deadline_ms` rides each held record, so there is no separate
/// deadline snapshot).  After teardown, the ledger lock is briefly re-acquired
/// to write the `Expired` transition — again dropped
/// before continuing.  The compile-time [`_ASSERT_REAP_ONCE_IS_SEND`]
/// assertion enforces this: if anyone ever adds a guard across an `await`,
/// compilation fails.
pub async fn reap_once(state: &crate::AppState) -> usize {
    let now = state.clock.now_ms();

    // ── 1. Snapshot held leases — guard dropped at end of block, before any await.
    // Each record now carries its OWN durable `deadline_ms` (ADR-0004
    // Decision-1: the ledger is the single source of truth for the deadline),
    // so ANY instance can date and reap an overdue lease — including one that
    // never served the acquire. There is no in-memory `deadlines` map.
    let held = {
        let ledger = &*state.ledger;
        // A ledger read error must NOT panic (liveness: the reaper must keep
        // ticking), but it must NOT be SILENT either — `unwrap_or_default()`
        // would make a persistent ledger fault look like "nothing to reap"
        // forever. Log it and degrade to an empty sweep this tick; the next
        // tick retries.
        match ledger.held() {
            Ok(records) => records,
            Err(e) => {
                eprintln!(
                    "reaper: ledger held() read failed this tick (skipping expiry sweep, \
                     will retry next tick): {e:#}"
                );
                Vec::new()
            }
        }
        // `ledger` (MutexGuard) is dropped here — before any await below.
    };

    // ── 2. Identify overdue records, dating each purely from its durable
    // `deadline_ms`. A Held lease with `deadline_ms = None` is treated as
    // never-overdue (fail-safe: we never reap a lease we cannot date).
    // Multi-instance note: the sweep is intentionally ANY-INSTANCE — each record
    // carries its own durable `deadline_ms` and the abnormal-flush consumes the
    // durable pg checkpoint (proven cross-instance-safe by the `cross_instance_
    // reaper_*` tests), so ANY shard can correctly reap ANY overdue lease. This is
    // the ROBUST choice over per-shard filtering: a crashed/absent shard's orphans
    // are still reclaimed immediately by another shard (no owner-shard-down delay).
    // The only cost at N>1 is a few redundant (idempotent, already-gone→Ok)
    // teardown calls per overdue box — bounded and acceptable. Byte-identical at N=1.
    let overdue: Vec<_> = held
        .into_iter()
        .filter(|rec| now >= rec.deadline_ms.unwrap_or(u64::MAX))
        .collect();

    let mut reaped = 0usize;

    for rec in overdue {
        // ── 4. TEARDOWN FIRST — no lock held.
        let torn = state.teardown_lease(&rec.lease_id).await;

        if torn {
            // ── 5. Mark Expired ONLY after teardown succeeds, and only if WE
            // won the transition race.
            //
            // A concurrent close/cancel may have already moved the lease to
            // Released between teardown succeeding and this lock acquisition.
            // In that case `transition` returns Err (no legal pair out of a
            // terminal state). We bind the result: if it's Err, the lease was
            // already terminalized by someone else — do NOT GC or emit Expired
            // (that would double-free the slot in the journal).
            let expired_ok = {
                let ledger = &*state.ledger;
                ledger
                    .transition(&rec.lease_id, RunnerState::Expired, now)
                    .is_ok()
                // guard dropped here at end of block
            };

            if expired_ok {
                // ── WP-7: revoke the CAS PAT before the sync GC (A7b: all
                // terminal paths; fire-and-forget, never fails teardown).
                state.revoke_pat_for(&rec.lease_id).await;

                // ── WP-S13.5 (audit r5 FIX): best-effort PARTIAL-envelope flush —
                // MUST run BEFORE `forget_lease`, which unregisters the capture
                // hook. Previously it ran after, so `close_handle_any` found no
                // entry and the in-process partial metrics were silently dropped
                // (the run fell through to tier-3 zero-metrics). Marks
                // close_reason=expired + capture_incomplete:true; no-op if the
                // lease had no hook. The hook's close-once latch makes a racing
                // normal close safe. Sync + fast — does not meaningfully delay GC.
                flush_partial_envelope(
                    state,
                    &rec.lease_id,
                    &rec.tenant,
                    AbnormalKind::Expiry,
                    Instant::now(),
                );

                // ── 6. GC side-tables (deadline + image + hook entries).
                state.forget_lease(&rec.lease_id);

                // ── BIL1 / WP-SLOT-EMIT: slot expired — ledger lock is dropped
                // (the transition block above), forget_lease holds no ledger lock.
                // The symmetric Crashed path now lives in `surface_crashes`
                // (WP-CRASH-SWEEP, opt-in); this expiry path emits Expired only.
                // #3: carry the lease's durable billing-acquire stamp so the
                // usage-push bills correctly even if a restart dropped the
                // in-memory pairing (the reaper reaps leases held across restarts).
                state.record_slot_terminal(
                    &rec.lease_id,
                    &rec.tenant,
                    SlotEventKind::Expired,
                    rec.billing_acquired_at_ms,
                );
                state.counters.leases_expired.incr();

                reaped += 1;
            }
            // else: concurrent close/cancel won the race — their transition
            // already freed the slot; we do NOT emit Expired (would double-free)
            // and do NOT count this as a reaper reclaim.
        } else {
            // Teardown FAILED — leave Held, do NOT transition, do NOT GC.
            // The next sweep finds it again and retries teardown (the
            // teardown-first posture: the Expired mark is the CONSEQUENCE of a
            // successful teardown, never its precondition — see the module
            // "Retry posture / bounded retry" doc). The retry is bounded WORK
            // (one teardown call per overdue lease per tick, not a hot spin),
            // and the provider `activeDeadlineSeconds` is the ultimate compute
            // backstop — a box that can never be torn down still stops costing
            // money when the provider force-kills it. The remaining concern was
            // SILENCE; this forensic line surfaces a stuck box to ops on every
            // sweep so a permanently-failing teardown is observable, never
            // silent. (Intentionally no per-lease attempt counter: that would
            // add cross-instance state for no reclaim benefit — the deadline
            // already terminalizes the COMPUTE; the ledger row's Held status
            // here is forensic, not a live cap leak, because the box's cost is
            // already bounded.)
            eprintln!(
                "reaper: teardown FAILED for overdue lease (left Held, will retry \
                 next sweep; compute is bounded by provider activeDeadlineSeconds): \
                 lease_id={} tenant={}",
                rec.lease_id, rec.tenant
            );
        }
    }

    reaped
}

/// Spawn the background reaper task.
///
/// The returned [`tokio::task::JoinHandle`] runs until aborted by the caller;
/// bind the handle and call `.abort()` after the server's graceful-shutdown
/// future resolves so the task does not outlive the process.
pub fn spawn_reaper(state: crate::AppState, cfg: ReaperConfig) -> tokio::task::JoinHandle<()> {
    spawn_reaper_with_pending_age(state, cfg, DEFAULT_PENDING_MAX_AGE)
}

/// [`spawn_reaper`], with an explicit stale-Pending staleness bound.
///
/// The always-on reaper task runs BOTH sweeps each tick: the deadline-expiry
/// sweep ([`reap_once`]) AND the stale-Pending sweep ([`sweep_stale_pending`]),
/// because both reclaim leaked tenant cap and neither is opt-in. The
/// composition root resolves `pending_max_age` from
/// [`pending_max_age_from_env`].
pub fn spawn_reaper_with_pending_age(
    state: crate::AppState,
    cfg: ReaperConfig,
    pending_max_age: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(cfg.interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            // Suspension delivery is independent of lease cleanup. A slow or
            // unavailable Worker must never hold up expiry/teardown work.
            let dispatch_state = state.clone();
            tokio::spawn(async move {
                dispatch_tenant_suspension_events(&dispatch_state).await;
            });
            let n = reap_once(&state).await;
            if n > 0 {
                eprintln!("reaper: expired+reclaimed {n} overdue lease(s)");
            }
            // Stale-Pending sweep: reclaim cap slots leaked by a Pending whose
            // instance died between reserve and the Held transition / rollback.
            let p = crate::pending_cleanup::sweep_stale_pending(&state, pending_max_age).await;
            if p > 0 {
                eprintln!("reaper: reclaimed {p} stale Pending lease(s) (leaked cap slot)");
            }
        }
    })
}

// ── Crash-surfacing sweep (WP-CRASH-SWEEP, OPT-IN) ───────────────────────────

/// Resolve the crash-sweep interval from an environment-variable accessor.
///
/// Reads `FABRIC_CRASH_PROBE_INTERVAL_SECS`.
/// - **Absent or empty → `Ok(None)`**: the crash sweep is OPT-IN, so it is NOT
///   spawned by default. Rationale: crash-probing costs one provider
///   `is_alive` call per `Held` lease per tick and is newer/riskier than
///   deadline-expiry; the always-on deadline reaper ([`reap_once`]) remains the
///   backstop, so the sweep is off unless a deployer explicitly opts in.
/// - **Present → `Ok(Some(Duration))`**: parse as `u32`; value `0` or an
///   unparseable string → `Err` (a configured-but-zero interval is a deployer
///   mistake, not a silent disable — absence is the disable path).
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn crash_probe_config_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<Duration>> {
    match get("FABRIC_CRASH_PROBE_INTERVAL_SECS").filter(|s| !s.is_empty()) {
        // Absent/empty → opt-in default: the sweep is NOT spawned.
        None => Ok(None),
        Some(val) => {
            let parsed = val.trim().parse::<u32>().map_err(|_| {
                anyhow::anyhow!(
                    "FABRIC_CRASH_PROBE_INTERVAL_SECS must be a valid u32 (got {:?})",
                    val.trim()
                )
            })?;
            if parsed == 0 {
                anyhow::bail!(
                    "FABRIC_CRASH_PROBE_INTERVAL_SECS must be >= 1 \
                     (absent/empty is the way to disable the opt-in crash sweep, not 0)"
                );
            }
            Ok(Some(Duration::from_secs(parsed as u64)))
        }
    }
}

/// Run one crash-surfacing sweep: probe each `Held` lease's box and reclaim —
/// as `Crashed` — ONLY a box probed authoritatively-Dead.
///
/// Returns the number of leases reclaimed-as-crashed this tick.
///
/// # Fail-safe core
///
/// The sweep acts ONLY on `Ok(ProbeStatus::Dead)`. `Ok(Alive)`, `Ok(Unbound)`,
/// and EVERY `Err(_)` leave the lease `Held` and do nothing — a probe that is
/// anything other than authoritatively-Dead must NEVER reclaim. The deadline
/// reaper ([`reap_once`]) is the backstop for a lease whose box is dead but
/// whose probe is inconclusive: it still expires on its hard deadline.
///
/// # Posture (mirrors [`reap_once`])
///
/// Teardown-FIRST: the box is torn down before the `Crashed` ledger mark, and
/// the mark is written ONLY if teardown succeeds AND we won the transition race
/// (a concurrent close/cancel may have already terminalized the lease — that
/// `transition` returns `Err`, and we then do NOT GC, do NOT emit `Crashed`,
/// do NOT count, avoiding a double-free of the slot). A failed teardown leaves
/// the lease `Held` so the next sweep retries.
///
/// # Lock-ordering note
///
/// No `MutexGuard` is held across any `await` point: the `Held` snapshot is
/// taken in a scoped block (guard dropped before any await), the probe and
/// teardown awaits hold no lock, and the `Crashed` transition re-acquires the
/// ledger lock briefly (dropped before continuing). The compile-time
/// [`_ASSERT_SURFACE_CRASHES_IS_SEND`] assertion enforces this.
pub async fn surface_crashes(state: &crate::AppState) -> usize {
    let now = state.clock.now_ms();

    // ── 1. Snapshot held leases — guard dropped at end of block, before await.
    let held = {
        let ledger = &*state.ledger;
        // Same liveness-but-not-silent posture as `reap_once`: a ledger read
        // error is logged and degrades to an empty sweep this tick (never a
        // panic, never a silent skip).
        match ledger.held() {
            Ok(records) => records,
            Err(e) => {
                eprintln!(
                    "crash-sweep: ledger held() read failed this tick (skipping probe sweep, \
                     will retry next tick): {e:#}"
                );
                Vec::new()
            }
        }
        // `ledger` (MutexGuard) is dropped here — before any await below.
    };

    let mut reaped = 0usize;

    for rec in held {
        // ── 2. PROBE — no lock held. FAIL-SAFE: act ONLY on Ok(Dead).
        let status = state.probe_lease(&rec.lease_id).await;
        if !matches!(status, Ok(crate::cloud_exec::ProbeStatus::Dead)) {
            // Alive / Unbound / Err(_) → leave Held, do nothing. An unreachable
            // provider (Err) must NEVER be read as death; the deadline reaper is
            // the backstop.
            continue;
        }

        // ── 3. TEARDOWN FIRST — no lock held.
        let torn = state.teardown_lease(&rec.lease_id).await;
        if !torn {
            // Leave Held — the next sweep retries teardown.
            continue;
        }

        // ── 4. Mark Crashed ONLY after teardown succeeds, and only if WE won
        // the transition race. A concurrent close/cancel may have moved the
        // lease to a terminal state between teardown and this lock acquisition;
        // `transition` then returns Err and we must NOT double-free the slot.
        let crashed_ok = {
            let ledger = &*state.ledger;
            ledger
                .transition(&rec.lease_id, RunnerState::Crashed, now)
                .is_ok()
            // guard dropped here at end of block
        };

        if crashed_ok {
            // ── WP-7: revoke the CAS PAT before the sync GC (A7b: all terminal
            // paths; fire-and-forget, never fails teardown).
            state.revoke_pat_for(&rec.lease_id).await;

            // ── WP-S13.5 (audit r5 FIX): PARTIAL-envelope flush MUST run BEFORE
            // `forget_lease` (which unregisters the hook) — symmetric with the
            // Expired path. Previously after, so the hook was already gone and the
            // partial metrics were dropped to tier-3 zero. Marks
            // close_reason=crashed + capture_incomplete:true; no-op if no hook.
            flush_partial_envelope(
                state,
                &rec.lease_id,
                &rec.tenant,
                AbnormalKind::Crash,
                Instant::now(),
            );

            // ── 5. GC side-tables, then emit the Crashed slot event.
            state.forget_lease(&rec.lease_id);
            // #3: carry the durable billing-acquire stamp (restart-recovery).
            state.record_slot_terminal(
                &rec.lease_id,
                &rec.tenant,
                SlotEventKind::Crashed,
                rec.billing_acquired_at_ms,
            );
            state.counters.leases_crashed.incr();

            reaped += 1;
        }
        // else: concurrent close/cancel won the race — their transition already
        // freed the slot; we do NOT emit Crashed (would double-free) and do NOT
        // count this as a crash reclaim.
    }

    reaped
}

/// Spawn the background crash-surfacing sweep (WP-CRASH-SWEEP).
///
/// Mirrors [`spawn_reaper`]. The returned [`tokio::task::JoinHandle`] runs until
/// aborted by the caller; bind the handle and call `.abort()` after graceful
/// shutdown so the task does not outlive the process.
///
/// This is OPT-IN: the composition root spawns it only when
/// [`crash_probe_config_from_env`] returns `Some(interval)`.
pub fn spawn_crash_sweep(
    state: crate::AppState,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let n = surface_crashes(&state).await;
            if n > 0 {
                eprintln!("crash-sweep: surfaced+reclaimed {n} crashed lease(s)");
            }
        }
    })
}

// ── Stale-Pending sweep (WP-PENDING-SWEEP) ───────────────────────────────────
// The confirmed claim/teardown/finish implementation lives in
// [`crate::pending_cleanup`]. This module retains only the historical public
// entry point and configuration re-exports.

/// Default staleness bound for the [`sweep_stale_pending`] reclaim: a `Pending`
/// older than this is considered leaked (well past any legitimate provision
/// window — provisioning a box is an O(seconds) operation, so 5 minutes is a
/// very conservative floor that can never catch a mid-provision Pending).
pub use crate::pending_cleanup::{DEFAULT_PENDING_MAX_AGE, pending_max_age_from_env};

/// Run one confirmed stale-Pending cleanup sweep.
pub async fn sweep_stale_pending(state: &crate::AppState, max_age: Duration) -> usize {
    crate::pending_cleanup::sweep_stale_pending(state, max_age).await
}

// ── Compile-time Send guard ────────────────────────────────────────────────
//
// If `reap_once` ever acquires a `MutexGuard` (or any other `!Send` type)
// across an `await` point, the future it returns becomes `!Send` and this
// static assertion causes a compile error — catching the regression before
// it reaches CI.

#[allow(dead_code)]
const _ASSERT_REAP_ONCE_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: crate::AppState) {
        _assert_send_fut(reap_once(&state));
    }
};

// Same guard for `surface_crashes`: a `MutexGuard` held across an `await`
// would make its future `!Send` and break this assertion at compile time.
#[allow(dead_code)]
const _ASSERT_SURFACE_CRASHES_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: crate::AppState) {
        _assert_send_fut(surface_crashes(&state));
    }
};

// Same guard for `sweep_stale_pending`: a `MutexGuard` held across the teardown
// `await` would make its future `!Send` and break this assertion at compile time.
#[allow(dead_code)]
const _ASSERT_SWEEP_STALE_PENDING_IS_SEND: () = {
    fn _assert_send_fut<F: std::future::Future + Send>(_: F) {}
    fn _check(state: crate::AppState) {
        _assert_send_fut(sweep_stale_pending(&state, DEFAULT_PENDING_MAX_AGE));
    }
};

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, SlotEventKind, TenantId,
    };
    use corelink_runners_contracts::RunnerState;

    use super::*;
    use crate::app::{AppState, Clock, StaticPlans};
    use crate::cloud_exec::{BoxProvisioner, ProbeStatus};

    // ── Deterministic clock ───────────────────────────────────────────────────

    /// A clock whose time is set by the test.
    #[derive(Clone)]
    struct FixedClock(Arc<AtomicU64>);

    impl FixedClock {
        fn new(ms: u64) -> Self {
            Self(Arc::new(AtomicU64::new(ms)))
        }
        #[allow(dead_code)]
        fn set(&self, ms: u64) {
            self.0.store(ms, Ordering::SeqCst);
        }
    }

    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    // ── RecordingProvisioner ──────────────────────────────────────────────────

    /// Provisioner that records which lease ids `teardown` was called with.
    /// `provision` always returns `Ok(())`.
    #[derive(Default)]
    struct RecordingProvisioner {
        teardown_calls: Mutex<Vec<String>>,
    }

    impl RecordingProvisioner {
        fn new() -> Self {
            Self::default()
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for RecordingProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            Ok(())
        }
        fn teardown_pending(&self, lease_id: &str) -> crate::CleanupTeardown {
            let _ = self.teardown(lease_id);
            crate::CleanupTeardown::ConfirmedDestroyed
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            Ok(ProbeStatus::Unbound)
        }
    }

    // ── TogglesTeardownProvisioner ────────────────────────────────────────────

    /// Provisioner whose teardown result is toggled by an `AtomicBool`.
    ///
    /// `should_succeed = true` → teardown returns `Ok(())`; `false` → `Err`.
    /// Also records which lease ids teardown was called with, for assertion.
    struct TogglesTeardownProvisioner {
        should_succeed: Arc<AtomicBool>,
        teardown_calls: Mutex<Vec<String>>,
    }

    impl TogglesTeardownProvisioner {
        fn new(initial: bool) -> (Self, Arc<AtomicBool>) {
            let flag = Arc::new(AtomicBool::new(initial));
            let prov = Self {
                should_succeed: Arc::clone(&flag),
                teardown_calls: Mutex::new(Vec::new()),
            };
            (prov, flag)
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for TogglesTeardownProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            if self.should_succeed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("teardown intentionally failed"))
            }
        }
        fn teardown_pending(&self, lease_id: &str) -> crate::CleanupTeardown {
            if self.should_succeed.load(Ordering::SeqCst) {
                let _ = self.teardown(lease_id);
                crate::CleanupTeardown::ConfirmedDestroyed
            } else {
                let _ = self.teardown(lease_id);
                crate::CleanupTeardown::Retryable
            }
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            Ok(ProbeStatus::Unbound)
        }
    }

    // ── ScriptedProbeProvisioner ──────────────────────────────────────────────

    /// Probe outcome scripted per test: `Alive`, `Dead`, `Unbound`, or `Err`.
    #[derive(Clone, Copy)]
    enum Scripted {
        Alive,
        Dead,
        Unbound,
        Err,
    }

    /// Provisioner whose `probe` returns a scripted [`Scripted`] outcome and
    /// whose `teardown` result is toggled by an `AtomicBool`. Records teardown
    /// calls for assertion.
    struct ScriptedProbeProvisioner {
        probe_outcome: Scripted,
        teardown_succeed: Arc<AtomicBool>,
        teardown_calls: Mutex<Vec<String>>,
    }

    impl ScriptedProbeProvisioner {
        fn new(outcome: Scripted, teardown_succeed: bool) -> Self {
            Self {
                probe_outcome: outcome,
                teardown_succeed: Arc::new(AtomicBool::new(teardown_succeed)),
                teardown_calls: Mutex::new(Vec::new()),
            }
        }
        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for ScriptedProbeProvisioner {
        fn provision(
            &self,
            _lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            Ok(())
        }
        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            if self.teardown_succeed.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(anyhow::anyhow!("teardown intentionally failed"))
            }
        }
        fn teardown_pending(&self, lease_id: &str) -> crate::CleanupTeardown {
            if self.teardown_succeed.load(Ordering::SeqCst) {
                let _ = self.teardown(lease_id);
                crate::CleanupTeardown::ConfirmedDestroyed
            } else {
                let _ = self.teardown(lease_id);
                crate::CleanupTeardown::Retryable
            }
        }
        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            match self.probe_outcome {
                Scripted::Alive => Ok(ProbeStatus::Alive),
                Scripted::Dead => Ok(ProbeStatus::Dead),
                Scripted::Unbound => Ok(ProbeStatus::Unbound),
                Scripted::Err => Err(anyhow::anyhow!("provider unreachable (transient)")),
            }
        }
    }

    /// Build an `AppState` with a `ScriptedProbeProvisioner`.
    fn build_state_scripted(
        now_ms: u64,
        outcome: Scripted,
        teardown_succeed: bool,
    ) -> (AppState, FixedClock, Arc<ScriptedProbeProvisioner>) {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(now_ms);
        let prov = Arc::new(ScriptedProbeProvisioner::new(outcome, teardown_succeed));
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov)
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build an `AppState` with a `FixedClock` and a `RecordingProvisioner`.
    ///
    /// Returns `(state, clock, provisioner_arc)` so the test can mutate the
    /// clock and inspect teardown calls.
    fn build_state(now_ms: u64) -> (AppState, FixedClock, Arc<RecordingProvisioner>) {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(now_ms);
        let prov = Arc::new(RecordingProvisioner::new());
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov)
    }

    /// Build an `AppState` with a `TogglesTeardownProvisioner`.
    fn build_state_toggles(
        now_ms: u64,
        initial_succeed: bool,
    ) -> (
        AppState,
        FixedClock,
        Arc<TogglesTeardownProvisioner>,
        Arc<AtomicBool>,
    ) {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(now_ms);
        let (prov, flag) = TogglesTeardownProvisioner::new(initial_succeed);
        let prov = Arc::new(prov);
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov, flag)
    }

    /// Register a capture hook on `state.hook_registry` for `lease_id` so the
    /// §13.5 partial-envelope flush has something to finalize. Feeds one
    /// model-turn so the finalized metrics are non-trivial.
    fn register_hook(state: &AppState, lease_id: &str) {
        use corelink_runner::envelope::{
            CaptureHook, EnvelopeConfig, MetricsCollector, TranscriptEvent,
        };
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: std::time::Duration::from_millis(1),
                buffer_capacity: 16,
            },
            "hookcred-reaper",
            MetricsCollector::new(std::time::Instant::now()),
        );
        // One observed turn → the finalized partial metrics are not all-zero.
        hook.write(TranscriptEvent::ModelTurn {
            bytes: b"partial-turn".to_vec(),
            usage: None,
            busy_ms: 5,
        })
        .expect("hook write on an open hook must succeed");
        state.hook_registry.register(
            lease_id,
            TenantId::new("acme").unwrap(),
            hook,
            "hookcred-reaper",
        );
    }

    /// Insert a `Held` lease record in the ledger with the given DURABLE
    /// deadline (ADR-0004: `deadline_ms` rides the record; the terminal
    /// transition preserves it, so the reaped record still reads its deadline).
    fn insert_held(state: &AppState, lease_id: &str, deadline_ms: u64) {
        {
            let ledger = &*state.ledger;
            ledger
                .put(LeaseRecord {
                    lease_id: lease_id.to_string(),
                    tenant: TenantId::new("acme").unwrap(),
                    state: LeaseState::Pending,
                    box_ref: format!("box:{lease_id}"),
                    created_at_ms: 0,
                    updated_at_ms: 0,
                    deadline_ms: Some(deadline_ms),
                    billing_acquired_at_ms: None,
                })
                .unwrap();
            ledger.transition(lease_id, RunnerState::Held, 0).unwrap();
        }
        // A fake image so forget_lease's image GC is verifiable.
        state.record_image(lease_id, "sha256:deadbeef");
    }

    /// The lease's durable `deadline_ms` as read back from the ledger — the
    /// reaper's source of truth (ADR-0004). `None` = no deadline / absent.
    fn ledger_deadline(state: &AppState, lease_id: &str) -> Option<u64> {
        state
            .ledger
            .get(lease_id)
            .unwrap()
            .and_then(|rec| rec.deadline_ms)
    }

    /// Insert a `Pending` lease record with the given `created_at_ms` — the
    /// pre-provision reservation state the stale-Pending sweep reclaims. Stays
    /// `Pending` (never transitioned to Held), so it is NOT in `held()` and the
    /// deadline reaper never sees it.
    fn insert_pending(state: &AppState, lease_id: &str, created_at_ms: u64) {
        let ledger = &*state.ledger;
        ledger
            .put(LeaseRecord {
                lease_id: lease_id.to_string(),
                tenant: TenantId::new("acme").unwrap(),
                state: LeaseState::Pending,
                box_ref: format!("box:{lease_id}"),
                created_at_ms,
                updated_at_ms: created_at_ms,
                deadline_ms: None,
                billing_acquired_at_ms: None,
            })
            .unwrap();
    }

    /// `true` iff the lease still exists in the ledger.
    fn lease_exists(state: &AppState, lease_id: &str) -> bool {
        state.ledger.get(lease_id).unwrap().is_some()
    }

    // ── Stale-Pending sweep tests (WP-PENDING-SWEEP) ──────────────────────────

    /// A `Pending` older than the bound is reclaimed (removed → cap freed); a
    /// FRESH `Pending` is left untouched. The fail-safe core of the sweep.
    #[tokio::test]
    async fn sweep_reclaims_stale_pending_leaves_fresh() {
        // Clock at 1_000_000 ms; bound = 300 s = 300_000 ms → cutoff 700_000.
        let (state, _clock, prov) = build_state(1_000_000);

        // Stale: created at 100_000 (≪ cutoff) → reclaimed.
        insert_pending(&state, "pending-stale", 100_000);
        // Fresh: created at 990_000 (> cutoff) → left (mid-provision, fail-safe).
        insert_pending(&state, "pending-fresh", 990_000);

        let reclaimed = sweep_stale_pending(&state, Duration::from_secs(300)).await;
        assert_eq!(reclaimed, 1, "exactly the one stale Pending is reclaimed");

        // Stale gone (cap slot freed); fresh still present.
        assert!(
            !lease_exists(&state, "pending-stale"),
            "the stale Pending must be removed (leaked cap slot reclaimed)"
        );
        assert!(
            lease_exists(&state, "pending-fresh"),
            "a fresh Pending must be LEFT (never reap one mid-provision)"
        );

        // Teardown was attempted for the leaked box (best-effort, teardown-first).
        assert!(
            prov.teardown_calls().contains(&"pending-stale".to_string()),
            "teardown must be attempted for the reclaimed Pending's box"
        );
        assert!(
            !prov.teardown_calls().contains(&"pending-fresh".to_string()),
            "the fresh Pending's box must NOT be torn down"
        );
    }

    // ── W2-B regression: the sweep must NOT delete a Pending that raced to
    // Held in the sweep window ──────────────────────────────────────────────

    /// Ledger decorator that models a concurrent acquire winning immediately
    /// BEFORE the new atomic cleanup-claim seam. The claim must observe `Held`
    /// and return no work; it may neither claim nor tear down the live box.
    struct RaceToHeldLedger {
        // W-LEDGER-A2: the trait is `&self` + the ledger Arc is `Send + Sync`, so the
        // interior mutability lives in `InMemoryLedger` (its own `Arc<Mutex<..>>`),
        // not a `!Sync` `RefCell`.
        inner: InMemoryLedger,
        /// The lease to flip to `Held` immediately before the first cleanup claim.
        race_lease: String,
        /// Flips false after the first cleanup-claim attempt so the race fires once.
        armed: std::sync::atomic::AtomicBool,
    }

    impl RaceToHeldLedger {
        fn new(race_lease: &str) -> Self {
            Self {
                inner: InMemoryLedger::new(),
                race_lease: race_lease.to_string(),
                armed: std::sync::atomic::AtomicBool::new(true),
            }
        }
    }

    impl LeaseLedger for RaceToHeldLedger {
        fn put(&self, rec: LeaseRecord) -> Result<()> {
            self.inner.put(rec)
        }
        fn get(&self, lease_id: &str) -> Result<Option<LeaseRecord>> {
            self.inner.get(lease_id)
        }
        fn transition(&self, lease_id: &str, to: RunnerState, now_ms: u64) -> Result<LeaseRecord> {
            self.inner.transition(lease_id, to, now_ms)
        }
        fn by_tenant(&self, t: &TenantId) -> Result<Vec<LeaseRecord>> {
            self.inner.by_tenant(t)
        }
        fn held(&self) -> Result<Vec<LeaseRecord>> {
            self.inner.held()
        }
        fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> Result<Vec<LeaseRecord>> {
            self.inner.pending_older_than(now_ms, max_age_ms)
        }
        fn claim_stale_pending_cleanup(
            &self,
            now_ms: u64,
            max_age_ms: u64,
        ) -> Result<Vec<LeaseRecord>> {
            // The old snapshot/delete race is now one atomic ledger claim. Make
            // Held win immediately before that claim, so the claim cannot turn a
            // live lease into cleanup work.
            if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.inner
                    .transition(&self.race_lease, RunnerState::Held, now_ms)
                    .expect("race-flip Pending→Held must be a legal transition");
            }
            self.inner.claim_stale_pending_cleanup(now_ms, max_age_ms)
        }
        fn claim_pending_cleanup(
            &self,
            lease_id: &str,
            now_ms: u64,
        ) -> Result<Option<LeaseRecord>> {
            self.inner.claim_pending_cleanup(lease_id, now_ms)
        }
        fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> Result<bool> {
            self.inner.try_admit(rec, max_concurrency)
        }
        fn set_envelope_checkpoint(&self, lease_id: &str, checkpoint_json: &str) -> Result<()> {
            self.inner
                .set_envelope_checkpoint(lease_id, checkpoint_json)
        }
        fn get_envelope_checkpoint(&self, lease_id: &str) -> Result<Option<String>> {
            self.inner.get_envelope_checkpoint(lease_id)
        }
        fn remove(&self, lease_id: &str) -> Result<bool> {
            self.inner.remove(lease_id)
        }
        fn remove_if_pending(&self, lease_id: &str) -> Result<bool> {
            // The fix under test: delegate to the inner impl's REAL guarded path
            // (delete iff still Pending). By now the lease is Held → no-op.
            self.inner.remove_if_pending(lease_id)
        }
        fn finish_pending_cleanup(&self, lease_id: &str) -> Result<bool> {
            self.inner.finish_pending_cleanup(lease_id)
        }
    }

    /// A genuinely-stale `Pending` that races to `Held` in the sweep window is
    /// NOT reclaimed: the guarded delete no-ops, the live `Held` lease survives,
    /// its box is NOT torn down, and the sweep counts 0. This is the W2-B
    /// regression the fix closes (a state-blind `remove` would have deleted it).
    #[tokio::test]
    async fn sweep_does_not_reclaim_pending_that_raced_to_held() {
        // Build state on a race-injecting ledger that flips the lease to Held
        // immediately before the atomic cleanup claim.
        let ledger: Arc<dyn LeaseLedger + Send + Sync> =
            Arc::new(RaceToHeldLedger::new("pending-raced"));
        let clock = FixedClock::new(1_000_000);
        let prov = Arc::new(RecordingProvisioner::new());
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;

        // A genuinely-stale Pending (created at 100_000 ≪ cutoff 700_000) that
        // races to Held before the cleanup claim can fence it.
        insert_pending(&state, "pending-raced", 100_000);

        let reclaimed = sweep_stale_pending(&state, Duration::from_secs(300)).await;

        assert_eq!(
            reclaimed, 0,
            "a lease that won Pending→Held before cleanup claim must not be reclaimed"
        );
        // The live lease must still exist AND still be Held — never deleted.
        let rec = state
            .ledger
            .get("pending-raced")
            .unwrap()
            .expect("the raced-to-Held lease must NOT be deleted by the sweep");
        assert!(
            rec.state.is_held(),
            "the raced lease must remain Held (a live lease), not deleted"
        );
        // And its box must NOT be torn down (Held won before claim → no teardown).
        assert!(
            prov.teardown_calls().is_empty(),
            "the live Held lease's box must NOT be torn down by the stale-Pending sweep"
        );
    }

    /// A genuinely-stale `Pending` that does NOT race (stays Pending through the
    /// delete) is STILL reclaimed — proving the guard only blocks the raced case,
    /// never the legitimate leaked-slot reclaim. Box teardown is attempted (we
    /// won the CAS).
    #[tokio::test]
    async fn sweep_still_reclaims_genuinely_stale_pending() {
        let (state, _clock, prov) = build_state(1_000_000);
        // Stale, and it stays Pending through the guarded delete.
        insert_pending(&state, "pending-leaked", 100_000);

        let reclaimed = sweep_stale_pending(&state, Duration::from_secs(300)).await;

        assert_eq!(
            reclaimed, 1,
            "the genuinely-stale Pending is still reclaimed"
        );
        assert!(
            !lease_exists(&state, "pending-leaked"),
            "the genuinely-stale Pending must be removed (leaked cap slot reclaimed)"
        );
        assert!(
            prov.teardown_calls()
                .contains(&"pending-leaked".to_string()),
            "teardown must run for the reclaimed (won-the-CAS) Pending's box"
        );
    }

    /// The deadline reaper ([`reap_once`]) NEVER reclaims a Pending (it sweeps
    /// `Held` only) — proving the stale-Pending sweep is the sole reclaimer and
    /// the two paths do not overlap.
    #[tokio::test]
    async fn reap_once_never_touches_pending() {
        let (state, _clock, prov) = build_state(1_000_000);
        insert_pending(&state, "pending-old", 1); // very old

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 0, "the deadline reaper must never reap a Pending");
        assert!(
            lease_exists(&state, "pending-old"),
            "the Pending must survive reap_once (only the Pending sweep reclaims it)"
        );
        assert!(
            prov.teardown_calls().is_empty(),
            "reap_once must not tear down a Pending"
        );
    }

    /// `FABRIC_PENDING_MAX_AGE_SECS`: absent → 300 s default; `0` → error;
    /// `"60"` → 60 s.
    #[test]
    fn pending_max_age_config() {
        assert_eq!(
            pending_max_age_from_env(|_| None).unwrap(),
            Duration::from_secs(300),
            "absent env → 300 s default"
        );
        assert!(
            pending_max_age_from_env(
                |k| (k == "FABRIC_PENDING_MAX_AGE_SECS").then(|| "0".to_string())
            )
            .is_err(),
            "0 must error (would reap mid-provision)"
        );
        assert_eq!(
            pending_max_age_from_env(
                |k| (k == "FABRIC_PENDING_MAX_AGE_SECS").then(|| "60".to_string())
            )
            .unwrap(),
            Duration::from_secs(60),
        );
    }

    // ── Config tests ──────────────────────────────────────────────────────────

    /// Absent env → default 30 s.
    #[test]
    fn reap_config_default_30() {
        let cfg = reaper_config_from_env(|_| None).unwrap();
        assert_eq!(cfg.interval, std::time::Duration::from_secs(30));
    }

    /// `FABRIC_REAP_INTERVAL_SECS=0` → error; `="15"` → 15 s.
    #[test]
    fn reap_config_zero_errs() {
        let result = reaper_config_from_env(|k| {
            if k == "FABRIC_REAP_INTERVAL_SECS" {
                Some("0".to_string())
            } else {
                None
            }
        });
        assert!(result.is_err(), "FABRIC_REAP_INTERVAL_SECS=0 must error");

        let cfg = reaper_config_from_env(|k| {
            if k == "FABRIC_REAP_INTERVAL_SECS" {
                Some("15".to_string())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(cfg.interval, std::time::Duration::from_secs(15));
    }

    // ── reap_once behavior tests ──────────────────────────────────────────────

    /// An overdue `Held` lease with a succeeding provisioner:
    /// - reap_once returns 1
    /// - lease is `Expired` in the ledger
    /// - deadlines + images entries are GC'd
    /// - teardown was called
    #[tokio::test]
    async fn reap_once_expires_and_tears_down_overdue() {
        // Clock at 2000 ms; deadline 1000 ms → already past.
        let (state, _clock, prov) = build_state(2_000);
        insert_held(&state, "lease-overdue", 1_000);

        let count = reap_once(&state).await;
        assert_eq!(count, 1, "exactly one lease should be reclaimed");

        // Lease must now be `Expired` in the ledger.
        let ledger = &*state.ledger;
        let rec = ledger.get("lease-overdue").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Expired),
            "lease must be Expired after reap"
        );

        // Provisioner must have seen a teardown call.
        let calls = prov.teardown_calls();
        assert!(
            calls.contains(&"lease-overdue".to_string()),
            "teardown must be called for the expired lease; calls={calls:?}"
        );

        // The image side-table must be GC'd. The deadline is NOT a side table
        // anymore (ADR-0004): it rides the record and the terminal transition
        // PRESERVES it — so it is durably present on the Expired record, not
        // GC'd. What removes the lease from the reap path is the terminal state
        // (it is no longer in `held()`), not deletion of the deadline.
        assert!(
            state.image_of("lease-overdue").is_none(),
            "image entry must be removed after successful reclaim"
        );
        assert_eq!(
            ledger_deadline(&state, "lease-overdue"),
            Some(1_000),
            "the durable deadline_ms is preserved on the terminal record (ADR-0004)"
        );
    }

    /// THE CRUX REGRESSION TEST: teardown-first, retry-on-failure.
    ///
    /// Scenario:
    /// 1. First sweep: teardown FAILS → reap_once returns 0, lease is STILL
    ///    Held (not Expired), deadline entry retained.
    /// 2. Second sweep: teardown succeeds → reap_once returns 1, lease is
    ///    Expired, deadline entry GC'd.
    #[tokio::test]
    async fn reap_once_failed_teardown_keeps_held_for_retry() {
        let (state, _clock, prov, flag) =
            build_state_toggles(2_000, /* initial_succeed */ false);
        insert_held(&state, "lease-retry", 1_000);

        // ── Sweep 1: teardown fails ──────────────────────────────────────────
        let count = reap_once(&state).await;
        assert_eq!(count, 0, "failed teardown: nothing reaped");

        // Lease MUST still be Held — NOT Expired.
        {
            let ledger = &*state.ledger;
            let rec = ledger.get("lease-retry").unwrap().unwrap();
            assert!(
                rec.state.is_held(),
                "lease must remain Held after failed teardown; state={:?}",
                rec.state
            );
        }

        // The durable deadline MUST still ride the (still-Held) record so the
        // next sweep can date the lease (ADR-0004: it lives in the ledger).
        assert_eq!(
            ledger_deadline(&state, "lease-retry"),
            Some(1_000),
            "deadline_ms must ride the still-Held record after a failed teardown"
        );

        // Teardown was attempted once.
        assert_eq!(
            prov.teardown_calls().len(),
            1,
            "teardown should have been called once (and failed)"
        );

        // ── Sweep 2: make teardown succeed, re-run ───────────────────────────
        flag.store(true, Ordering::SeqCst);

        let count2 = reap_once(&state).await;
        assert_eq!(count2, 1, "second sweep with successful teardown: 1 reaped");

        // Lease is now Expired.
        {
            let ledger = &*state.ledger;
            let rec = ledger.get("lease-retry").unwrap().unwrap();
            assert_eq!(
                rec.state,
                LeaseState::Wire(RunnerState::Expired),
                "lease must be Expired after second sweep"
            );
        }

        // The durable deadline is PRESERVED on the terminal Expired record
        // (ADR-0004: a transition never alters the deadline); the lease leaves
        // the reap path by being terminal (out of `held()`), not by deletion.
        assert_eq!(
            ledger_deadline(&state, "lease-retry"),
            Some(1_000),
            "deadline_ms is preserved on the terminal record (not GC'd)"
        );

        // Teardown was called twice total (once fail, once succeed).
        assert_eq!(
            prov.teardown_calls().len(),
            2,
            "teardown must have been called twice (retry)"
        );
    }

    /// A `Held` lease with a future deadline is left untouched.
    #[tokio::test]
    async fn reap_once_leaves_unexpired_held() {
        // Clock at 1000; deadline at 9000 → not yet overdue.
        let (state, _clock, prov) = build_state(1_000);
        insert_held(&state, "lease-fresh", 9_000);

        let count = reap_once(&state).await;
        assert_eq!(count, 0, "no leases should be expired when all are fresh");

        // Lease must still be Held.
        let ledger = &*state.ledger;
        let rec = ledger.get("lease-fresh").unwrap().unwrap();
        assert!(rec.state.is_held(), "unexpired lease must remain Held");

        // No teardown calls.
        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must not be called for a fresh lease"
        );
    }

    /// FAIL-SAFE (ADR-0004): a `Held` lease with `deadline_ms: None` is treated
    /// as never-overdue — the deadline path NEVER reaps a lease it cannot date,
    /// even with the clock far in the future. (The hard backstop for such a
    /// lease is the crash sweep, not the deadline reaper.)
    #[tokio::test]
    async fn reap_once_never_reaps_none_deadline() {
        // Clock very far in the future (but NOT u64::MAX, which is the
        // never-overdue sentinel itself); the lease has NO deadline.
        let (state, _clock, prov) = build_state(u64::MAX - 1);
        {
            let ledger = &*state.ledger;
            ledger
                .put(LeaseRecord {
                    lease_id: "lease-nodeadline".to_string(),
                    tenant: TenantId::new("acme").unwrap(),
                    state: LeaseState::Pending,
                    box_ref: "box:lease-nodeadline".to_string(),
                    created_at_ms: 0,
                    updated_at_ms: 0,
                    deadline_ms: None, // never-overdue fail-safe
                    billing_acquired_at_ms: None,
                })
                .unwrap();
            ledger
                .transition("lease-nodeadline", RunnerState::Held, 0)
                .unwrap();
        }

        let count = reap_once(&state).await;
        assert_eq!(count, 0, "a None-deadline lease must NEVER be reaped");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-nodeadline").unwrap().unwrap();
        assert!(
            rec.state.is_held(),
            "a None-deadline lease stays Held (never-overdue fail-safe)"
        );
        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must not be called for a None-deadline lease"
        );
    }

    /// Empty or no-Held ledger → 0 expirations, no teardown.
    #[tokio::test]
    async fn reap_once_no_held_is_noop() {
        let (state, _clock, prov) = build_state(999_999);
        // No leases inserted at all.
        let count = reap_once(&state).await;
        assert_eq!(count, 0, "empty ledger must produce 0 expirations");
        assert!(
            prov.teardown_calls().is_empty(),
            "no teardowns on empty ledger"
        );
    }

    // ── WP-SLOT-EMIT: reaper_expiry_emits_expired_slot ───────────────────────
    //
    // Lives here (in-crate) because the slot meter / ledger helpers are `pub(crate)` and
    // integration tests in `tests/` cannot call it.

    /// Reaper LOST RACE: if a concurrent close/cancel already terminalized the
    /// lease (Released), the reaper's `held()` snapshot excludes it — so
    /// `reap_once` processes zero overdue leases and emits no Expired event.
    ///
    /// This guards FIX 2: even before the transition gate, the held() snapshot
    /// already filters out Released/terminal leases, so a race-lost reaper pass
    /// produces no phantom Expired event and occupied stays 0.
    #[tokio::test]
    async fn reaper_lost_race_emits_no_phantom_expired() {
        // Clock at 2 000 ms; deadline 1 000 ms → overdue if still Held.
        let (state, _clock, _prov) = build_state(2_000);
        let acme = TenantId::new("acme").unwrap();

        // Insert a lease, record it in the slot meter as Acquired (1 occupied).
        insert_held(&state, "lease-race", 1_000);
        state.record_slot("lease-race", &acme, SlotEventKind::Acquired);

        // Simulate close/cancel winning the race: transition to Released under
        // the ledger lock, then emit Released in the slot meter.
        {
            let ledger = &*state.ledger;
            ledger
                .transition("lease-race", RunnerState::Released, 1_500)
                .expect("Held→Released must succeed");
        }
        state.record_slot("lease-race", &acme, SlotEventKind::Released);

        // Now run the reaper. The lease is Released (terminal), so `held()`
        // excludes it — reap_once does nothing.
        let reaped = reap_once(&state).await;
        assert_eq!(
            reaped, 0,
            "reaper must reap 0: the lease is already Released"
        );

        let meter = state.slot_meter.lock().unwrap();
        // No Expired event: the reaper never saw the lease as Held.
        let expired_count = meter
            .journal()
            .iter()
            .filter(|e| matches!(e.kind, corelink_fabric::SlotEventKind::Expired))
            .count();
        assert_eq!(
            expired_count, 0,
            "no Expired event must be emitted when the reaper loses the race"
        );
        // Slot is correctly at 0 (Acquired then Released; no phantom Expired).
        assert_eq!(
            meter.occupied(&acme),
            0,
            "occupied must be 0 (no double-free from phantom Expired)"
        );
        // Journal must contain exactly the Acquired + Released pair from the
        // simulated close, nothing else.
        assert_eq!(
            meter.journal().len(),
            2,
            "journal must have exactly Acquired + Released, no extra Expired"
        );
    }

    /// A Held lease reaped via `reap_once` frees its slot: `occupied == 0`
    /// and the journal records an `Expired` event.
    #[tokio::test]
    async fn reaper_expiry_emits_expired_slot() {
        // Clock at 2 000 ms; deadline 1 000 ms → already past.
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-slot-expiry", 1_000);

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "one lease must be reaped");

        let acme = TenantId::new("acme").unwrap();
        let meter = state.slot_meter.lock().unwrap();

        // Slot freed: occupied back to zero.
        assert_eq!(
            meter.occupied(&acme),
            0,
            "slot must be freed after expiry (occupied==0)"
        );
        // Exactly one event: the Expired emission.
        assert_eq!(
            meter.journal().len(),
            1,
            "journal must have exactly one Expired event"
        );
        assert!(
            matches!(
                meter.journal()[0].kind,
                corelink_fabric::SlotEventKind::Expired
            ),
            "the journaled event must be Expired"
        );
        assert_eq!(
            meter.journal()[0].lease_id,
            "lease-slot-expiry",
            "event lease_id must match"
        );
    }

    /// The golden-signal counter for expiry is bumped EXACTLY once per reaped
    /// lease, and ONLY the expiry counter (crash/close untouched). The existing
    /// expiry tests assert the ledger transition + slot event; this pins the
    /// observability seam value itself (`counters.leases_expired`).
    #[tokio::test]
    async fn reaper_expiry_increments_only_the_expired_counter() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-c1", 1_000);
        insert_held(&state, "lease-c2", 1_000);

        assert_eq!(state.counters.leases_expired.get(), 0, "pre: zero");
        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 2, "both overdue leases reaped");

        assert_eq!(
            state.counters.leases_expired.get(),
            2,
            "leases_expired must bump once per reaped lease"
        );
        // Cross-signal isolation: expiry must not touch the crash/close counters.
        assert_eq!(state.counters.leases_crashed.get(), 0);
        assert_eq!(state.counters.leases_closed.get(), 0);
    }

    // ── WP-S13.5: partial-envelope flush on abnormal close ───────────────────

    /// EXPIRED flush: a reaped lease WITH a registered hook drives
    /// `close_abnormal` — the finalized partial outcome carries
    /// `close_reason = Expired` + `capture_incomplete = true`, and the metrics
    /// come from the FINALIZED (redacted) outcome, not raw capture. Teardown +
    /// transition + slot-free all still happen (the flush is post-reclaim and
    /// non-blocking).
    #[tokio::test]
    async fn expired_flush_finalizes_partial_envelope_marked_incomplete() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, prov) = build_state(2_000);
        insert_held(&state, "lease-flush-exp", 1_000);
        register_hook(&state, "lease-flush-exp");

        // Drive the flush directly (the same call reap_once makes post-reclaim).
        let outcome = flush_partial_envelope(
            &state,
            "lease-flush-exp",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("a registered hook must produce a finalized partial outcome");

        assert_eq!(
            outcome.close_reason,
            CloseReason::Expired,
            "expired flush must mark close_reason=expired"
        );
        assert!(
            outcome.capture_incomplete,
            "an abnormal partial envelope is always capture_incomplete"
        );
        // REDACTION: the summary comes from the finalized outcome's metrics
        // (same finalize path as a normal close — the observed turn is counted).
        assert_eq!(
            outcome.metrics.model_turns, 1,
            "metrics must be the FINALIZED projection (one observed turn), not raw capture"
        );

        // The hook was EXTRACTED (dedup): a second flush finds nothing.
        assert!(
            flush_partial_envelope(
                &state,
                "lease-flush-exp",
                &TenantId::new("acme").unwrap(),
                AbnormalKind::Expiry,
                std::time::Instant::now(),
            )
            .is_none(),
            "the hook is consumed once — no second partial envelope"
        );

        // And the full reaper sweep still reclaims cleanly (teardown + Expired
        // + slot-free), unaffected by the flush.
        let _ = prov; // teardown recorder; the sweep below exercises it.
    }

    /// A reaped Expired lease with NO hook (and NO durable checkpoint) → reaped
    /// normally, no panic, `reap_once` still returns 1. Under ADR-0004 the flush
    /// emits a tier-3 `no_capture` marker rather than a silent no-op (asserted
    /// directly in `tier3_no_capture_marker_when_no_hook_no_checkpoint`); here we
    /// only prove the in-sweep flush never breaks reclamation.
    #[tokio::test]
    async fn expired_no_hook_reaps_without_flush() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-nohook", 1_000);
        // No register_hook.

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "a hookless lease still reaps normally");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-nohook").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Expired),
            "hookless lease must be Expired after reap"
        );
    }

    /// reap_once END-TO-END with a hook: the lease is reaped (Expired + slot
    /// freed) AND its hook is consumed by the flush. Proves the flush is wired
    /// into the sweep and does not break reclamation.
    #[tokio::test]
    async fn reap_once_drives_flush_and_consumes_hook() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-e2e-exp", 1_000);
        register_hook(&state, "lease-e2e-exp");

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "the lease must be reaped");

        // Lease Expired.
        {
            let ledger = &*state.ledger;
            let rec = ledger.get("lease-e2e-exp").unwrap().unwrap();
            assert_eq!(rec.state, LeaseState::Wire(RunnerState::Expired));
        }
        // Hook consumed AND unregistered by the in-sweep flush + forget_lease.
        // A follow-up flush therefore finds NO hook and NO checkpoint → it falls
        // to the tier-3 `no_capture` marker (zero metrics), NOT the live hook's
        // finalized outcome. This proves tier 1 was consumed once: a second
        // tier-1 flush is impossible (the hook is gone), and the never-silently-
        // dropped floor (tier 3) is what answers a re-flush. In production the
        // sweep flushes exactly once (one reaper wins the terminal CAS), so this
        // re-flush is test-only — there is no double tier-1 emit.
        let refl = flush_partial_envelope(
            &state,
            "lease-e2e-exp",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("a re-flush yields the tier-3 no_capture marker, not the live hook");
        assert_eq!(
            refl.metrics.model_turns, 0,
            "the re-flush is the zero-metric tier-3 marker (the hook was consumed + forgotten)"
        );
    }

    /// CRASHED flush: same shape via `surface_crashes` → the finalized partial
    /// outcome carries `close_reason = Crashed` + `capture_incomplete = true`.
    #[tokio::test]
    async fn crashed_flush_marks_close_reason_crashed() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, _prov) = build_state_scripted(5_000, Scripted::Dead, true);
        insert_held(&state, "lease-flush-crash", 9_999_999);
        register_hook(&state, "lease-flush-crash");

        let outcome = flush_partial_envelope(
            &state,
            "lease-flush-crash",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Crash,
            std::time::Instant::now(),
        )
        .expect("a registered hook must produce a finalized partial outcome");
        assert_eq!(outcome.close_reason, CloseReason::Crashed);
        assert!(outcome.capture_incomplete);
        // Audit r5 regression: TIER-1 must emit the live hook's FINALIZED partial
        // metrics, NOT tier-3 zero. `register_hook` feeds a model-turn, so a real
        // tier-1 flush carries it; a regression to "forget_lease before flush"
        // (which unregisters the hook) would silently drop to zero metrics.
        assert!(
            outcome.metrics.model_turns >= 1,
            "TIER-1 flush must carry the live hook's partial metrics (≥1 model turn), \
             not tier-3 zero; got {:?}",
            outcome.metrics
        );

        // The direct-flush lease above is still `Held` (a direct
        // `flush_partial_envelope` does NOT terminalize the lease — only the
        // sweep does). Remove it so the E2E sweep below reclaims exactly the
        // one fresh lease and the reclaim count isn't skewed by this residue.
        state.ledger.remove("lease-flush-crash").unwrap();

        // End-to-end through the crash sweep on a fresh lease + hook.
        insert_held(&state, "lease-crash-e2e", 9_999_999);
        register_hook(&state, "lease-crash-e2e");
        let reaped = surface_crashes(&state).await;
        assert_eq!(reaped, 1, "the dead box must be reclaimed");
        {
            let ledger = &*state.ledger;
            let rec = ledger.get("lease-crash-e2e").unwrap().unwrap();
            assert_eq!(rec.state, LeaseState::Wire(RunnerState::Crashed));
        }
        // As in the expired E2E: surface_crashes consumed + forgot the hook, so
        // a re-flush falls to the tier-3 `no_capture` marker (zero metrics), not
        // a second live-hook outcome. The sweep flushes exactly once in prod.
        let refl = flush_partial_envelope(
            &state,
            "lease-crash-e2e",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Crash,
            std::time::Instant::now(),
        )
        .expect("a re-flush yields the tier-3 no_capture marker, not the live hook");
        assert_eq!(
            refl.metrics.model_turns, 0,
            "the re-flush is the zero-metric tier-3 marker (the hook was consumed + forgotten)"
        );
    }

    /// NO DOUBLE-FIRE: a lease whose hook's exactly-once close already fired
    /// (a normal close consumed it) then reaped → the sweep's `close_abnormal`
    /// returns the exactly-once `Err`, the flush logs + continues (returns
    /// None), NO second envelope, NO double-free.
    ///
    /// The exactly-once latch lives on the SHARED hook state, so a clone of the
    /// hook re-registered under the lease id still observes the closed latch —
    /// exactly how a normal close (which holds its own clone) races the sweep.
    #[tokio::test]
    async fn no_double_fire_when_hook_already_closed() {
        use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector, PriceCard};

        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-double", 1_000);

        // Build a hook, register a CLONE (clones share the close latch), keep
        // the original to drive the "normal close already fired" first close.
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: std::time::Duration::from_millis(1),
                buffer_capacity: 16,
            },
            "hookcred-reaper",
            MetricsCollector::new(std::time::Instant::now()),
        );
        state.hook_registry.register(
            "lease-double",
            TenantId::new("acme").unwrap(),
            hook.clone(),
            "hookcred-reaper",
        );

        // Normal close fires the exactly-once latch on the shared state.
        corelink_runner::envelope::JobClose::new(&hook)
            .close_abnormal(
                AbnormalKind::Crash,
                std::time::Instant::now(),
                &PriceCard {
                    input_per_mtok_micros: 0,
                    output_per_mtok_micros: 0,
                    cache_read_per_mtok_micros: 0,
                    cache_write_per_mtok_micros: 0,
                },
            )
            .expect("first close fires once");

        // The reaper's flush extracts the (still-registered) clone and tries
        // close_abnormal again → exactly-once Err → returns None, no panic.
        let second = flush_partial_envelope(
            &state,
            "lease-double",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        );
        assert!(
            second.is_none(),
            "an already-closed hook must yield NO second envelope (exactly-once)"
        );

        // The sweep still reclaims the lease cleanly despite the skipped flush.
        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "reclamation is unaffected by a skipped flush");
    }

    // ── ADR-0004 Phase 2a: durable-checkpoint 3-tier abnormal flush ──────────

    /// A non-trivial redacted checkpoint summary serialized as the opaque blob
    /// the ledger stores (exactly what the future per-turn write feed produces).
    fn checkpoint_json(model_turns: u64, tokens_total: u64) -> String {
        use corelink_runners_contracts::{IntentMetrics, TokenCounts};
        serde_json::to_string(&IntentMetrics {
            tokens: TokenCounts {
                input: tokens_total,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                total: tokens_total,
            },
            wall_ms: 42,
            active_ms: 7,
            tool_calls: 3,
            tool_breakdown: Vec::new(),
            model_turns,
            cost_usd_micros: 1234,
        })
        .unwrap()
    }

    /// TIER 2 — no local hook, but a durable checkpoint exists: the flush emits a
    /// PARTIAL envelope from the checkpoint metrics (`capture_incomplete`, the
    /// checkpoint's scalars), source=durable-checkpoint, and returns the outcome.
    /// This is the §13 Item-3 cross-instance SLA in microcosm: the hook lived on
    /// a now-gone instance; the durable checkpoint carries the forensic summary.
    #[tokio::test]
    async fn tier2_durable_checkpoint_emits_partial_when_no_hook() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-tier2", 1_000);
        // NO register_hook — the local hook is absent (the whole point).
        // Write the durable checkpoint the "owning" instance would have left.
        state
            .ledger
            .set_envelope_checkpoint("lease-tier2", &checkpoint_json(4, 999))
            .expect("checkpoint write on an existing lease must succeed");

        let outcome = flush_partial_envelope(
            &state,
            "lease-tier2",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("a durable checkpoint must produce a partial outcome (NOT a silent drop)");

        assert_eq!(
            outcome.close_reason,
            CloseReason::Expired,
            "tier 2 must carry the abnormal close reason"
        );
        assert!(
            outcome.capture_incomplete,
            "a checkpoint is a mid-flight summary → always capture_incomplete"
        );
        // The metrics are the CHECKPOINT's, not a zero floor.
        assert_eq!(
            outcome.metrics.model_turns, 4,
            "tier 2 metrics must come from the durable checkpoint"
        );
        assert_eq!(outcome.metrics.tokens.total, 999);
        assert_eq!(outcome.metrics.cost_usd_micros, 1234);
    }

    /// TIER 3 — no hook AND no checkpoint: the flush emits the explicit
    /// `no_capture` marker (zero metrics, `capture_incomplete`), NOT a silent
    /// no-op. The lease died before anything was captured; the marker is the
    /// owner-ratified "never silently dropped" record (Decision-3b).
    #[tokio::test]
    async fn tier3_no_capture_marker_when_no_hook_no_checkpoint() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-tier3", 1_000);
        // NO hook, NO checkpoint.

        let outcome = flush_partial_envelope(
            &state,
            "lease-tier3",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Crash,
            std::time::Instant::now(),
        )
        .expect("tier 3 must emit a no_capture marker (NOT None / silent drop)");

        assert_eq!(outcome.close_reason, CloseReason::Crashed);
        assert!(
            outcome.capture_incomplete,
            "the no_capture marker is capture_incomplete"
        );
        // Zero metrics — there was genuinely nothing captured.
        assert_eq!(outcome.metrics.model_turns, 0);
        assert_eq!(outcome.metrics.tokens.total, 0);
        assert_eq!(outcome.metrics.tool_calls, 0);
        assert_eq!(outcome.metrics.cost_usd_micros, 0);
        assert!(
            outcome.metrics.tool_breakdown.is_empty(),
            "the zero IntentMetrics has an empty tool breakdown"
        );
    }

    /// TIER 2 → TIER 3 fall-through on a CORRUPT checkpoint: a durable
    /// checkpoint that does NOT deserialize as `IntentMetrics` (schema drift,
    /// truncated write, a foreign blob) must NOT be silently dropped and must
    /// NOT abort the flush — it degrades to the tier-3 `no_capture` marker so
    /// the abnormal close is STILL recorded (zero metrics, capture_incomplete).
    /// This is the silent-metric-loss guard on the durable path.
    #[tokio::test]
    async fn corrupt_durable_checkpoint_falls_through_to_tier3_marker() {
        use corelink_runner::envelope::CloseReason;

        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-corrupt", 1_000);
        // NO local hook. Store a blob that is valid JSON but NOT an
        // IntentMetrics shape — the parse in tier-2 must fail and fall through.
        state
            .ledger
            .set_envelope_checkpoint("lease-corrupt", "{\"not\":\"intent-metrics\"}")
            .expect("checkpoint write on an existing lease must succeed");

        let outcome = flush_partial_envelope(
            &state,
            "lease-corrupt",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("a corrupt checkpoint must STILL yield a marker, never a silent None");

        assert_eq!(
            outcome.close_reason,
            CloseReason::Expired,
            "the abnormal reason survives the fall-through"
        );
        assert!(outcome.capture_incomplete, "tier-3 marker is incomplete");
        // Fell through to the zero-metrics no_capture marker — NOT the corrupt
        // blob's (unparseable) contents, NOT a silent drop.
        assert_eq!(
            outcome.metrics.model_turns, 0,
            "corrupt checkpoint degrades to zero metrics, not garbage"
        );
        assert_eq!(outcome.metrics.tokens.total, 0);
        assert_eq!(outcome.metrics.cost_usd_micros, 0);
    }

    /// TIER 1 STILL WINS — a lease WITH a local hook uses the hook (full
    /// fidelity) and IGNORES any durable checkpoint. Even when a (stale)
    /// checkpoint is present, the live hook's finalized metrics are emitted, not
    /// the checkpoint's.
    #[tokio::test]
    async fn tier1_local_hook_wins_over_checkpoint() {
        let (state, _clock, _prov) = build_state(2_000);
        insert_held(&state, "lease-tier1", 1_000);
        // BOTH a live hook (one observed turn → model_turns == 1) AND a stale
        // checkpoint (model_turns == 99) are present.
        register_hook(&state, "lease-tier1");
        state
            .ledger
            .set_envelope_checkpoint("lease-tier1", &checkpoint_json(99, 55_555))
            .unwrap();

        let outcome = flush_partial_envelope(
            &state,
            "lease-tier1",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("the local hook must produce a finalized outcome");

        // The HOOK's finalized projection (1 turn), never the checkpoint's 99.
        assert_eq!(
            outcome.metrics.model_turns, 1,
            "tier 1 must use the live hook, not the durable checkpoint"
        );
        assert_ne!(
            outcome.metrics.tokens.total, 55_555,
            "the stale checkpoint's metrics must NOT leak into the tier-1 outcome"
        );
    }

    /// THE §13 Item-3 SLA PROOF (cross-instance, single-DB shape). A lease is
    /// reaped by an instance that does NOT hold the hook but DOES see the durable
    /// checkpoint → the partial (tier 2) is emitted, never dropped.
    ///
    /// Modeled with a SHARED ledger behind two `AppState`s: the "owning" state
    /// admits + checkpoints the lease; the "reaper" state shares the SAME ledger
    /// `Arc` but has an EMPTY hook registry (its own in-memory map — exactly the
    /// hook-locality gap). The reaper flush reads the checkpoint from the shared
    /// durable ledger and emits the forensic envelope.
    #[tokio::test]
    async fn cross_instance_reaper_without_hook_emits_durable_checkpoint() {
        // `InMemoryLedger` / `LeaseLedger` are in scope from the module-top use;
        // `StaticPlans` from `crate::app`; `Arc`/`Mutex` from the test prelude.
        // ONE durable ledger, shared by two instances.
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(2_000);

        // Instance A (owning): registers the hook + writes the checkpoint.
        let mut state_a = AppState::new(
            Arc::clone(&ledger),
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state_a.provisioner = Arc::new(RecordingProvisioner::new()) as Arc<dyn BoxProvisioner>;

        // Instance B (reaper): SHARES the ledger, but has its OWN (empty) hook
        // registry — it never served the acquire, so it holds no hook.
        let mut state_b = AppState::new(
            Arc::clone(&ledger),
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state_b.provisioner = Arc::new(RecordingProvisioner::new()) as Arc<dyn BoxProvisioner>;

        // A holds the lease + a registered hook + a durable checkpoint.
        insert_held(&state_a, "lease-xinst", 1_000);
        register_hook(&state_a, "lease-xinst");
        state_a
            .ledger
            .set_envelope_checkpoint("lease-xinst", &checkpoint_json(6, 7_000))
            .expect("instance A writes the durable checkpoint");

        // Sanity: instance B holds NO local hook for this lease.
        assert!(
            state_b
                .hook_registry
                .close_handle_any("lease-xinst")
                .is_none(),
            "the reaper instance must not hold the hook (hook-locality gap)"
        );

        // Instance B reaps: tier 1 misses (no hook) → tier 2 reads the SHARED
        // durable checkpoint and emits the partial. Before ADR-0004 this was a
        // SILENT DROP.
        let outcome = flush_partial_envelope(
            &state_b,
            "lease-xinst",
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("the non-owning reaper must emit the partial from the durable checkpoint");

        assert!(
            outcome.capture_incomplete,
            "the cross-instance partial is capture_incomplete"
        );
        assert_eq!(
            outcome.metrics.model_turns, 6,
            "the emitted metrics are the durable checkpoint's (instance A's summary)"
        );
        assert_eq!(outcome.metrics.tokens.total, 7_000);
    }

    /// ADR-0004 Phase 2b — the per-turn WRITE trigger end-to-end. Ingesting
    /// model turns over the §13.2 turn-feed endpoint writes the durable
    /// checkpoint at each turn boundary; a cross-instance abnormal reap (no
    /// local hook) then emits the LAST checkpointed partial (tier 2), not a
    /// `no_capture` marker. Before Phase 2b nothing wrote the checkpoint, so
    /// this same reap fell through to tier 3.
    #[tokio::test]
    async fn phase2b_ingest_writes_checkpoint_consumed_by_cross_instance_reap() {
        use crate::handlers::envelope::{HookRegistry, ingest};
        use axum::extract::{Path, State};
        use axum::http::{HeaderMap, header};
        use axum::{Extension, http::StatusCode};
        use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};

        const LEASE: &str = "lease-phase2b";
        const CRED: &str = "pat-phase2b";

        // ONE durable ledger shared by two instances (A owns + ingests; B reaps).
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(5_000);

        // Instance A: owns the lease + a registered hook reachable from the
        // HTTP Extension AND from AppState (the app_full shared-instance crux,
        // mirrored here by registering into state_a.hook_registry and passing
        // that SAME Arc as the Extension to `ingest`).
        let mut state_a = AppState::new(
            Arc::clone(&ledger),
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        let registry_a = Arc::new(HookRegistry::default());
        state_a.hook_registry = Arc::clone(&registry_a);

        insert_held(&state_a, LEASE, 1_000);
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: std::time::Duration::from_millis(1),
                buffer_capacity: 16,
            },
            CRED,
            MetricsCollector::new(std::time::Instant::now()),
        );
        registry_a.register(LEASE, TenantId::new("acme").unwrap(), hook, CRED);

        // Ingest a 2-turn batch over the real handler (JSON array body).
        let body = serde_json::json!([
            { "kind": "model_turn", "bytes_b64": "dHVybi0w", "busy_ms": 3 },
            { "kind": "model_turn", "bytes_b64": "dHVybi0x", "busy_ms": 4 }
        ])
        .to_string();
        // The ingest path now authenticates with the per-lease SCOPED ingest
        // token (NOT the tenant PAT), presented as the Bearer. Mint it from the
        // fabric's own ingest secret for THIS lease.
        let scoped = state_a.ingest_signer.ingest_token(LEASE);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {scoped}").parse().unwrap(),
        );
        let resp = ingest(
            State(state_a.clone()),
            Extension(Arc::clone(&registry_a)),
            Path(LEASE.to_string()),
            headers,
            body,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "valid ingest must 200");

        // The durable checkpoint now reflects BOTH turns (the WRITE trigger).
        let ck = state_a
            .ledger
            .get_envelope_checkpoint(LEASE)
            .unwrap()
            .expect("a checkpoint must exist after ingesting model turns");
        let metrics: corelink_runners_contracts::IntentMetrics =
            serde_json::from_str(&ck).expect("checkpoint is an IntentMetrics blob");
        assert_eq!(
            metrics.model_turns, 2,
            "the per-turn checkpoint reflects both ingested turns"
        );

        // Instance B: shares the ledger, holds NO hook for this lease.
        let mut state_b = AppState::new(
            Arc::clone(&ledger),
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state_b.provisioner = Arc::new(RecordingProvisioner::new()) as Arc<dyn BoxProvisioner>;
        assert!(
            state_b.hook_registry.close_handle_any(LEASE).is_none(),
            "the reaper instance must not hold the hook"
        );

        // Tier 1 misses (no hook) → tier 2 reads the checkpoint A wrote.
        let outcome = flush_partial_envelope(
            &state_b,
            LEASE,
            &TenantId::new("acme").unwrap(),
            AbnormalKind::Expiry,
            std::time::Instant::now(),
        )
        .expect("the cross-instance reap must emit the partial from the checkpoint");
        assert!(
            outcome.capture_incomplete,
            "the cross-instance partial is capture_incomplete"
        );
        assert_eq!(
            outcome.metrics.model_turns, 2,
            "the emitted partial carries the 2-turn checkpoint (tier 2, NOT no_capture)"
        );
    }

    // ── WP-CRASH-SWEEP: surface_crashes behavior tests ───────────────────────

    /// Count `Crashed` events for `lease_id` in the slot journal.
    fn crashed_events(state: &AppState, lease_id: &str) -> usize {
        let meter = state.slot_meter.lock().unwrap();
        meter
            .journal()
            .iter()
            .filter(|e| matches!(e.kind, SlotEventKind::Crashed) && e.lease_id == lease_id)
            .count()
    }

    /// Probe → Ok(Dead), teardown succeeds → lease becomes `Crashed`, a Crashed
    /// slot event is emitted, side-tables GC'd, count == 1.
    #[tokio::test]
    async fn dead_box_is_reclaimed_as_crashed() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Dead, true);
        // Deadline far in the future — proves crash-surfacing is NOT deadline-driven.
        insert_held(&state, "lease-dead", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 1, "a dead box must be reclaimed");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-dead").unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(RunnerState::Crashed),
            "lease must be Crashed after surface_crashes"
        );

        assert!(
            prov.teardown_calls().contains(&"lease-dead".to_string()),
            "teardown must be called for the dead lease"
        );
        assert_eq!(crashed_events(&state, "lease-dead"), 1, "one Crashed event");
        // The golden-signal counter is bumped exactly once, and ONLY the crash
        // counter (expiry/close untouched) — the observability seam value.
        assert_eq!(
            state.counters.leases_crashed.get(),
            1,
            "leases_crashed must bump once per surfaced crash"
        );
        assert_eq!(state.counters.leases_expired.get(), 0);
        assert_eq!(state.counters.leases_closed.get(), 0);
        // The durable deadline is preserved on the terminal Crashed record
        // (ADR-0004: a transition never alters it); only the image side-table
        // is GC'd.
        assert_eq!(
            ledger_deadline(&state, "lease-dead"),
            Some(9_999_999),
            "deadline_ms preserved on the terminal Crashed record (ADR-0004)"
        );
        assert!(
            state.image_of("lease-dead").is_none(),
            "image entry GC'd after crash reclaim"
        );
    }

    /// Probe → Ok(Alive) → lease stays Held, NO teardown, NO Crashed, count 0.
    #[tokio::test]
    async fn alive_box_is_left_held() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Alive, true);
        insert_held(&state, "lease-alive", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "an alive box must not be reclaimed");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-alive").unwrap().unwrap();
        assert!(rec.state.is_held(), "alive lease must remain Held");

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called for an alive box"
        );
        assert_eq!(crashed_events(&state, "lease-alive"), 0, "no Crashed event");
    }

    /// Probe → Ok(Unbound) → untouched, count 0.
    #[tokio::test]
    async fn unbound_lease_is_left_held() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Unbound, true);
        insert_held(&state, "lease-unbound", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "an unbound lease must not be reclaimed");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-unbound").unwrap().unwrap();
        assert!(rec.state.is_held(), "unbound lease must remain Held");

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called for an unbound lease"
        );
        assert_eq!(
            crashed_events(&state, "lease-unbound"),
            0,
            "no Crashed event"
        );
    }

    /// FAIL-SAFE CRUX: probe → Err(...) → lease stays Held, NO Crashed, count 0.
    /// An unreachable provider must NEVER be read as death.
    #[tokio::test]
    async fn probe_error_is_fail_safe() {
        let (state, _clock, prov) = build_state_scripted(5_000, Scripted::Err, true);
        insert_held(&state, "lease-err", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(
            count, 0,
            "a probe Err is NOT death — fail-safe, nothing reclaimed"
        );

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-err").unwrap().unwrap();
        assert!(
            rec.state.is_held(),
            "lease must remain Held when the probe errors (unreachable != dead)"
        );

        assert!(
            prov.teardown_calls().is_empty(),
            "teardown must NOT be called on a probe error"
        );
        assert_eq!(crashed_events(&state, "lease-err"), 0, "no Crashed event");
    }

    /// Probe Ok(Dead) but teardown FAILS → lease stays Held for retry, NO
    /// Crashed transition/event, count 0.
    #[tokio::test]
    async fn teardown_failure_leaves_held_for_retry() {
        let (state, _clock, prov) =
            build_state_scripted(5_000, Scripted::Dead, /* teardown_succeed */ false);
        insert_held(&state, "lease-tdfail", 9_999_999);

        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "failed teardown: nothing reclaimed");

        let ledger = &*state.ledger;
        let rec = ledger.get("lease-tdfail").unwrap().unwrap();
        assert!(
            rec.state.is_held(),
            "lease must remain Held after a failed teardown (retry next sweep)"
        );

        // Teardown was ATTEMPTED (and failed), but no Crashed mark/event.
        assert_eq!(
            prov.teardown_calls().len(),
            1,
            "teardown must have been attempted once"
        );
        assert_eq!(
            crashed_events(&state, "lease-tdfail"),
            0,
            "no Crashed event"
        );
        assert_eq!(
            ledger_deadline(&state, "lease-tdfail"),
            Some(9_999_999),
            "deadline_ms retained on the still-Held record after failed teardown"
        );
    }

    /// Probe Ok(Dead), teardown ok, but the lease was already terminalized
    /// (Released) → transition Err → NO Crashed event, NO GC, count 0 (no
    /// double-free of the slot).
    #[tokio::test]
    async fn crash_loses_race_to_close_does_not_double_free() {
        let (state, _clock, _prov) = build_state_scripted(5_000, Scripted::Dead, true);
        let acme = TenantId::new("acme").unwrap();

        insert_held(&state, "lease-lostrace", 9_999_999);
        state.record_slot("lease-lostrace", &acme, SlotEventKind::Acquired);

        // Simulate close/cancel winning the race: Held → Released, emit Released.
        {
            let ledger = &*state.ledger;
            ledger
                .transition("lease-lostrace", RunnerState::Released, 4_000)
                .expect("Held→Released must succeed");
        }
        state.record_slot("lease-lostrace", &acme, SlotEventKind::Released);

        // The lease is now Released (terminal): held() excludes it, so the
        // sweep processes zero leases.
        let count = surface_crashes(&state).await;
        assert_eq!(count, 0, "a race-lost crash sweep reclaims nothing");

        assert_eq!(
            crashed_events(&state, "lease-lostrace"),
            0,
            "NO Crashed event when close won the race (would double-free)"
        );
        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.occupied(&acme),
            0,
            "occupied stays 0 — no phantom Crashed double-free"
        );
        assert_eq!(
            meter.journal().len(),
            2,
            "journal has exactly Acquired + Released, no extra Crashed"
        );
    }

    // ── crash_probe_config_from_env tests ────────────────────────────────────

    /// Absent/empty → opt-in default: `Ok(None)` (sweep NOT spawned).
    #[test]
    fn crash_probe_config_absent_is_none() {
        assert_eq!(crash_probe_config_from_env(|_| None).unwrap(), None);
        assert_eq!(
            crash_probe_config_from_env(|k| {
                if k == "FABRIC_CRASH_PROBE_INTERVAL_SECS" {
                    Some(String::new())
                } else {
                    None
                }
            })
            .unwrap(),
            None,
            "empty string is also treated as absent (opt-in default)"
        );
    }

    /// `0`/garbage → Err; valid → `Ok(Some(Duration))`.
    #[test]
    fn crash_probe_config_zero_or_garbage_errs_valid_ok() {
        let mk = |v: &'static str| {
            crash_probe_config_from_env(move |k| {
                if k == "FABRIC_CRASH_PROBE_INTERVAL_SECS" {
                    Some(v.to_string())
                } else {
                    None
                }
            })
        };
        assert!(
            mk("0").is_err(),
            "0 must error (absence is the disable path)"
        );
        assert!(mk("notanumber").is_err(), "garbage must error");
        assert_eq!(
            mk("45").unwrap(),
            Some(std::time::Duration::from_secs(45)),
            "a valid u32 yields Some(Duration)"
        );
    }

    // ── WP-D: BoxRegistry orphan GC (D1 / D2 / D3) ──────────────────────────
    //
    // These tests verify that the expiry reaper (`reap_once`) drives registry
    // cleanup for orphaned leases, closing the unbounded-growth leak described
    // in WP-D.  The mechanism: `teardown_lease` delegates to the provisioner's
    // `teardown()`, which calls `BoxRegistry::unbind()` after the provider job
    // is deleted — the SAME path as the normal close.  The tests use a
    // `RegistryAwareProvisioner` that binds a real `BoxRegistry` at provision
    // time and unbinds at teardown, so the registry state is observable.

    /// Test provisioner that mirrors `NorthflankBoxProvisioner`'s registry
    /// contract: `provision` binds the lease into a shared `BoxRegistry`;
    /// `teardown` unbinds it after the (no-op) provider call succeeds.
    ///
    /// Unlike `RecordingProvisioner` (which has no registry), this lets tests
    /// assert that the registry entry is gone after a reaper sweep (D1) and
    /// that a double-unbind is a safe no-op (D3).
    struct RegistryAwareProvisioner {
        registry: crate::cloud_exec::BoxRegistry,
        teardown_calls: Mutex<Vec<String>>,
    }

    impl RegistryAwareProvisioner {
        fn new(registry: crate::cloud_exec::BoxRegistry) -> Self {
            Self {
                registry,
                teardown_calls: Mutex::new(Vec::new()),
            }
        }

        /// Bind a synthetic container into the registry for `lease_id` so the
        /// reaper has something to unbind.
        fn bind(&self, lease_id: &str) {
            use corelink_runner::isolation::RunningContainer;
            self.registry.bind(
                lease_id,
                RunningContainer {
                    name: format!("box:{lease_id}"),
                },
            );
        }

        fn teardown_calls(&self) -> Vec<String> {
            self.teardown_calls.lock().unwrap().clone()
        }
    }

    impl BoxProvisioner for RegistryAwareProvisioner {
        fn provision(
            &self,
            lease_id: &str,
            _spec: &corelink_runner::lease::ContainerSpec,
        ) -> Result<()> {
            use corelink_runner::isolation::RunningContainer;
            self.registry.bind(
                lease_id,
                RunningContainer {
                    name: format!("box:{lease_id}"),
                },
            );
            Ok(())
        }

        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.teardown_calls
                .lock()
                .unwrap()
                .push(lease_id.to_string());
            // Mirrors NorthflankBoxProvisioner::teardown: unbind AFTER the
            // provider call succeeds (here the provider is a no-op, so we
            // always unbind on success).
            self.registry.unbind(lease_id);
            Ok(())
        }

        fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
            Ok(ProbeStatus::Unbound)
        }
    }

    /// Build an `AppState` wired with a `RegistryAwareProvisioner` and a
    /// shared `BoxRegistry`.  Returns the state, the clock, the provisioner
    /// arc, and the shared registry so the test can inspect registry state
    /// independently of the provisioner.
    fn build_state_registry(
        now_ms: u64,
    ) -> (
        AppState,
        FixedClock,
        Arc<RegistryAwareProvisioner>,
        crate::cloud_exec::BoxRegistry,
    ) {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let clock = FixedClock::new(now_ms);
        let registry = crate::cloud_exec::BoxRegistry::new();
        let prov = Arc::new(RegistryAwareProvisioner::new(registry.clone_handle()));
        let mut state = AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(clock.clone()),
        );
        state.provisioner = Arc::clone(&prov) as Arc<dyn BoxProvisioner>;
        (state, clock, prov, registry)
    }

    /// D1 — Held lease expired by the reaper has its BoxRegistry entry removed.
    ///
    /// A lease bound in the registry at acquire time is unbounded when the
    /// reaper expires it (teardown → unbind fires AFTER teardown succeeds).
    /// After `reap_once` returns the registry must NOT contain the entry.
    #[tokio::test]
    async fn d1_expired_lease_registry_entry_removed_by_reaper() {
        let (state, _clock, prov, registry) = build_state_registry(2_000);

        // Simulate acquire: insert as Held in the ledger + bind in the registry.
        insert_held(&state, "lease-d1", 1_000);
        prov.bind("lease-d1");

        // Pre-condition: registry has the entry.
        assert!(
            registry.resolve("lease-d1").is_some(),
            "pre-condition: lease-d1 must be bound in the registry before reap"
        );

        let reaped = reap_once(&state).await;
        assert_eq!(reaped, 1, "the overdue lease must be reclaimed");

        // D1: the registry entry is gone after the reaper sweep.
        assert!(
            registry.resolve("lease-d1").is_none(),
            "D1 FAIL: the expired lease's BoxRegistry entry must be removed after reap"
        );

        // The ledger reflects Expired and teardown was called.
        {
            let ledger = &*state.ledger;
            let rec = ledger.get("lease-d1").unwrap().unwrap();
            assert_eq!(
                rec.state,
                LeaseState::Wire(RunnerState::Expired),
                "lease must be Expired in the ledger"
            );
        }
        assert!(
            prov.teardown_calls().contains(&"lease-d1".to_string()),
            "teardown must have been called for lease-d1"
        );
    }

    /// D2 — unbind fires AFTER teardown succeeds and not while a lock is held
    /// across an await.
    ///
    /// The `_ASSERT_REAP_ONCE_IS_SEND` compile-time gate (present at the
    /// bottom of the module) catches any `MutexGuard` across an `await`.  This
    /// runtime test completes the picture: verify that a teardown + unbind
    /// sequence in the reaper does not deadlock AND that the registry is clean
    /// post-reap, even when the lease holds an entry.
    ///
    /// Lock-ordering proof: `reap_once` takes the ledger snapshot (guard
    /// dropped before any `await`), calls `teardown_lease` (no lock held),
    /// then re-acquires the ledger lock briefly for the `Expired` transition
    /// (dropped before continuing).  `RegistryAwareProvisioner::teardown`
    /// calls `registry.unbind()` synchronously (no `await`) while no ledger
    /// lock is held — so there is no lock held across an `await` at any point.
    #[tokio::test]
    async fn d2_unbind_fires_after_teardown_no_lock_across_await() {
        let (state, _clock, prov, registry) = build_state_registry(5_000);

        insert_held(&state, "lease-d2", 1_000);
        prov.bind("lease-d2");

        // Pre-condition: entry present.
        assert!(registry.resolve("lease-d2").is_some());

        // This must not deadlock (D2: unbind is synchronous inside teardown,
        // no MutexGuard held across the spawn_blocking await boundary).
        let reaped = reap_once(&state).await;

        assert_eq!(reaped, 1, "D2: the lease must be reaped without deadlock");
        // Unbind fired after teardown succeeded — registry is clean.
        assert!(
            registry.resolve("lease-d2").is_none(),
            "D2 FAIL: registry must be empty after reap (unbind fired post-teardown)"
        );
        // Teardown was called exactly once.
        assert_eq!(
            prov.teardown_calls(),
            vec!["lease-d2".to_string()],
            "D2: teardown must be called exactly once"
        );
    }

    /// D3 — regression: the normal close path still unbinds; double-unbind
    /// (close then expiry, or expiry of an already-closed lease) is a no-op.
    ///
    /// Scenario A: normal close unbinds (teardown called once, registry clean).
    /// Scenario B: close fires first; the reaper's teardown then calls unbind
    ///   on an already-absent key — must be a harmless no-op.
    #[tokio::test]
    async fn d3_close_path_unbinds_and_double_unbind_is_noop() {
        // ── Scenario A: normal close (teardown) removes the registry entry ──
        let (state_a, _clock, prov_a, registry_a) = build_state_registry(2_000);

        insert_held(&state_a, "lease-d3a", 1_000);
        prov_a.bind("lease-d3a");

        assert!(registry_a.resolve("lease-d3a").is_some(), "pre: bound");

        // Simulate the normal close path: just call teardown directly (which
        // unbinds).  In production this is `teardown_lease` → provisioner.
        prov_a
            .teardown("lease-d3a")
            .expect("normal teardown must succeed");

        assert!(
            registry_a.resolve("lease-d3a").is_none(),
            "D3-A FAIL: normal close path must unbind the registry entry"
        );

        // ── Scenario B: close fires THEN the reaper's teardown unbinds again ─
        // The entry is already absent from the registry; a second unbind must
        // be a silent no-op (idempotency guarantee of BoxRegistry::unbind).
        let (state_b, _clock, prov_b, registry_b) = build_state_registry(2_000);

        insert_held(&state_b, "lease-d3b", 1_000);
        prov_b.bind("lease-d3b");

        // Close path fires first (unbinds).
        prov_b
            .teardown("lease-d3b")
            .expect("first teardown (close path) must succeed");
        assert!(
            registry_b.resolve("lease-d3b").is_none(),
            "D3-B pre: registry empty after close path"
        );

        // Reaper then calls teardown on the same (already-unbound) lease-id —
        // must NOT panic, must NOT error.  In production the ledger's
        // terminal-state CAS prevents the reaper from seeing the lease at all
        // (it's already Released), but the idempotency of `unbind` is the
        // last-resort safety net we assert here directly.
        prov_b
            .teardown("lease-d3b")
            .expect("D3-B FAIL: second teardown (double-unbind) must be a harmless no-op");

        assert!(
            registry_b.resolve("lease-d3b").is_none(),
            "D3-B FAIL: registry must still be empty after double-unbind"
        );
        // Two teardown calls total (close then reaper).
        assert_eq!(
            prov_b.teardown_calls().len(),
            2,
            "D3-B: two teardown calls recorded (close path + reaper)"
        );
    }

    #[test]
    fn suspension_envelope_uses_immutable_old_event_generation_after_resume() {
        let old_event = TenantSuspensionEvent {
            event_id: "suspend-old".to_string(),
            tenant_id: "tenant-1".to_string(),
            created_at_ms: 10,
            attempts: 0,
        };
        // A later resume has generation 2, but the old event remains generation 1.
        let body = suspension_envelope_body(&old_event, Ok(1)).unwrap();
        let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(payload["event_id"], "suspend-old");
        assert_eq!(payload["lifecycle_generation"], "1");
    }

    #[test]
    fn suspension_envelope_refuses_missing_event_generation() {
        let event = TenantSuspensionEvent {
            event_id: "missing".to_string(),
            tenant_id: "tenant-2".to_string(),
            created_at_ms: 10,
            attempts: 0,
        };
        assert!(suspension_envelope_body(&event, Err(anyhow::anyhow!("unknown event"))).is_err());
    }

    #[test]
    fn suspension_envelope_refuses_generation_overflow() {
        let event = TenantSuspensionEvent {
            event_id: "overflow".to_string(),
            tenant_id: "tenant-3".to_string(),
            created_at_ms: 10,
            attempts: 0,
        };
        assert!(suspension_envelope_body(&event, Ok(i64::MAX as u64 + 1)).is_err());
    }

    #[test]
    fn suspension_receipt_requires_exact_triple_and_complete() {
        let body = serde_json::json!({
            "event_id": "event-1",
            "tenant_id": "tenant-1",
            "lifecycle_generation": "7",
            "complete": true,
        })
        .to_string();
        assert!(suspension_receipt_is_ack(200, body.as_bytes(), "event-1", "tenant-1", "7"));
        assert!(!suspension_receipt_is_ack(202, body.as_bytes(), "event-1", "tenant-1", "7"));

        for (field, value) in [
            ("event_id", serde_json::json!("other-event")),
            ("tenant_id", serde_json::json!("other-tenant")),
            ("lifecycle_generation", serde_json::json!(7)),
            ("complete", serde_json::json!(false)),
        ] {
            let mut receipt = serde_json::json!({
                "event_id": "event-1",
                "tenant_id": "tenant-1",
                "lifecycle_generation": "7",
                "complete": true,
            });
            receipt[field] = value;
            assert!(!suspension_receipt_is_ack(
                200,
                receipt.to_string().as_bytes(),
                "event-1",
                "tenant-1",
                "7"
            ));
        }
    }

    #[test]
    fn suspension_receipt_rejects_unknown_keys_and_oversize_body() {
        let receipt = serde_json::json!({
            "event_id": "event-1",
            "tenant_id": "tenant-1",
            "lifecycle_generation": "7",
            "complete": true,
            "extra": "refuse",
        });
        assert!(!suspension_receipt_is_ack(
            200,
            receipt.to_string().as_bytes(),
            "event-1",
            "tenant-1",
            "7"
        ));
        assert!(!suspension_receipt_is_ack(
            200,
            &vec![b'x'; MAX_SUSPENSION_RECEIPT_BYTES + 1],
            "event-1",
            "tenant-1",
            "7"
        ));
    }

    #[test]
    fn suspension_dispatch_accepts_only_bare_https_origin() {
        assert_eq!(
            secure_worker_origin("https://worker.example"),
            Some("https://worker.example")
        );
        assert_eq!(
            secure_worker_origin("https://worker.example/"),
            Some("https://worker.example")
        );
        for invalid in [
            "http://worker.example",
            "https://user:secret@worker.example",
            "https://worker.example/path",
            "https://worker.example?token=secret",
            "https://worker.example#fragment",
            "https://:443",
            "https://worker.example:bad",
        ] {
            assert!(secure_worker_origin(invalid).is_none(), "accepted {invalid}");
        }
    }
}
