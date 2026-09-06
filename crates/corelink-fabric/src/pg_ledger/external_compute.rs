use super::PgLedger;
use crate::compute_budget::{
    ExternalComputeAdmission, ExternalComputeBaseline, ExternalComputeError,
    ExternalComputeReceipt, ExternalComputeReservation, ExternalComputeSettlement,
    ExternalComputeState, ExternalWorkloadKind,
};
use crate::compute_meter;
use crate::ledger::LeaseLedger;

fn invalid() -> anyhow::Error {
    anyhow::Error::new(ExternalComputeError::InvalidInput)
}
fn conflict() -> anyhow::Error {
    anyhow::Error::new(ExternalComputeError::Conflict)
}
fn checked_i64(value: u64) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| invalid())
}
fn checked_amount_text(value: &str) -> anyhow::Result<i64> {
    let amount = value
        .parse::<u128>()
        .map_err(|_| anyhow::anyhow!("invalid compute sum"))?;
    i64::try_from(amount).map_err(|_| invalid())
}
fn digest(value: &str) -> anyhow::Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(())
}
fn tenant(value: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| invalid())?;
    if id.is_nil() {
        return Err(invalid());
    }
    Ok(())
}
fn period(value: u32) -> anyhow::Result<()> {
    let month = value % 100;
    if (197001..=999912).contains(&value) && (1..=12).contains(&month) {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn workload(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":_./-".contains(&b))
    {
        return Err(invalid());
    }
    Ok(())
}
fn kind(value: &ExternalWorkloadKind) -> &'static str {
    match value {
        ExternalWorkloadKind::SpawnWorkerRunner => "spawn_worker_runner",
        ExternalWorkloadKind::Devenv => "devenv",
    }
}
fn validate(r: &ExternalComputeReservation) -> anyhow::Result<i64> {
    tenant(&r.tenant_id)?;
    period(r.period_key)?;
    workload(&r.workload_id)?;
    digest(&r.grant_digest)?;
    let id = uuid::Uuid::parse_str(&r.reservation_id).map_err(|_| invalid())?;
    if id.is_nil() {
        return Err(invalid());
    }
    if r.ceiling_vcpu_ms == 0
        || r.vcpu_count == 0
        || r.vcpu_count > 16
        || r.maximum_wall_ms == 0
        || r.maximum_wall_ms > 28_800_000
        || r.grant_expires_at_ms == 0
    {
        return Err(invalid());
    }
    checked_i64(r.ceiling_vcpu_ms)?;
    checked_i64(r.grant_expires_at_ms)?;
    let reserved = u64::from(r.vcpu_count)
        .checked_mul(r.maximum_wall_ms)
        .ok_or_else(invalid)?;
    if reserved == 0 || !compute_meter::fits_ledger(reserved) {
        return Err(invalid());
    }
    r.grant_expires_at_ms
        .checked_add(r.maximum_wall_ms)
        .ok_or_else(invalid)?;
    let _ = id;
    checked_i64(reserved)
}
fn receipt(r: &str, state: ExternalComputeState) -> ExternalComputeReceipt {
    ExternalComputeReceipt {
        reservation_id: r.to_string(),
        state,
    }
}
fn now_sql() -> &'static str {
    "(EXTRACT(EPOCH FROM (clock_timestamp() AT TIME ZONE 'UTC')) * 1000)::bigint"
}

pub(super) fn initialize(ledger: &PgLedger, b: ExternalComputeBaseline) -> anyhow::Result<()> {
    tenant(&b.tenant_id)?;
    period(b.period_key)?;
    digest(&b.evidence_digest)?;
    let external = checked_i64(b.external_vcpu_ms)?;
    ledger.block_on(async {
        let mut client = ledger.pool.get().await?; let tx = client.transaction().await?;
        tx.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&b.tenant_id]).await?;
        if let Some(row) = tx.query_opt("SELECT external_vcpu_ms,evidence_digest FROM external_compute_periods WHERE tenant=$1 AND period_key=$2", &[&b.tenant_id, &(b.period_key as i32)]).await? {
            if row.get::<_,i64>(0) == external && row.get::<_,String>(1) == b.evidence_digest { tx.commit().await?; return Ok(()); }
            tx.rollback().await.ok(); return Err(conflict());
        }
        let native: i64 = tx.query_one("SELECT COALESCE(accrued_vcpu_ms,0) FROM compute_accrual WHERE tenant=$1 AND period_key=$2", &[&b.tenant_id, &(b.period_key as i32)]).await?.get(0);
        let total = native.checked_add(external).ok_or_else(invalid)?;
        tx.execute("INSERT INTO external_compute_periods (tenant,period_key,external_vcpu_ms,evidence_digest) VALUES ($1,$2,$3,$4)", &[&b.tenant_id, &(b.period_key as i32), &external, &b.evidence_digest]).await?;
        tx.execute("INSERT INTO compute_accrual (tenant,period_key,accrued_vcpu_ms) VALUES ($1,$2,$3) ON CONFLICT (tenant,period_key) DO UPDATE SET accrued_vcpu_ms=$3", &[&b.tenant_id, &(b.period_key as i32), &total]).await?;
        tx.commit().await?; Ok(())
    })
}

pub(super) fn reserve(
    ledger: &PgLedger,
    r: ExternalComputeReservation,
) -> anyhow::Result<ExternalComputeAdmission> {
    let reserved = validate(&r)?;
    let reservation_id = r.reservation_id.clone();
    ledger.block_on(async {
        let _permit = ledger
            .admit_permits
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("admit semaphore closed: {e}"))?;
        let mut client = ledger.pool.get().await?; let tx = client.transaction().await?;
        tx.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&r.tenant_id]).await?;
        let clock = tx.query_one(&format!("SELECT {}, EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int, (EXTRACT(EPOCH FROM ((to_date($1::int::text, 'YYYYMM') + interval '1 month') AT TIME ZONE 'UTC')) * 1000)::bigint", now_sql()), &[&(r.period_key as i32)]).await?;
        let now: i64 = clock.get(0);
        let current_period: i32 = clock.get(1);
        let period_end: i64 = clock.get(2);
        if current_period != r.period_key as i32 || checked_i64(r.grant_expires_at_ms.checked_add(r.maximum_wall_ms).ok_or_else(invalid)?)? > period_end { tx.rollback().await.ok(); return Err(invalid()); }
        if checked_i64(r.grant_expires_at_ms)? <= now { tx.rollback().await.ok(); return Err(conflict()); }
        if let Some(row) = tx.query_opt("SELECT tenant,workload_kind,workload_id,period_key,ceiling_vcpu_ms,vcpu_count,maximum_wall_ms,grant_expires_at_ms,grant_digest,state,reserved_vcpu_ms FROM external_compute_reservations WHERE reservation_id=$1::text::uuid", &[&reservation_id]).await? {
            let same = row.get::<_,String>(0)==r.tenant_id && row.get::<_,String>(1)==kind(&r.workload_kind) && row.get::<_,String>(2)==r.workload_id && row.get::<_,i32>(3)==r.period_key as i32 && row.get::<_,i64>(4)==r.ceiling_vcpu_ms as i64 && row.get::<_,i32>(5)==r.vcpu_count as i32 && row.get::<_,i64>(6)==r.maximum_wall_ms as i64 && row.get::<_,i64>(7)==r.grant_expires_at_ms as i64 && row.get::<_,String>(8)==r.grant_digest;
            if !same { tx.rollback().await.ok(); return Err(conflict()); }
            let state: String = row.get(9); let result = match state.as_str() { "prepared" => Ok(ExternalComputeAdmission::Admitted(receipt(&r.reservation_id, ExternalComputeState::Prepared))), "active" => Ok(ExternalComputeAdmission::Admitted(receipt(&r.reservation_id, ExternalComputeState::Active))), _ => Err(conflict()) };
            tx.rollback().await.ok(); return result;
        }
        let baseline = tx.query_opt("SELECT 1 FROM external_compute_periods WHERE tenant=$1 AND period_key=$2", &[&r.tenant_id, &(r.period_key as i32)]).await?;
        if baseline.is_none() { tx.rollback().await.ok(); return Ok(ExternalComputeAdmission::BaselineRequired); }
        let row = tx.query_one("SELECT (COALESCE((SELECT accrued_vcpu_ms FROM compute_accrual WHERE tenant=$1 AND period_key=$2),0) + COALESCE((SELECT SUM(reserved_vcpu_ms) FROM leases WHERE tenant=$1 AND state IN ('pending','held') AND accrual_period_key=$2),0) + COALESCE((SELECT SUM(reserved_vcpu_ms) FROM external_compute_reservations WHERE tenant=$1 AND period_key=$2 AND state IN ('prepared','active')),0))::text", &[&r.tenant_id, &(r.period_key as i32)]).await?;
        let used = checked_amount_text(&row.get::<_, String>(0))?;
        if used.checked_add(reserved).ok_or_else(invalid)? > checked_i64(r.ceiling_vcpu_ms)? { tx.rollback().await.ok(); return Ok(ExternalComputeAdmission::OverCompute); }
        tx.execute("INSERT INTO external_compute_reservations (reservation_id,tenant,workload_kind,workload_id,period_key,ceiling_vcpu_ms,vcpu_count,maximum_wall_ms,grant_expires_at_ms,grant_digest,state,reserved_vcpu_ms) VALUES ($1::text::uuid,$2,$3,$4,$5,$6,$7,$8,$9,$10,'prepared',$11)", &[&reservation_id,&r.tenant_id,&kind(&r.workload_kind),&r.workload_id,&(r.period_key as i32),&checked_i64(r.ceiling_vcpu_ms)?,&(r.vcpu_count as i32),&checked_i64(r.maximum_wall_ms)?,&checked_i64(r.grant_expires_at_ms)?,&r.grant_digest,&reserved]).await?;
        tx.commit().await?; Ok(ExternalComputeAdmission::Admitted(receipt(&r.reservation_id, ExternalComputeState::Prepared)))
    })
}

async fn transition(
    ledger: &PgLedger,
    r: &ExternalComputeReservation,
    target: &str,
    allowed: &str,
) -> anyhow::Result<ExternalComputeReceipt> {
    validate(r)?;
    let id = r.reservation_id.clone();
    let mut client = ledger.pool.get().await?;
    let tx = client.transaction().await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtext($1))",
        &[&r.tenant_id],
    )
    .await?;
    let row = tx.query_opt("SELECT state,tenant,workload_kind,workload_id,period_key,ceiling_vcpu_ms,vcpu_count,maximum_wall_ms,grant_expires_at_ms,grant_digest FROM external_compute_reservations WHERE reservation_id=$1::text::uuid", &[&id]).await?.ok_or_else(conflict)?;
    let same = row.get::<_, String>(1) == r.tenant_id
        && row.get::<_, String>(2) == kind(&r.workload_kind)
        && row.get::<_, String>(3) == r.workload_id
        && row.get::<_, i32>(4) == r.period_key as i32
        && row.get::<_, i64>(5) == r.ceiling_vcpu_ms as i64
        && row.get::<_, i32>(6) == r.vcpu_count as i32
        && row.get::<_, i64>(7) == r.maximum_wall_ms as i64
        && row.get::<_, i64>(8) == r.grant_expires_at_ms as i64
        && row.get::<_, String>(9) == r.grant_digest;
    if !same {
        tx.rollback().await.ok();
        return Err(conflict());
    }
    let state: String = row.get(0);
    if state == target {
        tx.rollback().await.ok();
        return Ok(receipt(
            &r.reservation_id,
            match target {
                "active" => ExternalComputeState::Active,
                "cancelled" => ExternalComputeState::Cancelled,
                _ => ExternalComputeState::Prepared,
            },
        ));
    }
    if state != allowed {
        tx.rollback().await.ok();
        return Err(conflict());
    }
    if target == "active" {
        let clock = tx.query_one(&format!("SELECT {}, EXTRACT(YEAR FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int * 100 + EXTRACT(MONTH FROM (clock_timestamp() AT TIME ZONE 'UTC'))::int", now_sql()), &[]).await?;
        if clock.get::<_, i64>(0) >= checked_i64(r.grant_expires_at_ms)?
            || clock.get::<_, i32>(1) != r.period_key as i32
        {
            tx.rollback().await.ok();
            return Err(conflict());
        }
    }
    tx.execute(
        "UPDATE external_compute_reservations SET state=$2 WHERE reservation_id=$1::text::uuid AND state=$3",
        &[&id, &target, &allowed],
    )
    .await?;
    tx.commit().await?;
    Ok(receipt(
        &r.reservation_id,
        if target == "active" {
            ExternalComputeState::Active
        } else {
            ExternalComputeState::Cancelled
        },
    ))
}
pub(super) fn activate(
    l: &PgLedger,
    r: &ExternalComputeReservation,
) -> anyhow::Result<ExternalComputeReceipt> {
    l.block_on(transition(l, r, "active", "prepared"))
}
pub(super) fn cancel(
    l: &PgLedger,
    r: &ExternalComputeReservation,
) -> anyhow::Result<ExternalComputeReceipt> {
    l.block_on(transition(l, r, "cancelled", "prepared"))
}

pub(super) fn settle(
    l: &PgLedger,
    r: &ExternalComputeReservation,
    s: ExternalComputeSettlement,
) -> anyhow::Result<ExternalComputeReceipt> {
    validate(r)?;
    digest(&s.terminal_evidence_digest)?;
    let actual = checked_i64(s.actual_vcpu_ms)?;
    let id = r.reservation_id.clone();
    l.block_on(async {
        let mut client = l.pool.get().await?;
        let tx = client.transaction().await?;
        tx.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&r.tenant_id])
            .await?;
        let row = tx
            .query_opt("SELECT state,tenant,workload_kind,workload_id,period_key,ceiling_vcpu_ms,vcpu_count,maximum_wall_ms,grant_expires_at_ms,grant_digest,actual_vcpu_ms,terminal_evidence_digest FROM external_compute_reservations WHERE reservation_id=$1::text::uuid", &[&id])
            .await?
            .ok_or_else(conflict)?;
        let same = row.get::<_, String>(1) == r.tenant_id
            && row.get::<_, String>(2) == kind(&r.workload_kind)
            && row.get::<_, String>(3) == r.workload_id
            && row.get::<_, i32>(4) == r.period_key as i32
            && row.get::<_, i64>(5) == r.ceiling_vcpu_ms as i64
            && row.get::<_, i32>(6) == r.vcpu_count as i32
            && row.get::<_, i64>(7) == r.maximum_wall_ms as i64
            && row.get::<_, i64>(8) == r.grant_expires_at_ms as i64
            && row.get::<_, String>(9) == r.grant_digest;
        if !same { tx.rollback().await.ok(); return Err(conflict()); }
        let state: String = row.get(0);
        if state == "settled" {
            if row.get::<_, Option<i64>>(10) != Some(actual)
                || row.get::<_, Option<String>>(11).as_deref() != Some(s.terminal_evidence_digest.as_str())
            { tx.rollback().await.ok(); return Err(conflict()); }
            tx.rollback().await.ok();
            return Ok(receipt(&r.reservation_id, ExternalComputeState::Settled));
        }
        if state != "active" { tx.rollback().await.ok(); return Err(conflict()); }
        let accrued: i64 = tx.query_one("SELECT COALESCE((SELECT accrued_vcpu_ms FROM compute_accrual WHERE tenant=$1 AND period_key=$2),0)", &[&r.tenant_id, &(r.period_key as i32)]).await?.get(0);
        let total = accrued.checked_add(actual).ok_or_else(invalid)?;
        tx.execute("UPDATE external_compute_reservations SET state='settled',actual_vcpu_ms=$2,terminal_evidence_digest=$3 WHERE reservation_id=$1::text::uuid", &[&id, &actual, &s.terminal_evidence_digest]).await?;
        tx.execute("INSERT INTO compute_accrual (tenant,period_key,accrued_vcpu_ms) VALUES ($1,$2,$3) ON CONFLICT (tenant,period_key) DO UPDATE SET accrued_vcpu_ms=$3", &[&r.tenant_id, &(r.period_key as i32), &total]).await?;
        tx.commit().await?;
        Ok(receipt(&r.reservation_id, ExternalComputeState::Settled))
    })
}
