use std::env;

use super::{PgLedger, PgTlsMode};
use crate::LeaseLedger;
use crate::compute_budget::{
    ExternalComputeAdmission, ExternalComputeBaseline, ExternalComputeReservation,
    ExternalComputeSettlement, ExternalComputeState, ExternalWorkloadKind,
};
use crate::ledger::{AdmitOutcome, ComputeGate, LeaseRecord, LeaseState};
use crate::tenant::TenantId;

fn reservation(
    tenant: &str,
    id: &str,
    period_key: u32,
    expires_at_ms: u64,
) -> ExternalComputeReservation {
    ExternalComputeReservation {
        reservation_id: id.to_string(),
        tenant_id: tenant.to_string(),
        workload_kind: ExternalWorkloadKind::Devenv,
        workload_id: format!("budget:{id}"),
        period_key,
        ceiling_vcpu_ms: 1_000,
        vcpu_count: 1,
        maximum_wall_ms: 100,
        grant_expires_at_ms: expires_at_ms,
        grant_digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
    }
}

#[test]
fn real_pg_external_budget_lifecycle_is_idempotent_and_records_overrun() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("external compute pg tests: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let ledger = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        let clock = ledger
            .pool
            .get()
            .await?
            .query_one("SELECT EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int, (EXTRACT(EPOCH FROM (clock_timestamp() AT TIME ZONE 'UTC')) * 1000)::bigint", &[])
            .await?;
        let period_key: u32 = clock.get::<_, i32>(0) as u32;
        let expires_at_ms: u64 = clock.get::<_, i64>(1) as u64 + 60_000;
        ledger.initialize_external_compute_period(ExternalComputeBaseline {
            tenant_id: tenant.clone(),
            period_key,
            external_vcpu_ms: 10,
            evidence_digest: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
                .into(),
        })?;
        ledger.initialize_external_compute_period(ExternalComputeBaseline {
            tenant_id: tenant.clone(), period_key, external_vcpu_ms: 10,
            evidence_digest: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
        })?;
        assert!(ledger.initialize_external_compute_period(ExternalComputeBaseline {
            tenant_id: tenant.clone(), period_key, external_vcpu_ms: 11,
            evidence_digest: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into(),
        }).unwrap_err().downcast_ref::<crate::compute_budget::ExternalComputeError>().is_some());
        let r = reservation(&tenant, &uuid::Uuid::new_v4().to_string(), period_key, expires_at_ms);
        assert!(matches!(
            ledger.reserve_external_compute(r.clone())?,
            ExternalComputeAdmission::Admitted(_)
        ));
        assert_eq!(
            ledger.activate_external_compute(&r)?.state,
            ExternalComputeState::Active
        );
        assert_eq!(
            ledger.activate_external_compute(&r)?.state,
            ExternalComputeState::Active
        );
        let settled = ledger.settle_external_compute(
            &r,
            ExternalComputeSettlement {
                actual_vcpu_ms: 200,
                terminal_evidence_digest:
                    "f".repeat(64),
            },
        )?;
        assert_eq!(settled.state, ExternalComputeState::Settled);
        assert_eq!(
            ledger
                .settle_external_compute(
                    &r,
                    ExternalComputeSettlement {
                        actual_vcpu_ms: 200,
                        terminal_evidence_digest:
                            "f".repeat(64),
                    }
                )?
                .state,
            ExternalComputeState::Settled
        );
        let accrued: i64 = ledger.pool.get().await?.query_one(
            "SELECT accrued_vcpu_ms FROM compute_accrual WHERE tenant=$1 AND period_key=$2",
            &[&tenant, &(period_key as i32)]).await?.get(0);
        assert_eq!(accrued, 210, "baseline and actual overrun accrue exactly once");
        assert!(ledger.settle_external_compute(&r, ExternalComputeSettlement {
            actual_vcpu_ms: 200, terminal_evidence_digest: "e".repeat(64),
        }).is_err(), "a different terminal proof cannot rewrite settled usage");
        let missing = reservation(&uuid::Uuid::new_v4().to_string(), &uuid::Uuid::new_v4().to_string(), period_key, expires_at_ms);
        assert!(matches!(ledger.reserve_external_compute(missing)?, ExternalComputeAdmission::BaselineRequired));

        let cancel_id = uuid::Uuid::new_v4().to_string();
        let cancel = reservation(&tenant, &cancel_id, period_key, expires_at_ms);
        assert!(matches!(ledger.reserve_external_compute(cancel.clone())?, ExternalComputeAdmission::Admitted(_)));
        assert_eq!(ledger.cancel_external_compute(&cancel)?.state, ExternalComputeState::Cancelled);
        assert_eq!(ledger.cancel_external_compute(&cancel)?.state, ExternalComputeState::Cancelled);
        assert!(ledger.activate_external_compute(&cancel).is_err());

        let active_cancel = reservation(&tenant, &uuid::Uuid::new_v4().to_string(), period_key, expires_at_ms);
        ledger.reserve_external_compute(active_cancel.clone())?;
        ledger.activate_external_compute(&active_cancel)?;
        assert!(ledger.cancel_external_compute(&active_cancel).is_err());

        let overflow = reservation(&tenant, &uuid::Uuid::new_v4().to_string(), period_key, expires_at_ms);
        ledger.reserve_external_compute(overflow.clone())?;
        ledger.activate_external_compute(&overflow)?;
        assert!(ledger.settle_external_compute(&overflow, ExternalComputeSettlement { actual_vcpu_ms: i64::MAX as u64, terminal_evidence_digest: "f".repeat(64) }).is_err());
        assert_eq!(ledger.settle_external_compute(&overflow, ExternalComputeSettlement { actual_vcpu_ms: 210, terminal_evidence_digest: "f".repeat(64) })?.state, ExternalComputeState::Settled);
        let actual: i64 = ledger.pool.get().await?.query_one("SELECT actual_vcpu_ms FROM external_compute_reservations WHERE reservation_id=$1::text::uuid", &[&overflow.reservation_id]).await?.get(0);
        assert_eq!(actual, 210);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_native_and_external_reservations_share_the_budget_lock() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let first = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let second = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let tenant = uuid::Uuid::new_v4().to_string();
        let clock = first.pool.get().await?.query_one("SELECT EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int, (EXTRACT(EPOCH FROM (clock_timestamp() AT TIME ZONE 'UTC')) * 1000)::bigint", &[]).await?;
        let period: u32 = clock.get::<_, i32>(0) as u32;
        let expiry: u64 = clock.get::<_, i64>(1) as u64 + 60_000;
        first.initialize_external_compute_period(ExternalComputeBaseline { tenant_id: tenant.clone(), period_key: period, external_vcpu_ms: 0, evidence_digest: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into() })?;
        let mut external = reservation(&tenant, &uuid::Uuid::new_v4().to_string(), period, expiry);
        external.maximum_wall_ms = 600;
        let barrier = std::sync::Barrier::new(2);
        let native = LeaseRecord { lease_id: format!("native-{}", uuid::Uuid::new_v4()), tenant: TenantId::new(&tenant)?, state: LeaseState::Pending, box_ref: "native-box".into(), created_at_ms: 0, updated_at_ms: 0, deadline_ms: None, billing_acquired_at_ms: None };
        let gate = ComputeGate { period_key: period, ceiling_vcpu_ms: 1_000, box_vcpu_count: 1, new_reserved_vcpu_ms: 600 };
        let (external_result, native_result) = std::thread::scope(|scope| {
            let e = scope.spawn(|| { barrier.wait(); second.reserve_external_compute(external) });
            let n = scope.spawn(|| { barrier.wait(); first.try_admit_with_compute(native, 100, Some(gate)) });
            Ok::<_, anyhow::Error>((e.join().unwrap()?, n.join().unwrap()?))
        })?;
        let external_admitted = matches!(external_result, ExternalComputeAdmission::Admitted(_));
        let native_admitted = native_result == AdmitOutcome::Admitted;
        assert_eq!(external_admitted as u8 + native_admitted as u8, 1);
        Ok::<_, anyhow::Error>(())
    })
}

#[test]
fn real_pg_same_reservation_uuid_is_cross_tenant_collision_safe() -> anyhow::Result<()> {
    let Some(url) = env::var("TEST_DATABASE_URL").ok() else {
        eprintln!("external compute collision test: TEST_DATABASE_URL unset — skipping");
        return Ok(());
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let first = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let second = PgLedger::connect(&url, 4, PgTlsMode::Disable).await?;
        let clock = first.pool.get().await?.query_one(
            "SELECT EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int, (EXTRACT(EPOCH FROM (clock_timestamp() AT TIME ZONE 'UTC')) * 1000)::bigint", &[]).await?;
        let period = clock.get::<_, i32>(0) as u32;
        let expiry = clock.get::<_, i64>(1) as u64 + 60_000;
        let tenant_a = uuid::Uuid::new_v4().to_string();
        let tenant_b = uuid::Uuid::new_v4().to_string();
        let digest = "a".repeat(64);
        for tenant in [&tenant_a, &tenant_b] {
            first.initialize_external_compute_period(ExternalComputeBaseline {
                tenant_id: tenant.clone(), period_key: period, external_vcpu_ms: 0,
                evidence_digest: digest.clone(),
            })?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let a = reservation(&tenant_a, &id, period, expiry);
        let b = reservation(&tenant_b, &id, period, expiry);
        let a_for_thread = a.clone();
        let b_for_thread = b.clone();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let (a_result, b_result) = std::thread::scope(|scope| {
            let gate_a = barrier.clone();
            let gate_b = barrier.clone();
            let left = scope.spawn(|| { gate_a.wait(); first.reserve_external_compute(a_for_thread) });
            let right = scope.spawn(|| { gate_b.wait(); second.reserve_external_compute(b_for_thread) });
            (left.join().unwrap(), right.join().unwrap())
        });
        let a_won = matches!(a_result, Ok(ExternalComputeAdmission::Admitted(_)));
        let b_won = matches!(b_result, Ok(ExternalComputeAdmission::Admitted(_)));
        assert_eq!(a_won as u8 + b_won as u8, 1);
        let loser = if a_won { b_result } else { a_result };
        assert!(loser.unwrap_err().downcast_ref::<crate::compute_budget::ExternalComputeError>() == Some(&crate::compute_budget::ExternalComputeError::Conflict));
        let count_a: i64 = first.pool.get().await?.query_one("SELECT count(*) FROM external_compute_reservations WHERE tenant=$1", &[&tenant_a]).await?.get(0);
        let count_b: i64 = first.pool.get().await?.query_one("SELECT count(*) FROM external_compute_reservations WHERE tenant=$1", &[&tenant_b]).await?.get(0);
        assert_eq!(count_a + count_b, 1);

        let winner = if a_won { a } else { b };
        let mut divergent = if a_won { b } else { a };
        divergent.tenant_id = winner.tenant_id.clone();
        divergent.grant_digest = "b".repeat(64);
        let error = first.reserve_external_compute(divergent.clone()).unwrap_err();
        assert!(error.downcast_ref::<crate::compute_budget::ExternalComputeError>() == Some(&crate::compute_budget::ExternalComputeError::Conflict));
        assert!(first.activate_external_compute(&divergent).is_err());
        assert!(first.settle_external_compute(&divergent, ExternalComputeSettlement { actual_vcpu_ms: 1, terminal_evidence_digest: "c".repeat(64) }).is_err());
        assert_eq!(winner.reservation_id, id);
        Ok::<_, anyhow::Error>(())
    })
}
