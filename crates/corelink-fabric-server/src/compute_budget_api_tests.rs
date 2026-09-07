use super::*;
use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use base64::Engine as _;
use corelink_fabric::TenantId;
use corelink_fabric::compute_budget::{
    ExternalComputeAdmission, ExternalComputeReceipt, ExternalComputeReservation,
    ExternalComputeSettlement, ExternalComputeState,
};
use corelink_fabric::ledger::{LeaseLedger, LeaseRecord};
use corelink_runners_contracts::RunnerState;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::util::ServiceExt;

#[derive(Clone, Copy)]
enum Outcome {
    Over,
    Conflict,
    Receipt,
    Unavailable,
}

struct TestLedger {
    calls: AtomicUsize,
    outcome: Outcome,
    state: std::sync::Mutex<ExternalComputeState>,
    settle_calls: AtomicUsize,
}
impl TestLedger {
    fn new(outcome: Outcome) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            outcome,
            state: std::sync::Mutex::new(ExternalComputeState::Prepared),
            settle_calls: AtomicUsize::new(0),
        }
    }
}

impl LeaseLedger for TestLedger {
    fn reserve_external_compute(
        &self,
        _: corelink_fabric::compute_budget::ExternalComputeReservation,
    ) -> anyhow::Result<ExternalComputeAdmission> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            Outcome::Over => Ok(ExternalComputeAdmission::OverCompute),
            Outcome::Conflict => Err(anyhow::Error::new(
                corelink_fabric::compute_budget::ExternalComputeError::Conflict,
            )),
            Outcome::Unavailable => Err(anyhow::anyhow!("ledger unavailable")),
            Outcome::Receipt => Ok(ExternalComputeAdmission::Admitted(ExternalComputeReceipt {
                reservation_id: "22222222-2222-4222-8222-222222222222".into(),
                state: ExternalComputeState::Prepared,
            })),
        }
    }
    fn initialize_external_compute_period(
        &self,
        _: corelink_fabric::compute_budget::ExternalComputeBaseline,
    ) -> anyhow::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn activate_external_compute(
        &self,
        reservation: &ExternalComputeReservation,
    ) -> anyhow::Result<ExternalComputeReceipt> {
        *self.state.lock().unwrap() = ExternalComputeState::Active;
        Ok(ExternalComputeReceipt {
            reservation_id: reservation.reservation_id.clone(),
            state: ExternalComputeState::Active,
        })
    }
    fn cancel_external_compute(
        &self,
        reservation: &ExternalComputeReservation,
    ) -> anyhow::Result<ExternalComputeReceipt> {
        if *self.state.lock().unwrap() == ExternalComputeState::Active {
            return Err(anyhow::Error::new(
                corelink_fabric::compute_budget::ExternalComputeError::Conflict,
            ));
        }
        *self.state.lock().unwrap() = ExternalComputeState::Cancelled;
        Ok(ExternalComputeReceipt {
            reservation_id: reservation.reservation_id.clone(),
            state: ExternalComputeState::Cancelled,
        })
    }
    fn settle_external_compute(
        &self,
        reservation: &ExternalComputeReservation,
        _: ExternalComputeSettlement,
    ) -> anyhow::Result<ExternalComputeReceipt> {
        self.settle_calls.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().unwrap() = ExternalComputeState::Settled;
        Ok(ExternalComputeReceipt {
            reservation_id: reservation.reservation_id.clone(),
            state: ExternalComputeState::Settled,
        })
    }
    fn put(&self, _: LeaseRecord) -> anyhow::Result<()> {
        unreachable!()
    }
    fn get(&self, _: &str) -> anyhow::Result<Option<LeaseRecord>> {
        unreachable!()
    }
    fn transition(&self, _: &str, _: RunnerState, _: u64) -> anyhow::Result<LeaseRecord> {
        unreachable!()
    }
    fn by_tenant(&self, _: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        unreachable!()
    }
    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        unreachable!()
    }
    fn pending_older_than(&self, _: u64, _: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        unreachable!()
    }
    fn try_admit(&self, _: LeaseRecord, _: u32) -> anyhow::Result<bool> {
        unreachable!()
    }
    fn set_envelope_checkpoint(&self, _: &str, _: &str) -> anyhow::Result<()> {
        unreachable!()
    }
    fn get_envelope_checkpoint(&self, _: &str) -> anyhow::Result<Option<String>> {
        unreachable!()
    }
    fn remove(&self, _: &str) -> anyhow::Result<bool> {
        unreachable!()
    }
}

fn token() -> (String, Vec<u8>) {
    let now = now_ms();
    let payload = serde_json::to_vec(&json!({
        "v":1,"key_id":"issuer-1","tenant_id":"11111111-1111-4111-8111-111111111111",
        "workload_kind":"devenv","workload_id":"job/1","reservation_id":"22222222-2222-4222-8222-222222222222",
        "period_key":current_period(),"ceiling_vcpu_ms":"1000","vcpu_count":1,"maximum_wall_ms":1000,
        "issued_at_ms":now-1000,"expires_at_ms":now+5000
    })).unwrap();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let token = format!(
        "{}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&payload),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.sign(&payload).as_ref())
    );
    (token, key.public_key().as_ref().to_vec())
}

fn current_period() -> u32 {
    let days = (now_ms() / 86_400_000) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as u32) * 100 + month as u32
}

fn app(ledger: Arc<TestLedger>, admin: Option<&str>, public: Vec<u8>) -> Router {
    let mut keys = HashMap::new();
    keys.insert("issuer-1".into(), public);
    router(ledger, keys, admin.map(str::to_owned))
}

async fn reserve_request(app: Router, token: &str, body: impl Into<Body>) -> Response {
    app.oneshot(
        Request::post("/internal/v1/compute/reserve")
            .header("authorization", format!("ComputeGrant {token}"))
            .header("content-type", "application/json")
            .body(body.into())
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn compute_request(app: Router, path: &str, token: &str, body: impl Into<Body>) -> Response {
    app.oneshot(
        Request::post(path)
            .header("authorization", format!("ComputeGrant {token}"))
            .header("content-type", "application/json")
            .body(body.into())
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn invalid_signature_is_401_without_ledger_call() {
    let ledger = Arc::new(TestLedger::new(Outcome::Receipt));
    let (token, public) = token();
    let mut parts = token.split('.');
    let payload = parts.next().unwrap();
    let mut signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts.next().unwrap())
        .unwrap();
    signature[0] ^= 1;
    let tampered = format!(
        "{payload}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature)
    );
    let response = reserve_request(app(ledger.clone(), None, public), &tampered, "{}").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ledger.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reserve_maps_receipt_over_conflict_and_bad_body() {
    let (token, public) = token();
    for (outcome, status) in [
        (Outcome::Receipt, StatusCode::OK),
        (Outcome::Over, StatusCode::TOO_MANY_REQUESTS),
        (Outcome::Conflict, StatusCode::CONFLICT),
        (Outcome::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let ledger = Arc::new(TestLedger::new(outcome));
        assert_eq!(
            reserve_request(app(ledger, None, public.clone()), &token, "{}")
                .await
                .status(),
            status
        );
    }
    let ledger = Arc::new(TestLedger::new(Outcome::Receipt));
    assert_eq!(
        reserve_request(app(ledger, None, public), &token, "{\"bad\":true}")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn admin_gate_maps_wrong_key_and_success() {
    let ledger = Arc::new(TestLedger::new(Outcome::Receipt));
    let (_, public) = token();
    let app = app(ledger, Some("admin"), public);
    let body = format!(
        r#"{{"tenant_id":"11111111-1111-4111-8111-111111111111","period_key":{},"external_vcpu_ms":"0","evidence_digest":"0000000000000000000000000000000000000000000000000000000000000000"}}"#,
        current_period()
    );
    let wrong = Request::post("/internal/v1/admin/compute-baseline")
        .header("x-corelink-internal-auth", "wrong")
        .body(Body::from(body.clone()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(wrong).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let valid = Request::post("/internal/v1/admin/compute-baseline")
        .header("x-corelink-internal-auth", "admin")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        app.oneshot(valid).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn settlement_requires_provider_authority_and_preserves_active_capacity() {
    let ledger = Arc::new(TestLedger::new(Outcome::Receipt));
    let (token, public) = token();
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public.clone()),
            "/internal/v1/compute/reserve",
            &token,
            "{}"
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public.clone()),
            "/internal/v1/compute/activate",
            &token,
            "{}"
        )
        .await
        .status(),
        StatusCode::OK
    );
    let body = r#"{"actual_vcpu_ms":"0","terminal_evidence_digest":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public.clone()),
            "/internal/v1/compute/settle",
            &token,
            body
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public),
            "/internal/v1/compute/settle",
            &token,
            body
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(*ledger.state.lock().unwrap(), ExternalComputeState::Active);
    assert_eq!(ledger.settle_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn prepared_reservations_can_still_cancel() {
    let ledger = Arc::new(TestLedger::new(Outcome::Receipt));
    let (token, public) = token();
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public.clone()),
            "/internal/v1/compute/reserve",
            &token,
            "{}"
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        compute_request(
            app(ledger.clone(), None, public),
            "/internal/v1/compute/cancel",
            &token,
            "{}"
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        *ledger.state.lock().unwrap(),
        ExternalComputeState::Cancelled
    );
}
