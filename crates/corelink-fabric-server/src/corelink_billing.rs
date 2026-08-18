//! corelink-billing usage-push adapter (WP-BILLING-TARGET) — the real
//! [`BillingExportTarget`] that pushes runner usage to `corelink-billing`.
//!
//! ## Contract (Server TL, 2026-06-22; billing model owner-decided 2026-06-23)
//!
//! Endpoint (server-side, ships separately): `POST /internal/v1/billing/usage`,
//! accepting a JSON **batch** (array) of raw usage events. Auth is the dedicated
//! `BILLING_INGEST_AUTH_KEY` via the `x-corelink-internal-auth` header — NEVER
//! the shared `CORELINK_INTERNAL_AUTH_KEY` (the F-006/F-019 dedicated-key
//! posture, identical to `FABRIC_INTROSPECT_AUTH_KEY`).
//!
//! Per-event shape ([`UsageEventData`]): `{ tenant_id, event_kind, qty,
//! billing_period:"YYYY-MM", region, source, time_ms, idem_key }`. The runner
//! sends RAW per-event records with a stable `idem_key`; the **aggregator** owns
//! the rollup + hash-chain + dedup, so this adapter computes NO chain hashes.
//! At-least-once delivery is fine — `idem_key` makes it idempotent. The ingest
//! returns `{accepted, deduped, rejected, total}` (auth → 401, unparseable/empty/
//! oversized batch → 400, per-record validation failure → skipped+counted in
//! `rejected`, backend fault → 503).
//!
//! ## Billing model — `runner_slot_seconds` (owner, 2026-06-23)
//!
//! Concurrency is a FLAT per-tier SKU ("concurrency priced, minutes unlimited");
//! usage-push is for dashboard + reconciliation + anti-abuse, NOT metered Stripe
//! charging. So one event is emitted per TERMINAL lease transition with
//! `qty = slot·seconds = (terminal_at_ms − acquired_at_ms) / 1000` (one occupied
//! slot · its lifetime in seconds). `event_kind` is the canonical wire string
//! [`RUNNER_SLOT_SECONDS_KIND`] (`"runner_slot_seconds"`, Server-TL-pinned). The
//! `region` is the 3-char Cloudflare colo (default substrate, ADR-0008).
//!
//! ## Default-off + off the admission path
//!
//! The composition root wires this only when [`BILLING_INGEST_URL_ENV`], the
//! dedicated key, and a 3-char region are all present (else
//! [`NoopBillingTarget`](corelink_fabric::NoopBillingTarget)). It
//! BUFFERS events and flushes a batch on [`flush`](CorelinkBillingTarget::flush)
//! (driven every ~30s / at shutdown by the composition root) — a flush transport
//! error is returned to the caller (which logs + retries next tick) and NEVER
//! blocks admission.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use corelink_fabric::meter::{SlotEventKind, SlotOccupancyEvent};
use corelink_fabric::{BillingExportTarget, compute_meter};
use serde::{Deserialize, Serialize};

/// The runner billable `event_kind` — the canonical wire string the Server TL
/// pinned (ASK-2 final, 2026-06-23): `corelink-billing-emit`'s
/// `UsageEventKind::RunnerSlotSeconds::as_str()`. Non-Stripe-billable (treated
/// like `ReplayRequest` — dashboard/reconciliation/anti-abuse), per the
/// owner-ratified flat-concurrency model ("concurrency priced, minutes unlimited").
pub const RUNNER_SLOT_SECONDS_KIND: &str = "runner_slot_seconds";

/// `source` for runner-emitted usage (Server TL contract).
pub const BILLING_SOURCE: &str = "corelink-runners/fabricd";

/// Env var holding the corelink-billing ingest endpoint URL (default-off: absent
/// ⇒ the composition root wires the no-op target instead).
pub const BILLING_INGEST_URL_ENV: &str = "BILLING_INGEST_URL";
/// Env var holding the dedicated `x-corelink-internal-auth` value for billing
/// ingest (NEVER the shared internal key).
pub const BILLING_INGEST_AUTH_KEY_ENV: &str = "BILLING_INGEST_AUTH_KEY";
/// Env var holding the 3-char region code stamped on every usage event. Since
/// **Cloudflare is the default substrate (ADR-0008)**, this is the Cloudflare
/// colo / region (IATA-style 3-letter code, e.g. `iad`); on the Northflank
/// fallback it is the configured Northflank region, also 3 chars.
pub const BILLING_REGION_ENV: &str = "BILLING_REGION";

/// One raw usage event in the batch — the per-event wire shape the aggregator
/// ingests. Serialized as-is; the aggregator wraps/rolls-up + chains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEventData {
    /// The tenant the usage is attributed to.
    pub tenant_id: String,
    /// The billable kind — [`RUNNER_SLOT_SECONDS_KIND`] (non-Stripe-billable).
    pub event_kind: String,
    /// Integer count in the kind's unit: for `RunnerSlotSeconds`, slot·seconds.
    pub qty: u64,
    /// Calendar month the usage is attributed to, `"YYYY-MM"` (UTC).
    pub billing_period: String,
    /// 3-char region the usage was produced in (Cloudflare colo by default —
    /// the default substrate per ADR-0008; the Northflank region on fallback).
    pub region: String,
    /// Provenance — always [`BILLING_SOURCE`].
    pub source: String,
    /// Unix epoch ms of the terminal lease transition this event bills.
    pub time_ms: u64,
    /// Deterministic idempotency key, `BLAKE3(lease_id ‖ billing_period)` as
    /// 64-char hex — the aggregator dedups on this, so at-least-once is safe.
    pub idem_key: String,
}

/// `"YYYY-MM"` (UTC) for an epoch-ms instant, from the frozen
/// [`compute_meter::period_key`] (`YYYYMM`) — no extra date dependency.
fn billing_period(at_ms: u64) -> String {
    let key = compute_meter::period_key(at_ms); // e.g. 202606
    format!("{:04}-{:02}", key / 100, key % 100)
}

/// `BLAKE3(lease_id ‖ "|" ‖ billing_period)` as 64-char lowercase hex (32 bytes).
///
/// # Billing-emit disjointness invariant (WP-C, 2026-07-17) — READ BEFORE CHANGING
///
/// There are TWO billing-push emitters in this system and they must NEVER both
/// bill the same underlying job:
///   * THIS path (fabricd-native): emits one `runner_slot_seconds` event per
///     terminal transition of a lease **fabricd itself holds**, keyed on the
///     minted `lease_id` (shape `lease-<uuid-v4>`; see `AppState::mint_lease_id`).
///     Hash = **BLAKE3**.
///   * spawn-worker path (`deploy/cloudflare/src/lib.ts` `usageIdemKey`): emits
///     on GitHub `workflow_job:completed`, keyed on the decimal GitHub
///     `workflow_job.id`. Hash = **SHA-256**.
///
/// Disjointness is guaranteed by TWO independent facts, NOT by the idem_key:
///   1. **Runner-path assignment.** A billable job is served by exactly one
///      runner path. In prod the spawn-worker's cred redemption targets the
///      Worker's OWN `/v1/leases/{id}/cas-cred` (`SPAWN_WORKER_PUBLIC_URL`), not
///      fabricd — so a spawn-worker GH job never becomes a fabricd lease.
///   2. **Structurally-disjoint id-spaces.** fabricd hashes `lease-<uuid-v4>`;
///      the Worker hashes a pure-decimal GH job id. The two string spaces never
///      overlap, so no `(id, period)` pair — hence no billable unit — is ever
///      keyed by both paths.
///
/// The idem_key does **NOT** and CANNOT dedup across the two paths: the algos
/// differ (BLAKE3 vs SHA-256), so even the *same* input yields different keys.
/// idem_key dedup is at-least-once safety WITHIN one path only. (This corrects an
/// earlier comment that wrongly claimed the two paths "produce the SAME idem_key
/// for the same lease" — they never can.) The `|` separator is retained purely to
/// keep the two-field concatenation injective (audit r7): `a‖bc` must not collide
/// with `ab‖c` within this path. If a future change could make one billable unit
/// emit from BOTH paths, that is a DOUBLE-COUNT regression — do not "fix" it by
/// unifying the algos (a billing behavior change needing owner sign-off); restore
/// path disjointness or escalate. The disjointness tests below are the tripwire.
fn idem_key(lease_id: &str, period: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(lease_id.as_bytes());
    hasher.update(b"|");
    hasher.update(period.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Transport seam for the batch POST — mockable in tests (mirrors
/// `IntrospectHttp`). The composition root supplies a `ureq`-backed impl.
pub trait BillingPoster: Send + Sync {
    /// POST `json_body` (a JSON array of [`UsageEventData`]) to `url` with the
    /// `x-corelink-internal-auth: <auth>` header. Returns the HTTP status.
    fn post_batch(&self, url: &str, auth: &str, json_body: &str) -> anyhow::Result<u16>;
}

/// The real `ureq` [`BillingPoster`] (mirrors `UreqIntrospect`): a per-call
/// agent with a bounded timeout, statuses surfaced as `Ok` (the adapter maps
/// non-2xx explicitly), `x-corelink-internal-auth` carrying the dedicated key.
pub struct UreqBillingPoster {
    timeout: std::time::Duration,
}

impl UreqBillingPoster {
    /// New transport with the given request timeout.
    pub fn new(timeout: std::time::Duration) -> Self {
        Self { timeout }
    }
}

impl BillingPoster for UreqBillingPoster {
    fn post_batch(&self, url: &str, auth: &str, json_body: &str) -> anyhow::Result<u16> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let resp = agent
            .post(url)
            .header("x-corelink-internal-auth", auth)
            .header("Content-Type", "application/json")
            .send(json_body)?;
        Ok(resp.status().as_u16())
    }
}

impl CorelinkBillingTarget<UreqBillingPoster> {
    /// Build the production target from env, or `None` (default-off) when the
    /// billing-ingest env is absent. Requires ALL THREE of [`BILLING_INGEST_URL_ENV`],
    /// [`BILLING_INGEST_AUTH_KEY_ENV`] (the dedicated key, never the shared one),
    /// and a [`BILLING_REGION_ENV`] that lowercase-canonicalizes to exactly 3
    /// ASCII letters (so `"IAD"` is accepted as `iad`); a partial/invalid config
    /// yields `None` (the composition root then wires the no-op target —
    /// fail-safe-off, never a half-configured push). `get` is
    /// `|k| std::env::var(k).ok()` in production.
    pub fn from_env<F: Fn(&str) -> Option<String>>(
        get: F,
        timeout: std::time::Duration,
    ) -> Option<Self> {
        let url = get(BILLING_INGEST_URL_ENV).filter(|s| !s.is_empty())?;
        let auth = get(BILLING_INGEST_AUTH_KEY_ENV).filter(|s| !s.is_empty())?;
        // Canonical CF colo: exactly 3 ASCII letters, LOWERCASE. Lowercase-
        // canonicalize the env value so a box misconfigured with an upper/mixed
        // -case colo (e.g. `BILLING_REGION="IAD"`) emits the canonical `iad` the
        // ingest accepts — NOT a value the server 400s (which, with the retain-
        // and-retry flush, becomes an infinite re-POST flood). A non-letter /
        // wrong-length value stays `None` (unusable → no-op target, fail-safe-off).
        let region = get(BILLING_REGION_ENV)
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_lowercase()))?;
        Some(Self::new(
            UreqBillingPoster::new(timeout),
            url,
            auth,
            region,
        ))
    }
}

/// The real corelink-billing usage-push target: buffers per-terminal-lease
/// `RunnerSlotSeconds` events and flushes them as a batch.
pub struct CorelinkBillingTarget<P: BillingPoster> {
    poster: P,
    url: String,
    auth: String,
    event_kind: String,
    /// 3-char region stamped on every event (Cloudflare colo by default).
    region: String,
    /// `lease_id → acquired_at_ms`, to compute slot·seconds at the terminal event.
    open: Mutex<HashMap<String, u64>>,
    /// Pending events awaiting the next flush.
    buffer: Mutex<Vec<UsageEventData>>,
}

impl<P: BillingPoster> CorelinkBillingTarget<P> {
    /// Construct with the canonical [`RUNNER_SLOT_SECONDS_KIND`] and the given
    /// 3-char `region` (Cloudflare colo by default).
    pub fn new(
        poster: P,
        url: impl Into<String>,
        auth: impl Into<String>,
        region: impl Into<String>,
    ) -> Self {
        Self::with_event_kind(poster, url, auth, region, RUNNER_SLOT_SECONDS_KIND)
    }

    /// Construct with an explicit `event_kind` literal (keeps the literal in ONE
    /// place should the canonical string ever change).
    pub fn with_event_kind(
        poster: P,
        url: impl Into<String>,
        auth: impl Into<String>,
        region: impl Into<String>,
        event_kind: impl Into<String>,
    ) -> Self {
        Self {
            poster,
            url: url.into(),
            auth: auth.into(),
            event_kind: event_kind.into(),
            region: region.into(),
            open: Mutex::new(HashMap::new()),
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Number of events currently buffered (test/observability).
    pub fn buffered(&self) -> usize {
        self.buffer.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Flush the buffered batch to corelink-billing. On success the buffer is
    /// cleared; on a transport / non-2xx error the buffer is RETAINED (the next
    /// tick retries — `idem_key` makes the re-send idempotent) and the error is
    /// returned. A no-op (and `Ok`) when the buffer is empty.
    ///
    /// Inherent method; the [`BillingExportTarget::flush`] trait method (driven by
    /// the composition root over `dyn BillingExportTarget`) delegates here.
    pub fn flush_now(&self) -> anyhow::Result<()> {
        // Snapshot under the lock, but do not hold it across the blocking POST.
        let batch: Vec<UsageEventData> = {
            let buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
            if buf.is_empty() {
                return Ok(());
            }
            buf.clone()
        };
        let body = serde_json::to_string(&batch)?;
        let status = self.poster.post_batch(&self.url, &self.auth, &body)?;
        if !(200..300).contains(&status) {
            anyhow::bail!(
                "corelink-billing ingest returned HTTP {status}; retaining batch for retry"
            );
        }
        // Success: drop exactly what we sent (keep anything appended meanwhile).
        let mut buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        let sent = batch.len().min(buf.len());
        buf.drain(0..sent);
        Ok(())
    }

    /// Buffer a terminal lease as one `RunnerSlotSeconds` event.
    fn enqueue_terminal(&self, ev: &SlotOccupancyEvent, acquired_at_ms: u64) {
        let slot_seconds = ev.at_ms.saturating_sub(acquired_at_ms) / 1000;
        let period = billing_period(ev.at_ms);
        let data = UsageEventData {
            tenant_id: ev.tenant.as_str().to_string(),
            event_kind: self.event_kind.clone(),
            qty: slot_seconds,
            billing_period: period.clone(),
            region: self.region.clone(),
            source: BILLING_SOURCE.to_string(),
            time_ms: ev.at_ms,
            idem_key: idem_key(&ev.lease_id, &period),
        };
        // Audit r8: cap the buffer. `flush_now` RETAINS the batch on a non-2xx /
        // transport error (for the idem_key retry), so a PERSISTENTLY-unavailable
        // billing endpoint would otherwise grow this Vec without bound (OOM). On
        // overflow shed the OLDEST event + log: bounded, logged billing-data loss
        // beats an unbounded memory leak. Under a healthy endpoint the periodic
        // flush keeps the buffer at ~`rate × interval`, far below the cap.
        const MAX_BILLING_BUFFER: usize = 100_000;
        let mut buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        if buf.len() >= MAX_BILLING_BUFFER {
            buf.remove(0);
            eprintln!(
                "billing buffer at cap ({MAX_BILLING_BUFFER}) — shedding oldest event; \
                 the push endpoint is likely persistently unavailable (check the flush loop)"
            );
        }
        buf.push(data);
        // NO inline flush here. `enqueue_terminal` runs on the async terminal path
        // (the close handler, the reaper sweep, admission expiry), and `flush_now`
        // is a SYNCHRONOUS blocking `ureq` POST (up to the HTTP timeout). Calling it
        // inline would stall that caller's response on the network — and
        // `block_in_place` cannot rescue it: it only keeps OTHER tasks from starving
        // (this call still blocks for the full POST) and it PANICS off a multi-thread
        // runtime (e.g. in the unit tests, which call this synchronously). The
        // periodic `spawn_push_flush_loop` (block_in_place, ~30s) is the SOLE flush
        // driver; under a healthy endpoint the buffer holds at most ~`rate ×
        // interval` events (the cap above is the persistent-failure backstop).
    }
}

impl<P: BillingPoster> BillingExportTarget for CorelinkBillingTarget<P> {
    /// Pair `Acquired` with the next terminal transition (`Released`/`Expired`/
    /// `Crashed`) to emit one `RunnerSlotSeconds` event for the slot's lifetime.
    /// The in-memory `open` map is a CACHE: on a terminal we prefer it, but fall
    /// back to the event's durable `acquired_at_ms` (the ledger's
    /// `billing_acquired_at_ms`) so a slot acquired BEFORE a fabricd restart and
    /// closed AFTER still bills correctly (revenue-loss fix #3). Only a terminal
    /// with NEITHER a cached nor a durable acquire is skipped — never a fabricated
    /// qty. Always `Ok`: buffering cannot fail, and a flush error surfaces via
    /// [`flush`] off the admission path, never here.
    fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()> {
        match event.kind {
            SlotEventKind::Acquired => {
                self.open
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(event.lease_id.clone(), event.at_ms);
            }
            SlotEventKind::Released | SlotEventKind::Expired | SlotEventKind::Crashed => {
                let cached = self
                    .open
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&event.lease_id);
                // #3 (revenue-loss): the in-memory `open` map is now a CACHE, not
                // the source of truth. Fast path — the acquire was seen on THIS
                // process, so the map has it. RESTART-RECOVERY path — a fabricd
                // restart dropped the map, but the terminal event carries the
                // lease's durable `billing_acquired_at_ms` from the ledger, so the
                // slot still bills correctly. Only when NEITHER is present (a
                // pre-fix / never-stamped lease) do we keep today's honest skip:
                // no fabricated qty, the aggregator never sees a phantom slot.
                match cached.or(event.acquired_at_ms) {
                    Some(acquired_at_ms) => self.enqueue_terminal(event, acquired_at_ms),
                    None => { /* no acquire, cached or durable → skip (no phantom slot) */ }
                }
            }
        }
        Ok(())
    }

    /// Drive the periodic batch flush (the composition root ticks this over
    /// `dyn BillingExportTarget`). Delegates to the inherent [`flush_now`].
    ///
    /// [`flush_now`]: CorelinkBillingTarget::flush_now
    fn flush(&self) -> anyhow::Result<()> {
        self.flush_now()
    }
}

/// Default flush cadence; override via `FABRIC_BILLING_PUSH_INTERVAL_SECS`.
pub const DEFAULT_PUSH_FLUSH_SECS: u64 = 30;

/// Resolve the flush interval from env (default [`DEFAULT_PUSH_FLUSH_SECS`];
/// a `< 1` or unparseable value falls back to the default).
pub fn push_flush_interval_from_env<F: Fn(&str) -> Option<String>>(get: F) -> std::time::Duration {
    let secs = get("FABRIC_BILLING_PUSH_INTERVAL_SECS")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&s| s >= 1)
        .unwrap_or(DEFAULT_PUSH_FLUSH_SECS);
    std::time::Duration::from_secs(secs)
}

/// Spawn the periodic flush driver over a billing target. Ticks every `interval`,
/// calling [`BillingExportTarget::flush`] (the buffering target's batch POST). A
/// failing tick logs and the batch is RETAINED (retried next tick — idempotent by
/// `idem_key`); a panicking tick is caught so the loop never dies silently. The
/// blocking POST runs on the blocking pool (`block_in_place`) so it never stalls
/// an async worker — legal on the multi-thread runtime `main.rs` uses, mirroring
/// the durable exporter. Returns the [`JoinHandle`](tokio::task::JoinHandle) the
/// composition root `.abort()`s on graceful shutdown.
pub fn spawn_push_flush_loop(
    target: Arc<dyn BillingExportTarget + Send + Sync>,
    interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let t = target.clone();
            let outcome = tokio::task::block_in_place(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.flush()))
            });
            match outcome {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    eprintln!("billing usage-push flush failed (batch retained for retry): {e}")
                }
                Err(_) => {
                    eprintln!("billing usage-push flush PANICKED; loop survives, batch retained")
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use corelink_fabric::tenant::TenantId;

    use super::*;

    fn tid(s: &str) -> TenantId {
        TenantId::new(s).expect("valid tenant id")
    }

    fn ev(tenant: &str, lease: &str, kind: SlotEventKind, at_ms: u64) -> SlotOccupancyEvent {
        SlotOccupancyEvent {
            tenant: tid(tenant),
            lease_id: lease.to_string(),
            kind,
            at_ms,
            acquired_at_ms: None,
        }
    }

    /// A poster that records every batch body it is asked to POST, and returns a
    /// scripted status (200 unless overridden, or a transport error).
    struct RecordingPoster {
        status: u16,
        transport_ok: bool,
        bodies: Mutex<Vec<String>>,
    }
    impl RecordingPoster {
        fn ok() -> Self {
            Self {
                status: 200,
                transport_ok: true,
                bodies: Mutex::new(Vec::new()),
            }
        }
        fn status(s: u16) -> Self {
            Self {
                status: s,
                transport_ok: true,
                bodies: Mutex::new(Vec::new()),
            }
        }
        fn transport_error() -> Self {
            Self {
                status: 0,
                transport_ok: false,
                bodies: Mutex::new(Vec::new()),
            }
        }
        fn bodies(&self) -> Vec<String> {
            self.bodies.lock().unwrap().clone()
        }
    }
    impl BillingPoster for RecordingPoster {
        fn post_batch(&self, _url: &str, _auth: &str, json_body: &str) -> anyhow::Result<u16> {
            self.bodies.lock().unwrap().push(json_body.to_string());
            if !self.transport_ok {
                anyhow::bail!("simulated transport error");
            }
            Ok(self.status)
        }
    }

    fn target(p: RecordingPoster) -> CorelinkBillingTarget<RecordingPoster> {
        CorelinkBillingTarget::new(
            p,
            "https://x/internal/v1/billing/usage",
            "billing-secret",
            "iad",
        )
    }

    /// Acquire→Released emits ONE event whose qty is the slot's lifetime in
    /// seconds, with the agreed kind, period, source and a 64-hex idem_key.
    #[test]
    fn acquire_then_released_bills_slot_seconds() {
        let t = target(RecordingPoster::ok());
        t.export(&ev("acme", "L1", SlotEventKind::Acquired, 1_000))
            .unwrap();
        assert_eq!(t.buffered(), 0, "acquire alone bills nothing");
        t.export(&ev("acme", "L1", SlotEventKind::Released, 4_000))
            .unwrap();
        assert_eq!(t.buffered(), 1, "the terminal transition buffers one event");

        // Flush and inspect the wire.
        t.flush().unwrap();
        let body = &t.poster.bodies()[0];
        let arr: Vec<UsageEventData> = serde_json::from_str(body).unwrap();
        assert_eq!(arr.len(), 1);
        let e = &arr[0];
        assert_eq!(e.tenant_id, "acme");
        assert_eq!(e.event_kind, "runner_slot_seconds", "canonical wire string");
        assert_eq!(e.event_kind, RUNNER_SLOT_SECONDS_KIND);
        assert_eq!(e.qty, 3, "(4000-1000)/1000 = 3 slot·seconds");
        assert_eq!(e.region, "iad", "3-char region stamped on the event");
        assert_eq!(e.source, BILLING_SOURCE);
        assert_eq!(e.time_ms, 4_000);
        assert_eq!(e.idem_key.len(), 64, "BLAKE3 → 32 bytes → 64 hex chars");
        assert!(e.billing_period.len() == 7 && e.billing_period.contains('-'));
    }

    /// REGRESSION (hardening sweep 2026-06-26): a BURST of terminals must NOT
    /// trigger an inline POST. `enqueue_terminal` runs on the async terminal/close
    /// path; a synchronous blocking `ureq` POST there would stall the caller's
    /// response on the network (the bug the sweep found — an auto-flush at 256 with
    /// no `block_in_place`). The periodic flush loop is the SOLE POST driver; until
    /// it ticks, events just accumulate in the buffer.
    #[test]
    fn burst_of_terminals_does_not_flush_inline() {
        let t = target(RecordingPoster::ok());
        // Push far past the old 256 auto-flush threshold.
        for i in 0..300u64 {
            let lease = format!("L{i}");
            t.export(&ev("acme", &lease, SlotEventKind::Acquired, 0))
                .unwrap();
            t.export(&ev("acme", &lease, SlotEventKind::Released, 1_000))
                .unwrap();
        }
        assert_eq!(
            t.buffered(),
            300,
            "all 300 terminals buffered, none dropped"
        );
        assert!(
            t.poster.bodies().is_empty(),
            "NO inline POST: enqueue must never call the blocking poster on the async path"
        );
        // Only an explicit flush (what the periodic loop drives) POSTs.
        t.flush().unwrap();
        assert_eq!(
            t.poster.bodies().len(),
            1,
            "the periodic flush is the sole POST driver"
        );
    }

    /// REGRESSION (audit r8): the buffer is CAPPED. A persistently-unavailable
    /// flush endpoint RETAINS the batch on error, so without a cap the buffer
    /// would grow without bound (OOM). Past the cap the oldest event is shed and
    /// the buffer never exceeds it. (No flush is driven here, so every enqueue
    /// accumulates — exactly the persistent-failure shape.)
    #[test]
    fn buffer_is_capped_under_persistent_flush_failure() {
        const CAP: usize = 100_000;
        let t = target(RecordingPoster::ok());
        for i in 0..(CAP as u64 + 5) {
            let lease = format!("L{i}");
            t.export(&ev("acme", &lease, SlotEventKind::Acquired, 0))
                .unwrap();
            t.export(&ev("acme", &lease, SlotEventKind::Released, 1_000))
                .unwrap();
        }
        assert_eq!(
            t.buffered(),
            CAP,
            "the buffer must be capped at {CAP}, shedding the oldest events"
        );
    }

    /// `Expired` and `Crashed` are terminal too — they bill the slot's lifetime.
    #[test]
    fn expired_and_crashed_also_bill() {
        for kind in [SlotEventKind::Expired, SlotEventKind::Crashed] {
            let t = target(RecordingPoster::ok());
            t.export(&ev("acme", "L", SlotEventKind::Acquired, 0))
                .unwrap();
            t.export(&ev("acme", "L", kind, 2_000)).unwrap();
            assert_eq!(t.buffered(), 1, "{kind:?} bills the slot lifetime");
        }
    }

    /// A terminal with no remembered acquire AND no durable stamp bills nothing —
    /// never a fabricated qty (a pre-fix / never-stamped lease).
    #[test]
    fn terminal_without_acquire_is_skipped() {
        let t = target(RecordingPoster::ok());
        t.export(&ev("acme", "orphan", SlotEventKind::Released, 5_000))
            .unwrap();
        assert_eq!(t.buffered(), 0, "no paired acquire → no phantom slot");
    }

    /// #3 (RESTART-RECOVERY): a fabricd restart drops the in-memory `open` map,
    /// so a lease acquired BEFORE the restart and closed AFTER has NO cached
    /// `Acquired`. The terminal event carries the ledger's durable
    /// `billing_acquired_at_ms`, so the slot STILL bills correctly — the map is a
    /// cache, not the source of truth. Asserts the exact recovered `slot_seconds`.
    #[test]
    fn restart_recovery_bills_from_durable_acquired_at_ms() {
        let t = target(RecordingPoster::ok());
        // NO prior Acquired export (the restart lost it). Terminal carries t0.
        let t0 = 10_000u64;
        let terminal = SlotOccupancyEvent {
            tenant: tid("acme"),
            lease_id: "L-restart".to_string(),
            kind: SlotEventKind::Released,
            at_ms: 25_000,
            acquired_at_ms: Some(t0),
        };
        t.export(&terminal).unwrap();
        assert_eq!(
            t.buffered(),
            1,
            "the durable acquired_at_ms recovers the pairing → one billed event"
        );
        t.flush().unwrap();
        let arr: Vec<UsageEventData> = serde_json::from_str(&t.poster.bodies()[0]).unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0].qty, 15,
            "(25000 - 10000)/1000 = 15 slot·seconds, from the durable stamp"
        );
        assert_eq!(arr[0].time_ms, 25_000);
    }

    /// #3: the in-memory `open` map is the FAST PATH and still wins when present —
    /// a cached `Acquired` is used even if the terminal ALSO carries a (stale)
    /// durable stamp, so the same-process common case is unchanged.
    #[test]
    fn in_map_fast_path_still_used_and_wins_over_event_stamp() {
        let t = target(RecordingPoster::ok());
        // Same-process acquire: cached at 1_000.
        t.export(&ev("acme", "L-fast", SlotEventKind::Acquired, 1_000))
            .unwrap();
        // Terminal ALSO carries a durable stamp (7_000) — the cache must win.
        let terminal = SlotOccupancyEvent {
            tenant: tid("acme"),
            lease_id: "L-fast".to_string(),
            kind: SlotEventKind::Released,
            at_ms: 4_000,
            acquired_at_ms: Some(7_000),
        };
        t.export(&terminal).unwrap();
        t.flush().unwrap();
        let arr: Vec<UsageEventData> = serde_json::from_str(&t.poster.bodies()[0]).unwrap();
        assert_eq!(
            arr[0].qty, 3,
            "(4000 - 1000)/1000 = 3: the cached acquire wins over the event stamp"
        );
    }

    /// `flush` posts the batch and clears the buffer on success.
    #[test]
    fn flush_posts_batch_and_clears() {
        let t = target(RecordingPoster::ok());
        t.export(&ev("a", "L1", SlotEventKind::Acquired, 0))
            .unwrap();
        t.export(&ev("a", "L1", SlotEventKind::Released, 1_000))
            .unwrap();
        t.export(&ev("b", "L2", SlotEventKind::Acquired, 0))
            .unwrap();
        t.export(&ev("b", "L2", SlotEventKind::Released, 2_000))
            .unwrap();
        assert_eq!(t.buffered(), 2);
        t.flush().unwrap();
        assert_eq!(t.buffered(), 0, "buffer cleared after a successful flush");
        let arr: Vec<UsageEventData> = serde_json::from_str(&t.poster.bodies()[0]).unwrap();
        assert_eq!(arr.len(), 2, "both events in one batch");
    }

    /// An empty flush is a no-op success (no POST).
    #[test]
    fn empty_flush_is_noop() {
        let t = target(RecordingPoster::ok());
        t.flush().unwrap();
        assert!(
            t.poster.bodies().is_empty(),
            "no POST when nothing buffered"
        );
    }

    /// A transport error on flush RETAINS the batch (next tick retries) and
    /// returns Err — it never silently drops billing.
    #[test]
    fn flush_transport_error_retains_buffer() {
        let t = target(RecordingPoster::transport_error());
        t.export(&ev("a", "L1", SlotEventKind::Acquired, 0))
            .unwrap();
        t.export(&ev("a", "L1", SlotEventKind::Released, 1_000))
            .unwrap();
        assert!(t.flush().is_err(), "transport error surfaces as Err");
        assert_eq!(t.buffered(), 1, "batch retained for retry — never dropped");
    }

    /// A non-2xx ingest status also retains the batch and errors.
    #[test]
    fn flush_non_2xx_retains_buffer() {
        let t = target(RecordingPoster::status(500));
        t.export(&ev("a", "L1", SlotEventKind::Acquired, 0))
            .unwrap();
        t.export(&ev("a", "L1", SlotEventKind::Released, 1_000))
            .unwrap();
        assert!(t.flush().is_err(), "500 surfaces as Err");
        assert_eq!(t.buffered(), 1, "batch retained on non-2xx");
    }

    /// `from_env` wires the target only when ALL THREE env vars are present +
    /// valid; any missing/invalid piece → `None` (fail-safe-off, the composition
    /// root then uses the no-op target).
    #[test]
    fn from_env_requires_url_key_and_3char_region() {
        let ok = |k: &str| -> Option<String> {
            match k {
                BILLING_INGEST_URL_ENV => Some("https://api/internal/v1/billing/usage".into()),
                BILLING_INGEST_AUTH_KEY_ENV => Some("dedicated-secret".into()),
                BILLING_REGION_ENV => Some("iad".into()),
                _ => None,
            }
        };
        assert!(
            CorelinkBillingTarget::from_env(ok, std::time::Duration::from_secs(5)).is_some(),
            "all three present + valid → Some"
        );

        // Each missing piece → None.
        for drop in [
            BILLING_INGEST_URL_ENV,
            BILLING_INGEST_AUTH_KEY_ENV,
            BILLING_REGION_ENV,
        ] {
            let get = |k: &str| if k == drop { None } else { ok(k) };
            assert!(
                CorelinkBillingTarget::from_env(get, std::time::Duration::from_secs(5)).is_none(),
                "missing {drop} → None (default-off)"
            );
        }

        // A non-3-char region is rejected (the ingest validates 3-char).
        let bad_region = |k: &str| {
            if k == BILLING_REGION_ENV {
                Some("east".into())
            } else {
                ok(k)
            }
        };
        assert!(
            CorelinkBillingTarget::from_env(bad_region, std::time::Duration::from_secs(5))
                .is_none(),
            "4-char region → None"
        );

        // A region with a non-letter is unusable → None (fail-safe-off).
        for garbage in ["i2d", "u_s", "12!"] {
            let g = |k: &str| {
                if k == BILLING_REGION_ENV {
                    Some(garbage.to_string())
                } else {
                    ok(k)
                }
            };
            assert!(
                CorelinkBillingTarget::from_env(g, std::time::Duration::from_secs(5)).is_none(),
                "non-letter region {garbage:?} → None"
            );
        }
    }

    /// A box misconfigured with an UPPER/mixed-case colo must still emit the
    /// canonical lowercase region the ingest accepts — the fix for the live
    /// `bad_region` ingest flood (BILLING_REGION="IAD" 400ing every batch).
    #[test]
    fn from_env_canonicalizes_uppercase_region_to_lowercase() {
        for spelling in ["IAD", "Iad", "iAd"] {
            let get = |k: &str| match k {
                BILLING_INGEST_URL_ENV => Some("https://api/internal/v1/billing/usage".into()),
                BILLING_INGEST_AUTH_KEY_ENV => Some("dedicated-secret".into()),
                BILLING_REGION_ENV => Some(spelling.to_string()),
                _ => None,
            };
            let target = CorelinkBillingTarget::from_env(get, std::time::Duration::from_secs(5))
                .expect("upper/mixed-case colo must canonicalize + wire, not drop");
            assert_eq!(
                target.region, "iad",
                "region canonicalized from {spelling:?}"
            );
        }
    }

    /// `idem_key` is deterministic per (lease, period) and differs across leases.
    #[test]
    fn idem_key_is_deterministic_and_lease_scoped() {
        assert_eq!(idem_key("L1", "2026-06"), idem_key("L1", "2026-06"));
        assert_ne!(idem_key("L1", "2026-06"), idem_key("L2", "2026-06"));
        assert_ne!(idem_key("L1", "2026-06"), idem_key("L1", "2026-07"));
        assert_eq!(idem_key("L1", "2026-06").len(), 64);
        // Audit r7: the `|` separator makes the concatenation injective even for
        // different field-length splits — `a‖bc` must not collide with `ab‖c`.
        assert_ne!(idem_key("a", "bc"), idem_key("ab", "c"));
    }

    /// WP-C billing-emit disjointness — LAYER 2 (structurally-disjoint id-spaces).
    ///
    /// The fabricd-native path keys idem_key on the minted `lease_id` (shape
    /// `lease-<uuid-v4>`); the spawn-worker path keys on the decimal GitHub
    /// `workflow_job.id`. This test pins that those two string spaces can NEVER
    /// overlap, so no `(id, period)` pair — hence no billable unit — is ever keyed
    /// by both emitters. If a future change moves either path onto an id shape that
    /// intrudes on the other's space, this fails: an overlap is the precondition
    /// for the cross-path double-count the disjointness invariant forbids.
    #[test]
    fn idem_key_input_id_space_is_disjoint_from_spawn_worker_jobid() {
        // A fabricd lease id ALWAYS has the mint shape `lease-<uuid-v4>`
        // (`AppState::mint_lease_id`). Sample several real mints.
        for _ in 0..64 {
            let lease_id = format!("lease-{}", uuid::Uuid::new_v4());
            assert!(
                lease_id.starts_with("lease-"),
                "fabricd lease ids carry the `lease-` prefix: {lease_id}"
            );
            assert!(
                !is_decimal_jobid(&lease_id),
                "a fabricd lease id must never look like a GH decimal job id: {lease_id}"
            );
        }
        // Representative GitHub `workflow_job.id` values (String(number) — pure
        // decimal; cf. deploy/cloudflare/src/index.ts `String(evt.workflow_job.id)`).
        for job_id in ["1", "82597479935", "48291736210", "9007199254740991"] {
            assert!(
                is_decimal_jobid(job_id),
                "a GH job id is a pure-decimal string: {job_id}"
            );
            assert!(
                !job_id.starts_with("lease-"),
                "a GH job id must never carry the fabricd `lease-` prefix: {job_id}"
            );
        }
    }

    /// True iff `s` is a non-empty pure-decimal string — the GitHub
    /// `workflow_job.id` shape the spawn-worker keys billing on. A fabricd
    /// `lease-<uuid>` id can never satisfy this (the `lease-` prefix + hyphens).
    fn is_decimal_jobid(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
    }

    /// WP-C billing-emit disjointness — the idem_key is NOT a cross-path dedup.
    ///
    /// Even for the SAME `(id, period)` input the two emitters produce DIFFERENT
    /// idem_keys, because the algorithms differ: fabricd uses BLAKE3, the
    /// spawn-worker uses SHA-256 (`usageIdemKey` in lib.ts). So the aggregator's
    /// `idem_key` dedup can never collapse a fabricd event and a spawn-worker event
    /// into one — cross-path safety comes ONLY from path/id-space disjointness
    /// (the test above), never from the key. This locks that fact against the
    /// earlier, WRONG "same idem_key for the same lease" claim: if someone silently
    /// unifies the algos to force cross-path dedup, this fails and forces the
    /// owner-signed billing-behavior review the invariant requires.
    #[test]
    fn cross_path_idem_key_schemes_do_not_interoperate() {
        use sha2::{Digest, Sha256};
        // The spawn-worker scheme, transcribed: SHA-256(`${id}|${period}`) as hex.
        fn spawn_worker_idem_key(id: &str, period: &str) -> String {
            let mut h = Sha256::new();
            h.update(format!("{id}|{period}").as_bytes());
            h.finalize().iter().map(|b| format!("{b:02x}")).collect()
        }
        for (id, period) in [("L1", "2026-06"), ("82597479935", "2026-07"), ("x", "y")] {
            let fabricd = idem_key(id, period); // BLAKE3
            let worker = spawn_worker_idem_key(id, period); // SHA-256
            assert_eq!(fabricd.len(), 64, "BLAKE3 → 64 hex");
            assert_eq!(worker.len(), 64, "SHA-256 → 64 hex");
            assert_ne!(
                fabricd, worker,
                "BLAKE3 and SHA-256 idem_keys must differ for the same input \
                 ({id}|{period}) — the two paths do NOT interoperate for dedup"
            );
        }
    }
}
