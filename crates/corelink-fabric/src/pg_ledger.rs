//! Postgres lease ledger — the production [`LeaseLedger`] (WP-3-PGLEDGER).
//!
//! [`InMemoryLedger`](crate::ledger::InMemoryLedger) is dev/test;
//! [`FileLedger`](crate::ledger::FileLedger) is the single-process
//! restart-survival oracle. Neither is cross-instance cap-safe: two control
//! planes counting their own copies can both admit at cap. [`PgLedger`] is the
//! production backend — the authoritative §1 state machine in Postgres, with a
//! **cross-instance cap-safe** [`PgLedger::try_admit`] (per-tenant advisory
//! lock makes count+insert atomic across every instance pointed at the same
//! database).
//!
//! ## Sync trait over an async client
//!
//! The [`LeaseLedger`] trait is SYNC (`&mut self`) — it threads through ~15
//! call sites and `std::sync::Mutex` guards; making it async was rejected. The
//! Postgres clients (`tokio-postgres` / `deadpool-postgres`) are async, so each
//! method bridges with [`tokio::task::block_in_place`] +
//! [`Handle::block_on`](tokio::runtime::Handle::block_on). This is legal only on
//! a `rt-multi-thread` runtime (the server's runtime) — `block_in_place` moves
//! the blocking work off the current worker so the runtime is not starved.
//!
//! ## Fail-closed
//!
//! Connection/DDL failure → `Err` from [`PgLedger::connect`] (no half-open
//! ledger). An unknown DB state label → `Err` (never a silently coerced state).
//! `put` never overwrites; `transition` is a single conditional `UPDATE` that
//! errors on a lost race / illegal pair / unknown lease, exactly like the
//! in-memory matrix — the lifecycle reaper relies on that `Err`.
//!
//! ## State mapping
//!
//! The wire/ledger [`LeaseState`] is NOT given a `postgres-types` derive — the
//! frozen wire types stay clean. Mapping to the `lease_state` enum is local:
//! [`state_to_db`] / [`state_from_db`]. `u64` epoch-ms ↔ `bigint` is `as i64` /
//! `as u64`; epoch-ms fits in an `i64` for ~292 million years, so the round-trip
//! is lossless in practice.

use corelink_runners_contracts::RunnerState;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;
use tokio_postgres::NoTls;

use crate::ledger::{LeaseLedger, LeaseRecord, LeaseState};
use crate::tenant::TenantId;

/// Idempotent schema. Safe to run on every [`PgLedger::connect`] — the enum
/// create swallows `duplicate_object`, the table/indexes are `IF NOT EXISTS`.
const DDL: &str = "\
DO $$ BEGIN CREATE TYPE lease_state AS ENUM ('pending','held','released','expired','crashed');
  EXCEPTION WHEN duplicate_object THEN null; END $$;
CREATE TABLE IF NOT EXISTS leases (
  lease_id text PRIMARY KEY, tenant text NOT NULL, state lease_state NOT NULL,
  box_ref text NOT NULL, created_at_ms bigint NOT NULL, updated_at_ms bigint NOT NULL);
CREATE INDEX IF NOT EXISTS leases_tenant_active_idx ON leases (tenant) WHERE state IN ('pending','held');
CREATE INDEX IF NOT EXISTS leases_held_idx ON leases (lease_id) WHERE state = 'held';
";

/// The `lease_state` enum label for a [`LeaseState`].
///
/// Five flat tokens — identical vocabulary to the serde `rename_all` on
/// [`LeaseState`] and the SQL `CREATE TYPE`. Kept local so the frozen wire type
/// carries no DB derive.
fn state_to_db(s: &LeaseState) -> &'static str {
    match s {
        LeaseState::Pending => "pending",
        LeaseState::Wire(RunnerState::Held) => "held",
        LeaseState::Wire(RunnerState::Released) => "released",
        LeaseState::Wire(RunnerState::Expired) => "expired",
        LeaseState::Wire(RunnerState::Crashed) => "crashed",
    }
}

/// Parse a `lease_state` label back to a [`LeaseState`]. Unknown label → `Err`
/// (fail-closed — never coerce a label the DB shouldn't contain).
fn state_from_db(label: &str) -> anyhow::Result<LeaseState> {
    Ok(match label {
        "pending" => LeaseState::Pending,
        "held" => LeaseState::Wire(RunnerState::Held),
        "released" => LeaseState::Wire(RunnerState::Released),
        "expired" => LeaseState::Wire(RunnerState::Expired),
        "crashed" => LeaseState::Wire(RunnerState::Crashed),
        other => anyhow::bail!("unknown lease_state label {other:?} in DB (fail-closed)"),
    })
}

/// The legal `from` state for a §1 transition `to`, as a DB label — `None` if
/// `to` is not a legal destination of any single transition.
///
/// §1 matrix: `Pending -> Held` and `Held -> {Released|Expired|Crashed}`. The
/// `transition` UPDATE is conditioned on `state = <this label>`, so a row in any
/// other state (illegal source / terminal) matches zero rows and errors.
fn legal_from_for(to: &RunnerState) -> Option<&'static str> {
    match to {
        RunnerState::Held => Some("pending"),
        RunnerState::Released | RunnerState::Expired | RunnerState::Crashed => Some("held"),
    }
}

/// Render a [`LeaseRecord`] row read back from Postgres.
fn record_from_row(row: &tokio_postgres::Row) -> anyhow::Result<LeaseRecord> {
    let state_label: String = row.get("state");
    let created: i64 = row.get("created_at_ms");
    let updated: i64 = row.get("updated_at_ms");
    let tenant_raw: String = row.get("tenant");
    Ok(LeaseRecord {
        lease_id: row.get("lease_id"),
        tenant: TenantId::new(tenant_raw)?,
        state: state_from_db(&state_label)?,
        box_ref: row.get("box_ref"),
        created_at_ms: created as u64,
        updated_at_ms: updated as u64,
    })
}

/// Production [`LeaseLedger`] over Postgres — cross-instance cap-safe.
///
/// Holds a [`deadpool_postgres::Pool`] and a [`tokio::runtime::Handle`]; every
/// sync trait method runs its async body via `block_in_place` + `block_on`. Use
/// [`PgLedger::connect`] to build one (it applies the idempotent DDL once,
/// fail-closed).
pub struct PgLedger {
    pool: Pool,
    handle: Handle,
}

impl std::fmt::Debug for PgLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgLedger").finish_non_exhaustive()
    }
}

impl PgLedger {
    /// Build the pool, capture the current runtime handle, and apply the
    /// idempotent DDL once. Fail-closed: a connection or DDL error → `Err`
    /// (never a half-open ledger).
    ///
    /// Must be called from inside a Tokio `rt-multi-thread` runtime (the sync
    /// trait methods later rely on `block_in_place` on that runtime).
    pub async fn connect(database_url: &str, pool_size: usize) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        cfg.url = Some(database_url.to_string());
        cfg.pool = Some(deadpool_postgres::PoolConfig::new(pool_size));
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| anyhow::anyhow!("PgLedger: cannot build pool: {e}"))?;

        // Apply DDL once, fail-closed. `batch_execute` runs the whole script;
        // wrapping it keeps the enum-create + tables + indexes atomic.
        let client = pool
            .get()
            .await
            .map_err(|e| anyhow::anyhow!("PgLedger: cannot acquire connection for DDL: {e}"))?;
        client
            .batch_execute(&format!("BEGIN; {DDL} COMMIT;"))
            .await
            .map_err(|e| anyhow::anyhow!("PgLedger: DDL failed (fail-closed): {e}"))?;

        let handle = Handle::try_current()
            .map_err(|e| anyhow::anyhow!("PgLedger: must be built on a Tokio runtime: {e}"))?;
        Ok(Self { pool, handle })
    }

    /// Run an async body to completion, bridging the sync trait to the async
    /// client.
    ///
    /// On a multi-thread-runtime **worker** thread (the production path — the
    /// server's request handlers), use [`tokio::task::block_in_place`] so the
    /// blocking work is moved off the worker and the runtime is not starved.
    /// Off a worker thread (e.g. a `spawn_blocking` thread, or a test thread
    /// holding only a `Handle`), `block_in_place` is illegal, so fall back to a
    /// plain [`Handle::block_on`](tokio::runtime::Handle::block_on) — the thread
    /// is not a worker, so blocking it starves nothing.
    fn block_on<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let handle = self.handle.clone();
        match tokio::runtime::Handle::try_current() {
            // Inside a runtime worker → move the blocking off the worker.
            Ok(_) => tokio::task::block_in_place(move || handle.block_on(fut)),
            // Not on a worker (no current runtime) → safe to block directly.
            Err(_) => handle.block_on(fut),
        }
    }

    /// TRUNCATE the table — test-only helper for the conformance harness (each
    /// run starts from an empty ledger). Not part of the trait.
    #[cfg(test)]
    pub fn truncate_for_test(&self) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client.batch_execute("TRUNCATE TABLE leases").await?;
            Ok::<_, anyhow::Error>(())
        })
    }
}

impl LeaseLedger for PgLedger {
    fn put(&mut self, rec: LeaseRecord) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            // ON CONFLICT DO NOTHING + RETURNING: a duplicate lease_id inserts
            // zero rows, which we turn into the same fail-closed Err as the
            // in-memory `put` ("never overwrites").
            let rows = client
                .query(
                    "INSERT INTO leases \
                       (lease_id, tenant, state, box_ref, created_at_ms, updated_at_ms) \
                     VALUES ($1, $2, $3::text::lease_state, $4, $5, $6) \
                     ON CONFLICT (lease_id) DO NOTHING \
                     RETURNING lease_id",
                    &[
                        &rec.lease_id,
                        &rec.tenant.as_str(),
                        &state_to_db(&rec.state),
                        &rec.box_ref,
                        &(rec.created_at_ms as i64),
                        &(rec.updated_at_ms as i64),
                    ],
                )
                .await?;
            if rows.is_empty() {
                anyhow::bail!(
                    "lease {} already exists: put never overwrites",
                    rec.lease_id
                );
            }
            Ok(())
        })
    }

    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "SELECT lease_id, tenant, state::text AS state, box_ref, \
                            created_at_ms, updated_at_ms \
                     FROM leases WHERE lease_id = $1",
                    &[&lease_id],
                )
                .await?;
            match row {
                Some(r) => Ok(Some(record_from_row(&r)?)),
                None => Ok(None),
            }
        })
    }

    fn transition(
        &mut self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        // §1 legality is enforced IN the UPDATE: it only matches a row whose
        // current state is the unique legal predecessor of `to`. 0 rows → Err
        // (lost race / illegal pair / terminal source / unknown lease). The
        // reaper relies on this Err.
        let legal_from = legal_from_for(&to);
        let to_label = state_to_db(&LeaseState::Wire(to));
        self.block_on(async {
            let Some(legal_from) = legal_from else {
                anyhow::bail!("no legal transition into {to_label:?} (contract §1)");
            };
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "UPDATE leases \
                     SET state = $1::text::lease_state, updated_at_ms = $2 \
                     WHERE lease_id = $3 AND state = $4::text::lease_state \
                     RETURNING lease_id, tenant, state::text AS state, box_ref, \
                               created_at_ms, updated_at_ms",
                    &[&to_label, &(now_ms as i64), &lease_id, &legal_from],
                )
                .await?;
            match row {
                Some(r) => record_from_row(&r),
                None => anyhow::bail!(
                    "illegal/lost lease transition for {lease_id}: -> {to_label} \
                     (no row in the required source state; contract §1 fail-closed)"
                ),
            }
        })
    }

    fn remove(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        // Admission-rollback seam (the over-admission fix in the acquire path):
        // drop a reserved `Pending` row when provisioning fails so the slot +
        // cap free immediately. Unconditional delete by id — `Ok(true)` if a
        // row was removed, `Ok(false)` if the lease was already gone/unknown.
        self.block_on(async {
            let client = self.pool.get().await?;
            let n = client
                .execute("DELETE FROM leases WHERE lease_id = $1", &[&lease_id])
                .await?;
            Ok(n == 1)
        })
    }

    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.block_on(async {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT lease_id, tenant, state::text AS state, box_ref, \
                            created_at_ms, updated_at_ms \
                     FROM leases WHERE tenant = $1 ORDER BY lease_id",
                    &[&t.as_str()],
                )
                .await?;
            rows.iter().map(record_from_row).collect()
        })
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.block_on(async {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT lease_id, tenant, state::text AS state, box_ref, \
                            created_at_ms, updated_at_ms \
                     FROM leases WHERE state = 'held' ORDER BY lease_id",
                    &[],
                )
                .await?;
            rows.iter().map(record_from_row).collect()
        })
    }

    fn try_admit(&mut self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        // THE crux: cross-instance cap-safe admission. An EXPLICIT transaction
        // takes a per-tenant advisory lock as its OWN statement FIRST, then the
        // count-and-insert. The lock is held for the whole transaction
        // (`pg_advisory_xact_lock`), so every instance racing to admit this
        // tenant is serialized: the loser blocks on the lock, then sees the
        // winner's row in its count and inserts nothing.
        //
        // A single-statement CTE lock was tried and REJECTED: Postgres does not
        // order the `WITH lock AS (pg_advisory_xact_lock …)` node before the
        // count subquery, so two instances could both pass the count — the
        // cross-instance proof test caught exactly that. The lock MUST be its
        // own prior statement in an explicit transaction.
        //
        //   - 1 row returned  → admitted (inserted as caller-supplied Pending).
        //   - 0 rows returned → over cap (cap reached, or max_concurrency == 0,
        //                        for which `count < 0` never holds) → Ok(false).
        //   - duplicate lease_id → Err (the PRIMARY KEY conflict surfaces as a
        //                        DB error, matching `put`'s fail-closed contract).
        self.block_on(async {
            let mut client = self.pool.get().await?;
            let txn = client.transaction().await?;
            // 1. Serialize all admits for this tenant across instances. The lock
            //    is released at COMMIT/ROLLBACK (xact-scoped).
            txn.execute(
                "SELECT pg_advisory_xact_lock(hashtext($1))",
                &[&rec.tenant.as_str()],
            )
            .await?;
            // 2. Count-and-insert under the lock — now atomic across instances.
            let res = txn
                .query(
                    "INSERT INTO leases \
                       (lease_id, tenant, state, box_ref, created_at_ms, updated_at_ms) \
                     SELECT $1, $2, $3::text::lease_state, $4, $5, $5 \
                     WHERE (SELECT count(*) FROM leases \
                            WHERE tenant = $2 AND state IN ('pending','held')) < $6 \
                     RETURNING lease_id",
                    &[
                        &rec.lease_id,
                        &rec.tenant.as_str(),
                        &state_to_db(&rec.state),
                        &rec.box_ref,
                        &(rec.created_at_ms as i64),
                        &(max_concurrency as i64),
                    ],
                )
                .await;
            match res {
                Ok(rows) => {
                    txn.commit().await?;
                    Ok(!rows.is_empty())
                }
                // A duplicate lease_id trips the PRIMARY KEY — roll back and
                // surface a fail-closed Err, exactly like `put` admitting the
                // same id twice.
                Err(e) => {
                    let _ = txn.rollback().await;
                    Err(anyhow::anyhow!(
                        "try_admit failed for lease {} (duplicate id or DB error): {e}",
                        rec.lease_id
                    ))
                }
            }
        })
    }
}
