//! Tenant-plan persistence seam (M1 WAVE-0 frozen anchor).
//!
//! The **root contract** for the M1 multi-tenant control plane: the cap source
//! of truth ([`TenantPlan`](crate::tenant::TenantPlan), org = tenant per
//! ADR-0002) must outlive any single instance. [`TenantPlanRepository`] is the
//! seam the WAVE-1/WAVE-2 work-packages build against — the durable Postgres
//! impl ([`PgTenantPlanRepository`], WP-PERSIST) lands later against the frozen
//! `tenant_plans` DDL declared in [`crate::pg_ledger`].
//!
//! ## Sync trait over an (eventually) async backend
//!
//! Mirrors the crate's existing persistence seams ([`LeaseLedger`] /
//! [`BillingSink`]): the trait is **SYNC** (`&self`, `anyhow::Result`). The
//! Postgres impl bridges to the async client with
//! [`tokio::task::block_in_place`] + `block_on` exactly like
//! [`PgLedger`](crate::pg_ledger::PgLedger). Keeping the trait sync means the
//! caps callers (CP2 admission) thread through the same `std::sync::Mutex`
//! guards as the ledger — no async colouring across the control plane.
//!
//! [`LeaseLedger`]: crate::ledger::LeaseLedger
//! [`BillingSink`]: crate::billing_sink::BillingSink

use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;
use tokio_postgres::NoTls;

use crate::pg_ledger::{PgTlsMode, TENANT_PLANS_DDL};
use crate::tenant::{TenantId, TenantPlan};

/// Durable persistence for per-tenant plan caps — the M1 cap source of truth.
///
/// `load_all` hydrates the in-memory cap registry at boot; `upsert` persists a
/// plan change (tenant lifecycle API / billing-tier sync). Fail-closed by the
/// crate convention: any backend error is an `Err`, never a silently empty load
/// (an empty `load_all` must mean "no plans", never "the DB was unreachable").
pub trait TenantPlanRepository {
    /// Load every persisted tenant plan. The hydration path for the in-memory
    /// cap registry at boot. An empty `Vec` means "no plans persisted", NOT a
    /// backend failure (which is an `Err`).
    fn load_all(&self) -> anyhow::Result<Vec<(TenantId, TenantPlan)>>;

    /// Persist (insert-or-replace) one tenant's plan. Keyed on the tenant; a
    /// repeat upsert overwrites the prior plan for that tenant.
    fn upsert(&self, tenant: &TenantId, plan: &TenantPlan) -> anyhow::Result<()>;
}

/// In-memory [`TenantPlanRepository`] test double — an `RwLock` map.
///
/// Lets the WAVE-1/WAVE-2 cap-registry + tenant-lifecycle logic be exercised
/// WITHOUT a live Postgres (mirrors [`InMemoryLedger`](crate::ledger::InMemoryLedger)
/// / [`MemBillingSink`](crate::billing_sink::MemBillingSink)). Not a production
/// store: the map dies with the process and is per-instance (not cross-instance
/// cap-safe) — the durable [`PgTenantPlanRepository`] (WP-PERSIST) is that.
#[derive(Debug, Default)]
pub struct InMemTenantPlanRepo {
    plans: std::sync::RwLock<std::collections::HashMap<TenantId, TenantPlan>>,
}

impl InMemTenantPlanRepo {
    /// A fresh, empty repository.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct tenant plans currently held.
    pub fn len(&self) -> usize {
        self.plans.read().expect("InMemTenantPlanRepo rwlock").len()
    }

    /// Whether the repository holds no plans.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TenantPlanRepository for InMemTenantPlanRepo {
    fn load_all(&self) -> anyhow::Result<Vec<(TenantId, TenantPlan)>> {
        let plans = self.plans.read().expect("InMemTenantPlanRepo rwlock");
        Ok(plans.iter().map(|(t, p)| (t.clone(), p.clone())).collect())
    }

    fn upsert(&self, tenant: &TenantId, plan: &TenantPlan) -> anyhow::Result<()> {
        let mut plans = self.plans.write().expect("InMemTenantPlanRepo rwlock");
        plans.insert(tenant.clone(), plan.clone());
        Ok(())
    }
}

/// Durable [`TenantPlanRepository`] over Postgres — the M1 self-serve
/// **entitlement cache** (WP-PERSIST).
///
/// The cap source of truth that survives a restart and lets the fabric admit
/// without round-tripping `corelink-server` on every acquire: `load_all`
/// hydrates the in-memory cap registry at boot, `upsert` writes a plan change
/// back. Cross-instance durable (every instance pointed at the same database
/// sees the same plans), where [`InMemTenantPlanRepo`] dies with the process.
///
/// Owns a [`deadpool_postgres::Pool`] and a [`tokio::runtime::Handle`] and
/// bridges the SYNC trait to the async client with `block_in_place` + `block_on`
/// — IDENTICAL to [`PgLedger`](crate::pg_ledger::PgLedger) and
/// [`PgBillingSink`](crate::billing_sink::PgBillingSink). Build one with
/// [`PgTenantPlanRepository::connect`] (it applies [`TENANT_PLANS_DDL`] once,
/// fail-closed).
///
/// ## Column ↔ field mapping
///
/// [`TenantPlan`] carries three fields; the `tenant_plans` table has more. The
/// three are mapped exactly:
///
/// - `tenant`               ↔ `tenant`               (PRIMARY KEY)
/// - `max_concurrency`      ↔ `max_concurrency`
/// - `rate_ceiling_per_min` ↔ `rate_ceiling_per_min`
///
/// The remaining columns have NO `TenantPlan` field and are owned by the
/// tier-aware lifecycle path (WP-TENANT-LIFECYCLE-API), NOT this cap cache:
///
/// - `tier` / `ceiling_vcpu_ms` — written by the lifecycle API; this `upsert`
///   NEVER clobbers an existing value (the `ON CONFLICT` set-list omits them),
///   and seeds a fresh row with the `NOT NULL` defaults (empty-string tier,
///   ceiling `0` = disabled sentinel) so a cap-only writer can create a row.
/// - `created_at_ms` — stamped once on first insert, preserved on update.
/// - `updated_at_ms` — stamped now on both insert and update.
///
/// `load_all` reads back ONLY the three `TenantPlan` columns.
pub struct PgTenantPlanRepository {
    pool: Pool,
    handle: Handle,
}

impl std::fmt::Debug for PgTenantPlanRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgTenantPlanRepository")
            .finish_non_exhaustive()
    }
}

impl PgTenantPlanRepository {
    /// Build the pool, capture the runtime handle, and apply the idempotent
    /// [`TENANT_PLANS_DDL`] once — the SAME place + style the ledger / billing
    /// sink apply their schema, so the `tenant_plans` table exists on connect.
    /// Fail-closed: a connection or DDL error → `Err` (never a half-open repo).
    ///
    /// `tls` selects the transport (WP-B), mirroring
    /// [`PgLedger::connect`](crate::pg_ledger::PgLedger::connect): [`PgTlsMode::Disable`]
    /// is the plaintext `NoTls` path, [`PgTlsMode::Require`] a verify-full rustls
    /// pool (resolve from the environment with
    /// [`pg_tls_mode_from_env`](crate::pg_ledger::pg_tls_mode_from_env)).
    ///
    /// Must be called from inside a Tokio `rt-multi-thread` runtime (the sync
    /// trait methods later rely on `block_in_place` on that runtime).
    pub async fn connect(
        database_url: &str,
        pool_size: usize,
        tls: PgTlsMode,
    ) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        cfg.url = Some(database_url.to_string());
        // Bound the pool-acquire wait → `get()` fails fast under exhaustion
        // rather than stalling forever (same rationale as PgLedger / PgBillingSink).
        let mut pool_cfg = deadpool_postgres::PoolConfig::new(pool_size);
        pool_cfg.timeouts.wait = Some(std::time::Duration::from_secs(5));
        cfg.pool = Some(pool_cfg);
        // TLS branch reuses the ledger's resolved mode + the one verify-full
        // config builder. `Require` builds a verify-full rustls connector;
        // `Disable` is the plaintext `NoTls` path.
        let pool = match tls {
            PgTlsMode::Disable => cfg.create_pool(Some(Runtime::Tokio1), NoTls),
            PgTlsMode::Require => {
                let connector = tokio_postgres_rustls::MakeRustlsConnect::new(
                    crate::pg_ledger::rustls_verify_full_config(),
                );
                cfg.create_pool(Some(Runtime::Tokio1), connector)
            }
        }
        .map_err(|e| anyhow::anyhow!("PgTenantPlanRepository: cannot build pool: {e}"))?;

        // Apply the frozen tenant_plans DDL once, fail-closed (same as the
        // ledger / billing sink: `IF NOT EXISTS`, wrapped in a txn).
        let client = pool.get().await.map_err(|e| {
            anyhow::anyhow!("PgTenantPlanRepository: cannot acquire connection for DDL: {e}")
        })?;
        client
            .batch_execute(&format!("BEGIN; {TENANT_PLANS_DDL} COMMIT;"))
            .await
            .map_err(|e| {
                anyhow::anyhow!("PgTenantPlanRepository: DDL failed (fail-closed): {e}")
            })?;

        let handle = Handle::try_current().map_err(|e| {
            anyhow::anyhow!("PgTenantPlanRepository: must be built on a Tokio runtime: {e}")
        })?;
        // FAIL-CLOSED flavor check (mirrors PgBillingSink): the sync→async
        // `block_on` bridge uses `block_in_place`, which PANICS on a
        // current-thread runtime. Assert the multi-thread invariant HERE at
        // connect (boot) so a misconfiguration is a clear boot error, not a
        // first-call panic.
        if handle.runtime_flavor() != tokio::runtime::RuntimeFlavor::MultiThread {
            anyhow::bail!(
                "PgTenantPlanRepository requires a multi-thread Tokio runtime (its \
                 sync→async bridge uses block_in_place); the current runtime flavor is {:?}",
                handle.runtime_flavor()
            );
        }
        Ok(Self { pool, handle })
    }

    /// Run an async body to completion, bridging the sync trait to the async
    /// client (mirrors `PgLedger::block_on` / `PgBillingSink::block_on`). On a
    /// runtime worker → move the blocking off the worker with `block_in_place`;
    /// off a worker → block directly (it starves nothing).
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

    /// Current Unix epoch ms — the `created_at_ms` / `updated_at_ms` stamp. The
    /// trait `upsert` carries no timestamp (frozen), so the durable impl stamps
    /// internally.
    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// TRUNCATE the table — test-only helper for the gated regression harness.
    #[cfg(test)]
    pub fn truncate_for_test(&self) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client.batch_execute("TRUNCATE TABLE tenant_plans").await?;
            Ok::<_, anyhow::Error>(())
        })
    }
}

impl TenantPlanRepository for PgTenantPlanRepository {
    fn load_all(&self) -> anyhow::Result<Vec<(TenantId, TenantPlan)>> {
        self.block_on(async {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT tenant, max_concurrency, rate_ceiling_per_min \
                     FROM tenant_plans ORDER BY tenant",
                    &[],
                )
                .await?;
            rows.iter()
                .map(|row| {
                    let tenant_raw: String = row.get("tenant");
                    let tenant = TenantId::new(tenant_raw)?;
                    // `int` columns ↔ `u32` fields: the DDL caps are non-negative
                    // by construction (set from u32 plan values); read as i32 and
                    // map back. A negative value would mean a corrupt write — guard
                    // fail-closed rather than wrap.
                    let max_concurrency: i32 = row.get("max_concurrency");
                    let rate_ceiling_per_min: i32 = row.get("rate_ceiling_per_min");
                    let max_concurrency = u32::try_from(max_concurrency).map_err(|_| {
                        anyhow::anyhow!(
                            "tenant_plans.max_concurrency {max_concurrency} is negative \
                             for tenant {tenant} (corrupt row; fail-closed)"
                        )
                    })?;
                    let rate_ceiling_per_min =
                        u32::try_from(rate_ceiling_per_min).map_err(|_| {
                            anyhow::anyhow!(
                                "tenant_plans.rate_ceiling_per_min {rate_ceiling_per_min} is \
                                 negative for tenant {tenant} (corrupt row; fail-closed)"
                            )
                        })?;
                    Ok((
                        tenant.clone(),
                        TenantPlan {
                            tenant,
                            max_concurrency,
                            rate_ceiling_per_min,
                            repo_allowlist: Vec::new(),
                        },
                    ))
                })
                .collect()
        })
    }

    fn upsert(&self, tenant: &TenantId, plan: &TenantPlan) -> anyhow::Result<()> {
        let now = Self::now_ms();
        self.block_on(async {
            let client = self.pool.get().await?;
            // INSERT-or-replace keyed on `tenant`. The set-list on conflict
            // touches ONLY the cap fields + `updated_at_ms`; `tier`,
            // `ceiling_vcpu_ms`, and `created_at_ms` are PRESERVED (owned by the
            // tier-aware lifecycle path, not this cap cache). A fresh insert seeds
            // the NOT-NULL `tier`/`ceiling_vcpu_ms` with their disabled defaults so
            // a cap-only writer can create the row.
            client
                .execute(
                    "INSERT INTO tenant_plans \
                       (tenant, tier, max_concurrency, rate_ceiling_per_min, \
                        ceiling_vcpu_ms, created_at_ms, updated_at_ms) \
                     VALUES ($1, '', $2, $3, 0, $4, $4) \
                     ON CONFLICT (tenant) DO UPDATE SET \
                       max_concurrency      = EXCLUDED.max_concurrency, \
                       rate_ceiling_per_min = EXCLUDED.rate_ceiling_per_min, \
                       updated_at_ms        = EXCLUDED.updated_at_ms",
                    &[
                        &tenant.as_str(),
                        &(plan.max_concurrency as i32),
                        &(plan.rate_ceiling_per_min as i32),
                        &now,
                    ],
                )
                .await?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(s: &str) -> TenantId {
        TenantId::new(s).expect("valid tenant id")
    }

    fn plan(t: &TenantId, max_concurrency: u32, rate_ceiling_per_min: u32) -> TenantPlan {
        TenantPlan {
            tenant: t.clone(),
            max_concurrency,
            rate_ceiling_per_min,
            repo_allowlist: Vec::new(),
        }
    }

    /// The root-contract round-trip: an upserted plan is loaded back verbatim,
    /// and a repeat upsert for the same tenant overwrites (never duplicates).
    #[test]
    fn upsert_then_load_round_trips() {
        let repo = InMemTenantPlanRepo::new();
        assert!(repo.is_empty(), "fresh repo holds no plans");

        let acme = tenant("acme");
        let beta = tenant("beta");
        let acme_plan = plan(&acme, 8, 60);
        let beta_plan = plan(&beta, 2, 10);

        repo.upsert(&acme, &acme_plan).expect("upsert acme");
        repo.upsert(&beta, &beta_plan).expect("upsert beta");
        assert_eq!(repo.len(), 2, "two distinct tenants");

        let loaded = repo.load_all().expect("load_all");
        assert_eq!(loaded.len(), 2, "load returns every persisted plan");
        let by_tenant: std::collections::HashMap<TenantId, TenantPlan> =
            loaded.into_iter().collect();
        assert_eq!(by_tenant.get(&acme), Some(&acme_plan), "acme round-trips");
        assert_eq!(by_tenant.get(&beta), Some(&beta_plan), "beta round-trips");

        // Repeat upsert overwrites the same tenant (no duplicate row).
        let acme_plan_v2 = plan(&acme, 16, 120);
        repo.upsert(&acme, &acme_plan_v2).expect("upsert acme v2");
        assert_eq!(repo.len(), 2, "still two tenants — overwrite, not insert");
        let reloaded: std::collections::HashMap<TenantId, TenantPlan> =
            repo.load_all().expect("reload").into_iter().collect();
        assert_eq!(
            reloaded.get(&acme),
            Some(&acme_plan_v2),
            "the overwriting plan is the one loaded back"
        );
    }

    // ── PgTenantPlanRepository: gated on TEST_DATABASE_URL ─────────────────────
    //
    // Mirrors `pg_ledger::compute_ceiling_pg_tests` / `billing_sink`'s Pg tests:
    // ABSENT (the builder Mac / CI has no Postgres) → print a skip line and return
    // early so the suite stays green; PRESENT → exercise the real DDL apply + the
    // INSERT…ON CONFLICT round-trip against a live database. A process-unique
    // tenant nonce keeps parallel test binaries from colliding on rows (no
    // TRUNCATE needed → no cross-test interference).
    mod pg {
        use super::*;

        fn db_url() -> Option<String> {
            std::env::var("TEST_DATABASE_URL").ok()
        }

        fn rt() -> tokio::runtime::Runtime {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("multi-thread runtime")
        }

        fn connect(rt: &tokio::runtime::Runtime, url: &str) -> PgTenantPlanRepository {
            rt.block_on(async {
                PgTenantPlanRepository::connect(url, 4, crate::pg_ledger::PgTlsMode::Disable)
                    .await
                    .expect("PgTenantPlanRepository::connect")
            })
        }

        fn nonce(tag: &str) -> String {
            format!(
                "{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )
        }

        /// The durable round-trip: an upserted plan loads back verbatim, and a
        /// repeat upsert for the same tenant OVERWRITES the cap fields (never a
        /// duplicate row). The cross-instance / restart-survival counterpart of
        /// the in-memory `upsert_then_load_round_trips`.
        #[test]
        fn pg_upsert_then_load_round_trips() {
            let Some(url) = db_url() else {
                eprintln!("pg_upsert_then_load_round_trips: TEST_DATABASE_URL unset — skipping");
                return;
            };
            let rt = rt();
            let repo = connect(&rt, &url);

            let acme = TenantId::new(nonce("acme")).expect("valid tenant id");
            let acme_plan = TenantPlan {
                tenant: acme.clone(),
                max_concurrency: 8,
                rate_ceiling_per_min: 60,
                repo_allowlist: Vec::new(),
            };
            repo.upsert(&acme, &acme_plan).expect("upsert acme");

            let loaded: std::collections::HashMap<TenantId, TenantPlan> =
                repo.load_all().expect("load_all").into_iter().collect();
            assert_eq!(
                loaded.get(&acme),
                Some(&acme_plan),
                "the upserted plan round-trips through Postgres"
            );

            // Repeat upsert overwrites the cap fields for the same tenant — one
            // row, not two; the loaded plan is the overwriting one.
            let acme_plan_v2 = TenantPlan {
                tenant: acme.clone(),
                max_concurrency: 16,
                rate_ceiling_per_min: 120,
                repo_allowlist: Vec::new(),
            };
            repo.upsert(&acme, &acme_plan_v2).expect("upsert acme v2");
            let reloaded: std::collections::HashMap<TenantId, TenantPlan> =
                repo.load_all().expect("reload").into_iter().collect();
            assert_eq!(
                reloaded.get(&acme),
                Some(&acme_plan_v2),
                "the overwriting plan is the one loaded back (no duplicate row)"
            );
        }
    }
}
