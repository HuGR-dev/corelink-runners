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
//!
//! ## Transport security (WP-B — opt-in TLS)
//!
//! The DB connection is **plaintext by default** ([`PgTlsMode::Disable`] →
//! `NoTls`), preserving the original behavior for the live private-network
//! deployment. Setting `FABRIC_PG_TLS=require` ([`PgTlsMode::Require`]) wraps the
//! pool in a **verify-full** rustls transport so a TLS-required managed Postgres
//! (Neon / Supabase / RDS) can be used. "verify-full" means the full server
//! certificate chain is validated AND the SNI hostname from the URL is checked —
//! there is no certificate-verification bypass anywhere on this path (a TLS that
//! does not verify is worse than no TLS).
//!
//! The trust anchors are **[`webpki_roots`] — the bundled Mozilla public-CA
//! set**, NOT the OS trust store. This keeps the build hermetic and fully
//! deterministic (no system dependency, no OpenSSL). The deliberate consequence:
//! only servers with a certificate chaining to a **public** CA are trusted. A
//! Postgres fronted by a **private / internal CA is out of scope for M1** —
//! documented non-goal, not an oversight. The resolver is [`pg_tls_mode_from_env`].

use corelink_runners_contracts::RunnerState;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio::runtime::Handle;
use tokio_postgres::NoTls;

use crate::compute_meter;
use crate::ledger::{AdmitOutcome, ComputeGate, LeaseLedger, LeaseRecord, LeaseState};
use crate::tenant::TenantId;

/// Transport-security mode for the Postgres ledger connection (WP-B).
///
/// Opt-in via the `FABRIC_PG_TLS` env var; **default [`PgTlsMode::Disable`]**.
/// Resolve from the environment with [`pg_tls_mode_from_env`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgTlsMode {
    /// Plaintext connection (`NoTls`). The original, default behavior — the
    /// live private-network deployment is unaffected.
    Disable,
    /// TLS with **verify-full** posture: the server certificate chain is
    /// validated against the [`webpki_roots`] public-CA set and the SNI
    /// hostname (from the connection URL) is verified. No verification bypass.
    Require,
}

/// Resolve the [`PgTlsMode`] from an environment accessor (WP-B).
///
/// Unit-testable WITHOUT a live DB — pass any `Fn(&str) -> Option<String>`
/// (mirrors `reaper_config_from_env`'s closure pattern). Reads `FABRIC_PG_TLS`:
///
/// - absent / empty / `"disable"` → [`PgTlsMode::Disable`] (the default —
///   preserves today's exact `NoTls` behavior).
/// - `"require"` → [`PgTlsMode::Require`] (verify-full rustls; public-CA only).
/// - any other value → `Err` (fail-closed, like the other env parsers here — a
///   typo never silently downgrades transport security).
///
/// The value is trimmed and lowercased before matching (secret/env mounts often
/// append whitespace; the token vocabulary is case-insensitive).
pub fn pg_tls_mode_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<PgTlsMode> {
    match get("FABRIC_PG_TLS")
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        None => Ok(PgTlsMode::Disable),
        Some(v) => match v.as_str() {
            "disable" => Ok(PgTlsMode::Disable),
            "require" => Ok(PgTlsMode::Require),
            other => anyhow::bail!(
                "FABRIC_PG_TLS must be \"disable\" or \"require\" (got {other:?}); \
                 fail-closed — a typo never silently downgrades transport security"
            ),
        },
    }
}

/// Build a **verify-full** rustls client config for the `require` path (WP-B).
///
/// Trust anchors are the bundled [`webpki_roots`] Mozilla **public-CA** set
/// (hermetic — no OS trust store, no OpenSSL). Hostname/SNI verification stays
/// ON: this uses the safe default `with_root_certificates(..).with_no_client_auth()`
/// builder — there is NO `dangerous()` call and NO custom certificate verifier,
/// so a server presenting an untrusted or hostname-mismatched cert is rejected.
/// A Postgres behind a private CA will (correctly) fail to verify — that is the
/// documented M1 non-goal, not a bug.
///
/// `pub(crate)` so the durable billing sink ([`crate::billing_sink::PgBillingSink`])
/// reuses the SAME verify-full posture for its `Require` path rather than
/// duplicating the trust-anchor setup — there is exactly one TLS config builder
/// in the crate.
pub(crate) fn rustls_verify_full_config() -> rustls::ClientConfig {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    // Pin the `ring` crypto provider explicitly so config construction does not
    // depend on an ambient process-default provider being installed.
    rustls::ClientConfig::builder_with_provider(rustls::crypto::ring::default_provider().into())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the safe-default protocol versions")
        .with_root_certificates(root_store)
        .with_no_client_auth()
}

/// Idempotent schema. Safe to run on every [`PgLedger::connect`] — the enum
/// create swallows `duplicate_object`, the table/indexes are `IF NOT EXISTS`.
const DDL: &str = "\
DO $$ BEGIN CREATE TYPE lease_state AS ENUM ('pending','held','released','expired','crashed');
  EXCEPTION WHEN duplicate_object THEN null; END $$;
CREATE TABLE IF NOT EXISTS leases (
  lease_id text PRIMARY KEY, tenant text NOT NULL, state lease_state NOT NULL,
  box_ref text NOT NULL, created_at_ms bigint NOT NULL, updated_at_ms bigint NOT NULL);
-- ADR-0004 Decision-1: the durable lease-expiry deadline (epoch ms, nullable =
-- never-overdue). ADDITIVE + IDEMPOTENT so a fresh DB and an already-populated
-- one both apply cleanly; existing rows get NULL (never-overdue), preserving
-- the pre-ADR-0004 fail-safe until the next acquire writes a deadline.
ALTER TABLE leases ADD COLUMN IF NOT EXISTS deadline_ms bigint;
-- ADR-0004 Decision-2: the durable envelope checkpoint — an OPAQUE redacted
-- IntentMetrics summary (the ledger never parses it). ADDITIVE + IDEMPOTENT,
-- nullable = never-written. Lets ANY instance emit the abnormal forensic
-- envelope (§13 Item-3 SLA), not just the one holding the in-memory hook.
ALTER TABLE leases ADD COLUMN IF NOT EXISTS envelope_checkpoint text;
-- vCPU-h hard-ceiling (pricing.md §3; wave plan §8/§11/§13) — the compute state
-- is LEDGER-INTERNAL (NOT on the frozen wire-adjacent LeaseRecord). ADDITIVE +
-- IDEMPOTENT + NULLABLE: every pre-ceiling row keeps NULLs ⇒ accounting-OFF for
-- that lease (invisible to the admit Σ and the terminal accrual), so the
-- default-off path is byte-identical to today.
--   box_vcpu_count     — the serving box's vCPU count; the terminal-accrual multiplier.
--   accrual_period_key — period_key(created_at) (YYYYMM UTC); the Σ + accrual key.
--   reserved_vcpu_ms   — the constant worst case vcpu×ttl, IMMUTABLE after admit
--                        (P1-J): the rolling Σ sums this column directly, no per-row math.
--   accrued_at_ms      — idempotency stamp (P1-K): the terminal accrual fires once,
--                        gated `WHERE accrued_at_ms IS NULL`, across all 4 terminalizers.
ALTER TABLE leases ADD COLUMN IF NOT EXISTS box_vcpu_count int;
ALTER TABLE leases ADD COLUMN IF NOT EXISTS accrual_period_key int;
ALTER TABLE leases ADD COLUMN IF NOT EXISTS reserved_vcpu_ms bigint;
ALTER TABLE leases ADD COLUMN IF NOT EXISTS accrued_at_ms bigint;
CREATE INDEX IF NOT EXISTS leases_tenant_active_idx ON leases (tenant) WHERE state IN ('pending','held');
CREATE INDEX IF NOT EXISTS leases_held_idx ON leases (lease_id) WHERE state = 'held';
-- The durable per-(tenant, period) accrued vCPU·ms — the terminal half of the
-- ceiling invariant `accrued + Σ_reserved ≤ ceiling`. Upserted once per lease at
-- its terminal transition (idempotency-keyed on leases.accrued_at_ms).
CREATE TABLE IF NOT EXISTS compute_accrual (
  tenant text NOT NULL, period_key int NOT NULL, accrued_vcpu_ms bigint NOT NULL,
  PRIMARY KEY (tenant, period_key));
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
    // ADR-0004 Decision-1: nullable `bigint` ↔ `Option<u64>` (NULL → None =
    // never-overdue), same `as u64` epoch-ms mapping as the other time fields.
    let deadline: Option<i64> = row.get("deadline_ms");
    Ok(LeaseRecord {
        lease_id: row.get("lease_id"),
        tenant: TenantId::new(tenant_raw)?,
        state: state_from_db(&state_label)?,
        box_ref: row.get("box_ref"),
        created_at_ms: created as u64,
        updated_at_ms: updated as u64,
        deadline_ms: deadline.map(|d| d as u64),
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
    /// `tls` selects the transport (WP-B): [`PgTlsMode::Disable`] keeps the
    /// original plaintext `NoTls` pool (default; the live private-network
    /// deployment is unaffected), [`PgTlsMode::Require`] builds a verify-full
    /// rustls pool against the [`webpki_roots`] public-CA set. Resolve the mode
    /// from the environment with [`pg_tls_mode_from_env`].
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
        // Bound the pool-acquire wait (audit D2-P2). `PoolConfig::new` sets only
        // the max size, leaving deadpool's `timeouts.wait` at the default `None`
        // = wait forever. Under pool exhaustion `pool.get()` would then hang the
        // lease-admission hot path indefinitely. A bounded wait makes `get()`
        // return a `Timeout` error instead → mapped to `Err` → **fail-closed**
        // (reject the acquire) rather than a silent unbounded stall.
        // POOL FLOOR (wave plan §10 P0-E — deadpool starvation under the
        // compute ceiling). The accounting-on `transition` now holds a pooled
        // connection across the per-tenant advisory-lock wait (the default-off
        // path does NOT — it keeps the cheap autocommit UPDATE, so this floor
        // only bites once accounting is enabled). Under a same-tenant burst the
        // admits can drain a tiny pool all parked on the lock, while the `close`
        // that would release the lock + free a slot cannot get a connection →
        // 5 s timeout → 503 → self-amplifying. A small absolute floor keeps at
        // least one spare connection above the advisory-lock holders so a
        // releasing terminal transition is never starved. Deploy guidance
        // (caller-side): size `pool_size ≥ 2 × peak_concurrent_tenant_ops`.
        const POOL_FLOOR: usize = 4;
        let effective_pool_size = pool_size.max(POOL_FLOOR);
        let mut pool_cfg = deadpool_postgres::PoolConfig::new(effective_pool_size);
        pool_cfg.timeouts.wait = Some(std::time::Duration::from_secs(5));
        cfg.pool = Some(pool_cfg);
        // TLS branch (WP-B). `Disable` is the original `NoTls` path, byte-for-
        // byte unchanged. `Require` wraps the same pool builder in a verify-full
        // rustls connector (public-CA trust anchors, hostname verification on).
        let pool = match tls {
            PgTlsMode::Disable => cfg.create_pool(Some(Runtime::Tokio1), NoTls),
            PgTlsMode::Require => {
                let connector =
                    tokio_postgres_rustls::MakeRustlsConnect::new(rustls_verify_full_config());
                cfg.create_pool(Some(Runtime::Tokio1), connector)
            }
        }
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
            client
                .batch_execute("TRUNCATE TABLE leases, compute_accrual")
                .await?;
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
            //
            // COMPUTE-CEILING (wave plan §13 F3): `put` is a RECOVERY/test seam,
            // NEVER an admission path (admission is `try_admit_with_compute`). It
            // does NOT name the compute columns, so they default to NULL ⇒ the
            // inserted lease is accounting-OFF by construction: it can never
            // smuggle an accounting-on lease in below the admit Σ. (The assert
            // "box_vcpu_count IS None" is structural — there is no field on
            // `LeaseRecord` to carry it, and this INSERT never writes one.)
            let rows = client
                .query(
                    "INSERT INTO leases \
                       (lease_id, tenant, state, box_ref, created_at_ms, updated_at_ms, \
                        deadline_ms) \
                     VALUES ($1, $2, $3::text::lease_state, $4, $5, $6, $7) \
                     ON CONFLICT (lease_id) DO NOTHING \
                     RETURNING lease_id",
                    &[
                        &rec.lease_id,
                        &rec.tenant.as_str(),
                        &state_to_db(&rec.state),
                        &rec.box_ref,
                        &(rec.created_at_ms as i64),
                        &(rec.updated_at_ms as i64),
                        // ADR-0004: Option<u64> → nullable bigint (None → NULL).
                        &rec.deadline_ms.map(|d| d as i64),
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
                            created_at_ms, updated_at_ms, deadline_ms \
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
        //
        // COMPUTE-CEILING (wave plan §10 P0-E / §11.5 / P1-K, P1-I):
        // `transition` is respecified as an advisory-locked txn ONLY when the
        // lease is accounting-ON (`box_vcpu_count IS NOT NULL`). The default-OFF
        // path keeps the cheap AUTOCOMMIT `UPDATE` — byte-identical to today,
        // ZERO new pool pressure (no advisory wait on the close/cancel/reap hot
        // path → no deadpool starvation under a same-tenant burst).
        //
        // The branch is decided by a cheap pre-read of `box_vcpu_count` (+
        // `tenant`, needed for the lock key). `box_vcpu_count` is IMMUTABLE
        // after admit (the DDL never UPDATEs it), so the branch decision is
        // race-stable — no TOCTOU on the accounting-on/off classification.
        let legal_from = legal_from_for(&to);
        let is_terminal = matches!(
            to,
            RunnerState::Released | RunnerState::Expired | RunnerState::Crashed
        );
        let to_label = state_to_db(&LeaseState::Wire(to));
        self.block_on(async {
            let Some(legal_from) = legal_from else {
                anyhow::bail!("no legal transition into {to_label:?} (contract §1)");
            };

            // Cheap pre-read to classify accounting-on/off (immutable column).
            let mut client = self.pool.get().await?;
            let pre = client
                .query_opt(
                    "SELECT tenant, box_vcpu_count, accrual_period_key \
                     FROM leases WHERE lease_id = $1",
                    &[&lease_id],
                )
                .await?;
            let accounting_on = pre
                .as_ref()
                .map(|r| r.get::<_, Option<i32>>("box_vcpu_count").is_some())
                .unwrap_or(false);

            if !accounting_on {
                // DEFAULT-OFF: today's path EXACTLY — bare autocommit UPDATE, no
                // lock, no txn. (Also covers an unknown lease → 0 rows → Err.)
                let row = client
                    .query_opt(
                        // ADR-0004: the SET clause must NOT touch deadline_ms — a
                        // state change never alters the durable deadline; it is
                        // only RETURNed so the record carries it back unchanged.
                        "UPDATE leases \
                         SET state = $1::text::lease_state, updated_at_ms = $2 \
                         WHERE lease_id = $3 AND state = $4::text::lease_state \
                         RETURNING lease_id, tenant, state::text AS state, box_ref, \
                                   created_at_ms, updated_at_ms, deadline_ms",
                        &[&to_label, &(now_ms as i64), &lease_id, &legal_from],
                    )
                    .await?;
                return match row {
                    Some(r) => record_from_row(&r),
                    None => anyhow::bail!(
                        "illegal/lost lease transition for {lease_id}: -> {to_label} \
                         (no row in the required source state; contract §1 fail-closed)"
                    ),
                };
            }

            // ACCOUNTING-ON: advisory-locked txn. The lock is keyed on the SAME
            // `hashtext(tenant)` as `try_admit*`, so the terminal Σ→accrued
            // handoff serializes against every concurrent admit for this tenant
            // (no "left-Held-but-not-yet-accrued" over-admit window — §6 P0-2).
            let tenant: String = pre
                .as_ref()
                .expect("accounting_on implies the pre-read row exists")
                .get("tenant");
            let period_key: Option<i32> = pre
                .as_ref()
                .and_then(|r| r.get::<_, Option<i32>>("accrual_period_key"));

            let txn = client.transaction().await?;
            txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&tenant])
                .await?;

            // The winning terminal UPDATE, via a CTE so RETURNING can expose the
            // PRE-update `accrued_at_ms` (plain RETURNING reflects the NEW value).
            // `was_unaccrued` is the OLD `accrued_at_ms IS NULL`, the P1-K
            // idempotency key: the accrual fires once even if a close↔reaper race
            // somehow re-enters (the §1 matrix already blocks a second terminal
            // transition — terminal source matches 0 rows → Err — so this is
            // belt-and-braces). For a non-terminal transition (Pending→Held) the
            // stamp stays untouched ($5 = false).
            let stamp_terminal = is_terminal;
            let row = txn
                .query_opt(
                    "WITH prev AS ( \
                       SELECT lease_id, accrued_at_ms FROM leases \
                        WHERE lease_id = $3 AND state = $4::text::lease_state), \
                     upd AS ( \
                       UPDATE leases \
                          SET state = $1::text::lease_state, updated_at_ms = $2, \
                              accrued_at_ms = CASE WHEN $5 AND accrued_at_ms IS NULL \
                                                   THEN $2 ELSE accrued_at_ms END \
                        WHERE lease_id = $3 AND state = $4::text::lease_state \
                        RETURNING lease_id, tenant, state::text AS state, box_ref, \
                                  created_at_ms, updated_at_ms, deadline_ms, \
                                  box_vcpu_count) \
                     SELECT upd.*, (prev.accrued_at_ms IS NULL) AS was_unaccrued \
                       FROM upd JOIN prev USING (lease_id)",
                    &[
                        &to_label,
                        &(now_ms as i64),
                        &lease_id,
                        &legal_from,
                        &stamp_terminal,
                    ],
                )
                .await?;

            let Some(r) = row else {
                // Lost race / illegal pair / terminal source → roll back, Err.
                txn.rollback().await.ok();
                anyhow::bail!(
                    "illegal/lost lease transition for {lease_id}: -> {to_label} \
                     (no row in the required source state; contract §1 fail-closed)"
                );
            };

            // Accrue IFF: this is a winning TERMINAL transition AND the lease was
            // not already accrued (the idempotency stamp was NULL before now).
            let was_unaccrued: bool = r.get("was_unaccrued");
            if is_terminal && was_unaccrued {
                let box_vcpu: i32 = r.get("box_vcpu_count");
                let created: i64 = r.get("created_at_ms");
                // saturating_sub (P1-I): clock skew across instances / a provider
                // kill can make terminal < created → never underflow.
                let dur_ms = now_ms.saturating_sub(created as u64);
                let mut accrual = compute_meter::vcpu_ms(box_vcpu as u32, dur_ms);
                // i64 guard (P0-D): the per-lease accrual is bounded by the
                // i64-guarded reservation, but clamp defensively so the bigint
                // UPSERT can never RAISE `out of range`.
                if !compute_meter::fits_ledger(accrual) {
                    accrual = compute_meter::MAX_LEDGER_VCPU_MS;
                }
                let p = period_key.ok_or_else(|| {
                    anyhow::anyhow!(
                        "accounting-on lease {lease_id} has NULL accrual_period_key \
                         at terminal accrual (corrupt admit; fail-closed)"
                    )
                })?;
                // UPSERT += accrual. `LEAST(…, MAX_LEDGER_VCPU_MS)` clamps the
                // running sum so even a tenant nearing i64::MAX cannot make the
                // bigint arithmetic RAISE (Postgres errors on bigint overflow,
                // it does not wrap) — conservative (charges ≤ actual at the very
                // top), never a 503-storm. Honest tiers are ~1e9× under the cap.
                txn.execute(
                    "INSERT INTO compute_accrual (tenant, period_key, accrued_vcpu_ms) \
                     VALUES ($1, $2, $3) \
                     ON CONFLICT (tenant, period_key) DO UPDATE \
                       SET accrued_vcpu_ms = \
                           LEAST(compute_accrual.accrued_vcpu_ms + EXCLUDED.accrued_vcpu_ms, $4)",
                    &[
                        &tenant,
                        &p,
                        &(accrual as i64),
                        &(compute_meter::MAX_LEDGER_VCPU_MS as i64),
                    ],
                )
                .await?;
            }

            txn.commit().await?;
            record_from_row(&r)
        })
    }

    fn set_envelope_checkpoint(
        &mut self,
        lease_id: &str,
        checkpoint_json: &str,
    ) -> anyhow::Result<()> {
        // ADR-0004 Decision-2: overwrite the lease's opaque checkpoint blob.
        // Fail-closed: rowcount 0 (unknown lease) → Err, like every other
        // mutation. Idempotent overwrite for an existing lease.
        self.block_on(async {
            let client = self.pool.get().await?;
            let n = client
                .execute(
                    "UPDATE leases SET envelope_checkpoint = $2 WHERE lease_id = $1",
                    &[&lease_id, &checkpoint_json],
                )
                .await?;
            if n == 0 {
                anyhow::bail!(
                    "lease {lease_id} does not exist: cannot set envelope checkpoint (fail-closed)"
                );
            }
            Ok(())
        })
    }

    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        // The stored blob (opaque text), or None for an absent lease / NULL
        // (never-written) checkpoint — both collapse to None.
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "SELECT envelope_checkpoint FROM leases WHERE lease_id = $1",
                    &[&lease_id],
                )
                .await?;
            Ok(row.and_then(|r| r.get::<_, Option<String>>("envelope_checkpoint")))
        })
    }

    fn remove(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        // Admission-rollback seam (the over-admission fix in the acquire path):
        // drop a reserved `Pending` row when provisioning fails so the slot +
        // cap free immediately. Unconditional delete by id — `Ok(true)` if a
        // row was removed, `Ok(false)` if the lease was already gone/unknown.
        //
        // COMPUTE-CEILING (wave plan §13 F5): under accounting-ON, `remove` must
        // only ever target a `Pending` row. Deleting an accounting-on `Held`
        // lease would vanish its reservation from the Σ with NO terminal accrual
        // → silent under-count of real consumption. That is a contract
        // violation: the predicate `state = 'pending' OR box_vcpu_count IS NULL`
        // makes the DELETE fail-closed — an accounting-on Held lease is left
        // untouched (rowcount 0) and reported as a violation, so the consumed
        // vCPU·ms is never silently dropped. Accounting-OFF leases keep today's
        // exact unconditional-delete behavior (byte-identical).
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "DELETE FROM leases \
                     WHERE lease_id = $1 \
                       AND (state = 'pending' OR box_vcpu_count IS NULL) \
                     RETURNING (box_vcpu_count IS NOT NULL) AS was_accounting_on",
                    &[&lease_id],
                )
                .await?;
            if row.is_some() {
                return Ok(true);
            }
            // Nothing deleted: either the lease is absent/already-gone (today's
            // Ok(false)), OR it is an accounting-on, non-Pending lease we
            // REFUSED to drop. Distinguish so the latter is a loud violation.
            let still = client
                .query_opt(
                    "SELECT state::text AS state FROM leases \
                     WHERE lease_id = $1 AND box_vcpu_count IS NOT NULL \
                       AND state <> 'pending'",
                    &[&lease_id],
                )
                .await?;
            if let Some(r) = still {
                let state: String = r.get("state");
                anyhow::bail!(
                    "remove({lease_id}): refusing to drop an accounting-on lease in \
                     state {state:?} — `remove` is Pending-only under accounting-on \
                     (wave plan §13 F5; use `transition` to a terminal state so the \
                     consumed vCPU·ms accrues)"
                );
            }
            Ok(false)
        })
    }

    fn remove_if_pending(&mut self, lease_id: &str) -> anyhow::Result<bool> {
        // GUARDED admission-rollback for the stale-Pending sweep: the predicate
        // `state = 'pending'` is evaluated ATOMICALLY inside the DELETE, so a
        // lease that raced `Pending → Held` in the sweep window (between the
        // snapshot and this reclaim) is NOT deleted — the rowcount is 0 and the
        // sweep no-ops, leaving the now-`Held` live lease untouched. `Ok(true)`
        // iff a still-`Pending` row was removed; `Ok(false)` if absent OR no
        // longer Pending. This is the DB-atomic counterpart of the default
        // check-then-remove (the InMemory/File guard holds the same lock the
        // sweep does; the DB holds it in the single statement).
        self.block_on(async {
            let client = self.pool.get().await?;
            let n = client
                .execute(
                    "DELETE FROM leases WHERE lease_id = $1 AND state = 'pending'",
                    &[&lease_id],
                )
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
                            created_at_ms, updated_at_ms, deadline_ms \
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
                            created_at_ms, updated_at_ms, deadline_ms \
                     FROM leases WHERE state = 'held' ORDER BY lease_id",
                    &[],
                )
                .await?;
            rows.iter().map(record_from_row).collect()
        })
    }

    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        // cutoff = now - max_age (saturating). A Pending row whose created_at_ms
        // is STRICTLY BELOW the cutoff has outlived any legitimate provision
        // window and is reclaimable (the stale-Pending sweep). The comparison is
        // strict (`<`), matching the trait contract: a row sitting EXACTLY at the
        // bound has not yet sat LONGER than max_age, so it is not reclaimed
        // (consistent with InMemory / File). The filter is server-side so an
        // instance never pulls fresh, mid-provision Pendings.
        let cutoff = now_ms.saturating_sub(max_age_ms);
        self.block_on(async {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT lease_id, tenant, state::text AS state, box_ref, \
                            created_at_ms, updated_at_ms, deadline_ms \
                     FROM leases \
                     WHERE state = 'pending' AND created_at_ms < $1 \
                     ORDER BY lease_id",
                    &[&(cutoff as i64)],
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
                       (lease_id, tenant, state, box_ref, created_at_ms, updated_at_ms, \
                        deadline_ms) \
                     SELECT $1, $2, $3::text::lease_state, $4, $5, $5, $7 \
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
                        // ADR-0004: Option<u64> → nullable bigint (None → NULL).
                        &rec.deadline_ms.map(|d| d as i64),
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

    fn try_admit_with_compute(
        &mut self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        // DEFAULT-OFF (wave plan §11.1 / P2-H): `None` OR a `Some` gate whose
        // ceiling is 0 (the "disabled" sentinel) is EXACTLY today's `try_admit`
        // — the cheap count-and-insert under the existing advisory lock, the
        // compute columns left NULL. Byte-identical to the pre-ceiling path. A
        // ceiling of 0 is NEVER compared against (that would reject-all); it is a
        // pure branch.
        let active_gate = gate.filter(|g| g.ceiling_vcpu_ms > 0);
        let Some(g) = active_gate else {
            return Ok(if self.try_admit(rec, max_concurrency)? {
                AdmitOutcome::Admitted
            } else {
                AdmitOutcome::OverConcurrency
            });
        };

        // i64 GUARDS (P0-C/P0-D/P1-G): every value about to land in a SIGNED
        // bigint column is proven ≤ i64::MAX BEFORE the txn — saturating u64 math
        // is not enough, the `as i64` cast is the hazard (a value past i64::MAX
        // wraps NEGATIVE and would DEFEAT the ceiling). Fail-closed on any
        // over-i64 value (the caller already clamps `expiry_ms`; this is the
        // belt-and-braces guard at the storage boundary).
        if !compute_meter::fits_ledger(g.new_reserved_vcpu_ms) {
            anyhow::bail!(
                "try_admit_with_compute: new_reserved_vcpu_ms {} exceeds the i64 ledger \
                 bound (fail-closed — refusing to store a wrapping reservation)",
                g.new_reserved_vcpu_ms
            );
        }
        if !compute_meter::fits_ledger(g.ceiling_vcpu_ms) {
            anyhow::bail!(
                "try_admit_with_compute: ceiling_vcpu_ms {} exceeds the i64 ledger bound \
                 (an unlimited tier must be 0/disabled, not a huge value; fail-closed)",
                g.ceiling_vcpu_ms
            );
        }

        self.block_on(async {
            let mut client = self.pool.get().await?;
            let txn = client.transaction().await?;
            // 1. Serialize all admits for this tenant across instances (SAME key
            //    as `try_admit` + the accounting-on `transition`), so the gate
            //    read and the insert are atomic against every concurrent admit
            //    AND every concurrent terminal accrual.
            txn.execute(
                "SELECT pg_advisory_xact_lock(hashtext($1))",
                &[&rec.tenant.as_str()],
            )
            .await?;

            // 2. THE SINGLE-STATEMENT GATE (P0-G / §13 F4): ONE SELECT joins the
            //    concurrency count, the in-flight reservation Σ (over pending+held
            //    in period P — the cap's active set, P1-6), and the durable
            //    accrued (LEFT JOIN compute_accrual, 0 if absent). No inter-read
            //    window; the REPEATABLE-READ fallback is retracted (§13 F4).
            let gate_row = txn
                .query_one(
                    // `SUM(bigint)` widens to `numeric` (overflow-safe in Pg). The
                    // active set is bounded, so the honest Σ fits bigint; LEAST-clamp
                    // to the i64 bound BEFORE the `::bigint` cast so an adversarial Σ
                    // pins at the bound (the gate then rejects) rather than RAISING
                    // `bigint out of range`. Fail-closed, never a wrap.
                    "SELECT \
                       (SELECT count(*) FROM leases \
                          WHERE tenant = $1 AND state IN ('pending','held')) AS cnt, \
                       LEAST( \
                         (SELECT COALESCE(SUM(reserved_vcpu_ms), 0) FROM leases \
                            WHERE tenant = $1 AND state IN ('pending','held') \
                              AND accrual_period_key = $2), \
                         $3::bigint)::bigint AS sigma, \
                       COALESCE((SELECT accrued_vcpu_ms FROM compute_accrual \
                                   WHERE tenant = $1 AND period_key = $2), 0) AS accrued",
                    &[
                        &rec.tenant.as_str(),
                        &(g.period_key as i32),
                        &(compute_meter::MAX_LEDGER_VCPU_MS as i64),
                    ],
                )
                .await?;
            let cnt: i64 = gate_row.get("cnt");
            let sigma: i64 = gate_row.get("sigma");
            let accrued: i64 = gate_row.get("accrued");

            // CONCURRENCY first (mirrors `try_admit`'s `count < max`).
            if cnt >= i64::from(max_concurrency) {
                txn.rollback().await.ok();
                return Ok(AdmitOutcome::OverConcurrency);
            }
            // COMPUTE ceiling: `accrued + Σ + new ≤ ceiling`. Saturating across
            // the i64 reads (each individually ≤ i64::MAX) so the sum cannot wrap
            // before the comparison (P2-8). `accrued`/`sigma` are non-negative by
            // construction (every write is i64-guarded), so the `as u64` is exact.
            let projected = (accrued as u64)
                .saturating_add(sigma as u64)
                .saturating_add(g.new_reserved_vcpu_ms);
            if projected > g.ceiling_vcpu_ms {
                txn.rollback().await.ok();
                return Ok(AdmitOutcome::OverCompute);
            }

            // 3. ADMIT: insert the Pending row WITH the 3 compute columns set
            //    (reserved_vcpu_ms IMMUTABLE after admit — P1-J; accrued_at_ms
            //    left NULL for the terminal idempotency key). Under the lock the
            //    cnt/Σ/accrued the gate read cannot have changed, so this is the
            //    same atomic check-and-reserve `try_admit` gives for concurrency.
            let res = txn
                .query(
                    "INSERT INTO leases \
                       (lease_id, tenant, state, box_ref, created_at_ms, updated_at_ms, \
                        deadline_ms, box_vcpu_count, accrual_period_key, reserved_vcpu_ms) \
                     VALUES ($1, $2, $3::text::lease_state, $4, $5, $5, $6, $7, $8, $9) \
                     RETURNING lease_id",
                    &[
                        &rec.lease_id,
                        &rec.tenant.as_str(),
                        &state_to_db(&rec.state),
                        &rec.box_ref,
                        &(rec.created_at_ms as i64),
                        &rec.deadline_ms.map(|d| d as i64),
                        &(g.box_vcpu_count as i32),
                        &(g.period_key as i32),
                        &(g.new_reserved_vcpu_ms as i64),
                    ],
                )
                .await;
            match res {
                Ok(rows) => {
                    debug_assert_eq!(rows.len(), 1, "unconditional insert returns one row");
                    txn.commit().await?;
                    Ok(AdmitOutcome::Admitted)
                }
                Err(e) => {
                    txn.rollback().await.ok();
                    Err(anyhow::anyhow!(
                        "try_admit_with_compute failed for lease {} \
                         (duplicate id or DB error): {e}",
                        rec.lease_id
                    ))
                }
            }
        })
    }

    fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> anyhow::Result<u64> {
        // The durable accrued vCPU·ms for (tenant, period) — 0 if the row is
        // absent (no accounting, or a fresh period). Stored ≤ i64::MAX by every
        // write path, so the `as u64` is exact.
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "SELECT accrued_vcpu_ms FROM compute_accrual \
                     WHERE tenant = $1 AND period_key = $2",
                    &[&tenant.as_str(), &(period_key as i32)],
                )
                .await?;
            Ok(row
                .map(|r| r.get::<_, i64>("accrued_vcpu_ms") as u64)
                .unwrap_or(0))
        })
    }
}

#[cfg(test)]
mod tls_mode_tests {
    //! `pg_tls_mode_from_env` resolution (WP-B). No live DB needed — these drive
    //! the resolver with a closure, exactly like `reaper_config_from_env`'s tests.
    use super::{PgTlsMode, pg_tls_mode_from_env};

    /// Env accessor that returns `val` for `FABRIC_PG_TLS` and `None` otherwise.
    fn only_pg_tls(val: Option<&str>) -> impl Fn(&str) -> Option<String> + '_ {
        move |k: &str| {
            if k == "FABRIC_PG_TLS" {
                val.map(str::to_string)
            } else {
                None
            }
        }
    }

    /// Absent → the default, `Disable` (preserves today's `NoTls` behavior).
    #[test]
    fn absent_is_disable() {
        let mode = pg_tls_mode_from_env(only_pg_tls(None)).unwrap();
        assert_eq!(mode, PgTlsMode::Disable);
    }

    /// Empty string (and whitespace-only) → `Disable` (treated as absent).
    #[test]
    fn empty_is_disable() {
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some(""))).unwrap(),
            PgTlsMode::Disable
        );
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some("   "))).unwrap(),
            PgTlsMode::Disable
        );
    }

    /// `"disable"` → `Disable` (case/whitespace-insensitive).
    #[test]
    fn disable_is_disable() {
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some("disable"))).unwrap(),
            PgTlsMode::Disable
        );
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some("  DISABLE\n"))).unwrap(),
            PgTlsMode::Disable
        );
    }

    /// `"require"` → `Require` (case/whitespace-insensitive).
    #[test]
    fn require_is_require() {
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some("require"))).unwrap(),
            PgTlsMode::Require
        );
        assert_eq!(
            pg_tls_mode_from_env(only_pg_tls(Some(" Require "))).unwrap(),
            PgTlsMode::Require
        );
    }

    /// Any other value → `Err` (fail-closed; never a silent downgrade).
    #[test]
    fn garbage_is_err() {
        for bad in ["verify-full", "true", "1", "on", "tls", "prefer"] {
            assert!(
                pg_tls_mode_from_env(only_pg_tls(Some(bad))).is_err(),
                "FABRIC_PG_TLS={bad:?} must be rejected (fail-closed)"
            );
        }
    }

    /// The `require` path builds a verify-full config without panicking (the
    /// ring provider supports the safe-default protocol versions). This also
    /// exercises the webpki-roots trust-anchor load.
    #[test]
    fn verify_full_config_builds() {
        let cfg = super::rustls_verify_full_config();
        // Sanity: at least one ALPN-free default; the builder did not install a
        // dangerous (verification-bypassing) verifier — there is no API to assert
        // that directly, so we assert the config constructed and the root set is
        // non-empty, which is the load-bearing invariant.
        let _ = cfg;
        assert!(
            !webpki_roots::TLS_SERVER_ROOTS.is_empty(),
            "webpki-roots public-CA set must be non-empty"
        );
    }
}

// ── vCPU-h compute-ceiling acceptance (WP-E) ──────────────────────────────────
//
// Gated on `TEST_DATABASE_URL`, EXACTLY like `ledger_conformance::pg_runs` and
// `billing_sink`'s Pg tests: ABSENT (the builder Mac / CI has no Postgres) → each
// test prints a skip line and returns early, so the suite stays green; PRESENT →
// the real DB exercises the single-join gate, the conditional-lock accrual, the
// idempotency key, and an i64-bigint round-trip. Every test uses a UNIQUE tenant
// nonce so parallel processes never collide on the shared `leases`/`compute_accrual`
// tables (no TRUNCATE needed → no cross-test interference).
#[cfg(test)]
mod compute_ceiling_pg_tests {
    use std::sync::Mutex;

    use super::*;
    use crate::compute_meter::{self, MAX_LEDGER_VCPU_MS};
    use crate::ledger::{AdmitOutcome, ComputeGate, LeaseRecord, LeaseState};
    use corelink_runners_contracts::RunnerState;

    /// Serializes the multi-step compute-ceiling Pg tests against EACH OTHER
    /// (mirrors `ledger_conformance::pg_runs::PG_TEST_SERIAL`). Each test uses a
    /// UNIQUE tenant nonce so it never collides on ROWS, but the in-flight
    /// Σ-reservation + accrual tests assert on table state that must persist
    /// across their own steps — this lock keeps them from interleaving.
    ///
    /// NOTE (DB-suite convention): the SEPARATE `pg_runs` conformance module does
    /// a global `TRUNCATE` at startup; running BOTH Pg modules in parallel would
    /// let that truncate wipe these tests' in-flight rows. The repo's DB suite is
    /// therefore invoked `--test-threads=1` (the established convention — these
    /// tests skip entirely on CI, which has no Postgres). Under the default
    /// parallel run WITH a DB present, use `--test-threads=1`.
    static PG_CEILING_SERIAL: Mutex<()> = Mutex::new(());

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

    fn connect(rt: &tokio::runtime::Runtime, url: &str) -> PgLedger {
        rt.block_on(async {
            PgLedger::connect(url, 4, PgTlsMode::Disable)
                .await
                .expect("PgLedger::connect")
        })
    }

    /// A process-unique nonce so parallel test binaries never collide on the
    /// shared tables (we never TRUNCATE here — every tenant id is fresh).
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

    fn pending(lease_id: &str, tenant: &TenantId, created_ms: u64) -> LeaseRecord {
        LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: tenant.clone(),
            state: LeaseState::Pending,
            box_ref: format!("box-{lease_id}"),
            created_at_ms: created_ms,
            updated_at_ms: created_ms,
            deadline_ms: None,
        }
    }

    /// DEFAULT-OFF byte-identical: `None` gate AND a `Some` gate with ceiling 0
    /// both behave EXACTLY like `try_admit` (admit at cap, over-cap rejection),
    /// and leave the compute columns NULL (compute_accrued stays 0).
    #[test]
    fn default_off_is_byte_identical_to_try_admit() {
        let Some(url) = db_url() else {
            eprintln!("default_off_is_byte_identical: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let mut led = connect(&rt, &url);
        let t = TenantId::new(nonce("off")).unwrap();

        // None gate, cap 1 → first admits, second over-cap.
        let id1 = nonce("l1");
        let id2 = nonce("l2");
        assert_eq!(
            led.try_admit_with_compute(pending(&id1, &t, 1_000), 1, None)
                .unwrap(),
            AdmitOutcome::Admitted
        );
        assert_eq!(
            led.try_admit_with_compute(pending(&id2, &t, 1_000), 1, None)
                .unwrap(),
            AdmitOutcome::OverConcurrency
        );

        // ceiling == 0 (disabled sentinel) SKIPS the compute gate entirely.
        let t0 = TenantId::new(nonce("off0")).unwrap();
        let id3 = nonce("l3");
        let zero_gate = ComputeGate {
            period_key: 202406,
            ceiling_vcpu_ms: 0,
            box_vcpu_count: 8,
            new_reserved_vcpu_ms: u64::MAX, // would defeat any real ceiling — proven ignored
        };
        assert_eq!(
            led.try_admit_with_compute(pending(&id3, &t0, 1_000), 5, Some(zero_gate))
                .unwrap(),
            AdmitOutcome::Admitted
        );
        // No accrual key was written (ceiling-0 path = NULL columns ⇒ no accounting).
        assert_eq!(led.compute_accrued(&t0, 202406).unwrap(), 0);
    }

    /// CEILING-REACHED → OverCompute, and the rejected admit inserts NOTHING.
    #[test]
    fn ceiling_reached_rejects_with_over_compute() {
        let Some(url) = db_url() else {
            eprintln!("ceiling_reached: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let mut led = connect(&rt, &url);
        let t = TenantId::new(nonce("ceil")).unwrap();
        let period = 202406u32;
        // ceiling = 100 vCPU·ms. One lease reserves 60 (fits); a second reserving
        // 60 would project 120 > 100 → OverCompute.
        let g1 = ComputeGate {
            period_key: period,
            ceiling_vcpu_ms: 100,
            box_vcpu_count: 2,
            new_reserved_vcpu_ms: 60,
        };
        let id1 = nonce("c1");
        assert_eq!(
            led.try_admit_with_compute(pending(&id1, &t, 1_000), 100, Some(g1))
                .unwrap(),
            AdmitOutcome::Admitted
        );
        let g2 = ComputeGate {
            new_reserved_vcpu_ms: 60,
            ..g1
        };
        let id2 = nonce("c2");
        assert_eq!(
            led.try_admit_with_compute(pending(&id2, &t, 1_000), 100, Some(g2))
                .unwrap(),
            AdmitOutcome::OverCompute
        );
        // The rejected lease inserted nothing — the tenant still has exactly one.
        assert_eq!(led.by_tenant(&t).unwrap().len(), 1);
        // A lease that fits the remaining headroom (40) still admits — proves the
        // rejection was the ceiling, not a stuck gate.
        let g3 = ComputeGate {
            new_reserved_vcpu_ms: 40,
            ..g1
        };
        let id3 = nonce("c3");
        assert_eq!(
            led.try_admit_with_compute(pending(&id3, &t, 1_000), 100, Some(g3))
                .unwrap(),
            AdmitOutcome::Admitted
        );
    }

    /// IN-FLIGHT Σ reservation: the rolling Σ sums `reserved_vcpu_ms` over
    /// pending+held in the period. Two in-flight leases consume headroom; a
    /// terminal transition moves a lease from Σ into `accrued` (actual ≤ reserved).
    #[test]
    fn in_flight_sigma_and_terminal_handoff() {
        let Some(url) = db_url() else {
            eprintln!("in_flight_sigma: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let mut led = connect(&rt, &url);
        let t = TenantId::new(nonce("sig")).unwrap();
        let period = 202406u32;
        // ceiling 1000; box 4 vCPU; reserve 400 each (ttl 100 ms ⇒ 4*100=400).
        let mk = |new_reserved| ComputeGate {
            period_key: period,
            ceiling_vcpu_ms: 1_000,
            box_vcpu_count: 4,
            new_reserved_vcpu_ms: new_reserved,
        };
        let (id1, id2, id3) = (nonce("s1"), nonce("s2"), nonce("s3"));
        // created at t=1000, ttl 100 ⇒ reserved 400.
        assert_eq!(
            led.try_admit_with_compute(pending(&id1, &t, 1_000), 100, Some(mk(400)))
                .unwrap(),
            AdmitOutcome::Admitted
        );
        assert_eq!(
            led.try_admit_with_compute(pending(&id2, &t, 1_000), 100, Some(mk(400)))
                .unwrap(),
            AdmitOutcome::Admitted
        );
        // Σ is now 800; a 3rd reserving 400 → 1200 > 1000 → OverCompute.
        assert_eq!(
            led.try_admit_with_compute(pending(&id3, &t, 1_000), 100, Some(mk(400)))
                .unwrap(),
            AdmitOutcome::OverCompute
        );
        // Terminalize lease 1: Pending→Held→Released. Actual run = terminal−created.
        led.transition(&id1, RunnerState::Held, 1_050).unwrap();
        led.transition(&id1, RunnerState::Released, 1_060).unwrap();
        // Accrued = 4 vCPU × (1060−1000) = 240; lease 1 left Σ (was 400).
        assert_eq!(led.compute_accrued(&t, period).unwrap(), 240);
        // Σ now 400 (lease 2 only) + accrued 240 = 640; a 3rd reserving 400 →
        // 640+400 = 1040 > 1000 → still OverCompute, but reserving 300 fits (940).
        assert_eq!(
            led.try_admit_with_compute(pending(&nonce("s4"), &t, 1_000), 100, Some(mk(400)))
                .unwrap(),
            AdmitOutcome::OverCompute
        );
        assert_eq!(
            led.try_admit_with_compute(pending(&nonce("s5"), &t, 1_000), 100, Some(mk(300)))
                .unwrap(),
            AdmitOutcome::Admitted
        );
    }

    /// ONCE-ONLY accrual across terminalizers (P1-K idempotency key): a second
    /// terminal transition on an already-accrued lease (the close↔reaper race)
    /// must NOT double-accrue. We can't re-Release a Released lease (illegal per
    /// §1), so we prove the key via the cross-terminalizer property: a Held lease
    /// that is Expired accrues once; a subsequent illegal re-terminalize errors
    /// and leaves accrued unchanged.
    #[test]
    fn accrual_is_once_only_idempotency_keyed() {
        let Some(url) = db_url() else {
            eprintln!("accrual_once_only: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let mut led = connect(&rt, &url);
        let t = TenantId::new(nonce("once")).unwrap();
        let period = 202406u32;
        let g = ComputeGate {
            period_key: period,
            ceiling_vcpu_ms: 10_000,
            box_vcpu_count: 2,
            new_reserved_vcpu_ms: 1_000,
        };
        let id = nonce("o1");
        led.try_admit_with_compute(pending(&id, &t, 1_000), 100, Some(g))
            .unwrap();
        led.transition(&id, RunnerState::Held, 1_010).unwrap();
        led.transition(&id, RunnerState::Expired, 1_100).unwrap();
        // 2 vCPU × (1100−1000) = 200.
        assert_eq!(led.compute_accrued(&t, period).unwrap(), 200);
        // Any further terminal transition is illegal (terminal source) → Err, and
        // crucially does NOT add a second accrual (the row is now terminal, and
        // accrued_at_ms is stamped).
        assert!(led.transition(&id, RunnerState::Released, 1_200).is_err());
        assert_eq!(
            led.compute_accrued(&t, period).unwrap(),
            200,
            "accrual must be once-only (idempotency key)"
        );
    }

    /// i64-OVERFLOW round-trip through a REAL bigint column: drive the accrual
    /// UPSERT to near i64::MAX and prove (a) it stores losslessly, (b) the
    /// running sum CLAMPS at MAX_LEDGER_VCPU_MS rather than RAISING `bigint out of
    /// range`, and (c) an over-i64 reservation at admit is rejected fail-closed.
    #[test]
    fn i64_bigint_overflow_roundtrip() {
        let Some(url) = db_url() else {
            eprintln!("i64_overflow: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let mut led = connect(&rt, &url);

        // (c) over-i64 reservation → fail-closed Err (the `as i64` would wrap
        // negative and defeat the ceiling).
        let t_bad = TenantId::new(nonce("bad")).unwrap();
        let over = ComputeGate {
            period_key: 202406,
            ceiling_vcpu_ms: 1_000,
            box_vcpu_count: 1,
            new_reserved_vcpu_ms: MAX_LEDGER_VCPU_MS + 1,
        };
        assert!(
            led.try_admit_with_compute(pending(&nonce("b1"), &t_bad, 1_000), 100, Some(over))
                .is_err(),
            "an over-i64 reservation must be rejected fail-closed"
        );

        // (a)+(b) drive the accrual near i64::MAX through a real bigint column.
        // A huge box_vcpu × a huge run pins vcpu_ms at u64::MAX → clamped to
        // MAX_LEDGER_VCPU_MS by transition, then the UPSERT LEAST() clamps the
        // running sum so the bigint never overflows (Postgres RAISES on overflow).
        let t_big = TenantId::new(nonce("big")).unwrap();
        let period = 202406u32;
        let huge_box = u32::MAX; // vcpu_ms(u32::MAX, dur) saturates fast
        let g = ComputeGate {
            period_key: period,
            ceiling_vcpu_ms: MAX_LEDGER_VCPU_MS, // max real ceiling
            box_vcpu_count: huge_box,
            new_reserved_vcpu_ms: MAX_LEDGER_VCPU_MS, // i64-max reservation, admits
        };
        let id = nonce("big1");
        assert_eq!(
            led.try_admit_with_compute(pending(&id, &t_big, 0), 100, Some(g))
                .unwrap(),
            AdmitOutcome::Admitted
        );
        led.transition(&id, RunnerState::Held, 1).unwrap();
        // terminal at a now so large that vcpu_ms(u32::MAX, ~3e9) EXCEEDS i64::MAX
        // (i64::MAX / u32::MAX ≈ 2.15e9, so 3e9 overshoots) ⇒ clamped to
        // MAX_LEDGER_VCPU_MS by `transition`; the UPSERT then stores the clamped
        // value through a REAL bigint column without Postgres RAISING `out of
        // range`. Proves the i64 guard end-to-end against a live column.
        led.transition(&id, RunnerState::Crashed, 3_000_000_000)
            .unwrap();
        let accrued = led.compute_accrued(&t_big, period).unwrap();
        assert_eq!(
            accrued, MAX_LEDGER_VCPU_MS,
            "the accrual clamps at the i64 bound — a real bigint column, no overflow RAISE"
        );
        assert!(compute_meter::fits_ledger(accrued));
    }

    /// 2-INSTANCE over-ceiling regression: two INDEPENDENT `PgLedger` handles on
    /// the SAME database race to admit into a tenant whose ceiling has room for
    /// EXACTLY ONE of the two reservations. The single-join gate under the shared
    /// advisory lock must admit exactly one → OverCompute the other. This is the
    /// cross-instance loss-impossible proof InMemory/File cannot give.
    #[test]
    fn two_instance_over_ceiling_admits_exactly_one() {
        let Some(url) = db_url() else {
            eprintln!("two_instance_over_ceiling: TEST_DATABASE_URL unset — skipping");
            return;
        };
        let _serial = PG_CEILING_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let rt = rt();
        let t = TenantId::new(nonce("xinst")).unwrap();
        let period = 202406u32;
        // ceiling 100; each reservation 60 ⇒ only one of the two can fit.
        let gate = ComputeGate {
            period_key: period,
            ceiling_vcpu_ms: 100,
            box_vcpu_count: 1,
            new_reserved_vcpu_ms: 60,
        };
        let id_a = nonce("xa");
        let id_b = nonce("xb");

        let mut led_a = connect(&rt, &url);
        let mut led_b = connect(&rt, &url);

        let oa = std::sync::Arc::new(std::sync::Mutex::new(None::<AdmitOutcome>));
        let ob = std::sync::Arc::new(std::sync::Mutex::new(None::<AdmitOutcome>));
        let (oa2, ob2) = (std::sync::Arc::clone(&oa), std::sync::Arc::clone(&ob));
        let (ta, tb) = (t.clone(), t.clone());

        rt.block_on(async {
            let ha = tokio::task::spawn_blocking(move || {
                let r = led_a
                    .try_admit_with_compute(pending(&id_a, &ta, 1_000), 100, Some(gate))
                    .unwrap();
                *oa2.lock().unwrap() = Some(r);
            });
            let hb = tokio::task::spawn_blocking(move || {
                let r = led_b
                    .try_admit_with_compute(pending(&id_b, &tb, 1_000), 100, Some(gate))
                    .unwrap();
                *ob2.lock().unwrap() = Some(r);
            });
            ha.await.unwrap();
            hb.await.unwrap();
        });

        let a = oa.lock().unwrap().unwrap();
        let b = ob.lock().unwrap().unwrap();
        // Exactly one Admitted, the other OverCompute — never both admitted.
        let admitted = (a == AdmitOutcome::Admitted) as u8 + (b == AdmitOutcome::Admitted) as u8;
        assert_eq!(
            admitted, 1,
            "the single-join gate under the shared advisory lock must admit \
             EXACTLY ONE across the two instances (got a={a:?}, b={b:?})"
        );
        let other_over = a == AdmitOutcome::OverCompute || b == AdmitOutcome::OverCompute;
        assert!(
            other_over,
            "the loser must be OverCompute (got a={a:?}, b={b:?})"
        );
        let led = connect(&rt, &url);
        assert_eq!(
            led.by_tenant(&t).unwrap().len(),
            1,
            "exactly one lease admitted across both instances"
        );
    }
}
