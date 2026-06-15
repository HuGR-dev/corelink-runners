//! WP-A billing-exporter spawn loop (composition glue).
//!
//! Drains the in-memory [`SlotMeter`] journal into the durable
//! [`BillingSink`](corelink_fabric::BillingSink) every interval. The journal is
//! snapshotted UNDER the meter lock (a cheap clone), then persisted OUTSIDE the
//! lock — so a slow DB write never blocks the acquire / close / reap paths that
//! record slot events. The sink's `INSERT … ON CONFLICT DO NOTHING` (keyed on
//! the natural PK) makes re-exporting the resident journal window free and
//! multi-instance-safe; a failed tick loses nothing because the journal is
//! non-destructive (the window is re-tried next tick, as long as it has not aged
//! out — which is what the `journal_dropped` alarm surfaces).
//!
//! This is the OPT-IN server-side half of WP-A: the sink + the export-once logic
//! live in `corelink-fabric` (`billing_sink.rs`); the spawn + flag-gating
//! (`FABRIC_BILLING_EXPORT_INTERVAL_SECS`) are here + in `server.rs`. Default-off.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use corelink_fabric::{BillingSink, SlotMeter};

use crate::app::Clock;

/// Spawn the periodic billing-export loop over `meter`, persisting into `sink`
/// every `interval`. Returns the [`JoinHandle`](tokio::task::JoinHandle) the
/// composition root `.abort()`s on graceful shutdown (so the task never outlives
/// the process), exactly like the reaper / crash-sweep handles.
///
/// Must be spawned on a `rt-multi-thread` runtime: a [`BillingSink`] backed by
/// Postgres bridges sync→async with `block_in_place`, which is legal only on a
/// multi-thread worker — main.rs's `#[tokio::main]` provides exactly that.
pub fn spawn_export_loop(
    meter: Arc<Mutex<SlotMeter>>,
    clock: Arc<dyn Clock>,
    sink: Arc<dyn BillingSink + Send + Sync>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        // Skip missed ticks (a slow DB write must not pile up a tick backlog
        // that then hammers the DB — mirror the reaper's MissedTickBehavior).
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The `journal_dropped` watermark across ticks (the alarm is the delta).
        let mut prev_dropped: u64 = 0;
        loop {
            tick.tick().await;
            let now = clock.now_ms();

            // ── SUPERVISION (audit MEDIUM, two independent reviewers): the tick
            //    body is a detached `tokio::spawn` — an UNCAUGHT panic here would
            //    silently kill billing capture forever on a healthy-looking
            //    server (the `JoinHandle` is only `.abort()`ed, never awaited, so
            //    nothing observes the panic). Wrap the synchronous body in
            //    `catch_unwind` so a single bad tick logs LOUDLY and the loop
            //    survives — revenue capture never dies in silence. The body holds
            //    no lock across an `.await` (the only await is `tick.tick()`
            //    above), so catching here is sound.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Snapshot UNDER the lock (cheap clone), release BEFORE the DB
                // write — no lock is ever held across the persist I/O.
                let (events, now_dropped) = {
                    let m = meter.lock().unwrap_or_else(|p| p.into_inner());
                    (
                        m.journal().iter().cloned().collect::<Vec<_>>(),
                        m.journal_dropped(),
                    )
                };

                // Persist OUTSIDE the lock. A failed tick is logged and retried
                // next tick (nothing is lost: the journal is non-destructive and
                // the upsert is idempotent).
                if let Err(e) = sink.persist(&events, now) {
                    eprintln!("WARN billing-export: persist failed (retry next tick): {e}");
                }
                now_dropped
            }));

            let now_dropped = match outcome {
                Ok(d) => d,
                Err(_) => {
                    // A panicked tick: capture survives (loop continues). The
                    // watermark is NOT advanced, so the next successful tick
                    // re-attributes any drops since the last good tick.
                    eprintln!(
                        "ERROR billing-export: a tick PANICKED — capture continues (supervised); \
                         investigate the billing sink / meter immediately"
                    );
                    continue;
                }
            };

            let dropped_delta = now_dropped.saturating_sub(prev_dropped);
            prev_dropped = now_dropped;

            // ── Ops alarm: events aged out of the bounded journal before we
            //    could export them — they are NOT recoverable. Surface it so an
            //    operator tightens the interval or raises the journal cap.
            if dropped_delta > 0 {
                eprintln!(
                    "WARN billing-export: {dropped_delta} slot events aged out of the journal \
                     before export (ops alarm — increase export frequency \
                     [FABRIC_BILLING_EXPORT_INTERVAL_SECS] or the journal cap)"
                );
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use corelink_fabric::meter::{SlotEventKind, SlotOccupancyEvent};
    use corelink_fabric::{MemBillingSink, TenantId};

    use super::*;

    /// A test clock returning a fixed stamp (the exporter never does arithmetic
    /// on it — `now_ms` is just the `exported_at_ms` column value).
    struct FixedClock(AtomicU64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::Relaxed)
        }
    }

    fn ev(t: &TenantId, lease: &str, kind: SlotEventKind, at_ms: u64) -> SlotOccupancyEvent {
        SlotOccupancyEvent {
            tenant: t.clone(),
            lease_id: lease.to_string(),
            kind,
            at_ms,
        }
    }

    /// The spawned loop drains the meter journal into the sink on its ticks, and
    /// re-ticking the SAME resident window inserts nothing (idempotent) — proving
    /// the snapshot-then-persist wiring is correct against the MemBillingSink.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn export_loop_drains_journal_then_is_idempotent() {
        let t = TenantId::new("acme").unwrap();
        let meter = Arc::new(Mutex::new(SlotMeter::new()));
        {
            let mut m = meter.lock().unwrap();
            m.record(ev(&t, "l1", SlotEventKind::Acquired, 1));
            m.record(ev(&t, "l1", SlotEventKind::Released, 2));
            m.record(ev(&t, "l2", SlotEventKind::Acquired, 3));
        }
        let sink: Arc<MemBillingSink> = Arc::new(MemBillingSink::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock(AtomicU64::new(1_000)));

        let handle = spawn_export_loop(
            meter.clone(),
            clock,
            sink.clone() as Arc<dyn BillingSink + Send + Sync>,
            Duration::from_millis(10),
        );

        // Let a few ticks fire, then stop the loop.
        tokio::time::sleep(Duration::from_millis(60)).await;
        handle.abort();

        // The 3 journal events were persisted exactly once (idempotent across
        // the multiple ticks that ran over the same resident window).
        assert_eq!(sink.len(), 3, "all journal events persisted, no duplicates");
    }

    /// A sink that PANICS on its first `persist`, then delegates to an inner
    /// `MemBillingSink` — to prove the supervised loop survives a panicking tick.
    struct PanicOnceSink {
        panicked: std::sync::atomic::AtomicBool,
        inner: MemBillingSink,
    }
    impl BillingSink for PanicOnceSink {
        fn persist(
            &self,
            events: &[SlotOccupancyEvent],
            exported_at_ms: u64,
        ) -> anyhow::Result<usize> {
            if !self.panicked.swap(true, Ordering::SeqCst) {
                panic!("induced billing-sink panic on the first tick");
            }
            self.inner.persist(events, exported_at_ms)
        }
    }

    /// SUPERVISION (audit MEDIUM): a panic inside a tick must NOT kill the export
    /// loop — capture must survive and persist on a subsequent tick. Without the
    /// `catch_unwind` the task would die and `sink` would stay empty forever.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn export_loop_survives_a_panicking_tick() {
        let t = TenantId::new("acme").unwrap();
        let meter = Arc::new(Mutex::new(SlotMeter::new()));
        {
            let mut m = meter.lock().unwrap();
            m.record(ev(&t, "l1", SlotEventKind::Acquired, 1));
            m.record(ev(&t, "l1", SlotEventKind::Released, 2));
        }
        let sink: Arc<PanicOnceSink> = Arc::new(PanicOnceSink {
            panicked: std::sync::atomic::AtomicBool::new(false),
            inner: MemBillingSink::new(),
        });
        let clock: Arc<dyn Clock> = Arc::new(FixedClock(AtomicU64::new(1_000)));

        let handle = spawn_export_loop(
            meter.clone(),
            clock,
            sink.clone() as Arc<dyn BillingSink + Send + Sync>,
            Duration::from_millis(10),
        );

        // The FIRST tick panics; later ticks must still run and persist.
        tokio::time::sleep(Duration::from_millis(80)).await;
        handle.abort();

        assert!(
            sink.inner.len() >= 2,
            "loop must survive the panicking tick and persist on a later tick \
             (got {} rows)",
            sink.inner.len()
        );
    }
}
