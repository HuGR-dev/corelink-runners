//! Production binary (composition root) for the CoreLink Runners fabric server.
//!
//! Reads configuration from environment variables (see [`corelink_fabric_server::server`]),
//! assembles every seam via [`corelink_fabric_server::server::build_app`], and
//! serves the axum router on the configured TCP address.

use corelink_fabric_server::server::{build_app_and_state, config_from_env};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config_from_env(|k| std::env::var(k).ok())?;

    let (app, state) = build_app_and_state(&cfg)?;
    let reaper_cfg =
        corelink_fabric_server::reaper::reaper_config_from_env(|k| std::env::var(k).ok())?;
    let reaper_interval = reaper_cfg.interval;
    // Opt-in crash-surfacing sweep (FABRIC_CRASH_PROBE_INTERVAL_SECS). Cloned
    // state (all-Arc, cheap) BEFORE the reaper consumes `state`; absent env var
    // → None (not spawned) — the always-on deadline reaper remains the backstop.
    let crash_sweep_handle =
        corelink_fabric_server::server::maybe_spawn_crash_sweep_from_env(state.clone(), |k| {
            std::env::var(k).ok()
        })?;
    // ── CP4 queued fair admission (ADR-0005) — DEFAULT-OFF. The admission loop
    // is spawned ONLY under `FABRIC_ADMISSION_MODE=queue`, over a clone of the
    // SAME shared `AppState` (so it ticks the queue the handlers enqueue into).
    // Cloned BEFORE the reaper consumes `state`; absent/`reject` → None.
    let admission_handle = match cfg.admission_mode {
        corelink_fabric_server::admission::AdmissionMode::Queue => {
            Some(corelink_fabric_server::admission::spawn_admission_loop(
                state.clone(),
                cfg.admission_tick_interval,
            ))
        }
        corelink_fabric_server::admission::AdmissionMode::Reject => None,
    };
    // ── WP-A durable billing exporter — DEFAULT-OFF. Spawned ONLY under
    // `FABRIC_BILLING_EXPORT_INTERVAL_SECS` (which requires the pg ledger).
    // Borrows `state` (no clone) BEFORE the reaper consumes it; connect is async
    // (applies the sink DDL) so it propagates a fail-closed boot error.
    let billing_handle =
        corelink_fabric_server::server::maybe_spawn_billing_exporter(&state, &cfg).await?;
    // ── task #11 quota-headroom monitor — DEFAULT-OFF. Spawned ONLY when
    // `FABRIC_QUOTA_CHECK_INTERVAL_SECS` is set (opt-in). Advisory only —
    // adjusts nothing; logs structured QUOTA_HEADROOM_WARNING /
    // QUOTA_HEADROOM_EXCEEDED lines. Cloned BEFORE the reaper consumes `state`.
    let quota_headroom_handle = {
        let qcfg = corelink_fabric_server::quota_headroom::quota_check_config_from_env(|k| {
            std::env::var(k).ok()
        })?;
        qcfg.map(|c| {
            corelink_fabric_server::quota_headroom::spawn_quota_headroom_task(state.clone(), c)
        })
    };
    // ── ASK-2 billing usage-push FLUSH driver — DEFAULT-OFF. Spawned ONLY when
    // BILLING_INGEST_URL is set (the same gate that wires the real push target).
    // Borrows `&state` BEFORE the reaper consumes it; drives the buffered target's
    // batch POST every FABRIC_BILLING_PUSH_INTERVAL_SECS (default 30s).
    let (billing_push_handle, billing_push_target) =
        match corelink_fabric_server::server::maybe_spawn_billing_push_flush(&state, |k| {
            std::env::var(k).ok()
        }) {
            Some((h, t)) => (Some(h), Some(t)),
            None => (None, None),
        };
    let pending_max_age =
        corelink_fabric_server::reaper::pending_max_age_from_env(|k| std::env::var(k).ok())?;
    let reaper_handle = corelink_fabric_server::reaper::spawn_reaper_with_pending_age(
        state,
        reaper_cfg,
        pending_max_age,
    );

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;

    if cfg.mock_exec {
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║  MOCK EXECUTION BACKEND ACTIVE                               ║");
        eprintln!("║                                                              ║");
        eprintln!("║  Every exec returns a FAKE deterministic CheckResult.        ║");
        eprintln!("║  FOR OFFLINE ADAPTER DEVELOPMENT ONLY.                       ║");
        eprintln!("║  NEVER PRODUCTION.                                           ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
        eprintln!();
    }

    eprintln!("corelink-fabricd listening on {}", cfg.bind_addr);
    if cfg.mock_exec {
        eprintln!("cloud backend: MOCK (deterministic stub, offline adapter dev only)");
    } else {
        // Report the exec/spawn backend the wiring ACTUALLY resolved, in the
        // SAME 4-way selection as `build_app_and_state` (server.rs): BOTH present
        // ⇒ HYBRID (runner→Cloudflare, check-exec→Northflank, rota B); Cloudflare
        // only ⇒ Cloudflare (runner-only); Northflank only ⇒ Northflank; neither
        // ⇒ NoBox (every exec 503). Reporting the real selection means the log can
        // never claim "execs will 503" while a substrate is actually wired.
        use corelink_fabric_server::cloud_exec::{CloudBackendStatus, cloud_backend_status};
        let nf_status = cloud_backend_status(|k| std::env::var(k).ok());
        let nf_wired = matches!(nf_status, CloudBackendStatus::Wired);
        match corelink_cloud_engine::CloudflareConfig::from_env() {
            // Cloudflare env present. The `validate` only sharpens the operator
            // debug loop — the per-spawn floor in CloudflareEngine::spawn is the
            // hard backstop, so a WARN here does NOT hard-fail.
            Some(cf) => match cf.validate() {
                Ok(()) if nf_wired => eprintln!(
                    "exec backend: HYBRID (rota B) — runner→Cloudflare (url={}, disk={} MiB), \
                     check-exec→Northflank",
                    cf.spawn_worker_url, cf.runner_storage_mb
                ),
                Ok(()) => eprintln!(
                    "exec backend: Cloudflare substrate (url={}, disk={} MiB) — runner-only; \
                     a check-exec lease fails closed at spawn (set NORTHFLANK_* to serve checks)",
                    cf.spawn_worker_url, cf.runner_storage_mb
                ),
                Err(why) => {
                    eprintln!();
                    eprintln!(
                        "WARNING [R2]: CLOUDFLARE SUBSTRATE MISCONFIGURED — spawns WILL FAIL"
                    );
                    eprintln!("  {why}");
                    eprintln!();
                }
            },
            // No Cloudflare env ⇒ Northflank; ELSE NoBox. ONLY here (both
            // substrates off) can "execs will 503" be true.
            None => match nf_status {
                CloudBackendStatus::Wired => eprintln!(
                    "exec backend: Northflank (NORTHFLANK_API_TOKEN + NORTHFLANK_PROJECT_ID set)"
                ),
                CloudBackendStatus::PartialConfig { present, missing } => eprintln!(
                    "exec backend: NONE — {present} is set but {missing} is missing/empty; \
                     cloud exec is OFF and every exec will 503. Set {missing} to enable it."
                ),
                CloudBackendStatus::Off => eprintln!(
                    "exec backend: NONE — no CLOUDFLARE_SPAWN_* or NORTHFLANK_* configured; \
                     execs will 503 (fail-closed)"
                ),
            },
        }
    }
    // Report which lease ledger the wiring actually resolved (WP-4).  Never
    // print database_url — it may carry a password.
    use corelink_fabric::PgTlsMode;
    use corelink_fabric_server::server::LedgerBackend;
    match cfg.ledger_backend {
        LedgerBackend::Memory => {
            eprintln!("ledger backend: in-memory (leases reset on restart; single-instance only)")
        }
        LedgerBackend::Postgres => {
            // WP-B: report the resolved transport so an operator can confirm at a
            // glance whether the managed-PG connection is encrypted.
            let tls = match cfg.pg_tls {
                PgTlsMode::Disable => "tls=disable (plaintext NoTls)",
                PgTlsMode::Require => "tls=require (verify-full rustls, public-CA)",
            };
            eprintln!(
                "ledger backend: Postgres (persistent, multi-instance cap-safe; pool={}; {tls})",
                cfg.ledger_pool_size
            )
        }
    }
    eprintln!("reaper: started (interval={}s)", reaper_interval.as_secs());
    match &crash_sweep_handle {
        Some(_) => eprintln!("crash-sweep: started (FABRIC_CRASH_PROBE_INTERVAL_SECS set)"),
        None => eprintln!("crash-sweep: OFF (set FABRIC_CRASH_PROBE_INTERVAL_SECS to enable)"),
    }
    match &admission_handle {
        Some(_) => eprintln!(
            "admission: QUEUE mode (FABRIC_ADMISSION_MODE=queue; fair-queued over-cap acquires)"
        ),
        None => eprintln!("admission: reject mode (default; immediate-or-reject)"),
    }
    match &billing_handle {
        Some(_) => eprintln!(
            "billing-export: started (FABRIC_BILLING_EXPORT_INTERVAL_SECS set; SlotMeter → \
             durable billing_events table)"
        ),
        None => eprintln!(
            "billing-export: OFF (set FABRIC_BILLING_EXPORT_INTERVAL_SECS, requires pg ledger, to enable)"
        ),
    }
    match &billing_push_handle {
        Some(_) => eprintln!(
            "billing-push: started (BILLING_INGEST_URL set; per-lease usage → corelink-billing ingest)"
        ),
        None => eprintln!(
            "billing-push: OFF (set BILLING_INGEST_URL + BILLING_INGEST_AUTH_KEY + 3-char BILLING_REGION to enable)"
        ),
    }
    match &quota_headroom_handle {
        Some(_) => eprintln!(
            "quota-headroom: started (FABRIC_QUOTA_CHECK_INTERVAL_SECS set; advisory disk-headroom monitor)"
        ),
        None => eprintln!(
            "quota-headroom: OFF (set FABRIC_QUOTA_CHECK_INTERVAL_SECS to enable disk-headroom monitoring)"
        ),
    }
    // WP-C: report whether runtime tenant onboarding is armed (static mode only).
    match (&cfg.auth_backend, &cfg.admin_key) {
        (corelink_fabric_server::server::AuthBackend::Static, Some(_)) => {
            eprintln!(
                "admin-onboarding: armed (POST /internal/v1/admin/tenants, FABRIC_ADMIN_KEY set)"
            )
        }
        (corelink_fabric_server::server::AuthBackend::Static, None) => eprintln!(
            "admin-onboarding: OFF (set FABRIC_ADMIN_KEY to arm POST /internal/v1/admin/tenants)"
        ),
        _ => eprintln!(
            "admin-onboarding: n/a (CoreLink auth backend — plans come from introspection)"
        ),
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Shutdown: abort the background sweeps so they do not outlive the server.
    reaper_handle.abort();
    if let Some(h) = crash_sweep_handle {
        h.abort();
    }
    // CP4 (ADR-0005): abort the admission loop too (only set under queue mode).
    if let Some(h) = admission_handle {
        h.abort();
    }
    // WP-A: abort the billing exporter (only set under FABRIC_BILLING_EXPORT_*).
    if let Some(h) = billing_handle {
        h.abort();
    }
    // task #11: abort the quota-headroom monitor (only set under FABRIC_QUOTA_CHECK_INTERVAL_SECS).
    if let Some(h) = quota_headroom_handle {
        h.abort();
    }
    // ASK-2: abort the billing usage-push flush driver (only set under BILLING_INGEST_URL).
    if let Some(h) = billing_push_handle {
        // Final flush BEFORE abort (audit r4 #8): drain terminal-lease usage events
        // buffered since the last ~30s tick. `flush` is a sync blocking POST →
        // block_in_place (legal on the multi-thread runtime); bounded by the
        // target's own HTTP timeout. A failure is logged (idem_key makes the next
        // retry idempotent) and never blocks shutdown beyond that bound.
        if let Some(t) = billing_push_target {
            tokio::task::block_in_place(|| corelink_fabric::BillingExportTarget::flush(&*t))
                .unwrap_or_else(|e| {
                    eprintln!(
                        "final billing usage-push flush failed on shutdown: {e} \
                         (idem_key makes a retry idempotent)"
                    );
                });
        }
        h.abort();
    }
    Ok(())
}

/// Await SIGTERM or ctrl-c for graceful shutdown (bounded mid-acquire leak).
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }

    eprintln!("corelink-fabricd: shutdown signal received, draining…");
}
