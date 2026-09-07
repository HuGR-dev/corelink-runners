//! Transactional PostgreSQL suspension producer and outbox operations.

use super::PgLedger;
use crate::TenantSuspensionEvent;

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
            let generation = ensure_generation(&tx, &event.tenant_id, current.is_some()).await?;
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
                        anyhow::bail!("suspension pointer references missing outbox event");
                    };
                    let outbox_tenant: String = outbox.get(0);
                    if outbox_tenant != event.tenant_id {
                        anyhow::bail!("suspension pointer crosses tenant boundary");
                    }
                    let outbox_generation: i64 = tx
                        .query_one(
                            "SELECT generation FROM tenant_suspension_events WHERE event_id = $1",
                            &[&pointer],
                        )
                        .await?
                        .get(0);
                    if outbox_generation != generation {
                        anyhow::bail!(
                            "suspension event generation disagrees with tenant generation"
                        );
                    }
                    tx.commit().await?;
                    return Ok(());
                }
                let generated = next_event_id(&tx, &event.event_id).await?;
                insert_event(&tx, &generated, &event, generation).await?;
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
            insert_event(&tx, &generated, &event, generation).await?;
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
                let already_suspended = tx
                    .query_opt(
                        "SELECT 1 FROM fabric_suspended_tenants WHERE tenant_id=$1",
                        &[&tenant],
                    )
                    .await?
                    .is_some();
                ensure_generation(&tx, tenant, already_suspended).await?;
                tx.execute(
                    "INSERT INTO fabric_suspended_tenants (tenant_id) VALUES ($1) \
                     ON CONFLICT (tenant_id) DO NOTHING",
                    &[&tenant],
                )
                .await?;
            } else {
                let currently_suspended = tx
                    .query_opt(
                        "SELECT 1 FROM fabric_suspended_tenants WHERE tenant_id = $1",
                        &[&tenant],
                    )
                    .await?
                    .is_some();
                let generation = ensure_generation(&tx, tenant, currently_suspended).await?;
                if currently_suspended {
                    let next = generation
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("tenant lifecycle generation overflow"))?;
                    tx.execute(
                        "UPDATE tenant_lifecycle_generations SET generation=$2 WHERE tenant_id=$1",
                        &[&tenant, &next],
                    )
                    .await?;
                }
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

    pub(crate) fn pending_tenant_suspension_events_pg(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<TenantSuspensionEvent>> {
        self.block_on(async move {
            let client = self.pool.get().await?;
            let rows = client
                .query(
                    "SELECT event_id, tenant_id, created_at_ms, attempts \
                     FROM tenant_suspension_events WHERE delivered_at_ms IS NULL \
                     ORDER BY created_at_ms LIMIT $1",
                    &[&(limit.min(1000) as i64)],
                )
                .await?;
            rows.into_iter()
                .map(|row| {
                    Ok(TenantSuspensionEvent {
                        event_id: row.get(0),
                        tenant_id: row.get(1),
                        created_at_ms: row.get::<_, i64>(2) as u64,
                        attempts: row.get::<_, i32>(3).max(0) as u32,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn mark_tenant_suspension_event_delivered_pg(
        &self,
        event_id: &str,
    ) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client
                .execute(
                    "UPDATE tenant_suspension_events SET delivered_at_ms = \
                     (extract(epoch from clock_timestamp()) * 1000)::bigint \
                     WHERE event_id = $1 AND delivered_at_ms IS NULL",
                    &[&event_id],
                )
                .await?;
            Ok(())
        })
    }

    pub(crate) fn mark_tenant_suspension_event_attempt_pg(
        &self,
        event_id: &str,
    ) -> anyhow::Result<()> {
        self.block_on(async {
            let client = self.pool.get().await?;
            client
                .execute(
                    "UPDATE tenant_suspension_events SET attempts = attempts + 1 \
                     WHERE event_id = $1 AND delivered_at_ms IS NULL",
                    &[&event_id],
                )
                .await?;
            Ok(())
        })
    }
}

impl PgLedger {
    pub(crate) fn tenant_lifecycle_pg(
        &self,
        tenant: &str,
    ) -> anyhow::Result<crate::ledger::TenantLifecycle> {
        validate_tenant(tenant)?;
        self.block_on(async {
            let mut client = self.pool.get().await?;
            let tx = client.transaction().await?;
            tx.query_one(FENCE_SQL, &[&tenant]).await?;
            let suspended = tx
                .query_opt(
                    "SELECT 1 FROM fabric_suspended_tenants WHERE tenant_id=$1",
                    &[&tenant],
                )
                .await?
                .is_some();
            let generation = ensure_generation(&tx, tenant, suspended).await? as u64;
            tx.commit().await?;
            Ok(crate::ledger::TenantLifecycle {
                tenant_id: tenant.to_string(),
                generation,
                suspended,
            })
        })
    }

    pub(crate) fn tenant_suspension_generation_pg(&self, event_id: &str) -> anyhow::Result<u64> {
        if event_id.trim().is_empty() {
            anyhow::bail!("suspension event id is empty");
        }
        self.block_on(async {
            let client = self.pool.get().await?;
            let row = client
                .query_opt(
                    "SELECT generation FROM tenant_suspension_events WHERE event_id=$1",
                    &[&event_id],
                )
                .await?
                .ok_or_else(|| anyhow::anyhow!("unknown suspension event"))?;
            let generation: i64 = row.get(0);
            u64::try_from(generation).map_err(|_| anyhow::anyhow!("invalid suspension generation"))
        })
    }
}

async fn ensure_generation(
    tx: &tokio_postgres::Transaction<'_>,
    tenant: &str,
    suspended: bool,
) -> anyhow::Result<i64> {
    if let Some(row) = tx
        .query_opt(
            "SELECT generation FROM tenant_lifecycle_generations WHERE tenant_id=$1 FOR UPDATE",
            &[&tenant],
        )
        .await?
    {
        let generation: i64 = row.get(0);
        if generation < 0 {
            anyhow::bail!("negative tenant lifecycle generation");
        }
        return Ok(generation);
    }
    let generation = if suspended { 0 } else { 1 };
    tx.execute(
        "INSERT INTO tenant_lifecycle_generations (tenant_id,generation) VALUES ($1,$2)",
        &[&tenant, &generation],
    )
    .await?;
    Ok(generation)
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
    generation: i64,
) -> anyhow::Result<()> {
    tx.execute(
        "INSERT INTO tenant_suspension_events \
         (event_id, tenant_id, created_at_ms, attempts, generation) VALUES ($1, $2, $3, $4, $5)",
        &[
            &event_id,
            &event.tenant_id,
            &(event.created_at_ms as i64),
            &(event.attempts as i32),
            &generation,
        ],
    )
    .await?;
    Ok(())
}
