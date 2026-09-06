//! Transactional tenant-suspension producer for [`super::PgLedger`].
//!
//! Suspension state and its worker revoke event are one durable state machine.
//! A tenant advisory fence serializes both producer entry points, while the
//! nullable event pointer lets an upgrade repair old suspension rows safely.

use super::PgLedger;
use crate::ledger::TenantSuspensionEvent;

const FENCE_SQL: &str =
    "SELECT pg_advisory_xact_lock(hashtext('corelink:fabric:tenant-suspension:' || $1))";

fn validate_tenant(tenant: &str) -> anyhow::Result<()> {
    if tenant.trim().is_empty() {
        anyhow::bail!("tenant suspension requires a non-empty tenant id");
    }
    Ok(())
}

fn validate_event(event: &TenantSuspensionEvent) -> anyhow::Result<()> {
    validate_tenant(&event.tenant_id)?;
    if event.event_id.trim().is_empty() {
        anyhow::bail!("tenant suspension requires a non-empty event id");
    }
    if event.created_at_ms > i64::MAX as u64 {
        anyhow::bail!("tenant suspension created_at_ms exceeds PostgreSQL bigint");
    }
    if event.attempts > i32::MAX as u32 {
        anyhow::bail!("tenant suspension attempts exceeds PostgreSQL integer");
    }
    Ok(())
}

impl PgLedger {
    /// Persist one suspension epoch, including its worker outbox event.
    ///
    /// The existing row is authoritative. A valid pointer makes a repeated
    /// call a no-op, even when the caller supplies a newer clock or event id.
    /// A legacy NULL pointer is repaired in the same transaction after proving
    /// the generated event does not already exist.
    pub(crate) fn record_tenant_suspension_pg(
        &self,
        event: TenantSuspensionEvent,
    ) -> anyhow::Result<()> {
        validate_event(&event)?;
        self.block_on(async move {
            let mut client = self.pool.get().await?;
            let tx = client.transaction().await?;
            tx.query_one(FENCE_SQL, &[&event.tenant_id]).await?;

            let current = tx
                .query_opt(
                    "SELECT suspension_event_id FROM fabric_suspended_tenants \
                     WHERE tenant_id = $1 FOR UPDATE",
                    &[&event.tenant_id],
                )
                .await?;
            if let Some(row) = current {
                let pointer: Option<String> = row.get(0);
                if let Some(pointer) = pointer {
                    let outbox = tx
                        .query_opt(
                            "SELECT tenant_id FROM tenant_suspension_events \
                             WHERE event_id = $1",
                            &[&pointer],
                        )
                        .await?;
                    let Some(outbox) = outbox else {
                        anyhow::bail!(
                            "suspension row for tenant {:?} points to missing event {:?}",
                            event.tenant_id,
                            pointer
                        );
                    };
                    let outbox_tenant: String = outbox.get(0);
                    if outbox_tenant != event.tenant_id {
                        anyhow::bail!(
                            "suspension row for tenant {:?} points across tenant boundary",
                            event.tenant_id
                        );
                    }
                    tx.commit().await?;
                    return Ok(());
                }

                // Rows created before the pointer column was introduced are
                // repaired as a fresh epoch. Never reuse the caller id alone:
                // the sequence makes the persisted identity unique forever.
                let generated = next_event_id(&tx, &event.event_id).await?;
                insert_event(&tx, &generated, &event).await?;
                tx.execute(
                    "UPDATE fabric_suspended_tenants SET suspension_event_id = $2 \
                     WHERE tenant_id = $1",
                    &[&event.tenant_id, &generated],
                )
                .await?;
                tx.commit().await?;
                return Ok(());
            }

            let generated = next_event_id(&tx, &event.event_id).await?;
            insert_event(&tx, &generated, &event).await?;
            tx.execute(
                "INSERT INTO fabric_suspended_tenants (tenant_id, suspension_event_id) \
                 VALUES ($1, $2)",
                &[&event.tenant_id, &generated],
            )
            .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Set legacy suspension state under the same fence as epoch recording.
    pub(crate) fn set_tenant_suspended_pg(
        &self,
        tenant: &str,
        suspended: bool,
    ) -> anyhow::Result<()> {
        validate_tenant(tenant)?;
        self.block_on(async move {
            let mut client = self.pool.get().await?;
            let tx = client.transaction().await?;
            tx.query_one(FENCE_SQL, &[&tenant]).await?;
            if suspended {
                tx.execute(
                    "INSERT INTO fabric_suspended_tenants (tenant_id) VALUES ($1) \
                     ON CONFLICT (tenant_id) DO NOTHING",
                    &[&tenant],
                )
                .await?;
            } else {
                tx.execute(
                    "DELETE FROM fabric_suspended_tenants WHERE tenant_id = $1",
                    &[&tenant],
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }
}

async fn next_event_id(
    tx: &tokio_postgres::Transaction<'_>,
    prefix: &str,
) -> anyhow::Result<String> {
    let sequence: i64 = tx
        .query_one("SELECT nextval('fabric_tenant_suspension_event_seq')", &[])
        .await?
        .get(0);
    Ok(format!("{prefix}:epoch:{sequence}"))
}

async fn insert_event(
    tx: &tokio_postgres::Transaction<'_>,
    event_id: &str,
    event: &TenantSuspensionEvent,
) -> anyhow::Result<()> {
    tx.execute(
        "INSERT INTO tenant_suspension_events \
         (event_id, tenant_id, created_at_ms, attempts) VALUES ($1, $2, $3, $4)",
        &[
            &event_id,
            &event.tenant_id,
            &(event.created_at_ms as i64),
            &(event.attempts as i32),
        ],
    )
    .await?;
    Ok(())
}
