//! Cross-side conformance vector for the Cloudflare spawn-Worker HTTP contract
//! (`docs/spec/cloudflare-spawn-worker-contract.md`, FROZEN v0).
//!
//! The committed `conformance/cloudflare-spawn.json` is the **drift tripwire**:
//! it pins the canonical `POST /v1/spawn` request body shape and the success
//! response (`{"handle":...}`), byte-identical to what the Cloudflare Worker on
//! the other side must accept/emit (mirrors the frozen legacy CLW conformance
//! discipline). These golden tests drive the real [`CloudflareEngine`] over a
//! fake [`HttpTransport`] — no account, no network — capture the request the
//! engine actually emits, and assert its structural fields match the vector.
//! Any change to the engine's `spawn_body` shape (a renamed field, a dropped
//! `env` key, a different `jitconfig` lift, a moved `labels`/`expiry_ms`) makes
//! these tests FAIL — that is the point: the seam cannot drift silently.
//!
//! Public-API only: the test builds its OWN recording `HttpTransport` (the
//! in-module `RecordingTransport` in `cloudflare.rs` is private and owned by
//! that file). The response side asserts the engine's handle parse end-to-end:
//! feeding the vector's response body through `spawn` must yield a
//! `RunningContainer` whose name is the vector's `handle` (the public path
//! through the crate's private `parse_handle`).

use std::sync::{Arc, Mutex};

use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport, Method,
};
use corelink_runner::ContainerSpec;
use corelink_runner::isolation::Engine;

/// The env key the engine lifts into the top-level `jitconfig` field (mirrors
/// the private `JITCONFIG_ENV_KEY` in `cloudflare.rs`; transcribed here, as the
/// conformance discipline requires each side to transcribe — not import).
const JITCONFIG_ENV_KEY: &str = "CORELINK_RUNNER_JITCONFIG";

/// A transport that records the last request it was asked to send and returns a
/// canned response. The public-API test seam (the crate's own recording
/// transport is private to `cloudflare.rs`). The recorded slot is shared via an
/// `Arc<Mutex<…>>` so the test can read it back after the engine has consumed
/// the transport by value (`CloudflareEngine` takes ownership of `H`).
#[derive(Clone)]
struct RecordingTransport {
    status: u16,
    body: String,
    last: Arc<Mutex<Option<HttpRequest>>>,
}

impl RecordingTransport {
    fn new(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            last: Arc::new(Mutex::new(None)),
        }
    }
}

impl HttpTransport for RecordingTransport {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        *self
            .last
            .lock()
            .expect("recording transport mutex not poisoned") = Some(req.clone());
        Ok(HttpResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

/// Parse the committed conformance vector into (request, response) JSON values.
fn vector() -> (serde_json::Value, serde_json::Value) {
    let raw = include_str!("../../../conformance/cloudflare-spawn.json");
    let v: serde_json::Value =
        serde_json::from_str(raw).expect("conformance/cloudflare-spawn.json must be valid JSON");
    let request = v
        .get("request")
        .cloned()
        .expect("vector must carry a `request` object");
    let response = v
        .get("response")
        .cloned()
        .expect("vector must carry a `response` object");
    (request, response)
}

/// Build a RUNNER `ContainerSpec` whose fields mirror the vector's request:
/// the pinned image_digest, and an env map carrying the SAME jitconfig + CLW
/// key the vector pins. Public API only (`ContainerSpec` has all-public fields).
fn spec_from_vector(req: &serde_json::Value) -> ContainerSpec {
    let image = req["image_digest"]
        .as_str()
        .expect("vector request.image_digest must be a string")
        .to_string();

    // Reconstruct the env pairs from the vector's `env` object so the engine
    // emits the same map (and lifts the same jitconfig). Order the JIT key
    // first so the lift is unambiguous.
    let env_obj = req["env"]
        .as_object()
        .expect("vector request.env must be an object");
    let mut env: Vec<(String, String)> = Vec::with_capacity(env_obj.len());
    if let Some(jit) = env_obj.get(JITCONFIG_ENV_KEY).and_then(|v| v.as_str()) {
        env.push((JITCONFIG_ENV_KEY.to_string(), jit.to_string()));
    }
    for (k, v) in env_obj {
        if k == JITCONFIG_ENV_KEY {
            continue;
        }
        env.push((
            k.clone(),
            v.as_str()
                .expect("vector request.env values must be strings")
                .to_string(),
        ));
    }

    ContainerSpec {
        name: "corelink-job-cf-conformance".to_string(),
        image,
        tmp_root: "/tmp/job".to_string(),
        // A RUNNER lease (allow_egress == true, no_network == false): the only
        // path the Cloudflare v0 backend serves (ADR-0007 direct-CI).
        no_network: false,
        allow_egress: true,
        run_on_create: true,
        path_set: vec![],
        env,
    }
}

/// Build an engine whose config (labels, expiry_ms) matches the vector's
/// request, over a recording transport that returns the vector's response.
/// Returns the engine plus a shared handle on the transport's recorded-request
/// slot (the engine owns the transport by value, so the test reads the request
/// back through this `Arc`).
fn engine_from_vector(
    req: &serde_json::Value,
    resp_body: &str,
) -> (
    CloudflareEngine<RecordingTransport>,
    Arc<Mutex<Option<HttpRequest>>>,
) {
    let mut cfg = CloudflareConfig::new("https://spawn.example.dev", "conformance-spawn-token")
        .with_scoped_tokens("conformance-exec-token", "conformance-lifecycle-token");
    cfg.labels = req["labels"]
        .as_array()
        .expect("vector request.labels must be an array")
        .iter()
        .map(|l| {
            l.as_str()
                .expect("vector request.labels entries must be strings")
                .to_string()
        })
        .collect();
    cfg.expiry_ms = req["expiry_ms"]
        .as_u64()
        .expect("vector request.expiry_ms must be a u64");
    // A nominal runner disk so the disk floor passes (the floor is exercised by
    // the in-module tests; here we want the spawn to reach the transport).
    let transport = RecordingTransport::new(200, resp_body);
    let recorded = Arc::clone(&transport.last);
    (CloudflareEngine::new(transport, cfg), recorded)
}

/// The engine's emitted `POST /v1/spawn` body is byte-shape-equal to the
/// vector's `request` object. Drives a real `spawn` and compares the captured
/// body, parsed as JSON, field by field against the frozen vector.
#[test]
fn spawn_body_matches_conformance_vector() {
    let (req_vec, resp_vec) = vector();
    let resp_body = serde_json::to_string(&resp_vec).expect("vector response serializes");

    let spec = spec_from_vector(&req_vec);
    let (engine, recorded_slot) = engine_from_vector(&req_vec, &resp_body);

    let running = engine.spawn(&spec).expect("spawn must succeed");

    // The handle parse (private `parse_handle`) asserted through the public API:
    // the response body from the vector yields a container named for its handle.
    let expected_handle = resp_vec["handle"]
        .as_str()
        .expect("vector response.handle must be a string");
    assert_eq!(
        running.name, expected_handle,
        "spawn must return a RunningContainer named for the vector's response handle \
         (the public path through parse_handle)"
    );

    // Capture the request the engine actually emitted and parse the body.
    let recorded = recorded_slot
        .lock()
        .expect("recording transport mutex not poisoned")
        .clone()
        .expect("a spawn request must have been sent");
    assert_eq!(recorded.method, Method::Post, "spawn is a POST");
    assert_eq!(
        recorded.url, "https://spawn.example.dev/v1/spawn",
        "spawn hits the /v1/spawn endpoint"
    );
    let emitted: serde_json::Value = serde_json::from_str(
        recorded
            .json_body
            .as_deref()
            .expect("spawn must carry a JSON body"),
    )
    .expect("emitted spawn body must be valid JSON");

    // ── Structural assertions against the frozen vector ──────────────────────
    // image_digest: byte-identical, digest-pinned.
    assert_eq!(
        emitted["image_digest"], req_vec["image_digest"],
        "image_digest must match the vector (digest-pinned ref)"
    );

    // jitconfig: lifted from env into the top-level field, equal to the vector.
    assert_eq!(
        emitted["jitconfig"], req_vec["jitconfig"],
        "top-level jitconfig must match the vector"
    );

    // env: the SAME set of keys, with the SAME values (the Worker injects this
    // map; CORELINK_RUNNER_JITCONFIG must also remain in env per the contract).
    let emitted_env = emitted["env"]
        .as_object()
        .expect("emitted env must be an object");
    let vec_env = req_vec["env"]
        .as_object()
        .expect("vector env must be an object");
    let mut emitted_keys: Vec<&String> = emitted_env.keys().collect();
    let mut vec_keys: Vec<&String> = vec_env.keys().collect();
    emitted_keys.sort();
    vec_keys.sort();
    assert_eq!(
        emitted_keys, vec_keys,
        "emitted env keys must match the vector's env keys exactly"
    );
    for (k, v) in vec_env {
        assert_eq!(
            &emitted_env[k], v,
            "emitted env value for `{k}` must match the vector"
        );
    }
    // The contract specifically requires the JIT key be present in env too.
    assert!(
        emitted_env.contains_key(JITCONFIG_ENV_KEY),
        "the JIT config must remain in the env map for the container entrypoint"
    );

    // labels: array equal to the vector.
    assert_eq!(
        emitted["labels"], req_vec["labels"],
        "labels must match the vector"
    );

    // expiry_ms: numeric equal to the vector.
    assert_eq!(
        emitted["expiry_ms"], req_vec["expiry_ms"],
        "expiry_ms must match the vector"
    );

    // No unexpected top-level fields crept into the wire shape: the request
    // object's key set must be exactly what the contract names.
    let emitted_top = emitted
        .as_object()
        .expect("emitted body must be a JSON object");
    let mut top_keys: Vec<&String> = emitted_top.keys().collect();
    top_keys.sort();
    assert_eq!(
        top_keys,
        vec!["env", "expiry_ms", "image_digest", "jitconfig", "labels"],
        "the spawn body must carry exactly the contract's five top-level fields"
    );
}

/// The committed vector itself is well-formed against the frozen contract: the
/// request pins a digest-pinned image, carries the JIT key in env, and the
/// response carries a non-empty handle. (Guards the vector from rotting
/// independently of the engine.)
#[test]
fn conformance_vector_is_contract_shaped() {
    let (req, resp) = vector();

    let image = req["image_digest"]
        .as_str()
        .expect("request.image_digest is a string");
    assert!(
        image.contains("@sha256:"),
        "image_digest must be digest-pinned, got {image}"
    );

    let env = req["env"].as_object().expect("request.env is an object");
    assert!(
        env.contains_key(JITCONFIG_ENV_KEY),
        "request.env must carry CORELINK_RUNNER_JITCONFIG"
    );
    assert!(
        env.keys().any(|k| k.starts_with("CLW_")),
        "request.env must carry a CLW_* cache-identity key"
    );

    assert!(req["labels"].is_array(), "request.labels must be an array");
    assert!(
        req["expiry_ms"].as_u64().is_some_and(|ms| ms > 0),
        "request.expiry_ms must be a positive integer"
    );

    let handle = resp["handle"]
        .as_str()
        .expect("response.handle is a string");
    assert!(!handle.is_empty(), "response.handle must be non-empty");
}
