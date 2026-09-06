//! Real-Postgres producer checks for F008 suspension epochs.
//!
//! These tests intentionally fail when `TEST_DATABASE_URL` is absent. They
//! never truncate shared tables; every test uses a process-unique tenant.

use corelink_fabric::{LeaseLedger, PgLedger, TenantSuspensionEvent};
use tokio_postgres::NoTls;

fn database_url() -> anyhow::Result<String> {
    std::env::var("TEST_DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("TEST_DATABASE_URL is required; suspension tests cannot run"))
}

fn tenant(label: &str) -> String {
    format!(
        "f008-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos()
    )
}

fn event(tenant_id: &str, event_id: &str, created_at_ms: u64) -> TenantSuspensionEvent {
    TenantSuspensionEvent {
        event_id: event_id.to_owned(),
        tenant_id: tenant_id.to_owned(),
        created_at_ms,
        attempts: 0,
    }
}

async fn connect() -> anyhow::Result<(PgLedger, tokio_postgres::Client)> {
    let url = database_url()?;
    let ledger = PgLedger::connect(&url, 4, corelink_fabric::PgTlsMode::Disable).await?;
    let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;
    tokio::spawn(async move { connection.await.map(|_| ()).unwrap_or(()) });
    Ok((ledger, client))
}

#[test]
fn restart_repeat_same_clock_unsuspend_resuspend() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (ledger, db) = connect().await?;
        let tenant_id = tenant("cycle");
        ledger.record_tenant_suspension(event(&tenant_id, "admin-a", 7_000))?;
        ledger.record_tenant_suspension(event(&tenant_id, "admin-b", 7_000))?;
        let first_row = db
            .query_one(
                "SELECT suspension_event_id, (SELECT count(*) FROM tenant_suspension_events WHERE tenant_id = $1) FROM fabric_suspended_tenants WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?;
        let first: String = first_row.get(0);
        let first_count: i64 = first_row.get(1);
        assert_eq!(first_count, 1);
        drop(ledger);
        let (ledger, db) = connect().await?;
        ledger.set_tenant_suspended(&tenant_id, false)?;
        ledger.record_tenant_suspension(event(&tenant_id, "admin-c", 7_000))?;
        let second: String = db
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?
            .get(0);
        assert_ne!(first, second);
        let count: i64 = db
            .query_one(
                "SELECT count(*) FROM tenant_suspension_events WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?
            .get(0);
        assert_eq!(count, 2);
        Ok(())
    })
}

#[test]
fn repairs_legacy_null_pointer_atomically() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (ledger, db) = connect().await?;
        let tenant_id = tenant("legacy");
        db.execute(
            "INSERT INTO fabric_suspended_tenants (tenant_id, suspension_event_id) VALUES ($1, NULL)",
            &[&tenant_id],
        )
        .await?;
        ledger.record_tenant_suspension(event(&tenant_id, "legacy-admin", 8_000))?;
        let row = db
            .query_one(
                "SELECT s.suspension_event_id, e.tenant_id FROM fabric_suspended_tenants s JOIN tenant_suspension_events e ON e.event_id = s.suspension_event_id WHERE s.tenant_id = $1",
                &[&tenant_id],
            )
            .await?;
        let pointer: String = row.get(0);
        let event_tenant: String = row.get(1);
        assert!(pointer.starts_with("legacy-admin:epoch:"));
        assert_eq!(event_tenant, tenant_id);
        Ok(())
    })
}

#[test]
fn corrupt_pointer_fails_closed_and_rolls_back() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (ledger, db) = connect().await?;
        let tenant_id = tenant("rollback");
        db.execute(
            "INSERT INTO fabric_suspended_tenants (tenant_id, suspension_event_id) VALUES ($1, 'missing-event')",
            &[&tenant_id],
        )
        .await?;
        assert!(ledger
            .record_tenant_suspension(event(&tenant_id, "rollback-admin", 9_000))
            .is_err());
        let pointer: String = db
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?
            .get(0);
        assert_eq!(pointer, "missing-event");
        let count: i64 = db
            .query_one(
                "SELECT count(*) FROM tenant_suspension_events WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?
            .get(0);
        assert_eq!(count, 0);
        Ok(())
    })
}
