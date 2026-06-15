//! Durable billing exporter (WP-A) — the [`SlotMeter`] → durable-sink seam.
//!
//! The [`SlotMeter`](crate::billing::SlotMeter) accumulates
//! [`SlotOccupancyEvent`]s in a bounded in-memory journal that dies with the
//! process and is per-instance. This module is the **durable sink** plus the
//! **export-once** logic that drains the (read-only, non-destructive) journal
//! into it.
//!
//! ## Exactly-once is by DB PRIMARY KEY, not a cursor
//!
//! There is NO drain cursor. Export reads the resident journal window and
//! batch-upserts every event with `INSERT … ON CONFLICT DO NOTHING` keyed on
//! `(tenant, lease_id, kind, at_ms)`. Re-exporting the resident window is
//! therefore free and **multi-instance-safe**: two instances exporting
//! overlapping windows both upsert, and the conflicts are ignored — the table
//! converges to the union with no duplicates. Frequent ticks + the
//! constraint are the design; a dropped (aged-out) event is surfaced as an ops
//! alarm, never silently recovered.
//!
//! ## THE INVARIANT — raw occupancy only, never minutes
//!
//! This sink ONLY persists raw [`SlotOccupancyEvent`]s. There is deliberately
//! **NO duration / minutes / cost arithmetic anywhere in this module** — no
//! `at_ms` subtraction, no invoicing, no COGS math. The slot is the billable
//! unit (concurrency pricing, never per-minute; "never charge for the
//! customer's own compute twice"). `at_ms` is persisted verbatim as part of
//! the key and never subtracted. Pinned by `sink_has_no_duration_or_cost_math`.

use std::collections::HashSet;

use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;
use tokio_postgres::NoTls;

use crate::meter::{SlotEventKind, SlotOccupancyEvent};
use crate::pg_ledger::PgTlsMode;
use crate::tenant::TenantId;

/// The lowercase `kind` token persisted in `billing_events.kind` — the same
/// flat vocabulary as the meter's serde `rename_all = "snake_case"`. Kept local
/// so the frozen [`SlotOccupancyEvent`] carries no DB derive.
fn kind_to_db(k: SlotEventKind) -> &'static str {
    match k {
        SlotEventKind::Acquired => "acquired",
        SlotEventKind::Released => "released",
        SlotEventKind::Expired => "expired",
        SlotEventKind::Crashed => "crashed",
    }
}

/// The durable sink for raw slot-occupancy events.
///
/// `persist` upserts a batch keyed on `(tenant, lease_id, kind, at_ms)` and
/// returns the number of rows NEWLY inserted (i.e. not already present —
/// conflicts do not count). It only persists; it performs no duration / cost
/// arithmetic (see the module invariant).
pub trait BillingSink {
    /// Persist `events`, stamping `exported_at_ms` on freshly inserted rows.
    /// Returns the count of rows newly inserted (existing rows — conflicts —
    /// are not counted).
    fn persist(&self, events: &[SlotOccupancyEvent], exported_at_ms: u64) -> anyhow::Result<usize>;
}

/// The natural key of a billing event — the DB PRIMARY KEY, modelled in Rust so
/// the in-memory [`MemBillingSink`] reproduces the exact `ON CONFLICT` dedup
/// semantics without a database.
///
/// The `kind` component is the lowercase DB token (`kind_to_db`), NOT the
/// [`SlotEventKind`] enum — so this module never requires a `Hash` derive on the
/// FROZEN [`SlotEventKind`]. The persisted `kind` column is the string anyway,
/// so the string IS the natural key.
type EventKey = (TenantId, String, &'static str, u64);

/// The natural key for an event (mirrors the table PRIMARY KEY, keyed on the
/// persisted lowercase `kind` token).
fn event_key(ev: &SlotOccupancyEvent) -> EventKey {
    (
        ev.tenant.clone(),
        ev.lease_id.clone(),
        kind_to_db(ev.kind),
        ev.at_ms,
    )
}

/// In-memory test double for [`BillingSink`] — a `HashSet` over the natural key
/// that reproduces `ON CONFLICT (tenant, lease_id, kind, at_ms) DO NOTHING`
/// exactly. Lets the export-once logic (and its exactly-once / multi-instance
/// guarantees) be tested WITHOUT a live Postgres.
#[derive(Debug, Default)]
pub struct MemBillingSink {
    seen: std::sync::Mutex<HashSet<EventKey>>,
}

impl MemBillingSink {
    /// A fresh, empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct events persisted so far (post-dedup row count).
    pub fn len(&self) -> usize {
        self.seen.lock().expect("MemBillingSink mutex").len()
    }

    /// Whether the sink holds no events.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True iff this exact event (by natural key) has been persisted.
    pub fn contains(&self, ev: &SlotOccupancyEvent) -> bool {
        self.seen
            .lock()
            .expect("MemBillingSink mutex")
            .contains(&event_key(ev))
    }
}

impl BillingSink for MemBillingSink {
    fn persist(
        &self,
        events: &[SlotOccupancyEvent],
        _exported_at_ms: u64,
    ) -> anyhow::Result<usize> {
        // Model ON CONFLICT DO NOTHING: insert returns the count of keys that
        // were not already present. `exported_at_ms` is the would-be column on
        // a fresh row; it is NOT part of the key, so a re-export never updates a
        // row and never counts. (No arithmetic on it — see module invariant.)
        let mut seen = self.seen.lock().expect("MemBillingSink mutex");
        let mut inserted = 0usize;
        for ev in events {
            if seen.insert(event_key(ev)) {
                inserted += 1;
            }
        }
        Ok(inserted)
    }
}

/// Idempotent schema for the durable billing sink. Safe to run on every
/// [`PgBillingSink::connect`] — the table + index are `IF NOT EXISTS`. The
/// PRIMARY KEY `(tenant, lease_id, kind, at_ms)` IS the exactly-once mechanism.
const DDL: &str = "\
CREATE TABLE IF NOT EXISTS billing_events (
  tenant         text   NOT NULL,
  lease_id       text   NOT NULL,
  kind           text   NOT NULL,
  at_ms          bigint NOT NULL,
  exported_at_ms bigint NOT NULL,
  PRIMARY KEY (tenant, lease_id, kind, at_ms)
);
";

/// Production [`BillingSink`] over Postgres.
///
/// Owns a [`deadpool_postgres::Pool`] and a [`tokio::runtime::Handle`]; the sync
/// trait method bridges to the async client with `block_in_place` + `block_on`,
/// mirroring [`PgLedger`](crate::pg_ledger::PgLedger). `persist` is a single
/// batched multi-row `INSERT … ON CONFLICT (tenant, lease_id, kind, at_ms) DO
/// NOTHING RETURNING`, so the rows it RETURNs are exactly the newly inserted
/// ones — the conflict count is the dedup, free and multi-instance-safe.
pub struct PgBillingSink {
    pool: Pool,
    handle: Handle,
}

impl std::fmt::Debug for PgBillingSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgBillingSink").finish_non_exhaustive()
    }
}

impl PgBillingSink {
    /// Build the pool, capture the runtime handle, and apply the idempotent DDL
    /// once. Fail-closed: a connection or DDL error → `Err` (never a half-open
    /// sink). Mirrors [`PgLedger::connect`](crate::pg_ledger::PgLedger::connect)
    /// — bounded pool-acquire wait, the same TLS branch.
    ///
    /// Must be called from inside a Tokio `rt-multi-thread` runtime (the sync
    /// `persist` later relies on `block_in_place` on that runtime).
    pub async fn connect(
        database_url: &str,
        pool_size: usize,
        tls: PgTlsMode,
    ) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        cfg.url = Some(database_url.to_string());
        // Bound the pool-acquire wait → `get()` fails fast under exhaustion
        // rather than stalling forever (same rationale as PgLedger).
        let mut pool_cfg = deadpool_postgres::PoolConfig::new(pool_size);
        pool_cfg.timeouts.wait = Some(std::time::Duration::from_secs(5));
        cfg.pool = Some(pool_cfg);
        // TLS branch reuses the ledger's resolved mode. `Require` builds a
        // verify-full rustls connector; `Disable` is the plaintext `NoTls` path.
        let pool = match tls {
            PgTlsMode::Disable => cfg.create_pool(Some(Runtime::Tokio1), NoTls),
            PgTlsMode::Require => {
                let connector = tokio_postgres_rustls::MakeRustlsConnect::new(
                    crate::pg_ledger::rustls_verify_full_config(),
                );
                cfg.create_pool(Some(Runtime::Tokio1), connector)
            }
        }
        .map_err(|e| anyhow::anyhow!("PgBillingSink: cannot build pool: {e}"))?;

        let client = pool.get().await.map_err(|e| {
            anyhow::anyhow!("PgBillingSink: cannot acquire connection for DDL: {e}")
        })?;
        client
            .batch_execute(&format!("BEGIN; {DDL} COMMIT;"))
            .await
            .map_err(|e| anyhow::anyhow!("PgBillingSink: DDL failed (fail-closed): {e}"))?;

        let handle = Handle::try_current()
            .map_err(|e| anyhow::anyhow!("PgBillingSink: must be built on a Tokio runtime: {e}"))?;
        // FAIL-CLOSED flavor check (audit P2): `block_on` bridges sync→async with
        // `block_in_place`, which PANICS on a current-thread runtime. Today the
        // server is multi-thread (bare `#[tokio::main]`), but that is a non-local
        // invariant; assert it HERE at connect (boot) so a misconfiguration is a
        // clear boot error, never a first-export-tick panic that silently kills
        // the detached billing loop.
        if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
            anyhow::bail!(
                "PgBillingSink requires a multi-thread Tokio runtime (its sync→async \
                 bridge uses block_in_place); the current runtime flavor is {:?}",
                handle.runtime_flavor()
            );
        }
        Ok(Self { pool, handle })
    }

    /// Run an async body to completion, bridging the sync trait to the async
    /// client (mirrors `PgLedger::block_on`). On a runtime worker → move the
    /// blocking off the worker with `block_in_place`; off a worker → block
    /// directly (it starves nothing).
    fn block_on<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let handle = self.handle.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(move || handle.block_on(fut)),
            Err(_) => handle.block_on(fut),
        }
    }

    /// TRUNCATE the table — test-only helper for the integration harness.
    #[cfg(test)]
    pub fn truncate_for_test(&self) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client
                .batch_execute("TRUNCATE TABLE billing_events")
                .await?;
            Ok::<_, anyhow::Error>(())
        })
    }
}

impl BillingSink for PgBillingSink {
    fn persist(&self, events: &[SlotOccupancyEvent], exported_at_ms: u64) -> anyhow::Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        // Collapse intra-batch key duplicates (first occurrence wins) BEFORE the
        // INSERT, so the batch carries each `(tenant, lease_id, kind, at_ms)` at
        // most once. This makes the PG path provably equivalent to
        // `MemBillingSink` (both count distinct NEW keys) and avoids relying on
        // ON CONFLICT's intra-statement duplicate handling. Not arithmetic — a
        // set membership filter over the natural key.
        let mut seen_in_batch: HashSet<EventKey> = HashSet::with_capacity(events.len());
        let events: Vec<&SlotOccupancyEvent> = events
            .iter()
            .filter(|ev| seen_in_batch.insert(event_key(ev)))
            .collect();
        let exported = exported_at_ms as i64;
        self.block_on(async {
            let client = self.pool.get().await?;
            // One batched multi-row INSERT. Each event contributes a 5-tuple of
            // bind params; the placeholder list is generated to match. ON
            // CONFLICT DO NOTHING + RETURNING means the returned rows are
            // EXACTLY the newly inserted ones — re-exporting the resident window
            // (this instance or another) inserts zero and returns zero.
            let mut sql = String::from(
                "INSERT INTO billing_events \
                   (tenant, lease_id, kind, at_ms, exported_at_ms) VALUES ",
            );
            // Owned param values, then a parallel Vec of trait-object refs.
            let mut tenants: Vec<String> = Vec::with_capacity(events.len());
            let mut leases: Vec<String> = Vec::with_capacity(events.len());
            let mut kinds: Vec<&'static str> = Vec::with_capacity(events.len());
            let mut ats: Vec<i64> = Vec::with_capacity(events.len());
            for (i, ev) in events.iter().enumerate() {
                if i > 0 {
                    sql.push_str(", ");
                }
                let base = i * 5;
                // at_ms is bound VERBATIM (no subtraction): it is part of the
                // key, never a duration. exported_at_ms ($base+5) is the same
                // stamp for the whole batch.
                use std::fmt::Write as _;
                let _ = write!(
                    sql,
                    "(${}, ${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4,
                    base + 5,
                );
                tenants.push(ev.tenant.as_str().to_string());
                leases.push(ev.lease_id.clone());
                kinds.push(kind_to_db(ev.kind));
                ats.push(ev.at_ms as i64);
            }
            sql.push_str(
                " ON CONFLICT (tenant, lease_id, kind, at_ms) DO NOTHING \
                 RETURNING tenant",
            );

            let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                Vec::with_capacity(events.len() * 5);
            for i in 0..events.len() {
                params.push(&tenants[i]);
                params.push(&leases[i]);
                params.push(&kinds[i]);
                params.push(&ats[i]);
                params.push(&exported);
            }

            let rows = client.query(sql.as_str(), &params).await?;
            Ok::<usize, anyhow::Error>(rows.len())
        })
    }
}

/// What a single [`Exporter::export_once`] tick did.
///
/// Pure & deterministic given the meter state + the sink state + the previous
/// `journal_dropped` watermark. Carries NO duration / cost — just raw counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportReport {
    /// Rows NEWLY inserted this tick (the sink's `persist` return).
    pub persisted: usize,
    /// Resident journal events that were already present (ON CONFLICT skips).
    pub skipped_existing: usize,
    /// How many events the meter dropped (aged out of the bounded journal)
    /// SINCE the previous tick — `> 0` means events were lost before export and
    /// the caller's loop should WARN (ops alarm). NOT recovered here.
    pub journal_dropped: u64,
}

/// A small holder that tracks the previous `journal_dropped` watermark across
/// ticks so each [`Exporter::export_once`] can report the delta (the caller
/// loop WARNs when it is `> 0`).
#[derive(Debug, Default)]
pub struct Exporter {
    /// The meter's `journal_dropped()` as of the last tick — the watermark the
    /// delta is measured against.
    prev_journal_dropped: u64,
}

impl Exporter {
    /// A fresh exporter; the watermark starts at 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Export the meter's resident journal window into `sink` ONCE.
    ///
    /// Reads `meter.journal()` (read-only; the journal stays intact —
    /// non-destructive), batch-upserts it via `sink.persist`, and reports the
    /// `journal_dropped` delta since the previous tick (so the caller can WARN
    /// when events aged out before export). Pure & deterministic given the
    /// meter, the sink, the watermark, and `now_ms`.
    ///
    /// Dropped events are NOT recovered — the PRIMARY KEY + frequent ticks are
    /// the design. There is no duration / cost math here.
    pub fn export_once(
        &mut self,
        meter: &crate::billing::SlotMeter,
        sink: &dyn BillingSink,
        now_ms: u64,
    ) -> anyhow::Result<ExportReport> {
        // Read-only, non-destructive: the journal is borrowed, never drained.
        let events: Vec<SlotOccupancyEvent> = meter.journal().iter().cloned().collect();
        let total = events.len();
        let persisted = sink.persist(&events, now_ms)?;
        // Every resident event not newly inserted was a conflict (already
        // persisted) — the multi-instance-safe re-export of the window.
        let skipped_existing = total - persisted;

        // Drop-delta since the last tick: a positive delta is the ops alarm.
        let now_dropped = meter.journal_dropped();
        let dropped_delta = now_dropped.saturating_sub(self.prev_journal_dropped);
        self.prev_journal_dropped = now_dropped;

        Ok(ExportReport {
            persisted,
            skipped_existing,
            journal_dropped: dropped_delta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::SlotMeter;

    fn tenant(s: &str) -> TenantId {
        TenantId::new(s).expect("valid tenant id")
    }

    fn ev(t: &TenantId, lease: &str, kind: SlotEventKind, at_ms: u64) -> SlotOccupancyEvent {
        SlotOccupancyEvent {
            tenant: t.clone(),
            lease_id: lease.to_string(),
            kind,
            at_ms,
        }
    }

    /// Fill a meter with a small, varied event set.
    fn meter_with_n_events(t: &TenantId) -> (SlotMeter, usize) {
        let mut m = SlotMeter::new();
        m.record(ev(t, "l1", SlotEventKind::Acquired, 1));
        m.record(ev(t, "l1", SlotEventKind::Released, 2));
        m.record(ev(t, "l2", SlotEventKind::Acquired, 3));
        m.record(ev(t, "l2", SlotEventKind::Expired, 4));
        m.record(ev(t, "l3", SlotEventKind::Acquired, 5));
        m.record(ev(t, "l3", SlotEventKind::Crashed, 6));
        (m, 6)
    }

    /// A1: `export_once` persists ALL N journal events to the sink (exact count
    /// + exact membership).
    #[test]
    fn a1_export_persists_all_journal_events() {
        let t = tenant("acme");
        let (m, n) = meter_with_n_events(&t);
        let sink = MemBillingSink::new();
        let mut exp = Exporter::new();

        let report = exp.export_once(&m, &sink, 1_000).expect("export");
        assert_eq!(report.persisted, n, "all N events persisted");
        assert_eq!(report.skipped_existing, 0, "nothing pre-existed");
        assert_eq!(sink.len(), n, "sink holds exactly N rows");
        // Exact membership: every journaled event is in the sink.
        for e in m.journal() {
            assert!(sink.contains(e), "missing event in sink: {e:?}");
        }
        // Journal stays intact (non-destructive read).
        assert_eq!(m.journal().len(), n, "journal not drained by export");
    }

    /// A2: exactly-once — exporting the SAME meter twice persists N total; the
    /// second tick reports `persisted == 0`, `skipped_existing == N` (ON
    /// CONFLICT dedup via the natural key).
    #[test]
    fn a2_exactly_once_second_export_is_all_conflicts() {
        let t = tenant("acme");
        let (m, n) = meter_with_n_events(&t);
        let sink = MemBillingSink::new();
        let mut exp = Exporter::new();

        let first = exp.export_once(&m, &sink, 1_000).expect("first export");
        assert_eq!(first.persisted, n);

        let second = exp.export_once(&m, &sink, 2_000).expect("second export");
        assert_eq!(second.persisted, 0, "re-export inserts nothing");
        assert_eq!(second.skipped_existing, n, "every event was a conflict");
        assert_eq!(sink.len(), n, "still exactly N rows — no duplicates");
    }

    /// A3: multi-instance-safe — two independent sinks/exporters over
    /// OVERLAPPING event sets converge to the union with no duplicates when both
    /// drain into the SAME sink (the ON CONFLICT model). Models two control-
    /// plane instances upserting overlapping resident windows.
    #[test]
    fn a3_multi_instance_overlapping_windows_converge_to_union() {
        let t = tenant("acme");
        // Instance A's window: events 1..=4.
        let mut ma = SlotMeter::new();
        ma.record(ev(&t, "l1", SlotEventKind::Acquired, 1));
        ma.record(ev(&t, "l1", SlotEventKind::Released, 2));
        ma.record(ev(&t, "l2", SlotEventKind::Acquired, 3));
        ma.record(ev(&t, "l2", SlotEventKind::Released, 4));
        // Instance B's window: events 3..=6 — OVERLAPS A on 3 and 4.
        let mut mb = SlotMeter::new();
        mb.record(ev(&t, "l2", SlotEventKind::Acquired, 3));
        mb.record(ev(&t, "l2", SlotEventKind::Released, 4));
        mb.record(ev(&t, "l3", SlotEventKind::Acquired, 5));
        mb.record(ev(&t, "l3", SlotEventKind::Crashed, 6));

        // ONE shared durable sink; two independent exporters (two instances).
        let sink = MemBillingSink::new();
        let mut exp_a = Exporter::new();
        let mut exp_b = Exporter::new();

        let ra = exp_a.export_once(&ma, &sink, 100).expect("A export");
        let rb = exp_b.export_once(&mb, &sink, 200).expect("B export");

        // A inserts all 4; B inserts only its 2 non-overlapping (5, 6) — the
        // overlap (3, 4) conflicts.
        assert_eq!(ra.persisted, 4);
        assert_eq!(rb.persisted, 2, "overlap deduped via ON CONFLICT");
        assert_eq!(
            rb.skipped_existing, 2,
            "the 2 overlapping events conflicted"
        );
        // The union is exactly 6 distinct events (1..=6), no duplicates.
        assert_eq!(sink.len(), 6, "converged to the union, no duplicates");

        // Order-independence: replaying both windows again is a total no-op.
        let ra2 = exp_a.export_once(&ma, &sink, 300).expect("A re-export");
        let rb2 = exp_b.export_once(&mb, &sink, 400).expect("B re-export");
        assert_eq!(ra2.persisted, 0);
        assert_eq!(rb2.persisted, 0);
        assert_eq!(sink.len(), 6, "still the union — idempotent");
    }

    /// A5: when `journal_dropped` increases between ticks, the report's
    /// `journal_dropped` reflects the DELTA (so the caller's loop can WARN). The
    /// watermark advances, so a subsequent tick with no new drops reports 0.
    #[test]
    fn a5_report_reflects_journal_dropped_delta() {
        let t = tenant("acme");
        let mut m = SlotMeter::new();
        let sink = MemBillingSink::new();
        let mut exp = Exporter::new();

        // Tick 1: no drops yet.
        m.record(ev(&t, "l1", SlotEventKind::Acquired, 1));
        let r1 = exp.export_once(&m, &sink, 10).expect("tick1");
        assert_eq!(r1.journal_dropped, 0, "no drops on the first tick");

        // Force the journal past its cap so events age out — drive it via the
        // PUBLIC API only (record), recording until `journal_dropped` rises
        // (the cap constant is private to billing.rs; we never depend on its
        // exact value). Then export and assert the delta equals what dropped
        // since tick 1.
        let mut i = 0u64;
        while m.journal_dropped() == 0 {
            let kind = if i.is_multiple_of(2) {
                SlotEventKind::Acquired
            } else {
                SlotEventKind::Released
            };
            m.record(ev(&t, "lx", kind, 100 + i));
            i += 1;
        }
        let dropped_now = m.journal_dropped();
        assert!(
            dropped_now > 0,
            "the journal must have dropped (cap exceeded)"
        );

        let r2 = exp.export_once(&m, &sink, 20).expect("tick2");
        assert_eq!(
            r2.journal_dropped, dropped_now,
            "report carries the full drop delta since the last tick"
        );

        // Tick 3: no new drops → delta back to 0 (watermark advanced).
        let r3 = exp.export_once(&m, &sink, 30).expect("tick3");
        assert_eq!(r3.journal_dropped, 0, "watermark advanced — no new drops");
    }

    /// A7 (charter): source oracle — the production half of this module derives
    /// NO billable time: no per-minute / minutes math, no cost / invoicing, and
    /// (the load-bearing one) NO subtraction of timestamps to synthesize a
    /// duration. The sink persists raw occupancy events only; `at_ms` is a key
    /// component, never a delta. (Concurrency pricing, never per-minute.)
    ///
    /// Note: the std `Duration` TYPE is legitimate infra (the pool-acquire
    /// timeout) and is NOT billing-duration arithmetic — so the oracle targets
    /// the actual anti-patterns (minutes / per-minute / cost / invoice / `at_ms`
    /// subtraction), not the bare word "duration".
    #[test]
    fn sink_has_no_billable_time_math() {
        let source = include_str!("billing_sink.rs");
        let marker = ["#[cfg(te", "st)]"].concat();
        let production = source
            .split(&marker)
            .next()
            .expect("split always yields the production half");
        let minutes_needle = ["min", "utes"].concat();
        let per_min_needle = ["per", "_min"].concat();
        let cost_needle = ["cost", "_usd"].concat();
        let invoice_needle = ["invoic", "e"].concat();
        let billable_needle = ["bill", "able"].concat();
        for line in production.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue; // prose may name the words it forbids
            }
            let code = trimmed.split("//").next().unwrap_or("").to_lowercase();
            for (needle, label) in [
                (&minutes_needle, "minutes"),
                (&per_min_needle, "per-minute"),
                (&cost_needle, "cost"),
                (&invoice_needle, "invoice"),
                (&billable_needle, "billable-time"),
            ] {
                assert!(
                    !code.contains(needle),
                    "billing sink must have no {label} arithmetic/code: {line:?}"
                );
            }
            // The load-bearing check: no subtraction of timestamps (`at_ms` is a
            // key, never a synthesized duration delta).
            assert!(
                !code.contains("at_ms -") && !code.contains("at_ms-"),
                "billing sink must never subtract at_ms (no synthesized duration): {line:?}"
            );
        }
    }

    /// The empty-journal case: exporting a fresh meter persists nothing and
    /// reports zeros (no panic, no drop alarm).
    #[test]
    fn empty_journal_exports_nothing() {
        let m = SlotMeter::new();
        let sink = MemBillingSink::new();
        let mut exp = Exporter::new();
        let r = exp.export_once(&m, &sink, 1).expect("export");
        assert_eq!(r.persisted, 0);
        assert_eq!(r.skipped_existing, 0);
        assert_eq!(r.journal_dropped, 0);
        assert!(sink.is_empty());
    }

    // ── PgBillingSink: gated on TEST_DATABASE_URL (mirrors ledger_conformance's
    //    pg_runs — skip cleanly when the env var is absent). ────────────────────
    mod pg_runs {
        use super::*;
        use crate::pg_ledger::PgTlsMode;

        /// `TEST_DATABASE_URL`, or `None` (the gate is OFF — skip cleanly).
        fn db_url() -> Option<String> {
            std::env::var("TEST_DATABASE_URL").ok()
        }

        /// A multi-thread runtime (required for `PgBillingSink`'s
        /// `block_in_place`).
        fn rt() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("multi-thread runtime")
        }

        fn connect(rt: &tokio::runtime::Runtime, url: &str) -> PgBillingSink {
            rt.block_on(async {
                PgBillingSink::connect(url, 4, PgTlsMode::Disable)
                    .await
                    .expect("PgBillingSink::connect")
            })
        }

        /// PG: real DDL + idempotent upsert against Postgres — a full export-
        /// once, a re-export (exactly-once), and a second instance over an
        /// overlapping window (multi-instance-safe union). Skips cleanly without
        /// a DB so CI stays green.
        #[test]
        fn pg_billing_sink_idempotent_upsert() {
            let Some(url) = db_url() else {
                eprintln!(
                    "pg_billing_sink_idempotent_upsert: TEST_DATABASE_URL unset — \
                     skipping (expected on CI)"
                );
                return;
            };
            let rt = rt();
            let sink = connect(&rt, &url);
            sink.truncate_for_test().expect("truncate");

            // Unique tenant per run so parallel processes don't collide.
            let tenant_str = format!(
                "bill-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let t = tenant(&tenant_str);
            let (m, n) = meter_with_n_events(&t);
            let mut exp = Exporter::new();

            // First export: all N persist.
            let r1 = exp.export_once(&m, &sink, 1_000).expect("first export");
            assert_eq!(r1.persisted, n, "all N rows inserted against real PG");

            // Re-export the SAME window: ON CONFLICT → zero inserted.
            let r2 = exp.export_once(&m, &sink, 2_000).expect("re-export");
            assert_eq!(r2.persisted, 0, "idempotent upsert — no duplicate rows");
            assert_eq!(r2.skipped_existing, n);

            // A second INDEPENDENT sink (another instance) over an overlapping
            // window converges to the union, no duplicates.
            let sink_b = connect(&rt, &url);
            let mut mb = SlotMeter::new();
            // Overlaps m on (l3, Crashed, 6); adds one new event.
            mb.record(ev(&t, "l3", SlotEventKind::Crashed, 6));
            mb.record(ev(&t, "l9", SlotEventKind::Acquired, 99));
            let mut exp_b = Exporter::new();
            let rb = exp_b.export_once(&mb, &sink_b, 3_000).expect("instance B");
            assert_eq!(rb.persisted, 1, "only the one new event inserts");
            assert_eq!(rb.skipped_existing, 1, "the overlap conflicts");

            // Direct DB count: exactly N + 1 distinct rows for this tenant.
            let total = rt.block_on(async {
                let client = sink.pool.get().await.expect("client");
                let row = client
                    .query_one(
                        "SELECT count(*)::bigint AS c FROM billing_events WHERE tenant = $1",
                        &[&t.as_str()],
                    )
                    .await
                    .expect("count");
                row.get::<_, i64>("c")
            });
            assert_eq!(
                total,
                (n + 1) as i64,
                "converged union, no duplicates in PG"
            );
        }
    }
}
