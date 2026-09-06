use std::env;

use super::{PgLedger, PgTlsMode};
use crate::LeaseLedger;
use crate::compute_budget::{
    ExternalComputeAdmission, ExternalComputeBaseline, ExternalComputeReservation,
    ExternalComputeSettlement, ExternalComputeState, ExternalWorkloadKind,
};
use crate::ledger::{AdmitOutcome, ComputeGate, LeaseRecord, LeaseState};
use crate::tenant::TenantId;
use corelink_runners_contracts::RunnerState;

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
                    "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into(),
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
                            "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba"
                                .into(),
                    }
                )?
                .state,
            ExternalComputeState::Settled
        );
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
        assert!(ledger.settle_external_compute(&overflow, ExternalComputeSettlement { actual_vcpu_ms: u64::MAX, terminal_evidence_digest: "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into() }).is_err());
        assert_eq!(ledger.settle_external_compute(&overflow, ExternalComputeSettlement { actual_vcpu_ms: 210, terminal_evidence_digest: "fedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcbafedcba".into() })?.state, ExternalComputeState::Settled);
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
        let second = first.clone();
        let tenant = uuid::Uuid::new_v4().to_string();
        let clock = first.pool.get().await?.query_one("SELECT EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int, (EXTRACT(EPOCH FROM (clock_timestamp() AT TIME ZONE 'UTC')) * 1000)::bigint", &[]).await?;
        let period: u32 = clock.get::<_, i32>(0) as u32;
        let expiry: u64 = clock.get::<_, i64>(1) as u64 + 60_000;
        first.initialize_external_compute_period(ExternalComputeBaseline { tenant_id: tenant.clone(), period_key: period, external_vcpu_ms: 0, evidence_digest: "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".into() })?;
        let external = reservation(&tenant, &uuid::Uuid::new_v4().to_string(), period, expiry);
        let native = LeaseRecord { lease_id: format!("native-{}", uuid::Uuid::new_v4()), tenant: TenantId::new(&tenant)?, state: LeaseState::Pending, box_ref: "native-box".into(), created_at_ms: 0, updated_at_ms: 0, deadline_ms: None, billing_acquired_at_ms: None };
        let gate = ComputeGate { period_key: period, ceiling_vcpu_ms: 1_000, box_vcpu_count: 1, new_reserved_vcpu_ms: 600 };
        let (external_result, native_result) = std::thread::scope(|scope| {
            let e = scope.spawn(|| second.reserve_external_compute(external));
            let n = scope.spawn(|| first.try_admit_with_compute(native, 100, Some(gate)));
            (e.join().unwrap()?, n.join().unwrap()?)
        });
        let external_admitted = matches!(external_result, ExternalComputeAdmission::Admitted(_));
        let native_admitted = native_result == AdmitOutcome::Admitted;
        assert_eq!(external_admitted as u8 + native_admitted as u8, 1);
        Ok::<_, anyhow::Error>(())
    })
}
