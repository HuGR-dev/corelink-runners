use std::sync::{Arc, Mutex};

use corelink_fabric_server::mint_readiness::{MintReadiness, accepts_probe_response};
use corelink_fabric_server::{MintHttp, MintHttpResponse};

#[derive(Default)]
struct Recording {
    calls: Mutex<Vec<(String, String, Option<String>, String)>>,
    responses: Mutex<Vec<anyhow::Result<MintHttpResponse>>>,
}

impl MintHttp for Recording {
    fn post(
        &self,
        url: &str,
        auth: &str,
        bearer: Option<&str>,
        body: &str,
    ) -> anyhow::Result<MintHttpResponse> {
        self.calls.lock().unwrap().push((
            url.to_owned(),
            auth.to_owned(),
            bearer.map(str::to_owned),
            body.to_owned(),
        ));
        self.responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| Err(anyhow::anyhow!("transient")))
    }
}

fn good() -> MintHttpResponse {
    MintHttpResponse {
        status: 400,
        body: r#"{"error":"BAD_REQUEST","message":"job_id required","request_id":"req-1"}"#.into(),
    }
}

#[test]
fn classifier_rejects_wrong_status_and_untyped_bodies() {
    assert!(accepts_probe_response(good()));
    assert!(!accepts_probe_response(MintHttpResponse {
        status: 400,
        body: r#"{"error":"BAD_REQUEST","message":"wrong","request_id":"req-1"}"#.into(),
    }));
    assert!(!accepts_probe_response(MintHttpResponse {
        status: 200,
        body: "ok".into(),
    }));
    assert!(!accepts_probe_response(MintHttpResponse {
        status: 400,
        body: format!(
            r#"{{"error":"BAD_REQUEST","message":"job_id required","request_id":"{}"}}"#,
            "x".repeat(4097)
        ),
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_waiters_share_one_exact_probe_and_no_bearer() {
    let transport = Arc::new(Recording::default());
    transport.responses.lock().unwrap().push(Ok(good()));
    let readiness = MintReadiness::new(
        "https://dispatcher.example",
        "dispatcher-secret",
        transport.clone(),
    );

    let (a, b) = tokio::join!(readiness.wait_ready(), readiness.wait_ready());
    assert!(a && b);
    let calls = transport.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].0,
        "https://dispatcher.example/internal/v1/runner/mint"
    );
    assert_eq!(calls[0].1, "dispatcher-secret");
    assert_eq!(calls[0].2, None);
    assert_eq!(calls[0].3, "{}");
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_waiter_does_not_start_a_second_probe() {
    let transport = Arc::new(Recording::default());
    transport
        .responses
        .lock()
        .unwrap()
        .extend((0..3).map(|_| Err(anyhow::anyhow!("transient"))));
    let readiness = MintReadiness::new("https://dispatcher.example", "secret", transport.clone());

    let waiter = tokio::spawn({
        let readiness = readiness.clone();
        async move { readiness.wait_ready().await }
    });
    waiter.abort();
    let _ = readiness.wait_ready().await;
    assert_eq!(transport.calls.lock().unwrap().len(), 3);
}
