//! Acceptance suite for `corelink-cloud-engine`.
//!
//! Drives [`NorthflankEngine`] against a programmable [`FakeHttp`] transport —
//! zero account dependency, zero network. Provider "failures" are scripted as
//! non-2xx HTTP responses, not transport errors.

use std::cell::RefCell;
use std::collections::VecDeque;

use anyhow::Result;
use corelink_cloud_engine::{
    HttpRequest, HttpResponse, HttpTransport, Method, NorthflankConfig, NorthflankEngine,
};
use corelink_runner::isolation::{Engine, RunningContainer};
use corelink_runner::lease::ContainerSpec;

// ── Pinned / unpinned image constants ────────────────────────────────────────

const PINNED: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const UNPINNED: &str = "alpine:3.20";

// ── FakeHttp transport ────────────────────────────────────────────────────────

/// Programmable test transport: pops scripted responses in order, records every
/// request. Never returns `Err` itself — provider failures are scripted as
/// non-2xx status codes.
struct FakeHttp {
    /// Scripted responses to return in FIFO order.
    queue: RefCell<VecDeque<HttpResponse>>,
    /// Every request received, in order.
    recorded: RefCell<Vec<HttpRequest>>,
}

impl FakeHttp {
    fn new(responses: Vec<HttpResponse>) -> Self {
        Self {
            queue: RefCell::new(responses.into_iter().collect()),
            recorded: RefCell::new(Vec::new()),
        }
    }

    /// Number of requests recorded so far.
    fn request_count(&self) -> usize {
        self.recorded.borrow().len()
    }

    /// The nth recorded request (0-based), panicking if out of range.
    fn nth_request(&self, n: usize) -> HttpRequest {
        self.recorded.borrow()[n].clone()
    }

    /// The most recent recorded request.
    #[allow(dead_code)]
    fn last_request(&self) -> HttpRequest {
        let r = self.recorded.borrow();
        r.last().expect("no requests recorded").clone()
    }

    /// All recorded requests.
    fn all_requests(&self) -> Vec<HttpRequest> {
        self.recorded.borrow().clone()
    }
}

impl HttpTransport for FakeHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let resp = self.queue.borrow_mut().pop_front().unwrap_or_else(|| {
            panic!(
                "FakeHttp script exhausted — no response scripted for request: {:?} {}",
                req.method.as_str(),
                req.url
            )
        });
        self.recorded.borrow_mut().push(req.clone());
        Ok(resp)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resp(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.to_string(),
    }
}

/// Shared-ownership engine so tests can inspect the FakeHttp after driving the engine.
/// Tests are single-threaded; `Rc` (not `Arc`) avoids the `arc_with_non_send_sync` lint
/// that fires when `Arc` wraps a non-`Sync` type like `RefCell`.
use std::rc::Rc;

struct RcHttp(Rc<FakeHttp>);

impl HttpTransport for RcHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        self.0.send(req)
    }
}

fn engine_shared(responses: Vec<HttpResponse>) -> (NorthflankEngine<RcHttp>, Rc<FakeHttp>) {
    let mut cfg = NorthflankConfig::new("proj", "nf_tok_test");
    cfg.poll_interval_ms = 0;
    let fake = Rc::new(FakeHttp::new(responses));
    let http = RcHttp(Rc::clone(&fake));
    let engine = NorthflankEngine::new(http, cfg);
    (engine, fake)
}

fn engine_with_max_polls(
    responses: Vec<HttpResponse>,
    max_polls: u32,
) -> (NorthflankEngine<RcHttp>, Rc<FakeHttp>) {
    let mut cfg = NorthflankConfig::new("proj", "nf_tok_test");
    cfg.poll_interval_ms = 0;
    cfg.max_poll_attempts = max_polls;
    let fake = Rc::new(FakeHttp::new(responses));
    let http = RcHttp(Rc::clone(&fake));
    let engine = NorthflankEngine::new(http, cfg);
    (engine, fake)
}

/// Build a valid, isolated `ContainerSpec` for the test.
fn pinned_spec(name: &str) -> ContainerSpec {
    ContainerSpec {
        name: name.to_string(),
        image: PINNED.to_string(),
        tmp_root: "/tmp/job".to_string(),
        no_network: true,
        allow_egress: false,
        run_on_create: false,
        path_set: vec![],
        env: vec![],
    }
}

/// Build a `RunningContainer` directly (no real spawn).
fn container(name: &str) -> RunningContainer {
    RunningContainer {
        name: name.to_string(),
    }
}

// ── Test 1: spawn_creates_job_with_pinned_image ───────────────────────────────

#[test]
fn spawn_creates_job_with_pinned_image() {
    let create_resp = resp(200, r#"{"data":{"id":"corelink-c2b-z1"}}"#);
    let (engine, fake) = engine_shared(vec![create_resp]);

    let spec = pinned_spec("corelink-c2b-z1");
    let result = engine.spawn(&spec).expect("spawn should succeed");

    assert_eq!(fake.request_count(), 1, "exactly 1 request");
    let req = fake.nth_request(0);
    assert!(
        req.url.ends_with("/projects/proj/jobs"),
        "request URL should end with /projects/proj/jobs, got: {}",
        req.url
    );
    assert_eq!(req.method, Method::Post, "should be POST");
    let body = req.json_body.expect("should have a JSON body");
    assert!(
        body.contains(PINNED),
        "body should contain the pinned image, got: {body}"
    );

    // The Northflank job name is derived from the spec name via an injective,
    // Northflank-legal transform (P2 fix) — NOT the raw spec name verbatim. The
    // load-bearing invariant is that the name in the create-job body and the
    // name on the returned RunningContainer AGREE, so every later job_url()
    // addresses exactly this job. (Distinct leases can never collide onto one
    // Northflank job — see northflank::tests::job_name_is_injective_*.)
    let parsed: serde_json::Value =
        serde_json::from_str(&body).expect("create-job body should be JSON");
    let body_name = parsed["name"].as_str().expect("body should carry a name");
    assert_eq!(
        result.name, body_name,
        "RunningContainer.name MUST equal the name in the create-job body"
    );
    assert!(
        result.name.starts_with("nf-"),
        "derived job name should carry the nf- prefix, got: {}",
        result.name
    );
}

// ── Test 2: spawn_rejects_unpinned_image_before_provider ─────────────────────

#[test]
fn spawn_rejects_unpinned_image_before_provider() {
    let (engine, fake) = engine_shared(vec![]); // empty script

    let spec = ContainerSpec {
        name: "corelink-job-test-unpinned".to_string(),
        image: UNPINNED.to_string(),
        tmp_root: "/tmp/job".to_string(),
        no_network: true,
        allow_egress: false,
        run_on_create: false,
        path_set: vec![],
        env: vec![],
    };
    let result = engine.spawn(&spec);

    assert!(result.is_err(), "spawn with unpinned image must fail");
    assert_eq!(
        fake.request_count(),
        0,
        "no requests should reach the provider"
    );
}

// ── Test 3: spawn_rejects_non_isolated_spec ───────────────────────────────────

#[test]
fn spawn_rejects_non_isolated_spec() {
    let (engine, fake) = engine_shared(vec![]); // empty script

    let spec = ContainerSpec {
        name: "corelink-job-test-non-isolated".to_string(),
        image: PINNED.to_string(),
        tmp_root: "/tmp/job".to_string(),
        no_network: false, // violates isolation floor
        allow_egress: false,
        run_on_create: false,
        path_set: vec![],
        env: vec![],
    };
    let result = engine.spawn(&spec);

    assert!(result.is_err(), "spawn with no_network=false must fail");
    assert_eq!(
        fake.request_count(),
        0,
        "no requests should reach the provider"
    );
}

// ── Test 4: every_request_carries_bearer_token ────────────────────────────────

#[test]
fn every_request_carries_bearer_token() {
    // Script: spawn (POST create-job), exec (PATCH set-cmd, POST run, GET poll SUCCESS)
    let responses = vec![
        resp(200, r#"{"data":{"id":"job1"}}"#), // create-job
        resp(200, "{}"),                        // PATCH set-command
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST trigger-run
        resp(200, r#"{"status":"SUCCESS"}"#),   // GET poll
    ];
    let (engine, fake) = engine_shared(responses);

    let spec = pinned_spec("corelink-job-job1");
    engine.spawn(&spec).expect("spawn");
    let c = container("job1");
    engine.exec(&c, &["echo", "hi"]).expect("exec");

    let reqs = fake.all_requests();
    assert!(!reqs.is_empty(), "should have recorded some requests");
    for req in &reqs {
        assert_eq!(
            req.bearer_token,
            "nf_tok_test",
            "request {} {} must carry bearer_token == nf_tok_test",
            req.method.as_str(),
            req.url
        );
    }
}

// ── Test 5: exec_sets_command_then_runs_to_success ───────────────────────────

#[test]
fn exec_sets_command_then_runs_to_success() {
    let responses = vec![
        resp(200, "{}"),                        // PATCH set-command
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST trigger-run
        resp(200, r#"{"status":"SUCCESS"}"#),   // GET poll
    ];
    let (engine, fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec(&c, &["sh", "-c", "echo hello"]).expect("exec");

    assert_eq!(result, Some(0), "SUCCESS should map to exit code 0");

    let reqs = fake.all_requests();
    assert_eq!(reqs.len(), 3, "expected 3 requests: PATCH, POST, GET");

    // Request 0: PATCH to the job URL
    let patch = &reqs[0];
    assert_eq!(patch.method, Method::Patch, "first request must be PATCH");
    assert!(
        patch.url.ends_with("/projects/proj/jobs/myjob"),
        "PATCH URL should end with /projects/proj/jobs/myjob, got: {}",
        patch.url
    );
    let patch_body = patch.json_body.as_deref().unwrap_or("");
    assert!(
        patch_body.contains("sh") && patch_body.contains("echo hello"),
        "PATCH body should contain the command argv, got: {patch_body}"
    );

    // Request 1: POST to runs URL
    let post = &reqs[1];
    assert_eq!(post.method, Method::Post, "second request must be POST");
    assert!(
        post.url.ends_with("/runs"),
        "POST URL should end with /runs, got: {}",
        post.url
    );

    // Request 2: GET to the specific run URL
    let get = &reqs[2];
    assert_eq!(get.method, Method::Get, "third request must be GET");
    assert!(
        get.url.contains("/runs/run1"),
        "GET URL should contain /runs/run1, got: {}",
        get.url
    );
}

// ── Test 6: exec_failure_status_maps_to_nonzero ──────────────────────────────

#[test]
fn exec_failure_status_maps_to_nonzero() {
    let responses = vec![
        resp(200, "{}"),                        // PATCH
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST
        resp(200, r#"{"status":"FAILURE"}"#),   // GET poll
    ];
    let (engine, fake) = engine_shared(responses);
    let _ = fake; // not inspecting requests here

    let c = container("myjob");
    let result = engine.exec(&c, &["false"]).expect("exec should return Ok");

    assert_eq!(result, Some(1), "FAILURE should map to exit code 1");
}

// ── Test 7: exec_polls_until_terminal ────────────────────────────────────────

#[test]
fn exec_polls_until_terminal() {
    let responses = vec![
        resp(200, "{}"),                        // PATCH
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST
        resp(200, r#"{"status":"PENDING"}"#),   // GET poll 1 — non-terminal
        resp(200, r#"{"status":"SUCCESS"}"#),   // GET poll 2 — terminal
    ];
    let (engine, fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]).expect("exec");
    assert_eq!(result, Some(0));

    // Count GET requests to .../runs/run1
    let poll_reqs: Vec<_> = fake
        .all_requests()
        .into_iter()
        .filter(|r| r.method == Method::Get && r.url.contains("/runs/run1"))
        .collect();
    assert!(
        poll_reqs.len() >= 2,
        "should have polled at least twice, got {} polls",
        poll_reqs.len()
    );
}

// ── Test 8: exec_fails_closed_when_never_terminal ────────────────────────────

#[test]
fn exec_fails_closed_when_never_terminal() {
    // max_poll_attempts = 3; script 3 non-terminal RUNNING polls
    let responses = vec![
        resp(200, "{}"),                        // PATCH
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST
        resp(200, r#"{"status":"RUNNING"}"#),   // poll 1
        resp(200, r#"{"status":"RUNNING"}"#),   // poll 2
        resp(200, r#"{"status":"RUNNING"}"#),   // poll 3
    ];
    let (engine, _fake) = engine_with_max_polls(responses, 3);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]);
    assert!(
        result.is_err(),
        "exec must fail closed when run never becomes terminal"
    );
}

// ── Test 9: provider_5xx_fails_closed ────────────────────────────────────────

#[test]
fn provider_5xx_fails_closed() {
    // Case A: spawn with 500 create-job response
    {
        let (engine, _fake) = engine_shared(vec![resp(500, "Internal Server Error")]);
        let spec = pinned_spec("corelink-job-myjob");
        assert!(
            engine.spawn(&spec).is_err(),
            "spawn with 500 create-job must fail closed"
        );
    }

    // Case B: exec with PATCH 200 then POST-run 500
    {
        let responses = vec![
            resp(200, "{}"),                    // PATCH ok
            resp(500, "Internal Server Error"), // POST trigger-run 500
        ];
        let (engine, _fake) = engine_shared(responses);
        let c = container("myjob");
        assert!(
            engine.exec(&c, &["true"]).is_err(),
            "exec must fail closed when trigger-run returns 500"
        );
    }
}

// ── Test 10: exec_captured_parses_structured_logs_per_stream ─────────────────

#[test]
fn exec_captured_parses_structured_logs_per_stream() {
    // Structured CRI JSON logs: two stdout entries and one stderr entry.
    // exec_captured must return clean messages (no ts/stream prefix) in the
    // correct buckets, live-confirmed shape.
    let logs_body = r#"{"data":[
        {"containerId":"abc","type":"runtime","ts":"2026-06-12T19:54:09.849Z","log":"2026-06-12T19:54:09.849348083Z stdout F hello-from-northflank"},
        {"containerId":"abc","type":"runtime","ts":"2026-06-12T19:54:09.850Z","log":"2026-06-12T19:54:09.850000000Z stdout F second stdout line"},
        {"containerId":"abc","type":"runtime","ts":"2026-06-12T19:54:09.851Z","log":"2026-06-12T19:54:09.851000000Z stderr F some warning"}
    ]}"#;
    let responses = vec![
        resp(200, "{}"),                        // PATCH set-command
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST trigger-run
        resp(200, r#"{"status":"SUCCESS"}"#),   // GET poll
        resp(200, logs_body),                   // GET logs
    ];
    let (engine, _fake) = engine_shared(responses);

    let c = container("myjob");
    let out = engine
        .exec_captured(&c, &["make", "build"])
        .expect("exec_captured");

    assert_eq!(out.code, Some(0), "exit code should be 0");
    assert_eq!(
        out.stdout, "hello-from-northflank\nsecond stdout line",
        "stdout should be clean messages joined with newline, no ts/stream prefix"
    );
    assert_eq!(
        out.stderr, "some warning",
        "stderr should contain the stderr message, no ts/stream prefix"
    );
}

// ── Test 11: delete_job_idempotent_on_404 ────────────────────────────────────

#[test]
fn delete_job_idempotent_on_404() {
    // Case A: DELETE 200 → Ok
    {
        let (engine, _fake) = engine_shared(vec![resp(200, "{}")]);
        let c = container("myjob");
        assert!(engine.delete_job(&c).is_ok(), "DELETE 200 should be Ok");
    }

    // Case B: DELETE 404 → Ok (idempotent)
    {
        let (engine, _fake) = engine_shared(vec![resp(404, "Not Found")]);
        let c = container("myjob");
        assert!(
            engine.delete_job(&c).is_ok(),
            "DELETE 404 should be Ok (already-gone is idempotent)"
        );
    }

    // Case C: DELETE 500 → Err
    {
        let (engine, _fake) = engine_shared(vec![resp(500, "Internal Server Error")]);
        let c = container("myjob");
        assert!(
            engine.delete_job(&c).is_err(),
            "DELETE 500 should be Err (fail-closed)"
        );
    }
}

// ── Test 12: is_alive_reflects_presence ──────────────────────────────────────

#[test]
fn is_alive_reflects_presence() {
    // Case A: GET 200 → Ok(true)
    {
        let (engine, _fake) = engine_shared(vec![resp(200, "{}")]);
        let c = container("myjob");
        let alive = engine.is_alive(&c).expect("is_alive");
        assert!(alive, "GET 200 should report alive=true");
    }

    // Case B: GET 404 → Ok(false)
    {
        let (engine, _fake) = engine_shared(vec![resp(404, "Not Found")]);
        let c = container("myjob");
        let alive = engine.is_alive(&c).expect("is_alive");
        assert!(!alive, "GET 404 should report alive=false");
    }
}

// ── Test 13: exec_captured_fails_closed_when_logs_5xx ────────────────────────

#[test]
fn exec_captured_fails_closed_when_logs_5xx() {
    // Script: PATCH set-command 200, POST trigger-run 200, GET poll SUCCESS 200,
    // then GET logs 500 → exec_captured must return Err.
    let responses = vec![
        resp(200, "{}"),                               // PATCH set-command
        resp(200, r#"{"data":{"id":"r1"}}"#),          // POST trigger-run
        resp(200, r#"{"data":{"status":"SUCCESS"}}"#), // GET poll → success
        resp(500, "Internal Server Error"),            // GET logs → 500
    ];
    let (engine, _fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec_captured(&c, &["echo", "hi"]);
    assert!(
        result.is_err(),
        "exec_captured must fail closed when logs endpoint returns 500"
    );
}

// ── Test 14: set_command_failure_aborts_before_run ───────────────────────────

#[test]
fn set_command_failure_aborts_before_run() {
    // PATCH set-command returns 500 — exec must Err immediately, and NO request
    // to any `.../runs` URL should have been recorded.
    let responses = vec![
        resp(500, "Internal Server Error"), // PATCH set-command fails
    ];
    let (engine, fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]);
    assert!(
        result.is_err(),
        "exec must fail when set-command returns 500"
    );

    let runs_reqs: Vec<_> = fake
        .all_requests()
        .into_iter()
        .filter(|r| r.url.contains("/runs"))
        .collect();
    assert_eq!(
        runs_reqs.len(),
        0,
        "no requests to any .../runs URL should be made after set-command failure, \
         got: {runs_reqs:?}"
    );
}

// ── Test 15: classify_failure_precedence_both_tokens ─────────────────────────

#[test]
fn classify_failure_precedence_both_tokens() {
    // Poll body has both a SUCCESS token and a FAILURE token as string values —
    // failure precedence means the run maps to exit code 1.
    let responses = vec![
        resp(200, "{}"),                                                    // PATCH
        resp(200, r#"{"data":{"id":"r1"}}"#),                               // POST
        resp(200, r#"{"data":{"status":"COMPLETED","result":"FAILURE"}}"#), // GET poll
    ];
    let (engine, _fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]).expect("exec must return Ok");
    assert_eq!(
        result,
        Some(1),
        "FAILURE token must take precedence over COMPLETED — expected exit code 1"
    );
}

// ── Test 16: classify_no_false_positive_on_error_count ───────────────────────

#[test]
fn classify_no_false_positive_on_error_count() {
    // Body has `"errorCount":0` (numeric) and `"errors":[]` (empty array) —
    // neither is a string value, so they must not trigger failure classification.
    // `"status":"SUCCESS"` is the only string value → success → exit code 0.
    let responses = vec![
        resp(200, "{}"),                      // PATCH
        resp(200, r#"{"data":{"id":"r1"}}"#), // POST
        resp(
            200,
            r#"{"data":{"status":"SUCCESS"},"errorCount":0,"errors":[]}"#,
        ), // GET poll
    ];
    let (engine, _fake) = engine_shared(responses);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]).expect("exec must return Ok");
    assert_eq!(
        result,
        Some(0),
        "errorCount:0 and errors:[] must not flip SUCCESS to failure — expected exit code 0"
    );
}

// ── Test 17: garbled_run_response_fails_closed ───────────────────────────────

#[test]
fn garbled_run_response_fails_closed() {
    // POST-run body is valid JSON but missing `data.id` → parse_id fails → Err.
    {
        let responses = vec![
            resp(200, "{}"),             // PATCH set-command
            resp(200, r#"{"data":{}}"#), // POST trigger-run — no id
        ];
        let (engine, _fake) = engine_with_max_polls(responses, 2);
        let c = container("myjob");
        let result = engine.exec(&c, &["true"]);
        assert!(
            result.is_err(),
            "exec must fail when POST-run response has no data.id"
        );
    }

    // POST-run body is not JSON at all → parse_id fails → Err.
    {
        let responses = vec![
            resp(200, "{}"),      // PATCH set-command
            resp(200, "notjson"), // POST trigger-run — not JSON
        ];
        let (engine, _fake) = engine_with_max_polls(responses, 2);
        let c = container("myjob");
        let result = engine.exec(&c, &["true"]);
        assert!(
            result.is_err(),
            "exec must fail when POST-run response is not valid JSON"
        );
    }
}

// ── Test 18: zero_poll_budget_fails_closed ────────────────────────────────────

#[test]
fn zero_poll_budget_fails_closed() {
    // max_poll_attempts=0 means the loop body never executes → the "did not reach
    // a terminal state" bail fires immediately after POST-run succeeds.
    let responses = vec![
        resp(200, "{}"),                      // PATCH set-command
        resp(200, r#"{"data":{"id":"r1"}}"#), // POST trigger-run
    ];
    let (engine, _fake) = engine_with_max_polls(responses, 0);

    let c = container("myjob");
    let result = engine.exec(&c, &["true"]);
    assert!(
        result.is_err(),
        "exec must fail closed when max_poll_attempts=0"
    );
}

// ── Test 19: probe_reports_isolation_by_presence ─────────────────────────────

#[test]
fn probe_reports_isolation_by_presence() {
    use corelink_runner::isolation::IsolationProbe;

    // Case A: GET 200 → IsolationProbe { tmp_is_private: true, net_is_isolated: true }
    // and fully_isolated() == true.
    {
        let (engine, _fake) = engine_shared(vec![resp(200, r#"{"data":{"id":"job1"}}"#)]);
        let c = container("job1");
        let spec = pinned_spec("corelink-job-job1");
        let probe = engine.probe(&c, &spec).expect("probe should succeed");
        assert_eq!(
            probe,
            IsolationProbe {
                tmp_is_private: true,
                net_is_isolated: true,
            },
            "GET 200 → both isolation flags true"
        );
        assert!(
            probe.fully_isolated(),
            "fully_isolated() must be true on GET 200"
        );
    }

    // Case B: GET 404 → both false.
    {
        let (engine, _fake) = engine_shared(vec![resp(404, "Not Found")]);
        let c = container("job1");
        let spec = pinned_spec("corelink-job-job1");
        let probe = engine.probe(&c, &spec).expect("probe should succeed");
        assert_eq!(
            probe,
            IsolationProbe {
                tmp_is_private: false,
                net_is_isolated: false,
            },
            "GET 404 → both isolation flags false"
        );
        assert!(
            !probe.fully_isolated(),
            "fully_isolated() must be false on GET 404"
        );
    }
}

// ── Test 20: is_alive_5xx_fails_closed ───────────────────────────────────────

#[test]
fn is_alive_5xx_fails_closed() {
    // After the ENGINE FIX 2, a 5xx is indeterminate → is_alive returns Err.
    let (engine, _fake) = engine_shared(vec![resp(500, "Internal Server Error")]);
    let c = container("myjob");
    let result = engine.is_alive(&c);
    assert!(
        result.is_err(),
        "is_alive with HTTP 500 must return Err (fail-closed), not Ok(false)"
    );
}

// ── Test 21: shell_join_quotes_argv_in_patch_body ────────────────────────────

#[test]
fn shell_join_quotes_argv_in_patch_body() {
    // argv contains a single-quote in one element; shell_join POSIX-escapes it.
    // The expected rendered form: 'sh' '-c' 'echo '\''hi'\'''
    let responses = vec![
        resp(200, "{}"),                               // PATCH set-command
        resp(200, r#"{"data":{"id":"r1"}}"#),          // POST trigger-run
        resp(200, r#"{"data":{"status":"SUCCESS"}}"#), // GET poll
    ];
    let (engine, fake) = engine_shared(responses);

    let c = container("myjob");
    engine
        .exec(&c, &["sh", "-c", "echo 'hi'"])
        .expect("exec should succeed");

    // Find the recorded PATCH request (first request)
    let patch_req = fake.nth_request(0);
    assert_eq!(
        patch_req.method,
        Method::Patch,
        "first request must be PATCH"
    );
    let body = patch_req.json_body.expect("PATCH must have a body");
    // POSIX single-quote escaping: each arg is wrapped in '…', and a literal
    // ' is escaped as '\'' (close-quote, backslash-quote, re-open-quote).
    // The json_body is a raw JSON string so backslashes appear as \\'' in the
    // serialized form; we check the JSON wire representation directly.
    assert!(
        body.contains(r#"'sh' '-c' 'echo '\\''hi'\\'''"#),
        "PATCH body must contain POSIX-shell-quoted argv (JSON-encoded), got: {body}"
    );
}

// ── Test 22: bearer_on_every_request_in_full_flow ────────────────────────────

#[test]
fn bearer_on_every_request_in_full_flow() {
    // Full flow: spawn (POST create-job) → exec_captured (PATCH, POST run, GET poll,
    // GET logs) → delete_job (DELETE). Every request must carry bearer_token == "nf_tok_test".
    let responses = vec![
        resp(200, r#"{"data":{"id":"job1"}}"#), // POST create-job
        resp(200, "{}"),                        // PATCH set-command
        resp(200, r#"{"data":{"id":"r1"}}"#),   // POST trigger-run
        resp(200, r#"{"data":{"status":"SUCCESS"}}"#), // GET poll
        resp(200, "log output\n"),              // GET logs
        resp(200, "{}"),                        // DELETE job
    ];
    let (engine, fake) = engine_shared(responses);

    let spec = pinned_spec("corelink-job-job1");
    let c = engine.spawn(&spec).expect("spawn");
    engine
        .exec_captured(&c, &["echo", "hi"])
        .expect("exec_captured");
    engine.delete_job(&c).expect("delete_job");

    let reqs = fake.all_requests();
    assert!(
        reqs.len() >= 6,
        "expected at least 6 requests, got {}",
        reqs.len()
    );
    for req in &reqs {
        assert_eq!(
            req.bearer_token,
            "nf_tok_test",
            "{} {} must carry bearer_token == nf_tok_test",
            req.method.as_str(),
            req.url
        );
    }
}

// ── Test 22b: fetch_logs_defensive_on_non_json ───────────────────────────────

#[test]
fn fetch_logs_defensive_on_non_json() {
    // A logs body that is not the expected JSON shape: exec_captured must
    // return the raw body as stdout, no panic or Err.
    let responses = vec![
        resp(200, "{}"),                        // PATCH set-command
        resp(200, r#"{"data":{"id":"run1"}}"#), // POST trigger-run
        resp(200, r#"{"status":"SUCCESS"}"#),   // GET poll
        resp(200, "plain text"),                // GET logs — not structured JSON
    ];
    let (engine, _fake) = engine_shared(responses);

    let c = container("myjob");
    let out = engine
        .exec_captured(&c, &["echo", "hi"])
        .expect("exec_captured must succeed even on non-JSON logs body");

    assert_eq!(out.code, Some(0), "exit code should be 0");
    assert_eq!(
        out.stdout, "plain text",
        "non-JSON logs body should be returned verbatim as stdout"
    );
    assert_eq!(
        out.stderr, "",
        "stderr should be empty for non-JSON fallback"
    );
}

// ── Test 22c: from_env_team_id_sets_team_scoped_base_url ─────────────────────

#[test]
fn from_env_team_id_sets_team_scoped_base_url() {
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_API_TOKEN" => Some("tok".to_string()),
        "NORTHFLANK_PROJECT_ID" => Some("proj".to_string()),
        "NORTHFLANK_TEAM_ID" => Some("humangr".to_string()),
        _ => None,
    })
    .expect("config must be Some");

    assert_eq!(
        cfg.base_url, "https://api.northflank.com/v1/teams/humangr",
        "NORTHFLANK_TEAM_ID should produce a team-scoped base_url"
    );
}

// ── Test 22d: from_env_explicit_base_url_beats_team_id ───────────────────────

#[test]
fn from_env_explicit_base_url_beats_team_id() {
    let explicit = "https://my-proxy.example.com/v1";
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_API_TOKEN" => Some("tok".to_string()),
        "NORTHFLANK_PROJECT_ID" => Some("proj".to_string()),
        "NORTHFLANK_BASE_URL" => Some(explicit.to_string()),
        "NORTHFLANK_TEAM_ID" => Some("humangr".to_string()),
        _ => None,
    })
    .expect("config must be Some");

    assert_eq!(
        cfg.base_url, explicit,
        "NORTHFLANK_BASE_URL must take precedence over NORTHFLANK_TEAM_ID"
    );
}

// ── FailingHttp transport ─────────────────────────────────────────────────────

/// Test transport that ALWAYS returns a transport-layer `Err` — models DNS, TLS,
/// or connection-timeout failures (no HTTP response is ever obtained).
struct FailingHttp;

impl HttpTransport for FailingHttp {
    fn send(&self, _req: &HttpRequest) -> Result<HttpResponse> {
        Err(anyhow::anyhow!(
            "simulated transport failure (DNS/TLS/timeout)"
        ))
    }
}

/// Build a `NorthflankEngine` over the always-failing transport.
fn engine_failing() -> NorthflankEngine<FailingHttp> {
    let mut cfg = NorthflankConfig::new("proj", "nf_tok_test");
    cfg.poll_interval_ms = 0;
    NorthflankEngine::new(FailingHttp, cfg)
}

// ── Test T1: spawn_transport_error_fails_closed ───────────────────────────────

#[test]
fn spawn_transport_error_fails_closed() {
    // A transport-layer error (DNS/TLS/timeout) on the create-job POST must
    // propagate as Err — never fabricate an Ok RunningContainer.
    let engine = engine_failing();
    let spec = pinned_spec("corelink-c2b-z1");
    assert!(
        engine.spawn(&spec).is_err(),
        "spawn must return Err on transport-layer failure (fail-closed)"
    );
}

// ── Test T2: exec_transport_error_fails_closed ────────────────────────────────

#[test]
fn exec_transport_error_fails_closed() {
    // A transport-layer error on the very first outbound call (PATCH set-command)
    // must propagate as Err — exec must never return Ok on network failure.
    let engine = engine_failing();
    let c = container("myjob");
    assert!(
        engine.exec(&c, &["echo", "hi"]).is_err(),
        "exec must return Err on transport-layer failure (fail-closed)"
    );
}

// ── Test T3: exec_captured_transport_error_fails_closed ──────────────────────

#[test]
fn exec_captured_transport_error_fails_closed() {
    // exec_captured calls set_command first (PATCH); a transport error there
    // must propagate immediately as Err.
    let engine = engine_failing();
    let c = container("myjob");
    assert!(
        engine.exec_captured(&c, &["make", "build"]).is_err(),
        "exec_captured must return Err on transport-layer failure (fail-closed)"
    );
}

// ── Test T4: is_alive_transport_error_fails_closed ───────────────────────────

#[test]
fn is_alive_transport_error_fails_closed() {
    // is_alive uses self.send() which propagates the transport Err via ?.
    // A network-layer failure must NOT be silently swallowed as Ok(false) —
    // it is indeterminate, not "absent", and must surface as Err.
    let engine = engine_failing();
    let c = container("myjob");
    assert!(
        engine.is_alive(&c).is_err(),
        "is_alive must return Err on transport-layer failure (not silent false)"
    );
}

// ── Test T5: delete_job_transport_error_fails_closed ─────────────────────────

#[test]
fn delete_job_transport_error_fails_closed() {
    // delete_job uses self.send(); a transport error propagates via ?.
    // Never treat a network failure as idempotent success.
    let engine = engine_failing();
    let c = container("myjob");
    assert!(
        engine.delete_job(&c).is_err(),
        "delete_job must return Err on transport-layer failure (fail-closed)"
    );
}

// ── Test T6: probe_non_2xx_reports_not_isolated ───────────────────────────────

#[test]
fn probe_non_2xx_reports_not_isolated() {
    use corelink_runner::isolation::IsolationProbe;

    // probe uses self.send() (not send_2xx) and returns Ok with alive = resp.is_success().
    // A 500 → alive=false → both isolation flags false → fully_isolated()==false.
    // This is the fail-safe direction: an absent/unhealthy job is reported as not isolated,
    // never as falsely isolated.
    let (engine, _fake) = engine_shared(vec![resp(500, "Internal Server Error")]);
    let c = container("job1");
    let spec = pinned_spec("corelink-job-job1");
    let probe = engine
        .probe(&c, &spec)
        .expect("probe must return Ok on a non-2xx HTTP response (transport succeeded)");
    assert_eq!(
        probe,
        IsolationProbe {
            tmp_is_private: false,
            net_is_isolated: false,
        },
        "HTTP 500 from probe must yield both isolation flags false"
    );
    assert!(
        !probe.fully_isolated(),
        "fully_isolated() must be false when probe receives HTTP 500"
    );
}

// ── Test T7: probe_transport_error_fails_closed ───────────────────────────────

#[test]
fn probe_transport_error_fails_closed() {
    // probe uses self.send() which propagates a transport Err via ?.
    // A network-layer failure must surface as Err — never fabricate isolation state.
    let engine = engine_failing();
    let c = container("job1");
    let spec = pinned_spec("corelink-job-job1");
    assert!(
        engine.probe(&c, &spec).is_err(),
        "probe must return Err on transport-layer failure (fail-closed)"
    );
}

// ── Test 23: spawn_sends_pinned_image_verbatim ────────────────────────────────

#[test]
fn spawn_sends_pinned_image_verbatim() {
    // The create-job request body must contain the full digest-pinned image string
    // verbatim — the X4 by-digest integrity basis.
    let create_resp = resp(200, r#"{"data":{"id":"job1"}}"#);
    let (engine, fake) = engine_shared(vec![create_resp]);

    let spec = pinned_spec("corelink-job-job1"); // uses PINNED constant
    engine.spawn(&spec).expect("spawn");

    let req = fake.nth_request(0);
    let body = req.json_body.expect("create-job must have a body");
    assert!(
        body.contains(PINNED),
        "create-job body must contain the exact digest-pinned image \
         `{PINNED}`, got: {body}"
    );
}
