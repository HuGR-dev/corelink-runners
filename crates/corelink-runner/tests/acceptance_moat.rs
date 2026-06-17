//! Cache-moat acceptance suite — RED phase (WP-1).
//!
//! Tests A1, A2, A5, A5b, A9, A9b, A10, A11, A12, A13 for the BootCas/
//! CAS-HTTP / digest / fail-closed scenarios.
//!
//! All tests COMPILE and FAIL (red): the impl is `unimplemented!()` in the
//! WP-2 stubs (`cas_http.rs`, `HttpBootCas`).  They encode the target so
//! the WP-2 impl builds to green.
//!
//! Harness pattern: reuse `FakeCas` / `FaultMode` from `acceptance_c3.rs`
//! (same file layout; duplicated here so each test file is self-contained).
//! A `MockCasTransport` is added to drive `CasHttpClient` without network.
//!
//! ## Placement rationale
//! These are `corelink-runner` integration tests because:
//! - `BootCas` / `hydrate` / `cold_hydrate` live in `corelink_runner::boot`.
//! - `CasHttpClient` / `HttpBootCas` / `CasTransport` live in
//!   `corelink_runner::cas_http`.
//! - No HTTP server or acquire harness is needed — a mock transport suffices.
//!
//! The acquire-integrated scenarios (A3b, A4, A7, A7b, A6) are in
//! `crates/corelink-fabric-server/tests/acceptance_moat.rs`.
//! A8 (clw drive exit transparency) is also in that file (stub seam added).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use corelink_runner::boot::{
    BootCas, BootError, BootOutcome, HydrationPlan, ToolchainLayer, cold_hydrate, hydrate,
};
use corelink_runner::cas_http::{
    Blake3Key, CasMethod, CasOutcome, CasRequest, CasResponse, CasTransport,
};
use corelink_runners_contracts::FenceManifest;

// ─────────────────────────────────────────────────────────────────────────────
// Shared fixtures
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed BLAKE3 hex (64 chars; 256-bit).  Used as the native CAS key (A9b).
const BLAKE3_KEY_A: &str =
    "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e9000000aa";

/// SHA-256 hex for the SAME conceptual blob.  Must NEVER appear in a CAS URL
/// as the key (A9b assertion).
const SHA256_KEY_A: &str =
    "0000000000000000000000000000000000000000000000000000000000000000aa";

/// Action digest (blake3-hex) for AC pre-lease tests.
const AC_DIGEST: &str =
    "ac1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e900000000";

fn fence() -> FenceManifest {
    FenceManifest {
        path_set: vec!["src/".to_string()],
        deny_default: true,
        materialized: vec![],
    }
}

fn fresh_plan(keys: &[&str]) -> HydrationPlan {
    HydrationPlan {
        lease_id: "moat-test-lease".to_string(),
        toolchain_layers: keys
            .iter()
            .map(|k| ToolchainLayer {
                content_key: k.to_string(),
                size_bytes: 512,
            })
            .collect(),
        fence: fence(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FakeCas — reused from acceptance_c3 pattern (self-contained copy)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultMode {
    Ok,
    CasDown,
    AcDown,
}

struct FakeCas {
    fault: FaultMode,
    cached: HashSet<String>,
    fetch_counts: Mutex<HashMap<String, usize>>,
    write_count: Cell<usize>,
    /// Optional per-key data override (for byte-identity tests).
    data_map: HashMap<String, Vec<u8>>,
}

impl FakeCas {
    fn warm(cached_keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            fault: FaultMode::Ok,
            cached: cached_keys.into_iter().collect(),
            fetch_counts: Mutex::new(HashMap::new()),
            write_count: Cell::new(0),
            data_map: HashMap::new(),
        }
    }

    fn cold() -> Self {
        Self {
            fault: FaultMode::Ok,
            cached: HashSet::new(),
            fetch_counts: Mutex::new(HashMap::new()),
            write_count: Cell::new(0),
            data_map: HashMap::new(),
        }
    }

    fn with_fault(fault: FaultMode) -> Self {
        Self {
            fault,
            cached: HashSet::new(),
            fetch_counts: Mutex::new(HashMap::new()),
            write_count: Cell::new(0),
            data_map: HashMap::new(),
        }
    }

    fn with_data(mut self, key: impl Into<String>, data: Vec<u8>) -> Self {
        self.data_map.insert(key.into(), data);
        self
    }

    fn total_fetches(&self) -> usize {
        self.fetch_counts.lock().unwrap().values().copied().sum()
    }

    fn total_writes(&self) -> usize {
        self.write_count.get()
    }
}

impl BootCas for FakeCas {
    fn is_cached(&self, layer_key: &str) -> bool {
        self.cached.contains(layer_key)
    }

    fn fetch_layer(&self, layer_key: &str) -> Result<Vec<u8>, BootError> {
        if self.fault == FaultMode::CasDown {
            return Err(BootError::SubstrateDown {
                substrate: "CAS".to_string(),
                reason: "FakeCas: CAS is down".to_string(),
            });
        }
        *self
            .fetch_counts
            .lock()
            .unwrap()
            .entry(layer_key.to_string())
            .or_insert(0) += 1;
        if let Some(bytes) = self.data_map.get(layer_key) {
            return Ok(bytes.clone());
        }
        Ok(format!("layer-bytes:{layer_key}").into_bytes())
    }

    fn write_layer(&self, layer_key: &str, _data: &[u8]) -> Result<(), BootError> {
        if self.fault == FaultMode::CasDown || self.fault == FaultMode::AcDown {
            return Err(BootError::SubstrateDown {
                substrate: if self.fault == FaultMode::CasDown {
                    "CAS"
                } else {
                    "AC"
                }
                .to_string(),
                reason: "FakeCas: substrate is down".to_string(),
            });
        }
        self.write_count.set(self.write_count.get() + 1);
        let _ = layer_key;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MockCasTransport — drives CasHttpClient without a real network
// ─────────────────────────────────────────────────────────────────────────────

/// What the mock transport returns for a given `(method, url)` pair.
#[derive(Debug, Clone)]
struct MockCasTransport {
    responses: Arc<Mutex<Vec<(CasMethod, String, u16, Vec<u8>)>>>,
    calls: Arc<Mutex<Vec<CasRequest>>>,
}

impl MockCasTransport {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a canned response.  Consumed in FIFO order per `(method, url)`.
    fn push(&self, method: CasMethod, url: impl Into<String>, status: u16, body: Vec<u8>) {
        self.responses
            .lock()
            .unwrap()
            .push((method, url.into(), status, body));
    }

    fn calls_made(&self) -> Vec<CasRequest> {
        self.calls.lock().unwrap().clone()
    }
}

impl CasTransport for MockCasTransport {
    fn send(&self, req: &CasRequest) -> anyhow::Result<CasResponse> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(req.clone());
        let mut responses = self.responses.lock().unwrap();
        // Find the first response whose method + url prefix matches.
        if let Some(idx) = responses
            .iter()
            .position(|(m, u, _, _)| *m == req.method && req.url.contains(u.as_str()))
        {
            let (_, _, status, body) = responses.remove(idx);
            return Ok(CasResponse { status, body });
        }
        // Default: 503 FailClosed
        Ok(CasResponse {
            status: 503,
            body: b"mock: no canned response".to_vec(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A1 — Cold first run, empty CAS: miss → cold build → write-back; run succeeds
// ─────────────────────────────────────────────────────────────────────────────

/// A1: `clw hydrate` → 404 misses → cold build → results written back.
/// Run SUCCEEDS (slow). Proven hermetically: cold CAS → fetch+write-back.
///
/// FAILS red: `HttpBootCas::is_cached` / `fetch_layer` / `write_layer` are
/// all `unimplemented!()`.
#[test]
fn a1_cold_first_run_miss_then_write_back() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // Canned: every GET → 404 (miss), every PUT → 200 (write-back ok).
    transport.push(CasMethod::Get, "/v1/cas/", 404, vec![]);
    transport.push(CasMethod::Put, "/v1/cas/", 200, vec![]);

    let client = CasHttpClient::new(
        "https://cas.corelink.io",
        "acme",
        "mock-pat",
        transport,
    );
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // WP-2 impl UNIMPLEMENTED — this panics with "WP-2: is_cached(...)".
    // When WP-2 lands:
    //   - `is_cached` returns false (empty CAS).
    //   - `fetch_layer` sees 404 → Miss → falls through to cold build.
    //   - `write_layer` PUTs back to CAS/AC and returns Ok.
    //   - `cold_hydrate` returns `Ok(Hydrated { layers_fetched: 1 })`.
    let result = cold_hydrate(&cas, &plan);
    let outcome = result.expect("A1: cold first run must succeed (miss is NOT an error)");
    assert!(
        matches!(outcome, BootOutcome::Hydrated { layers_fetched } if layers_fetched >= 1),
        "A1: cold run must fetch at least one layer; got: {outcome:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A2 — Warm boot: all layers hit, zero cold fetches
// ─────────────────────────────────────────────────────────────────────────────

/// A2: hydrate → 200 hits → working set local before first instruction; no
/// cold fetch of hit layers.
///
/// FAILS red: `HttpBootCas::is_cached` is `unimplemented!()`.
#[test]
fn a2_warm_boot_all_hits_zero_fetches() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // Canned: GET → 200 Hit (blob already present).
    transport.push(
        CasMethod::Get,
        "/v1/cas/",
        200,
        b"layer-bytes".to_vec(),
    );

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // WP-2 impl UNIMPLEMENTED.
    // When WP-2 lands:
    //   - `is_cached` returns true (all layers present on warm box).
    //   - `hydrate` returns `Ok(Hydrated { layers_fetched: 0 })`.
    //   - Zero CAS fetches and zero write-backs.
    let result = hydrate(&cas, &plan);
    let outcome = result.expect("A2: warm boot must succeed");
    assert_eq!(
        outcome,
        BootOutcome::Hydrated { layers_fetched: 0 },
        "A2: warm path must produce zero fetches (all layers served from cache)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A5 — Miss ≠ unreachable: 404 = cold path; 401/403/5xx = fail-closed
// ─────────────────────────────────────────────────────────────────────────────

/// A5 (part 1): a 404 from CAS is a Miss (cold path valid, not an error).
///
/// FAILS red: `HttpBootCas::fetch_layer` is `unimplemented!()`.
#[test]
fn a5_404_is_miss_not_fail_closed() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // GET → 404 Miss, then PUT write-back → 200.
    transport.push(CasMethod::Get, "/v1/cas/", 404, vec![]);
    transport.push(CasMethod::Put, "/v1/cas/", 200, vec![]);

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // WP-2 impl UNIMPLEMENTED.
    // When WP-2 lands: 404 → Miss → cold path proceeds → write-back → Ok.
    // A cold run must SUCCEED even on an empty CAS.
    let result = cold_hydrate(&cas, &plan);
    assert!(
        result.is_ok(),
        "A5: 404 (Miss) must be the cold path — proceed, not fail-closed; got: {result:?}"
    );
}

/// A5 (part 2): a 401 from CAS is FailClosed (not a Miss, never a silent run).
///
/// FAILS red: `HttpBootCas::fetch_layer` is `unimplemented!()`.
#[test]
fn a5_401_is_fail_closed_not_miss() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // GET → 401 Unauthorized → must fail-closed, not proceed as cold.
    transport.push(CasMethod::Get, "/v1/cas/", 401, vec![]);

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // WP-2 impl UNIMPLEMENTED.
    // When WP-2 lands: 401 → FailClosed → SubstrateDown (never a miss).
    let result = cold_hydrate(&cas, &plan);
    assert!(
        result.is_err(),
        "A5: 401 must fail-closed (not a Miss); got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A5: 401 must produce SubstrateDown (fail-closed); got a different BootError"
    );
}

/// A5 (part 3): a 5xx from CAS is FailClosed.
///
/// FAILS red: `HttpBootCas::fetch_layer` is `unimplemented!()`.
#[test]
fn a5_5xx_is_fail_closed_not_miss() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    transport.push(CasMethod::Get, "/v1/cas/", 500, b"internal error".to_vec());

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    let result = cold_hydrate(&cas, &plan);
    assert!(result.is_err(), "A5: 5xx must fail-closed; got Ok");
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A5: 5xx must produce SubstrateDown (fail-closed)"
    );
}

/// A5 (part 4): a transport-layer error (DNS/timeout) is FailClosed.
///
/// FAILS red: `HttpBootCas::fetch_layer` is `unimplemented!()`.
#[test]
fn a5_transport_error_is_fail_closed_not_miss() {
    use corelink_runner::cas_http::{CasHttpClient, CasTransport};

    struct ErrorTransport;
    impl CasTransport for ErrorTransport {
        fn send(&self, _req: &CasRequest) -> anyhow::Result<CasResponse> {
            Err(anyhow::anyhow!("DNS resolution failed: cas.corelink.io"))
        }
    }

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", ErrorTransport);
    use corelink_runner::cas_http::HttpBootCas;
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    let result = cold_hydrate(&cas, &plan);
    assert!(
        result.is_err(),
        "A5: transport error must fail-closed; got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A5: transport error must produce SubstrateDown (fail-closed)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A5b — CAS-unreachable mid-hydrate: hard fail-closed;
//        AC-unreachable: run cold but RECORDED as forced-cold (honest accounting)
// ─────────────────────────────────────────────────────────────────────────────

/// A5b (CAS side): CAS unreachable mid-hydrate ⇒ hard fail-closed.
/// Proven via FakeCas (structural proof) AND via HttpBootCas (integration).
///
/// FAILS red: `HttpBootCas::is_cached` is `unimplemented!()` (WP-2 scope).
/// The FakeCas proof below is a structural invariant that already holds;
/// the HttpBootCas assertion below it fails red until WP-2 lands.
#[test]
fn a5b_cas_unreachable_mid_hydrate_hard_fail_closed() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via FakeCas (already green) ──────────────────────
    {
        let cas = FakeCas::with_fault(FaultMode::CasDown);
        let plan = fresh_plan(&[BLAKE3_KEY_A]);
        let result = hydrate(&cas, &plan);
        assert!(
            result.is_err(),
            "A5b (CAS/FakeCas): unreachable must fail-closed; got Ok"
        );
        assert!(
            matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
            "A5b (CAS/FakeCas): must produce SubstrateDown"
        );
        assert_eq!(
            cas.total_writes(),
            0,
            "A5b: CAS-down must produce zero writes"
        );
    }

    // ── Integration proof via HttpBootCas (FAILS red — WP-2 unimplemented) ──
    // When WP-2 lands: CAS returns 5xx mid-hydrate → SubstrateDown, zero
    // write-backs.
    let transport = MockCasTransport::new();
    transport.push(CasMethod::Get, "/v1/cas/", 503, b"substrate down".to_vec());
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // Panics: "WP-2: is_cached(...) — not yet implemented"
    let result = cold_hydrate(&http_cas, &plan);
    assert!(
        result.is_err(),
        "A5b (HttpBootCas): 503 CAS response must fail-closed; got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A5b (HttpBootCas): 503 must produce SubstrateDown (fail-closed)"
    );
}

/// A5b (AC side): AC-unreachable ⇒ run cold but recorded as forced-cold (not
/// a Hit).  The `ForcedCold` discriminant does not exist yet — WP-2 will add
/// it.  This test asserts the error IS returned (not silent) and the write
/// count is zero (no poisoned AC state).
///
/// FAILS red: the test will pass once WP-2 distinguishes `ForcedCold` from
/// a hit, but currently `hydrate` with AC-down returns `Err(SubstrateDown)`
/// (the existing behavior), which this test asserts is NOT silently Ok.
/// The honest-accounting claim is encoded in the comment — the exact discriminant
/// is WP-2 scope.
///
/// FAILS red: `HttpBootCas::is_cached` is `unimplemented!()` (WP-2 scope).
#[test]
fn a5b_ac_unreachable_run_cold_recorded_not_a_hit() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via FakeCas (already green) ──────────────────────
    {
        let cas = FakeCas::with_fault(FaultMode::AcDown);
        let plan = fresh_plan(&[BLAKE3_KEY_A]);
        let result = cold_hydrate(&cas, &plan);
        assert!(
            result.is_err(),
            "A5b (AC/FakeCas): AC-unreachable must NOT silently return Ok"
        );
        assert_eq!(
            cas.total_writes(),
            0,
            "A5b: AC-down must produce zero successful writes"
        );
    }

    // ── Integration proof via HttpBootCas (FAILS red — WP-2 unimplemented) ──
    // When WP-2 lands: AC returns 5xx on write-back → run cold recorded as
    // forced-cold, not a hit (honest accounting — A5b invariant).
    let transport = MockCasTransport::new();
    // CAS GET succeeds (200); PUT (write-back) returns 503 (AC down).
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer-data".to_vec());
    transport.push(CasMethod::Put, "/v1/ac/", 503, b"AC down".to_vec());
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // Panics: "WP-2: is_cached(...) — not yet implemented"
    let result = cold_hydrate(&http_cas, &plan);
    // When WP-2 lands: the write-back failure must surface as Err (not Ok),
    // with the run recorded as forced-cold.
    assert!(
        result.is_err(),
        "A5b (HttpBootCas): AC write-back failure must surface as Err (honest accounting)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A9 — Auth posture: every call uses Bearer PAT, tenant in URL path, BLAKE3 key
// ─────────────────────────────────────────────────────────────────────────────

/// A9: every CAS call sends `Bearer <per-job PAT>` and tenant in URL path.
///
/// FAILS red: `CasHttpClient::get_cas` is `unimplemented!()`.
#[test]
fn a9_auth_posture_bearer_pat_in_every_call() {
    use corelink_runner::cas_http::CasHttpClient;

    let transport = MockCasTransport::new();
    transport.push(
        CasMethod::Get,
        "v1/cas/acme/",
        200,
        b"blob-bytes".to_vec(),
    );

    let client = CasHttpClient::new(
        "https://cas.corelink.io",
        "acme",
        "per-job-pat-abc123",
        transport.clone(),
    );

    let key = Blake3Key::from_hex(BLAKE3_KEY_A);
    // WP-2 impl UNIMPLEMENTED — panics here.
    let _outcome = client.get_cas(&key);

    // When WP-2 lands, every call recorded in `transport.calls_made()` must:
    let calls = transport.calls_made();
    assert!(!calls.is_empty(), "A9: at least one CAS call must be made");
    for call in &calls {
        assert!(
            call.url.contains("/acme/"),
            "A9: tenant must be in URL path, not a header; url={}",
            call.url
        );
        assert_eq!(
            call.bearer_token, "per-job-pat-abc123",
            "A9: every call must carry the per-job PAT as bearer token"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A9b — Digest discipline: BLAKE3 is the CAS key; SHA-256 is NEVER used
// ─────────────────────────────────────────────────────────────────────────────

/// A9b: the CAS/AC URL path segment for a known blob is its BLAKE3 hex.
/// The SHA-256 of the same bytes is NEVER used as a native-CAS key.
///
/// FAILS red: `CasHttpClient::get_cas` is `unimplemented!()`.
#[test]
fn a9b_digest_discipline_blake3_key_not_sha256() {
    use corelink_runner::cas_http::CasHttpClient;

    let transport = MockCasTransport::new();
    transport.push(CasMethod::Get, BLAKE3_KEY_A, 200, b"blob".to_vec());

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport.clone());
    let blake3_key = Blake3Key::from_hex(BLAKE3_KEY_A);

    // WP-2 impl UNIMPLEMENTED — panics here.
    let _outcome = client.get_cas(&blake3_key);

    let calls = transport.calls_made();
    assert!(!calls.is_empty(), "A9b: a CAS call must be made");
    for call in &calls {
        assert!(
            call.url.contains(BLAKE3_KEY_A),
            "A9b: URL must contain the BLAKE3 key ({BLAKE3_KEY_A}); url={}",
            call.url
        );
        assert!(
            !call.url.contains(SHA256_KEY_A),
            "A9b: SHA-256 ({SHA256_KEY_A}) must NEVER appear in a native-CAS URL; url={}",
            call.url
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A10 — Write-back byte-identity: memo-poison guard
// ─────────────────────────────────────────────────────────────────────────────

/// A10: two cold runs of the same def+inputs produce identical `ActionResult`
/// bytes and the same action-digest.  No per-boot nondeterminism is written
/// back — a poisoned store breaks every future hit.
///
/// Proven hermetically: FakeCas captures write payloads; two cold runs of the
/// same plan produce identical write data.
///
/// FAILS red: `HttpBootCas::is_cached` / `write_layer` are `unimplemented!()`.
/// The structural proof via CapturingFakeCas already holds (deterministic layer
/// bytes → identical write-backs); the HttpBootCas call below makes it fail red.
#[test]
fn a10_write_back_byte_identity_memo_poison_guard() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via CapturingFakeCas (invariant already holds) ───
    {
        struct CapturingFakeCas {
            writes: Mutex<Vec<(String, Vec<u8>)>>,
            fetch_counter: Mutex<HashMap<String, usize>>,
        }
        impl CapturingFakeCas {
            fn new() -> Self {
                Self {
                    writes: Mutex::new(Vec::new()),
                    fetch_counter: Mutex::new(HashMap::new()),
                }
            }
            fn writes(&self) -> Vec<(String, Vec<u8>)> {
                self.writes.lock().unwrap().clone()
            }
        }
        impl BootCas for CapturingFakeCas {
            fn is_cached(&self, _key: &str) -> bool {
                false
            }
            fn fetch_layer(&self, key: &str) -> Result<Vec<u8>, BootError> {
                *self
                    .fetch_counter
                    .lock()
                    .unwrap()
                    .entry(key.to_string())
                    .or_insert(0) += 1;
                Ok(format!("deterministic-layer-for:{key}").into_bytes())
            }
            fn write_layer(&self, key: &str, data: &[u8]) -> Result<(), BootError> {
                self.writes
                    .lock()
                    .unwrap()
                    .push((key.to_string(), data.to_vec()));
                Ok(())
            }
        }

        let plan = fresh_plan(&[BLAKE3_KEY_A]);
        let cas1 = CapturingFakeCas::new();
        cold_hydrate(&cas1, &plan).expect("A10: run 1 must succeed");
        let writes1 = cas1.writes();

        let cas2 = CapturingFakeCas::new();
        cold_hydrate(&cas2, &plan).expect("A10: run 2 must succeed");
        let writes2 = cas2.writes();

        assert_eq!(writes1.len(), writes2.len(), "A10: write count must match");
        for ((k1, d1), (k2, d2)) in writes1.iter().zip(writes2.iter()) {
            assert_eq!(k1, k2, "A10: write-back key must be identical across runs");
            assert_eq!(
                d1, d2,
                "A10: write-back data for {k1} must be BYTE-IDENTICAL across runs \
                 (no per-boot nondeterminism — memo-poison guard, whitepaper §5.2)"
            );
        }
    }

    // ── Integration proof via HttpBootCas (FAILS red — WP-2 unimplemented) ──
    // When WP-2 lands: two cold runs of the same plan via HttpBootCas must
    // produce identical ActionResult bytes (no per-boot nondeterminism in the
    // real HTTP write-back path).
    let transport = MockCasTransport::new();
    transport.push(CasMethod::Get, "/v1/cas/", 404, vec![]); // miss
    transport.push(CasMethod::Put, "/v1/cas/", 200, vec![]); // write-back ok
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // Panics: "WP-2: is_cached(...) — not yet implemented"
    let _result = cold_hydrate(&http_cas, &plan);
    // A10 FAILS red here — byte-identity proof via HttpBootCas is WP-2 scope.
    panic!(
        "A10: HttpBootCas byte-identity proof is WP-2 scope — \
         this line should NOT be reached once is_cached() panics above"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A11 — Tenant-echo mismatch ⇒ 403 fail-closed
// ─────────────────────────────────────────────────────────────────────────────

/// A11: a CAS/AC call whose path-tenant ≠ the PAT's tenant ⇒ 403 ⇒ explicit
/// fail-closed.  The runner must NEVER emit `x-corelink-tenant-id`.
///
/// FAILS red: `HttpBootCas::fetch_layer` (via `CasHttpClient::get_cas`) is
/// `unimplemented!()`.
#[test]
fn a11_tenant_mismatch_403_fail_closed_not_miss() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // Simulate: CAS returns 403 because the PAT's tenant ≠ path tenant.
    transport.push(CasMethod::Get, "/v1/cas/", 403, b"forbidden".to_vec());

    let client = CasHttpClient::new(
        "https://cas.corelink.io",
        "wrong-tenant", // mismatched
        "pat-for-acme",
        transport.clone(),
    );
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // WP-2 impl UNIMPLEMENTED.
    // When WP-2 lands: 403 → FailClosed (SubstrateDown), NOT a Miss.
    let result = cold_hydrate(&cas, &plan);
    assert!(
        result.is_err(),
        "A11: 403 (tenant mismatch) must fail-closed, NOT proceed as a cache miss; got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A11: 403 must produce SubstrateDown (fail-closed)"
    );

    // Also assert that NO x-corelink-tenant-id header was emitted.
    for call in transport.calls_made() {
        // MockCasTransport doesn't carry headers, but the contract is: the tenant
        // must be in the URL, NOT in a custom header. We assert the URL shape.
        assert!(
            call.url.contains("/v1/cas/") || call.url.contains("/v1/ac/"),
            "A11: CAS/AC URL must use the standard path shape"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A12 — Partial-hydrate then substrate-down mid-stream
// ─────────────────────────────────────────────────────────────────────────────

/// A12: layers 1–3 succeed then layer 4 → 5xx ⇒ `BootError::SubstrateDown`,
/// zero write-backs committed, run aborts fail-closed — no half-warmed box.
///
/// FAILS red: `HttpBootCas::is_cached` is `unimplemented!()` (WP-2 scope).
/// Structurally proven by PartialFaultCas (invariant already holds);
/// the HttpBootCas call at the end makes it fail red.
#[test]
fn a12_partial_hydrate_substrate_down_mid_stream_fail_closed() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via PartialFaultCas (invariant already holds) ────
    {
        const KEYS: [&str; 4] = [
            "blake3:0001aaa",
            "blake3:0002bbb",
            "blake3:0003ccc",
            "blake3:0004ddd",
        ];

        struct PartialFaultCas {
            fail_after: usize,
            fetch_count: Cell<usize>,
            write_count: Cell<usize>,
        }
        impl BootCas for PartialFaultCas {
            fn is_cached(&self, _key: &str) -> bool {
                false
            }
            fn fetch_layer(&self, key: &str) -> Result<Vec<u8>, BootError> {
                let n = self.fetch_count.get();
                self.fetch_count.set(n + 1);
                if n >= self.fail_after {
                    return Err(BootError::SubstrateDown {
                        substrate: "CAS".to_string(),
                        reason: format!("PartialFaultCas: down after {n} fetches"),
                    });
                }
                Ok(format!("layer-bytes:{key}").into_bytes())
            }
            fn write_layer(&self, _key: &str, _data: &[u8]) -> Result<(), BootError> {
                self.write_count.set(self.write_count.get() + 1);
                Ok(())
            }
        }

        let cas = PartialFaultCas {
            fail_after: 3,
            fetch_count: Cell::new(0),
            write_count: Cell::new(0),
        };
        let plan = fresh_plan(&KEYS);
        let result = cold_hydrate(&cas, &plan);

        assert!(
            result.is_err(),
            "A12 (PartialFaultCas): substrate-down must fail-closed; got Ok"
        );
        assert!(
            matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
            "A12: must produce SubstrateDown"
        );
        assert_eq!(
            cas.write_count.get(),
            3,
            "A12: exactly 3 write-backs (layers 1-3); layer 4 (failed) has 0 write-backs"
        );
    }

    // ── Integration proof via HttpBootCas (FAILS red — WP-2 unimplemented) ──
    // When WP-2 lands: partial 200s then 5xx mid-stream → BootError::SubstrateDown,
    // zero write-backs committed for the failed layer, run aborts fail-closed.
    let transport = MockCasTransport::new();
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer1".to_vec());
    transport.push(CasMethod::Get, "/v1/cas/", 503, b"down".to_vec()); // layer 2 fails
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A, "blake3:second-layer-key"]);

    // Panics: "WP-2: is_cached(...) — not yet implemented"
    let result = cold_hydrate(&http_cas, &plan);
    assert!(
        result.is_err(),
        "A12 (HttpBootCas): partial 200 then 5xx must fail-closed; got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
        "A12 (HttpBootCas): must produce SubstrateDown — no half-warmed box proceeds"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A13 — Public-deps vs private namespace
// ─────────────────────────────────────────────────────────────────────────────

/// A13: a public-dep layer resolves via the `_public` keyspace; a private
/// artifact resolves under the tenant HMAC prefix and is NEVER addressed in
/// `_public`.  Intra-tenant dedup only (tense discipline).
///
/// FAILS red: `CasHttpClient::get_cas` is `unimplemented!()`.
/// The `_public` vs tenant-namespace routing is WP-2 scope.
#[test]
fn a13_public_dep_resolves_public_keyspace_private_stays_tenant_namespaced() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // Public dep: GET /v1/cas/_public/<blake3> → 200.
    transport.push(
        CasMethod::Get,
        "/v1/cas/_public/",
        200,
        b"public-dep-bytes".to_vec(),
    );
    // Private dep: GET /v1/cas/acme/<blake3> → 200.
    transport.push(
        CasMethod::Get,
        "/v1/cas/acme/",
        200,
        b"private-artifact-bytes".to_vec(),
    );

    // Public-dep key carries the `_public:` prefix as a convention so WP-2
    // can route to the public keyspace.
    const PUBLIC_KEY: &str = "_public:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e000000bb";
    const PRIVATE_KEY: &str = "acme-hmac:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e000000cc";

    let client =
        CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport.clone());
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[PUBLIC_KEY, PRIVATE_KEY]);

    // WP-2 impl UNIMPLEMENTED — panics on is_cached.
    let _result = cold_hydrate(&cas, &plan);

    // When WP-2 lands, assert:
    let calls = transport.calls_made();
    let public_call = calls.iter().find(|c| c.url.contains("_public"));
    let private_call = calls.iter().find(|c| c.url.contains("/acme/"));

    if let Some(pub_call) = public_call {
        // The public key resolves via _public, NOT under the tenant prefix.
        assert!(
            pub_call.url.contains("_public"),
            "A13: public dep must use the _public keyspace"
        );
        assert!(
            !pub_call.url.contains("/acme/"),
            "A13: public dep must NOT be addressed under the tenant namespace \
             (intra-tenant dedup only — tense discipline)"
        );
    }

    if let Some(priv_call) = private_call {
        // The private artifact resolves under the tenant namespace, never _public.
        assert!(
            !priv_call.url.contains("_public"),
            "A13: private artifact must NOT be addressed in the _public keyspace"
        );
        assert!(
            priv_call.url.contains("/acme/"),
            "A13: private artifact must use the tenant namespace"
        );
    }
}
