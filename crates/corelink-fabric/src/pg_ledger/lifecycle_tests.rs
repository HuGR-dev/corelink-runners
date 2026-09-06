use std::env;
use std::sync::{Arc, Barrier};

use super::{PgLedger, PgTlsMode};
use crate::LeaseLedger;
use crate::TenantSuspensionEvent;

fn event(tenant: &str, id: &str, at: u64) -> TenantSuspensionEvent {
    TenantSuspensionEvent {
        event_id: id.into(),
        tenant_id: tenant.into(),
        created_at_ms: at,
        attempts: 0,
    }
}

#[test]
fn real_pg_lifecycle_generation_is_exact_across_same_clock_resume() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let observer = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        let first = event(&tenant, "suspend-a", 7);
        ledger.record_tenant_suspension(first.clone())?;
        let pointer: String = ledger
            .pool
            .get()
            .await?
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id=$1",
                &[&tenant],
            )
            .await?
            .get(0);
        assert_eq!(ledger.tenant_lifecycle(&tenant)?.generation, 1);
        assert!(ledger.tenant_lifecycle(&tenant)?.suspended);
        assert_eq!(ledger.tenant_suspension_generation(&pointer)?, 1);
        ledger.record_tenant_suspension(first.clone())?;
        let repeated_pointer: String = ledger
            .pool
            .get()
            .await?
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id=$1",
                &[&tenant],
            )
            .await?
            .get(0);
        assert_eq!(repeated_pointer, pointer);
        assert_eq!(ledger.tenant_lifecycle(&tenant)?.generation, 1);
        ledger.set_tenant_suspended(&tenant, false)?;
        assert_eq!(ledger.tenant_lifecycle(&tenant)?.generation, 2);
        assert_eq!(observer.tenant_lifecycle(&tenant)?.generation, 2);
        ledger.set_tenant_suspended(&tenant, false)?;
        assert_eq!(ledger.tenant_lifecycle(&tenant)?.generation, 2);
        let second = event(&tenant, "suspend-b", 7);
        ledger.record_tenant_suspension(second.clone())?;
        let second_pointer: String = ledger
            .pool
            .get()
            .await?
            .query_one(
                "SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id=$1",
                &[&tenant],
            )
            .await?
            .get(0);
        assert_ne!(second_pointer, pointer);
        assert_eq!(ledger.tenant_suspension_generation(&second_pointer)?, 2);
        assert_eq!(observer.tenant_suspension_generation(&pointer)?, 1);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_legacy_event_is_generation_zero_and_missing_pointer_repairs_current(
) -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        ledger.set_tenant_suspended(&tenant, false)?;
        let db = ledger.pool.get().await?;
        db.execute("INSERT INTO tenant_suspension_events (event_id,tenant_id,created_at_ms,attempts,generation) VALUES ($1,$2,0,0,0)", &[&format!("legacy-{tenant}"), &tenant]).await?;
        assert_eq!(ledger.tenant_suspension_generation(&format!("legacy-{tenant}"))?, 0);
        ledger.set_tenant_suspended(&tenant, true)?;
        db.execute("UPDATE fabric_suspended_tenants SET suspension_event_id=NULL WHERE tenant_id=$1", &[&tenant]).await?;
        ledger.record_tenant_suspension(event(&tenant, "repair", 9))?;
        let repaired: String = ledger.pool.get().await?.query_one("SELECT suspension_event_id FROM fabric_suspended_tenants WHERE tenant_id=$1", &[&tenant]).await?.get(0);
        assert_eq!(ledger.tenant_suspension_generation(&repaired)?, 1);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_generation_overflow_refuses_resume_and_keeps_suspended() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        ledger.record_tenant_suspension(event(&tenant, "overflow", 1))?;
        let db = ledger.pool.get().await?;
        db.execute(
            "UPDATE tenant_lifecycle_generations SET generation=$2 WHERE tenant_id=$1",
            &[&tenant, &i64::MAX],
        )
        .await?;
        assert!(ledger.set_tenant_suspended(&tenant, false).is_err());
        assert!(ledger.tenant_lifecycle(&tenant)?.suspended);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_direct_legacy_suspension_resumes_at_generation_one_and_unknown_event_refuses(
) -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        let db = ledger.pool.get().await?;
        db.execute(
            "INSERT INTO fabric_suspended_tenants (tenant_id,suspension_event_id) VALUES ($1,NULL)",
            &[&tenant],
        )
        .await?;
        ledger.set_tenant_suspended(&tenant, false)?;
        let lifecycle = ledger.tenant_lifecycle(&tenant)?;
        assert_eq!(lifecycle.generation, 1);
        assert!(!lifecycle.suspended);
        assert!(ledger
            .tenant_suspension_generation("missing-event")
            .is_err());
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_fresh_persisted_suspension_reports_legacy_zero_before_resume() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        let db = ledger.pool.get().await?;
        db.execute(
            "INSERT INTO fabric_suspended_tenants (tenant_id,suspension_event_id) VALUES ($1,NULL)",
            &[&tenant],
        )
        .await?;
        let before = ledger.tenant_lifecycle(&tenant)?;
        assert_eq!(before.generation, 0);
        assert!(before.suspended);
        ledger.set_tenant_suspended(&tenant, false)?;
        let after = ledger.tenant_lifecycle(&tenant)?;
        assert_eq!(after.generation, 1);
        assert!(!after.suspended);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_two_connections_resume_once_under_same_tenant_lock() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let first = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let second = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        first.record_tenant_suspension(event(&tenant, "concurrent-suspend", 11))?;
        let a = first.clone();
        let b = second.clone();
        let tenant_a = tenant.clone();
        let tenant_b = tenant.clone();
        let barrier = Arc::new(Barrier::new(2));
        let barrier_a = Arc::clone(&barrier);
        let barrier_b = Arc::clone(&barrier);
        let (left, right) = tokio::join!(
            tokio::task::spawn_blocking(move || {
                barrier_a.wait();
                a.set_tenant_suspended(&tenant_a, false)
            }),
            tokio::task::spawn_blocking(move || {
                barrier_b.wait();
                b.set_tenant_suspended(&tenant_b, false)
            }),
        );
        left??;
        right??;
        assert_eq!(first.tenant_lifecycle(&tenant)?.generation, 2);
        assert!(!first.tenant_lifecycle(&tenant)?.suspended);
        first.set_tenant_suspended(&tenant, false)?;
        assert_eq!(second.tenant_lifecycle(&tenant)?.generation, 2);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_negative_generation_is_rejected_without_deleting_suspension(
) -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("lifecycle pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        ledger.record_tenant_suspension(event(&tenant, "negative-generation", 12))?;
        let db = ledger.pool.get().await?;
        assert!(db
            .execute(
            "UPDATE tenant_lifecycle_generations SET generation=-1 WHERE tenant_id=$1",
            &[&tenant],
        )
        .await
        .is_err());
        assert!(ledger.tenant_lifecycle(&tenant)?.suspended);
        let still_suspended: bool = db
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM fabric_suspended_tenants WHERE tenant_id=$1)",
                &[&tenant],
            )
            .await?
            .get(0);
        assert!(still_suspended);
        Ok::<_, anyhow::Error>(())
    })
}
