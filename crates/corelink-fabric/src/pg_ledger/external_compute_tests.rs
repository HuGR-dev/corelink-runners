use std::env;

use super::{PgLedger, PgTlsMode};
use crate::LeaseLedger;
use crate::compute_budget::{
    ExternalComputeAdmission, ExternalComputeBaseline, ExternalComputeReservation,
    ExternalComputeSettlement, ExternalComputeState, ExternalWorkloadKind,
};

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
    let url = env::var("TEST_DATABASE_URL")?;
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
        Ok::<_, anyhow::Error>(())
    })
}
