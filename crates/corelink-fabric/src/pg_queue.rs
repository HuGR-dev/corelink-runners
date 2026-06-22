//! Cross-instance fair-admission queue (M1 WP-CROSS-INSTANCE-QUEUE).
//!
//! The single-process [`FairScheduler`](crate::scheduler::FairScheduler) is the
//! in-memory deficit round-robin dispatcher; it is NOT cross-instance fair —
//! two control planes each running their own scheduler can starve a tenant the
//! other is serving. This module is the **durable** counterpart: a
//! deficit-ordered admission queue SHARED across instances in Postgres, so two
//! instances pull from ONE fair queue instead of two independent ones.
//!
//! ## The cross-instance fairness model
//!
//! Per-tenant fairness is carried by an explicit, durable **deficit** counter:
//!
//! - [`PgAdmissionQueue::enqueue`] stamps each new row's `deficit` with the
//!   tenant's CURRENT admit-count (the durable count of admits that tenant has
//!   already won, from the companion `pg_admission_deficit` table). A tenant
//!   that has been served a lot carries a HIGH deficit; a tenant owed service
//!   carries a LOW one.
//! - The dequeue order is the frozen `(deficit, enqueued_at_ms)`: lowest
//!   deficit first (the tenant owed the most service), ties broken by oldest
//!   enqueue (FIFO within a deficit tier).
//! - [`PgAdmissionQueue::admit`] removes the served row AND increments that
//!   tenant's admit-count, so the tenant's NEXT enqueue is stamped one tier
//!   higher — the cross-instance analogue of the in-memory cursor rotation.
//!
//! Because the order, the deficit counter, and the rows all live in ONE
//! database, every instance reads the SAME fair order. Two instances each run
//! `SELECT … ORDER BY deficit, enqueued_at_ms FOR UPDATE SKIP LOCKED LIMIT 1`,
//! so they pull DISJOINT heads of the global fair queue (no double-serve), and
//! the deficit accounting holds fairness ACROSS them — exactly the property the
//! per-instance scheduler cannot give.
//!
//! ## Mirrors [`PgLedger`](crate::pg_ledger)
//!
//! Same pool/bridge/TLS discipline: a [`deadpool_postgres::Pool`] + a captured
//! [`Handle`], every public method bridges the (implicit) sync caller to the
//! async client via [`block_in_place`](tokio::task::block_in_place) +
//! [`Handle::block_on`](tokio::runtime::Handle::block_on); the same
//! [`PgTlsMode`](crate::pg_ledger::PgTlsMode) verify-full / plaintext branch;
//! fail-closed [`connect`](PgAdmissionQueue::connect) that applies the idempotent
//! DDL once. The enqueue↔stamp and the admit↔increment are each made atomic
//! across instances by a per-tenant `pg_advisory_xact_lock(hashtext(tenant))` —
//! the SAME advisory-lock key the ledger's `try_admit` uses, so the deficit
//! accounting never races.

use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;
use tokio_postgres::NoTls;

use crate::pg_ledger::{PgTlsMode, rustls_verify_full_config};
use crate::tenant::TenantId;

/// One pending admission waiting in the cross-instance fair queue.
///
/// `lease_request_id` is the natural key (the table PRIMARY KEY). It is a
/// `String` to match the crate's `lease_id` convention everywhere
/// ([`LeaseRecord.lease_id`](crate::ledger::LeaseRecord) is `String`; there is
/// no `uuid` dependency in this crate — the id is opaque text on the wire).
///
/// `deficit` is the deficit-round-robin counter (lower = owed more service);
/// `(deficit, enqueued_at_ms)` is the total dequeue order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAdmission {
    /// Tenant the queued admission belongs to (org = tenant, ADR-0002).
    pub tenant: TenantId,
    /// Opaque admission-request id — the queue's natural key (PRIMARY KEY).
    pub lease_request_id: String,
    /// Unix epoch ms the request was enqueued (FIFO tiebreaker within a deficit).
    pub enqueued_at_ms: i64,
    /// Deficit-round-robin counter — lower = owed more service, dequeued first.
    pub deficit: i64,
}

/// Idempotent schema for the cross-instance fair-admission queue.
///
/// `IF NOT EXISTS` so it is safe to apply on every connect, mirroring the
/// [`PgLedger`](crate::pg_ledger) DDL convention. The dequeue index pins the
/// frozen `(deficit, enqueued_at_ms)` ordering — lowest deficit first, oldest
/// enqueue breaking ties.
///
/// The companion `pg_admission_deficit` table is the per-tenant durable
/// admit-count: the source of the `deficit` an enqueue stamps, and what an
/// admit increments. It is the cross-instance fairness state (the durable
/// analogue of the in-memory scheduler's cursor/owed-set) — without it, fairness
/// would not survive a restart and could not be shared between instances.
pub const PG_ADMISSION_QUEUE_DDL: &str = "\
CREATE TABLE IF NOT EXISTS pg_admission_queue (
  tenant           text   NOT NULL,
  lease_request_id text   PRIMARY KEY,
  enqueued_at_ms   bigint NOT NULL,
  deficit          bigint NOT NULL
);
CREATE INDEX IF NOT EXISTS pg_admission_queue_order_idx
  ON pg_admission_queue (deficit, enqueued_at_ms);
CREATE TABLE IF NOT EXISTS pg_admission_deficit (
  tenant       text   PRIMARY KEY,
  admit_count  bigint NOT NULL
);
";

/// Production cross-instance fair-admission queue over Postgres.
///
/// Holds a [`deadpool_postgres::Pool`] and a [`tokio::runtime::Handle`]; every
/// public method runs its async body via `block_in_place` + `block_on` (the
/// SAME bridge as [`PgLedger`](crate::pg_ledger::PgLedger), so the queue can be
/// called from the synchronous call sites that also touch the ledger). Use
/// [`PgAdmissionQueue::connect`] to build one — it applies the idempotent DDL
/// once, fail-closed.
pub struct PgAdmissionQueue {
    pool: Pool,
    handle: Handle,
}

impl std::fmt::Debug for PgAdmissionQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgAdmissionQueue").finish_non_exhaustive()
    }
}

impl PgAdmissionQueue {
    /// Build the pool, capture the current runtime handle, and apply
    /// [`PG_ADMISSION_QUEUE_DDL`] once. Fail-closed: a connection or DDL error →
    /// `Err` (never a half-open queue).
    ///
    /// `tls` selects the transport, mirroring
    /// [`PgLedger::connect`](crate::pg_ledger::PgLedger::connect):
    /// [`PgTlsMode::Disable`] keeps the original plaintext `NoTls` pool (default;
    /// the live private-network deployment is unaffected),
    /// [`PgTlsMode::Require`] builds a verify-full rustls pool against the
    /// `webpki_roots` public-CA set (the SAME `rustls_verify_full_config` the
    /// ledger uses — one TLS posture in the crate). Resolve the mode from the
    /// environment with
    /// [`pg_tls_mode_from_env`](crate::pg_ledger::pg_tls_mode_from_env).
    ///
    /// Must be called from inside a Tokio `rt-multi-thread` runtime (the bridge
    /// methods later rely on `block_in_place` on that runtime).
    pub async fn connect(
        database_url: &str,
        pool_size: usize,
        tls: PgTlsMode,
    ) -> anyhow::Result<Self> {
        let mut cfg = Config::new();
        cfg.url = Some(database_url.to_string());
        // Bound the pool-acquire wait so a pool-exhaustion never hangs the
        // admission hot path indefinitely — fail-closed, exactly as PgLedger.
        const POOL_FLOOR: usize = 2;
        let mut pool_cfg = deadpool_postgres::PoolConfig::new(pool_size.max(POOL_FLOOR));
        pool_cfg.timeouts.wait = Some(std::time::Duration::from_secs(5));
        cfg.pool = Some(pool_cfg);

        let pool = match tls {
            PgTlsMode::Disable => cfg.create_pool(Some(Runtime::Tokio1), NoTls),
            PgTlsMode::Require => {
                let connector =
                    tokio_postgres_rustls::MakeRustlsConnect::new(rustls_verify_full_config());
                cfg.create_pool(Some(Runtime::Tokio1), connector)
            }
        }
        .map_err(|e| anyhow::anyhow!("PgAdmissionQueue: cannot build pool: {e}"))?;

        let client = pool.get().await.map_err(|e| {
            anyhow::anyhow!("PgAdmissionQueue: cannot acquire connection for DDL: {e}")
        })?;
        client
            .batch_execute(&format!("BEGIN; {PG_ADMISSION_QUEUE_DDL} COMMIT;"))
            .await
            .map_err(|e| anyhow::anyhow!("PgAdmissionQueue: DDL failed (fail-closed): {e}"))?;

        let handle = Handle::try_current().map_err(|e| {
            anyhow::anyhow!("PgAdmissionQueue: must be built on a Tokio runtime: {e}")
        })?;
        Ok(Self { pool, handle })
    }

    /// Run an async body to completion, bridging a sync caller to the async
    /// client — byte-identical to [`PgLedger::block_on`]: on a runtime worker
    /// use [`block_in_place`](tokio::task::block_in_place) so the runtime is not
    /// starved; off a worker fall back to a plain
    /// [`Handle::block_on`](tokio::runtime::Handle::block_on).
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

    /// Enqueue one pending admission, stamping its `deficit` with the tenant's
    /// current durable admit-count so the row takes its fair place in the global
    /// `(deficit, enqueued_at_ms)` order.
    ///
    /// Atomic across instances: a per-tenant `pg_advisory_xact_lock` serializes
    /// the read-of-admit-count + insert against this tenant's concurrent admits
    /// (which mutate the same counter), so the stamped deficit can never be a
    /// torn/stale value. A duplicate `lease_request_id` (PRIMARY KEY) is a
    /// fail-closed `Err`, matching the ledger's `put` contract.
    ///
    /// Returns the stamped [`PendingAdmission`] (the row as persisted), so the
    /// caller can observe the deficit tier it landed in.
    pub fn enqueue(
        &self,
        tenant: &TenantId,
        lease_request_id: &str,
        enqueued_at_ms: i64,
    ) -> anyhow::Result<PendingAdmission> {
        let tenant = tenant.clone();
        let lease_request_id = lease_request_id.to_string();
        self.block_on(async move {
            let mut client = self.pool.get().await?;
            let txn = client.transaction().await?;
            // Serialize this tenant's enqueue/admit deficit accounting across
            // instances (SAME advisory-lock key the ledger's try_admit uses).
            txn.execute(
                "SELECT pg_advisory_xact_lock(hashtext($1))",
                &[&tenant.as_str()],
            )
            .await?;
            // The tenant's current admit-count = the deficit to stamp (0 if the
            // tenant has never been served). A row that fits at this tier is
            // dequeued before a row of a tenant already served more often.
            let deficit: i64 = txn
                .query_opt(
                    "SELECT admit_count FROM pg_admission_deficit WHERE tenant = $1",
                    &[&tenant.as_str()],
                )
                .await?
                .map(|r| r.get::<_, i64>("admit_count"))
                .unwrap_or(0);
            let res = txn
                .query_opt(
                    "INSERT INTO pg_admission_queue \
                       (tenant, lease_request_id, enqueued_at_ms, deficit) \
                     VALUES ($1, $2, $3, $4) \
                     ON CONFLICT (lease_request_id) DO NOTHING \
                     RETURNING lease_request_id",
                    &[
                        &tenant.as_str(),
                        &lease_request_id,
                        &enqueued_at_ms,
                        &deficit,
                    ],
                )
                .await?;
            if res.is_none() {
                txn.rollback().await.ok();
                anyhow::bail!(
                    "admission request {lease_request_id} already queued: enqueue never overwrites"
                );
            }
            txn.commit().await?;
            Ok(PendingAdmission {
                tenant,
                lease_request_id,
                enqueued_at_ms,
                deficit,
            })
        })
    }

    /// Pop the fair head of the GLOBAL queue (lowest `(deficit, enqueued_at_ms)`)
    /// and CLAIM it for this instance, removing it from the queue.
    ///
    /// The crux of cross-instance no-double-serve: the head row is selected
    /// `FOR UPDATE SKIP LOCKED` inside a transaction, so a SECOND instance
    /// dequeuing concurrently SKIPS the row this instance has locked and pulls
    /// the NEXT fair head instead. The two instances therefore pull DISJOINT
    /// rows of the same global fair order — never the same one twice. The claimed
    /// row is deleted in the same transaction (admission consumes the queue
    /// slot), so once committed it is gone for everyone.
    ///
    /// NOTE: this does NOT bump the tenant's admit-count — a dequeue only
    /// SELECTS the fair candidate; the authoritative cap reservation
    /// (`try_admit`) runs in the caller, and only a tenant that actually WINS
    /// admission should advance its deficit. Call [`PgAdmissionQueue::admit`]
    /// after a successful reservation to record the win; on a lost cap race
    /// re-enqueue via [`PgAdmissionQueue::enqueue`] with the original
    /// `enqueued_at_ms`. Returns `Ok(None)` when the queue is empty (for this
    /// instance — every visible head is locked by another instance or there is
    /// no work).
    pub fn dequeue_next(&self) -> anyhow::Result<Option<PendingAdmission>> {
        self.dequeue_head(None)
    }

    /// Claim the fair head whose `lease_request_id` is one of `local_ids` — the
    /// production cross-instance dispatch path.
    ///
    /// # Why scope the claim to local ids
    ///
    /// The waiter context (the oneshot waker, the minted lease/spec, the PAT)
    /// is inherently PER-INSTANCE: an over-cap acquire parks inside the HTTP
    /// request future on the instance that received it, so ONLY that instance can
    /// finalize the lease and wake the client. If an instance claimed-and-deleted
    /// a row whose waiter lives on a DIFFERENT instance, that client would never
    /// be served. So each instance dispatches only the rows IT enqueued —
    /// `local_ids` is the set of its live waiters.
    ///
    /// Crucially, fairness stays GLOBAL: the `ORDER BY deficit, enqueued_at_ms`
    /// is over the WHOLE table (every instance's rows), and the `deficit` is
    /// stamped from the cross-instance admit-count, so an instance picks its OWN
    /// most-owed waiter only when that waiter is the global fair head among its
    /// locals — a tenant heavily served on EITHER instance carries a high deficit
    /// everywhere, so its rows sink behind an owed tenant's rows on every
    /// instance. The `FOR UPDATE SKIP LOCKED` still guarantees no double-serve if
    /// two instances ever share an id (they do not, ids are unique).
    pub fn dequeue_next_local(
        &self,
        local_ids: &[String],
    ) -> anyhow::Result<Option<PendingAdmission>> {
        if local_ids.is_empty() {
            return Ok(None);
        }
        self.dequeue_head(Some(local_ids))
    }

    /// Shared claim core: the fair head (optionally restricted to `only_ids`),
    /// selected `FOR UPDATE SKIP LOCKED` and deleted in one transaction.
    fn dequeue_head(
        &self,
        only_ids: Option<&[String]>,
    ) -> anyhow::Result<Option<PendingAdmission>> {
        self.block_on(async {
            let mut client = self.pool.get().await?;
            let txn = client.transaction().await?;
            // SKIP LOCKED: a head another instance already locked is invisible
            // here, so we pull the next fair candidate — disjoint heads, no
            // double-serve. The optional `lease_request_id = ANY($1)` filter
            // scopes the claim to this instance's local waiters (production
            // path); `None` selects the global head (used by the disjointness
            // test). LIMIT 1: one fair head per call.
            let row = match only_ids {
                Some(ids) => {
                    txn.query_opt(
                        "SELECT tenant, lease_request_id, enqueued_at_ms, deficit \
                           FROM pg_admission_queue \
                          WHERE lease_request_id = ANY($1) \
                          ORDER BY deficit, enqueued_at_ms \
                          FOR UPDATE SKIP LOCKED \
                          LIMIT 1",
                        &[&ids],
                    )
                    .await?
                }
                None => {
                    txn.query_opt(
                        "SELECT tenant, lease_request_id, enqueued_at_ms, deficit \
                           FROM pg_admission_queue \
                          ORDER BY deficit, enqueued_at_ms \
                          FOR UPDATE SKIP LOCKED \
                          LIMIT 1",
                        &[],
                    )
                    .await?
                }
            };
            let Some(row) = row else {
                txn.rollback().await.ok();
                return Ok(None);
            };
            let lease_request_id: String = row.get("lease_request_id");
            // Consume the slot: delete the claimed row in the SAME txn so the
            // lock + the row are released/removed together at commit.
            txn.execute(
                "DELETE FROM pg_admission_queue WHERE lease_request_id = $1",
                &[&lease_request_id],
            )
            .await?;
            let tenant_raw: String = row.get("tenant");
            let pending = PendingAdmission {
                tenant: TenantId::new(tenant_raw)?,
                lease_request_id,
                enqueued_at_ms: row.get("enqueued_at_ms"),
                deficit: row.get("deficit"),
            };
            txn.commit().await?;
            Ok(Some(pending))
        })
    }

    /// Record that `tenant` WON an admission — increment its durable admit-count
    /// so its next enqueue is stamped one deficit tier higher (the cross-instance
    /// rotation). Idempotent UPSERT; serialized on the SAME per-tenant advisory
    /// lock as [`enqueue`](Self::enqueue) so the counter never races a concurrent
    /// stamp on another instance.
    ///
    /// Called by the caller AFTER a dequeued candidate wins its authoritative
    /// `try_admit` reservation — never on a dequeue alone (a lost cap race must
    /// not advance the deficit, or a tenant would be penalized for losing a race
    /// it did not get served by).
    pub fn admit(&self, tenant: &TenantId) -> anyhow::Result<()> {
        let tenant = tenant.clone();
        self.block_on(async move {
            let mut client = self.pool.get().await?;
            let txn = client.transaction().await?;
            txn.execute(
                "SELECT pg_advisory_xact_lock(hashtext($1))",
                &[&tenant.as_str()],
            )
            .await?;
            txn.execute(
                "INSERT INTO pg_admission_deficit (tenant, admit_count) \
                 VALUES ($1, 1) \
                 ON CONFLICT (tenant) DO UPDATE \
                   SET admit_count = pg_admission_deficit.admit_count + 1",
                &[&tenant.as_str()],
            )
            .await?;
            txn.commit().await?;
            Ok(())
        })
    }

    /// Remove a queued row by id WITHOUT advancing any deficit (the give-up /
    /// timeout / shed path). `Ok(true)` iff a row was removed. Used when a queued
    /// waiter abandons its request, so a later dequeue never claims a slot for a
    /// request nobody owns — the durable analogue of the in-memory
    /// `evict_waiter`.
    pub fn remove(&self, lease_request_id: &str) -> anyhow::Result<bool> {
        let lease_request_id = lease_request_id.to_string();
        self.block_on(async move {
            let client = self.pool.get().await?;
            let n = client
                .execute(
                    "DELETE FROM pg_admission_queue WHERE lease_request_id = $1",
                    &[&lease_request_id],
                )
                .await?;
            Ok(n == 1)
        })
    }

    /// Count of queued admissions for one tenant (test/observability).
    pub fn pending(&self, tenant: &TenantId) -> anyhow::Result<usize> {
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_one(
                    "SELECT count(*) AS n FROM pg_admission_queue WHERE tenant = $1",
                    &[&tenant.as_str()],
                )
                .await?;
            Ok(row.get::<_, i64>("n") as usize)
        })
    }

    /// TRUNCATE both tables — test-only helper so each run starts empty.
    #[cfg(test)]
    pub fn truncate_for_test(&self) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client
                .batch_execute("TRUNCATE TABLE pg_admission_queue, pg_admission_deficit")
                .await?;
            Ok::<_, anyhow::Error>(())
        })
    }
}

/// Pure deficit-ordering core (no DB) — the cross-instance fair order is the
/// total order `(deficit, enqueued_at_ms)` ascending. The DB pins this in the
/// `ORDER BY deficit, enqueued_at_ms` of [`PgAdmissionQueue::dequeue_next`] +
/// the `pg_admission_queue_order_idx`; this function is the testable spec of the
/// SAME comparison, so the ordering rule can be verified without a live DB.
///
/// Returns [`std::cmp::Ordering`]: `Less` means `a` is dequeued BEFORE `b`.
#[must_use]
pub fn fair_order(a: &PendingAdmission, b: &PendingAdmission) -> std::cmp::Ordering {
    a.deficit
        .cmp(&b.deficit)
        .then(a.enqueued_at_ms.cmp(&b.enqueued_at_ms))
}

#[cfg(test)]
mod ordering_tests {
    //! Pure unit tests for the deficit-ordering rule — no DB. These pin the
    //! frozen `(deficit, enqueued_at_ms)` total order the DB `ORDER BY` and index
    //! implement, so the cross-instance fairness contract is verified even where
    //! `TEST_DATABASE_URL` is unset (the builder Mac / CI).
    use std::cmp::Ordering;

    use super::{PendingAdmission, fair_order};
    use crate::tenant::TenantId;

    fn pa(tenant: &str, id: &str, enqueued_at_ms: i64, deficit: i64) -> PendingAdmission {
        PendingAdmission {
            tenant: TenantId::new(tenant).unwrap(),
            lease_request_id: id.to_string(),
            enqueued_at_ms,
            deficit,
        }
    }

    #[test]
    fn lower_deficit_dequeues_first() {
        // Tenant owed more service (deficit 0) beats a heavily-served tenant
        // (deficit 5) even though the latter enqueued earlier.
        let owed = pa("a", "owed", 1_000, 0);
        let served = pa("b", "served", 1, 5);
        assert_eq!(fair_order(&owed, &served), Ordering::Less);
        assert_eq!(fair_order(&served, &owed), Ordering::Greater);
    }

    #[test]
    fn equal_deficit_breaks_on_enqueue_fifo() {
        // Same deficit tier → oldest enqueue first (FIFO within a tier).
        let older = pa("a", "older", 100, 3);
        let newer = pa("a", "newer", 200, 3);
        assert_eq!(fair_order(&older, &newer), Ordering::Less);
        assert_eq!(fair_order(&newer, &older), Ordering::Greater);
    }

    #[test]
    fn fully_equal_keys_are_equal_order() {
        let x = pa("a", "x", 100, 3);
        let y = pa("a", "y", 100, 3);
        assert_eq!(fair_order(&x, &y), Ordering::Equal);
    }

    #[test]
    fn sort_yields_global_fair_order() {
        // A mixed batch sorts into exactly the order dequeue_next pulls them.
        let mut batch = [
            pa("served", "s2", 50, 2),
            pa("owed", "o1", 999, 0),
            pa("served", "s1", 10, 2),
            pa("mid", "m1", 30, 1),
            pa("owed", "o2", 1_000, 0),
        ];
        batch.sort_by(fair_order);
        let ids: Vec<&str> = batch.iter().map(|p| p.lease_request_id.as_str()).collect();
        // deficit 0 (oldest-first: o1@999 then o2@1000), then deficit 1 (m1),
        // then deficit 2 (s1@10 then s2@50).
        assert_eq!(ids, vec!["o1", "o2", "m1", "s1", "s2"]);
    }

    #[test]
    fn deficit_round_robin_alternates_tenants() {
        // Model two admits: each enqueue stamps the tenant's current admit-count
        // as deficit, each admit bumps that count by 1. With A and B alternately
        // enqueuing+winning, the fair order interleaves them — neither starves.
        let mut rows: Vec<PendingAdmission> = Vec::new();
        let mut t = 0i64;
        // A enqueues 3 in a burst, B enqueues 3 in a burst (B later in time).
        // Each tenant's deficit = how many it has already won across the burst
        // (0,1,2) — i.e. the per-tenant admit-count, which equals i here.
        for i in 0..3 {
            t += 1;
            rows.push(pa("a", &format!("a{i}"), t, i as i64));
        }
        for i in 0..3 {
            t += 1;
            rows.push(pa("b", &format!("b{i}"), t, i as i64));
        }
        rows.sort_by(fair_order);
        let ids: Vec<&str> = rows.iter().map(|p| p.lease_request_id.as_str()).collect();
        // deficit 0: a0 (t1) then b0 (t4); deficit 1: a1 (t2) then b1 (t5);
        // deficit 2: a2 (t3) then b2 (t6) — A and B interleave per tier, so a
        // burst from A never starves B (the cross-instance fairness property).
        assert_eq!(ids, vec!["a0", "b0", "a1", "b1", "a2", "b2"]);
    }
}

// ── Cross-instance Postgres regression suite (WP-CROSS-INSTANCE-QUEUE) ─────────
//
// Gated on `TEST_DATABASE_URL`, EXACTLY like the ledger's `compute_ceiling_pg_tests`
// + `pg_runs`: ABSENT (the builder Mac / CI has no Postgres) → each test prints a
// skip line and returns early, so the suite stays green; PRESENT → the real DB
// exercises the deficit stamp, the SKIP-LOCKED disjoint dequeue (two instances),
// the admit increment, and the global fair ordering. Every test uses a UNIQUE
// tenant nonce so parallel processes never collide on the shared tables.
#[cfg(test)]
mod pg_tests {
    use super::*;
    use crate::pg_ledger::PgTlsMode;

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

    fn connect(rt: &tokio::runtime::Runtime, url: &str) -> PgAdmissionQueue {
        rt.block_on(async {
            PgAdmissionQueue::connect(url, 4, PgTlsMode::Disable)
                .await
                .expect("PgAdmissionQueue::connect")
        })
    }

    /// A nonce-suffixed tenant id ([a-z0-9-] only) so parallel test binaries
    /// never collide on the shared tables.
    fn tid(tag: &str) -> TenantId {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        TenantId::new(format!("{tag}-{}-{n}", std::process::id())).unwrap()
    }

    /// Enqueue then dequeue a single tenant's rows: FIFO within a tier (the
    /// deficit is identical until an admit bumps it), and dequeue removes the row.
    #[test]
    fn enqueue_dequeue_single_tenant_fifo() {
        let Some(url) = db_url() else {
            eprintln!("enqueue_dequeue_single_tenant_fifo: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let rt = rt();
        let q = connect(&rt, &url);
        let t = tid("fifo");
        q.enqueue(&t, "r1", 100).unwrap();
        q.enqueue(&t, "r2", 200).unwrap();
        assert_eq!(q.pending(&t).unwrap(), 2);
        // Same deficit tier (no admits yet) → oldest enqueue first.
        let a = q.dequeue_next().unwrap().expect("a head");
        assert_eq!(a.lease_request_id, "r1");
        let b = q.dequeue_next().unwrap().expect("a head");
        assert_eq!(b.lease_request_id, "r2");
        // Queue drained for this tenant.
        assert_eq!(q.pending(&t).unwrap(), 0);
        q.remove("r1").ok();
        q.remove("r2").ok();
    }

    /// Duplicate lease_request_id is fail-closed (PRIMARY KEY), like `put`.
    #[test]
    fn duplicate_enqueue_is_err() {
        let Some(url) = db_url() else {
            eprintln!("duplicate_enqueue_is_err: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let rt = rt();
        let q = connect(&rt, &url);
        let t = tid("dup");
        let id = format!("dup-{}", std::process::id());
        q.enqueue(&t, &id, 100).unwrap();
        assert!(
            q.enqueue(&t, &id, 200).is_err(),
            "duplicate id must fail closed"
        );
        q.remove(&id).ok();
    }

    /// The deficit stamp + admit increment give cross-tenant fairness: a burst
    /// from one tenant does NOT starve another. After A wins one admit, A's next
    /// enqueue is stamped a higher deficit, so B's owed (lower-deficit) row
    /// dequeues ahead of A's second.
    #[test]
    fn deficit_interleaves_tenants_across_a_burst() {
        let Some(url) = db_url() else {
            eprintln!(
                "deficit_interleaves_tenants_across_a_burst: TEST_DATABASE_URL unset — skipping"
            );
            return;
        };
        let rt = rt();
        let q = connect(&rt, &url);
        let a = tid("a");
        let b = tid("b");
        let a0 = format!("a0-{}", std::process::id());
        let a1 = format!("a1-{}", std::process::id());
        let b0 = format!("b0-{}", std::process::id());
        // A enqueues two BEFORE B enqueues one (A would FIFO-win both naively).
        q.enqueue(&a, &a0, 10).unwrap(); // deficit 0
        // A wins a0 → bump A's admit-count to 1.
        let head = q.dequeue_next().unwrap().expect("head");
        assert_eq!(head.lease_request_id, a0);
        q.admit(&a).unwrap();
        // Now A enqueues again (stamped deficit 1) and B enqueues (deficit 0).
        q.enqueue(&a, &a1, 20).unwrap(); // deficit 1 (A already served once)
        q.enqueue(&b, &b0, 30).unwrap(); // deficit 0 (B never served)
        // B's owed (deficit-0) row dequeues BEFORE A's deficit-1 row, even though
        // A enqueued earlier — the cross-tenant fairness property.
        let next = q.dequeue_next().unwrap().expect("head");
        assert_eq!(
            next.lease_request_id, b0,
            "owed tenant B dequeues ahead of served A"
        );
        let last = q.dequeue_next().unwrap().expect("head");
        assert_eq!(last.lease_request_id, a1);
        q.remove(&a1).ok();
        q.remove(&b0).ok();
    }

    /// SKIP LOCKED: two concurrent dequeues on the SAME queue pull DISJOINT rows
    /// (no double-serve). We hold instance-1's dequeue transaction OPEN (lock the
    /// head) while instance-2 dequeues — it must skip the locked head and pull the
    /// next one.
    #[test]
    fn skip_locked_two_instances_disjoint() {
        let Some(url) = db_url() else {
            eprintln!("skip_locked_two_instances_disjoint: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let rt = rt();
        // Two independent queues over the SAME database = two instances.
        let q1 = connect(&rt, &url);
        let q2 = connect(&rt, &url);
        let t = tid("skip");
        q1.enqueue(&t, "k1", 100).unwrap();
        q1.enqueue(&t, "k2", 200).unwrap();

        // Drive both dequeue transactions on the runtime, holding instance-1's
        // FOR UPDATE lock open across instance-2's dequeue to prove the skip.
        rt.block_on(async {
            let mut c1 = q1.pool.get().await.unwrap();
            let txn1 = c1.transaction().await.unwrap();
            // Instance-1 locks the fair head (k1) but does NOT commit yet.
            let r1 = txn1
                .query_one(
                    "SELECT lease_request_id FROM pg_admission_queue \
                      ORDER BY deficit, enqueued_at_ms FOR UPDATE SKIP LOCKED LIMIT 1",
                    &[],
                )
                .await
                .unwrap();
            let head1: String = r1.get("lease_request_id");
            assert_eq!(head1, "k1");

            // Instance-2 dequeues concurrently: it must SKIP the locked k1 and
            // pull k2 — disjoint, never the same row twice.
            let mut c2 = q2.pool.get().await.unwrap();
            let txn2 = c2.transaction().await.unwrap();
            let r2 = txn2
                .query_one(
                    "SELECT lease_request_id FROM pg_admission_queue \
                      ORDER BY deficit, enqueued_at_ms FOR UPDATE SKIP LOCKED LIMIT 1",
                    &[],
                )
                .await
                .unwrap();
            let head2: String = r2.get("lease_request_id");
            assert_eq!(
                head2, "k2",
                "instance-2 must skip the head instance-1 locked"
            );
            assert_ne!(head1, head2, "no double-serve: disjoint heads");
            txn1.rollback().await.ok();
            txn2.rollback().await.ok();
        });
        q1.remove("k1").ok();
        q1.remove("k2").ok();
    }

    /// `remove` drops a queued row without advancing any deficit (the give-up /
    /// timeout path), and reports whether a row was actually removed.
    #[test]
    fn remove_drops_without_admit() {
        let Some(url) = db_url() else {
            eprintln!("remove_drops_without_admit: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let rt = rt();
        let q = connect(&rt, &url);
        let t = tid("rm");
        let id = format!("rm-{}", std::process::id());
        q.enqueue(&t, &id, 100).unwrap();
        assert!(q.remove(&id).unwrap(), "existing row removed");
        assert!(!q.remove(&id).unwrap(), "second remove is a no-op");
        assert_eq!(q.pending(&t).unwrap(), 0);
    }
}
