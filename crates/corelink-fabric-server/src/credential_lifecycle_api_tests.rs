use super::*;
use axum::body::Body;
use axum::http::Request;
use corelink_fabric::ledger::{LeaseLedger, LeaseRecord, TenantLifecycle};
use corelink_fabric::TenantId;
use corelink_runners_contracts::RunnerState;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::util::ServiceExt;

const TENANT: &str = "11111111-1111-4111-8111-111111111111";

struct TestLedger {
    calls: AtomicUsize,
    result: anyhow::Result<TenantLifecycle>,
}

impl LeaseLedger for TestLedger {
    fn tenant_lifecycle(&self, _: &str) -> anyhow::Result<TenantLifecycle> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.result {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(anyhow::anyhow!(error.to_string())),
        }
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

fn app(ledger: Arc<TestLedger>, key: Option<&str>) -> Router {
    router(ledger, key.map(str::to_owned))
}

fn request(key: &str, tenant: &str) -> Request<Body> {
    Request::get(format!(
        "/internal/v1/credentials/tenants/{tenant}/lifecycle"
    ))
    .header(INTERNAL_AUTH_HEADER, key)
    .body(Body::empty())
    .unwrap()
}

#[tokio::test]
async fn wrong_key_is_401_without_ledger_lookup() {
    let ledger = Arc::new(TestLedger {
        calls: AtomicUsize::new(0),
        result: Ok(TenantLifecycle {
            tenant_id: TENANT.into(),
            generation: 1,
            suspended: false,
        }),
    });
    let response = app(ledger.clone(), Some("issuer"))
        .oneshot(request("wrong", TENANT))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ledger.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn response_preserves_suspension_and_decimal_generation() {
    let ledger = Arc::new(TestLedger {
        calls: AtomicUsize::new(0),
        result: Ok(TenantLifecycle {
            tenant_id: TENANT.into(),
            generation: i64::MAX as u64,
            suspended: true,
        }),
    });
    let response = app(ledger, Some("issuer"))
        .oneshot(request("issuer", TENANT))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(
        body.as_ref(),
        format!(
            r#"{{"tenant_id":"{TENANT}","generation":"9223372036854775807","suspended":true}}"#
        )
        .as_bytes()
    );
}

#[tokio::test]
async fn missing_key_backend_failure_and_invalid_tenant_fail_closed() {
    let missing = Arc::new(TestLedger {
        calls: AtomicUsize::new(0),
        result: Ok(TenantLifecycle {
            tenant_id: TENANT.into(),
            generation: 1,
            suspended: false,
        }),
    });
    assert_eq!(
        app(missing.clone(), None)
            .oneshot(request("issuer", TENANT))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(missing.calls.load(Ordering::SeqCst), 0);
    let failed = Arc::new(TestLedger {
        calls: AtomicUsize::new(0),
        result: Err(anyhow::anyhow!("backend secret")),
    });
    assert_eq!(
        app(failed.clone(), Some("issuer"))
            .oneshot(request("issuer", TENANT))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
    let invalid = Arc::new(TestLedger {
        calls: AtomicUsize::new(0),
        result: Ok(TenantLifecycle {
            tenant_id: TENANT.into(),
            generation: 1,
            suspended: false,
        }),
    });
    assert_eq!(
        app(invalid.clone(), Some("issuer"))
            .oneshot(request("issuer", "not-a-uuid"))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(invalid.calls.load(Ordering::SeqCst), 0);
}
