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
//! returns `{accepted, deduped, total}` (auth → 401, bad batch → 400, fault → 503).
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
use std::sync::Mutex;

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

/// Flush the buffer once it reaches this many events (the time-based ~30s flush
/// is driven by the composition root; this bounds memory between ticks).
const BATCH_MAX: usize = 256;

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

/// `BLAKE3(lease_id ‖ billing_period)` as 64-char lowercase hex (32 bytes).
fn idem_key(lease_id: &str, period: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(lease_id.as_bytes());
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
    /// and a 3-char [`BILLING_REGION_ENV`]; a partial/invalid config yields `None`
    /// (the composition root then wires the no-op target — fail-safe-off, never a
    /// half-configured push). `get` is `|k| std::env::var(k).ok()` in production.
    pub fn from_env<F: Fn(&str) -> Option<String>>(
        get: F,
        timeout: std::time::Duration,
    ) -> Option<Self> {
        let url = get(BILLING_INGEST_URL_ENV).filter(|s| !s.is_empty())?;
        let auth = get(BILLING_INGEST_AUTH_KEY_ENV).filter(|s| !s.is_empty())?;
        let region = get(BILLING_REGION_ENV).filter(|s| s.chars().count() == 3)?;
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
    pub fn flush(&self) -> anyhow::Result<()> {
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
        let mut buf = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        buf.push(data);
        let full = buf.len() >= BATCH_MAX;
        drop(buf);
        if full {
            // Best-effort auto-flush to bound memory; a failure is retained for
            // the next tick (never propagated into the admission path).
            let _ = self.flush();
        }
    }
}

impl<P: BillingPoster> BillingExportTarget for CorelinkBillingTarget<P> {
    /// Pair `Acquired` with the next terminal transition (`Released`/`Expired`/
    /// `Crashed`) to emit one `RunnerSlotSeconds` event for the slot's lifetime.
    /// A terminal with no remembered acquire (e.g. process restart lost the
    /// in-memory pairing) is skipped — never a fabricated qty. Always `Ok`:
    /// buffering cannot fail, and a flush error surfaces via [`flush`] off the
    /// admission path, never here.
    fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()> {
        match event.kind {
            SlotEventKind::Acquired => {
                self.open
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(event.lease_id.clone(), event.at_ms);
            }
            SlotEventKind::Released | SlotEventKind::Expired | SlotEventKind::Crashed => {
                let acquired = self
                    .open
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&event.lease_id);
                if let Some(acquired_at_ms) = acquired {
                    self.enqueue_terminal(event, acquired_at_ms);
                }
                // else: no paired acquire on this instance → skip (no fabricated
                // usage; the aggregator never sees a phantom slot).
            }
        }
        Ok(())
    }
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

    /// A terminal with no remembered acquire (lost pairing) bills nothing — never
    /// a fabricated qty.
    #[test]
    fn terminal_without_acquire_is_skipped() {
        let t = target(RecordingPoster::ok());
        t.export(&ev("acme", "orphan", SlotEventKind::Released, 5_000))
            .unwrap();
        assert_eq!(t.buffered(), 0, "no paired acquire → no phantom slot");
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
    }

    /// `idem_key` is deterministic per (lease, period) and differs across leases.
    #[test]
    fn idem_key_is_deterministic_and_lease_scoped() {
        assert_eq!(idem_key("L1", "2026-06"), idem_key("L1", "2026-06"));
        assert_ne!(idem_key("L1", "2026-06"), idem_key("L2", "2026-06"));
        assert_ne!(idem_key("L1", "2026-06"), idem_key("L1", "2026-07"));
        assert_eq!(idem_key("L1", "2026-06").len(), 64);
    }
}
