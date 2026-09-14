use crate::ledger::{LeaseRecord, LeaseState, provider_binding::validate_provider_ref};

use super::{PgLedger, pending_cleanup_pg, record_from_row};

pub(crate) fn bind_provider_ref(
    ledger: &PgLedger,
    lease_id: &str,
    provider_ref: &str,
) -> anyhow::Result<LeaseRecord> {
    validate_provider_ref(provider_ref)?;
    ledger.block_on(async {
        let mut client = ledger.pool.get().await?;
        let pre = client
            .query_opt(
                "SELECT tenant, box_vcpu_count IS NOT NULL AS accounting_on \
                 FROM leases WHERE lease_id = $1",
                &[&lease_id],
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("unknown lease {lease_id}"))?;
        let tenant: String = pre.get("tenant");
        let accounting_on: bool = pre.get("accounting_on");
        let txn = client.transaction().await?;
        if accounting_on {
            txn.query_one("SELECT pg_advisory_xact_lock(hashtext($1))", &[&tenant])
                .await?;
        }
        if !pending_cleanup_pg::lock_unclaimed_lease(&txn, lease_id).await? {
            anyhow::bail!("lease {lease_id} is unknown or cleanup-claimed");
        }
        let row = txn
            .query_one(
                "SELECT lease_id, tenant, state::text AS state, box_ref, \
                        created_at_ms, updated_at_ms, deadline_ms, \
                        billing_acquired_at_ms \
                 FROM leases WHERE lease_id = $1",
                &[&lease_id],
            )
            .await?;
        let current = record_from_row(&row)?;
        if !matches!(current.state, LeaseState::Pending) {
            anyhow::bail!("lease {lease_id} is not Pending");
        }
        if current.box_ref == provider_ref {
            txn.commit().await?;
            return Ok(current);
        }
        let marker = format!("box:{lease_id}");
        if current.box_ref != marker {
            anyhow::bail!("lease {lease_id} already has a different provider reference");
        }
        let updated = txn
            .query_one(
                "UPDATE leases SET box_ref = $2 \
                 WHERE lease_id = $1 AND state = 'pending'::lease_state AND box_ref = $3 \
                 RETURNING lease_id, tenant, state::text AS state, box_ref, \
                           created_at_ms, updated_at_ms, deadline_ms, \
                           billing_acquired_at_ms",
                &[&lease_id, &provider_ref, &marker],
            )
            .await?;
        let result = record_from_row(&updated)?;
        txn.commit().await?;
        Ok(result)
    })
}
