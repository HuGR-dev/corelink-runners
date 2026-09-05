//! PostgreSQL implementation of the internal stale-Pending cleanup claim.
//!
//! The claim table is deliberately separate from `lease_state`: clients must
//! never observe a sixth lifecycle state.  Every mutator which can consume a
//! Pending row takes the lease row lock and checks this table after that lock is
//! acquired; this module supplies the claim and final-delete halves of that
//! fence.

use crate::ledger::{LeaseRecord, LeaseState};
use crate::pg_ledger::{PgLedger, record_from_row};
use tokio_postgres::Transaction;

const RECORD_COLUMNS: &str = "lease_id, tenant, state::text AS state, box_ref, \
    created_at_ms, updated_at_ms, deadline_ms, billing_acquired_at_ms";

/// Strict stale-age cutoff, representable by PostgreSQL's `bigint`.
pub(crate) fn strict_cutoff_ms(now_ms: u64, max_age_ms: u64) -> anyhow::Result<i64> {
    let now_ms = checked_epoch_ms(now_ms, "now_ms")?;
    let max_age_ms = checked_epoch_ms(max_age_ms, "max_age_ms")?;
    Ok(now_ms.saturating_sub(max_age_ms))
}

fn checked_epoch_ms(value: u64, field: &str) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| {
        anyhow::anyhow!("pending cleanup {field} exceeds PostgreSQL bigint range (fail-closed)")
    })
}

/// Claim stale Pending leases, also returning Pending rows already claimed by a
/// prior sweep. Each row has its own short transaction: that keeps advisory
/// locks tenant-scoped and avoids holding unrelated tenant locks while a slow
/// contender owns one row.
pub(crate) fn claim_stale_pending_cleanup(
    ledger: &PgLedger,
    now_ms: u64,
    max_age_ms: u64,
) -> anyhow::Result<Vec<LeaseRecord>> {
    let cutoff = strict_cutoff_ms(now_ms, max_age_ms)?;
    let claimed_at_ms = checked_epoch_ms(now_ms, "now_ms")?;
    ledger.block_on(async {
        let client = ledger.pool.get().await?;
        // This is only a work list. `claim_one` re-reads and locks every row, so
        // neither this snapshot nor the LEFT JOIN decides correctness.
        let rows = client
            .query(
                "SELECT l.lease_id FROM leases l \
                 LEFT JOIN pending_cleanup_claims c USING (lease_id) \
                 WHERE l.state = 'pending' AND (l.created_at_ms < $1 OR c.lease_id IS NOT NULL) \
                 ORDER BY l.lease_id",
                &[&cutoff],
            )
            .await?;
        let ids: Vec<String> = rows.iter().map(|row| row.get("lease_id")).collect();
        drop(client);

        let mut claimed = Vec::with_capacity(ids.len());
        for lease_id in ids {
            if let Some(record) = claim_one(ledger, &lease_id, cutoff, claimed_at_ms).await? {
                claimed.push(record);
            }
        }
        Ok(claimed)
    })
}

/// Take the parent row lock, then inspect the claim table in a fresh statement.
/// Callers that need a tenant advisory lock must acquire it before this helper.
pub(crate) async fn lock_unclaimed_lease(
    txn: &Transaction<'_>,
    lease_id: &str,
) -> anyhow::Result<bool> {
    let locked = txn
        .query_opt(
            "SELECT 1 FROM leases WHERE lease_id = $1 FOR UPDATE",
            &[&lease_id],
        )
        .await?;
    if locked.is_none() {
        return Ok(false);
    }
    Ok(txn
        .query_opt(
            "SELECT 1 FROM pending_cleanup_claims WHERE lease_id = $1",
            &[&lease_id],
        )
        .await?
        .is_none())
}

/// Shared rollback removal fence. `pending_only` selects `remove_if_pending`;
/// ordinary `remove` retains its accounting-on Held fail-closed error.
pub(crate) fn remove(
    ledger: &PgLedger,
    lease_id: &str,
    pending_only: bool,
) -> anyhow::Result<bool> {
    ledger.block_on(async {
        let mut client = ledger.pool.get().await?;
        let pre = client
            .query_opt(
                "SELECT tenant, box_vcpu_count IS NOT NULL AS accounting_on \
                 FROM leases WHERE lease_id = $1",
                &[&lease_id],
            )
            .await?;
        let Some(pre) = pre else {
            return Ok(false);
        };
        let tenant: String = pre.get("tenant");
        let accounting_on: bool = pre.get("accounting_on");
        let txn = client.transaction().await?;
        if accounting_on {
            txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&tenant])
                .await?;
        }
        if !lock_unclaimed_lease(&txn, lease_id).await? {
            txn.rollback().await.ok();
            return Ok(false);
        }
        let row = if pending_only {
            txn.query_opt(
                "DELETE FROM leases WHERE lease_id = $1 AND state = 'pending' RETURNING lease_id",
                &[&lease_id],
            )
            .await?
        } else {
            txn.query_opt(
                "DELETE FROM leases \
                 WHERE lease_id = $1 \
                   AND (state = 'pending' OR box_vcpu_count IS NULL) \
                 RETURNING lease_id",
                &[&lease_id],
            )
            .await?
        };
        if row.is_some() {
            txn.commit().await?;
            return Ok(true);
        }
        if !pending_only {
            let still = txn
                .query_opt(
                    "SELECT state::text AS state FROM leases \
                     WHERE lease_id = $1 AND box_vcpu_count IS NOT NULL \
                       AND state <> 'pending'",
                    &[&lease_id],
                )
                .await?;
            if let Some(row) = still {
                let state: String = row.get("state");
                txn.rollback().await.ok();
                anyhow::bail!(
                    "remove({lease_id}): refusing to drop an accounting-on lease in \
                     state {state:?} — `remove` is Pending-only under accounting-on \
                     (wave plan §13 F5; use `transition` to a terminal state so the \
                     consumed vCPU·ms accrues)"
                );
            }
        }
        txn.commit().await?;
        Ok(false)
    })
}

/// Delete only a Pending lease still owned by this cleanup claim.
pub(crate) fn finish_pending_cleanup(ledger: &PgLedger, lease_id: &str) -> anyhow::Result<bool> {
    ledger.block_on(async {
        let mut client = ledger.pool.get().await?;
        let pre = client
            .query_opt(
                "SELECT tenant, box_vcpu_count IS NOT NULL AS accounting_on \
                 FROM leases WHERE lease_id = $1",
                &[&lease_id],
            )
            .await?;
        let Some(pre) = pre else {
            return Ok(false);
        };
        let tenant: String = pre.get("tenant");
        let accounting_on: bool = pre.get("accounting_on");
        let txn = client.transaction().await?;
        // Keep the existing tenant-lock-before-row-lock ordering for
        // accounting-on rows. Their reservation is part of the tenant Σ until
        // this deletion commits.
        if accounting_on {
            txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&tenant])
                .await?;
        }
        let state = txn
            .query_opt(
                "SELECT state::text AS state FROM leases WHERE lease_id = $1 FOR UPDATE",
                &[&lease_id],
            )
            .await?;
        let Some(state) = state else {
            txn.rollback().await.ok();
            return Ok(false);
        };
        let state: String = state.get("state");
        if state != "pending" {
            txn.rollback().await.ok();
            return Ok(false);
        }
        let owned = txn
            .query_opt(
                "SELECT 1 FROM pending_cleanup_claims WHERE lease_id = $1",
                &[&lease_id],
            )
            .await?
            .is_some();
        if !owned {
            txn.rollback().await.ok();
            return Ok(false);
        }
        let deleted = txn
            .execute(
                "DELETE FROM leases WHERE lease_id = $1 AND state = 'pending'",
                &[&lease_id],
            )
            .await?;
        txn.commit().await?;
        Ok(deleted == 1)
    })
}

async fn claim_one(
    ledger: &PgLedger,
    lease_id: &str,
    cutoff: i64,
    claimed_at_ms: i64,
) -> anyhow::Result<Option<LeaseRecord>> {
    let mut client = ledger.pool.get().await?;
    // `box_vcpu_count` is immutable after admission. Read it before beginning
    // the transaction solely to choose the pre-existing advisory-lock ordering;
    // the locked read below remains the authoritative state/predicate check.
    let pre = client
        .query_opt(
            "SELECT tenant, box_vcpu_count IS NOT NULL AS accounting_on \
             FROM leases WHERE lease_id = $1",
            &[&lease_id],
        )
        .await?;
    let Some(pre) = pre else {
        return Ok(None);
    };
    let tenant: String = pre.get("tenant");
    let accounting_on: bool = pre.get("accounting_on");
    let txn = client.transaction().await?;
    if accounting_on {
        txn.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&tenant])
            .await?;
    }
    let row = txn
        .query_opt(
            &format!("SELECT {RECORD_COLUMNS} FROM leases WHERE lease_id = $1 FOR UPDATE"),
            &[&lease_id],
        )
        .await?;
    let Some(row) = row else {
        txn.rollback().await.ok();
        return Ok(None);
    };
    let record = record_from_row(&row)?;
    if !matches!(record.state, LeaseState::Pending) {
        txn.rollback().await.ok();
        return Ok(None);
    }
    let already_claimed = txn
        .query_opt(
            "SELECT 1 FROM pending_cleanup_claims WHERE lease_id = $1",
            &[&lease_id],
        )
        .await?
        .is_some();
    let created_at_ms = i64::try_from(record.created_at_ms).map_err(|_| {
        anyhow::anyhow!(
            "lease {lease_id} created_at_ms exceeds PostgreSQL bigint range (corrupt row)"
        )
    })?;
    if !already_claimed && created_at_ms >= cutoff {
        txn.rollback().await.ok();
        return Ok(None);
    }
    if !already_claimed {
        txn.execute(
            "INSERT INTO pending_cleanup_claims (lease_id, claimed_at_ms) VALUES ($1, $2)",
            &[&lease_id, &claimed_at_ms],
        )
        .await?;
    }
    txn.commit().await?;
    Ok(Some(record))
}
