//! F008 producer checks against a real disposable PostgreSQL database.

use corelink_fabric::{LeaseLedger, PgLedger, PgTlsMode, TenantSuspensionEvent};
use tokio_postgres::NoTls;

fn url() -> anyhow::Result<String> {
    std::env::var("TEST_DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("TEST_DATABASE_URL is required; tests cannot run"))
}

fn tenant(label: &str) -> String {
    format!(
        "f008-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos()
    )
}

fn event(tenant_id: &str, id: &str, clock: u64) -> TenantSuspensionEvent {
    TenantSuspensionEvent {
        event_id: id.into(),
        tenant_id: tenant_id.into(),
        created_at_ms: clock,
        attempts: 0,
    }
}

async fn open() -> anyhow::Result<(PgLedger, tokio_postgres::Client)> {
    let database_url = url()?;
    let ledger = PgLedger::connect(&database_url, 4, PgTlsMode::Disable).await?;
    let (db, connection) = tokio_postgres::connect(&database_url, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok((ledger, db))
}

#[test]
fn repeat_restart_unsuspend_resuspend_same_clock_gets_new_epoch() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (ledger, db) = open().await?;
        let tenant_id = tenant("cycle");
        ledger.record_tenant_suspension(event(&tenant_id, "admin-a", 7_000))?;
        ledger.record_tenant_suspension(event(&tenant_id, "admin-b", 7_000))?;
        let first_row = db
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id = $1",
                &[&tenant_id],
            )
            .await?;
        let first: String = first_row.get(0);
        drop(ledger);
        let (ledger, db) = open().await?;
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
fn legacy_pointer_repairs_and_corrupt_pointer_rolls_back() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (ledger, db) = open().await?;
        let legacy = tenant("legacy");
        db.execute("INSERT INTO fabric_suspended_tenants (tenant_id, suspension_event_id) VALUES ($1, NULL)", &[&legacy]).await?;
        ledger.record_tenant_suspension(event(&legacy, "legacy-admin", 8_000))?;
        let pointer: String = db
            .query_one("SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id = $1", &[&legacy])
            .await?
            .get(0);
        assert!(pointer.starts_with("legacy-admin:epoch:"));

        let corrupt = tenant("rollback");
        db.execute("INSERT INTO fabric_suspended_tenants (tenant_id, suspension_event_id) VALUES ($1, 'missing-event')", &[&corrupt]).await?;
        assert!(ledger.record_tenant_suspension(event(&corrupt, "rollback-admin", 9_000)).is_err());
        let count: i64 = db
            .query_one("SELECT count(*) FROM tenant_suspension_events WHERE tenant_id = $1", &[&corrupt])
            .await?
            .get(0);
        assert_eq!(count, 0);
        Ok(())
    })
}
