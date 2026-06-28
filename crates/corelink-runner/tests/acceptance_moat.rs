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
use corelink_runner::cas_http::{Blake3Key, CasMethod, CasRequest, CasResponse, CasTransport};
use corelink_runners_contracts::FenceManifest;

// ─────────────────────────────────────────────────────────────────────────────
// Shared fixtures
// ─────────────────────────────────────────────────────────────────────────────

/// Fixed BLAKE3 hex (64 chars; 256-bit).  Used as the native CAS key (A9b).
const BLAKE3_KEY_A: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e9000000aa";

/// SHA-256 hex for the SAME conceptual blob.  Must NEVER appear in a CAS URL
/// as the key (A9b assertion).
const SHA256_KEY_A: &str = "0000000000000000000000000000000000000000000000000000000000000000aa";

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
#[allow(dead_code)] // `Ok` is the no-fault mode; variants kept for symmetry with acceptance_c3.
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

#[allow(dead_code)] // Helpers retained for test symmetry with acceptance_c3; not all used here.
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

    fn fetch_layer(&self, layer_key: &str) -> Result<Option<Vec<u8>>, BootError> {
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
            return Ok(Some(bytes.clone()));
        }
        Ok(Some(format!("layer-bytes:{layer_key}").into_bytes()))
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

/// Canned-response entry: (method, url-prefix, status, body).
type CannedResponse = (CasMethod, String, u16, Vec<u8>);

/// What the mock transport returns for a given `(method, url)` pair.
#[derive(Debug, Clone)]
struct MockCasTransport {
    responses: Arc<Mutex<Vec<CannedResponse>>>,
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

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "mock-pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    // A1 (post poison-fix): an empty CAS — every GET is a 404 MISS — hydrates
    // ZERO layers. The run SUCCEEDS (proceeds fully cold; "miss is NOT an error"),
    // and CRUCIALLY writes NOTHING back: a miss must not poison the CAS with an
    // empty-bytes sentinel nor false-mark the layer cached. (The old code wrote the
    // empty miss-bytes back and counted it as a fetched layer — the A1 poison bug.)
    let result = cold_hydrate(&cas, &plan);
    let outcome = result.expect("A1: cold first run must succeed (miss is NOT an error)");
    assert!(
        matches!(outcome, BootOutcome::Hydrated { layers_fetched: 0 }),
        "A1: empty CAS → 0 layers hydrated, run still succeeds; got: {outcome:?}"
    );
    // Poison guard: a 404 miss must produce ZERO write-back PUTs.
    let puts = cas
        .client
        .transport
        .calls_made()
        .iter()
        .filter(|c| c.method == CasMethod::Put)
        .count();
    assert_eq!(
        puts, 0,
        "A1 poison guard: a 404 miss must trigger NO write-back PUT"
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
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer-bytes".to_vec());

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

    let client = CasHttpClient::new(
        "https://cas.corelink.io",
        "acme",
        "mock-pat",
        ErrorTransport,
    );
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

/// A5b (HARDENED — cold-review finding): assert the `ForcedCold` discriminant.
///
/// The CHOSEN ASYMMETRY (A5b):
/// - CAS-unreachable mid-fetch (can't get layer) ⇒ hard fail-closed
///   (`Err(SubstrateDown)` — run cannot proceed without the layer).
/// - AC-unreachable on write-back (fetch succeeded, write failed) ⇒ run cold
///   RECORDED as `BootOutcome::ForcedCold` (NOT a `Hydrated` hit; honest
///   accounting — the runner never silently records a partial or poisoned store).
///
/// This test asserts the chosen asymmetry explicitly: CAS-down → error,
/// AC/write-back-down → Ok(ForcedCold).
#[test]
fn a5b_ac_unreachable_run_cold_recorded_not_a_hit() {
    use corelink_runner::boot::BootOutcome;
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via FakeCas — AC-unreachable → ForcedCold ───────
    // A5b HARDENED: AC-down on write-back → Ok(ForcedCold), NOT is_err().
    {
        let cas = FakeCas::with_fault(FaultMode::AcDown);
        let plan = fresh_plan(&[BLAKE3_KEY_A]);
        let result = cold_hydrate(&cas, &plan);
        // AC-unreachable on write-back → ForcedCold (not an error, honest accounting).
        assert!(
            result.is_ok(),
            "A5b (AC/FakeCas): AC-unreachable on write-back must be Ok(ForcedCold), \
             not an error (honest accounting — the fetch succeeded)"
        );
        assert!(
            matches!(result.unwrap(), BootOutcome::ForcedCold { .. }),
            "A5b (AC/FakeCas): must produce ForcedCold — NOT Hydrated \
             (write-back failed; result not stored; NOT a cache hit)"
        );
        assert_eq!(
            cas.total_writes(),
            0,
            "A5b: AC-down must produce zero successful writes (no poisoned store)"
        );
    }

    // ── Integration proof via HttpBootCas ────────────────────────────────
    // AC write-back returns 503 → run cold recorded as ForcedCold (honest accounting).
    let transport = MockCasTransport::new();
    // CAS GET succeeds (200 Hit); write-back PUT returns 503 (AC down).
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer-data".to_vec());
    transport.push(CasMethod::Put, "/v1/cas/", 503, b"AC down".to_vec());
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    let result = cold_hydrate(&http_cas, &plan);
    // A5b HARDENED: write-back failure → Ok(ForcedCold), not an error.
    // The run proceeds (fetch succeeded), but is RECORDED as forced-cold
    // (the result is not stored; it is NOT a cache hit — honest accounting).
    assert!(
        result.is_ok(),
        "A5b (HttpBootCas): AC write-back failure must be Ok(ForcedCold), \
         not an error (fetch succeeded; honest accounting)"
    );
    assert!(
        matches!(result.unwrap(), BootOutcome::ForcedCold { .. }),
        "A5b (HttpBootCas): must produce ForcedCold — NOT Hydrated \
         (write-back failed; A5b asymmetry: AC-unreachable ≠ CAS-unreachable)"
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
    transport.push(CasMethod::Get, "v1/cas/acme/", 200, b"blob-bytes".to_vec());

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

/// A10 (HARDENED — cold-review finding): assert byte-identity at the
/// `CasHttpClient::put_cas`/`put_ac` BODY level (two runs of the same input ⇒
/// identical PUT bytes), not just the `cold_hydrate` pass-through.
///
/// Two cold runs of the same def+inputs produce identical action-digest AND
/// byte-identical stored `ActionResult`.  No per-boot nondeterminism is written
/// back — a poisoned store breaks every future hit (whitepaper §5.2).
///
/// Structural proof (CapturingFakeCas) + integration proof (MockCasTransport PUT
/// body inspection).  Both must pass.
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
            fn fetch_layer(&self, key: &str) -> Result<Option<Vec<u8>>, BootError> {
                *self
                    .fetch_counter
                    .lock()
                    .unwrap()
                    .entry(key.to_string())
                    .or_insert(0) += 1;
                Ok(Some(format!("deterministic-layer-for:{key}").into_bytes()))
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

    // ── Integration proof via HttpBootCas: byte-identity at the PUT body level ──
    // Two cold runs of the same plan via HttpBootCas must produce identical
    // PUT body bytes (no per-boot nondeterminism in the real HTTP write-back path).
    // This is the HARDENED assertion: inspect the CasRequest.body of the PUT call,
    // not just the cold_hydrate pass-through (cold-review finding, WP-2-final).
    {
        // Run 1: cold, CAS HIT (200 + deterministic bytes) → a REAL write-back.
        // The A10 invariant is byte-identity of the written-back CONTENT, which
        // only exists on a hit — a 404 miss now writes nothing (the poison fix).
        let transport1 = MockCasTransport::new();
        transport1.push(
            CasMethod::Get,
            "/v1/cas/",
            200,
            b"toolchain-layer-A-content".to_vec(),
        );
        transport1.push(CasMethod::Put, "/v1/cas/", 200, vec![]);
        let client1 = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport1);
        let http_cas1 = HttpBootCas::new(client1);
        let plan = fresh_plan(&[BLAKE3_KEY_A]);
        cold_hydrate(&http_cas1, &plan).expect("A10: run 1 (HttpBootCas) must succeed");
        let calls1 = http_cas1.client.transport.calls_made();
        let put_bodies1: Vec<Vec<u8>> = calls1
            .iter()
            .filter(|c| c.method == CasMethod::Put)
            .map(|c| c.body.clone())
            .collect();
        assert!(
            !put_bodies1.is_empty(),
            "A10: run 1 must produce at least one PUT (write-back)"
        );

        // Run 2: same input — same HIT bytes → byte-identical write-back.
        let transport2 = MockCasTransport::new();
        transport2.push(
            CasMethod::Get,
            "/v1/cas/",
            200,
            b"toolchain-layer-A-content".to_vec(),
        );
        transport2.push(CasMethod::Put, "/v1/cas/", 200, vec![]);
        let client2 = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport2);
        let http_cas2 = HttpBootCas::new(client2);
        cold_hydrate(&http_cas2, &plan).expect("A10: run 2 (HttpBootCas) must succeed");
        let calls2 = http_cas2.client.transport.calls_made();
        let put_bodies2: Vec<Vec<u8>> = calls2
            .iter()
            .filter(|c| c.method == CasMethod::Put)
            .map(|c| c.body.clone())
            .collect();

        // A10 HARDENED: byte-identity at the PUT body level (memo-poison guard).
        // Two cold runs of the same input ⇒ identical PUT bytes.
        assert_eq!(
            put_bodies1.len(),
            put_bodies2.len(),
            "A10: PUT count must match across runs"
        );
        for (b1, b2) in put_bodies1.iter().zip(put_bodies2.iter()) {
            assert_eq!(
                b1, b2,
                "A10: PUT body MUST be BYTE-IDENTICAL across cold runs of the same input \
                 (no per-boot nondeterminism in write-back — memo-poison guard, whitepaper §5.2)"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A10b — write-path fail-open guard (audit round 2): a PUT-404 is an ERROR
// ─────────────────────────────────────────────────────────────────────────────

/// A write-back PUT that returns 404 is an ERROR (unknown tenant/bucket/route —
/// a misconfig), NOT a cache "miss": it must NOT be treated as a successful write.
/// `cold_hydrate` records `ForcedCold` (the write failed → not a hit) and the layer
/// is NEVER marked cached. (The old code mapped PUT-404 → Miss → success and
/// false-marked it cached — a silent write-path fail-open.)
#[test]
fn a10b_put_404_write_back_is_fail_closed_not_silent_success() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // GET hit → a write-back is attempted; PUT 404 → the target is not found.
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer-bytes".to_vec());
    transport.push(CasMethod::Put, "/v1/cas/", 404, vec![]);
    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A]);

    let outcome =
        cold_hydrate(&cas, &plan).expect("the run proceeds (forced-cold), not a hard error");
    assert!(
        matches!(outcome, BootOutcome::ForcedCold { .. }),
        "a PUT-404 write-back must force-cold (the write FAILED), never silently \
         succeed; got: {outcome:?}"
    );
    // And the layer must NOT be marked cached after a failed write.
    assert!(
        !cas.is_cached(BLAKE3_KEY_A),
        "a failed (404) write-back must NOT mark the layer cached (fail-open guard)"
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

    // A11 HARDENED (cold-review finding): assert `x-corelink-tenant-id` is NEVER
    // emitted on ANY request.  The `CasRequest.headers` field carries all
    // additional headers; it must be empty or must not contain that key.
    for call in transport.calls_made() {
        // The tenant must be in the URL path, NOT in a custom header.
        assert!(
            call.url.contains("/v1/cas/") || call.url.contains("/v1/ac/"),
            "A11: CAS/AC URL must use the standard path shape"
        );
        // HARDENED: check the headers field directly.
        // `x-corelink-tenant-id` is server-trusted only; the runner MUST NOT emit it.
        for (header_name, _header_val) in &call.headers {
            assert_ne!(
                header_name.to_lowercase(),
                "x-corelink-tenant-id",
                "A11: the runner MUST NEVER emit x-corelink-tenant-id \
                 (server-trusted header — stripped at the edge; tenant is in the URL path)"
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A12 — Partial-hydrate then substrate-down mid-stream
// ─────────────────────────────────────────────────────────────────────────────

/// A12 (HARDENED — cold-review finding): assert fail-closed ABORT
/// (`is_err()`/`SubstrateDown`) on mid-hydrate substrate-down.
///
/// The assertion is: the result is `Err(SubstrateDown)` — NOT a `Hydrated`
/// outcome. Partial valid content-addressed layer writes ARE acceptable (the
/// CAS is content-addressed; a partial prior write of layers 1–N is harmless
/// and idempotent on retry). We do NOT assert `write_count == 0`: asserting
/// zero writes would be too strict for the mid-stream case.
#[test]
fn a12_partial_hydrate_substrate_down_mid_stream_fail_closed() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    // ── Structural proof via PartialFaultCas ─────────────────────────────
    // A12 HARDENED: assert is_err()/SubstrateDown — NOT zero writes.
    // Partial writes of earlier layers (1–3) are content-addressed and
    // idempotent; only the failed layer is absent. The key invariant is:
    // no half-warmed box proceeds as if cold after a mid-stream failure.
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
            fn fetch_layer(&self, key: &str) -> Result<Option<Vec<u8>>, BootError> {
                let n = self.fetch_count.get();
                self.fetch_count.set(n + 1);
                if n >= self.fail_after {
                    return Err(BootError::SubstrateDown {
                        substrate: "CAS".to_string(),
                        reason: format!("PartialFaultCas: down after {n} fetches"),
                    });
                }
                Ok(Some(format!("layer-bytes:{key}").into_bytes()))
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

        // A12 HARDENED: assert fail-closed ABORT (is_err/SubstrateDown),
        // NOT a Hydrated outcome.  Partial writes (layers 1–3 here) are
        // content-addressed and acceptable; layer 4 failed → hard abort.
        assert!(
            result.is_err(),
            "A12 (PartialFaultCas): substrate-down must fail-closed; got Ok"
        );
        assert!(
            matches!(result.unwrap_err(), BootError::SubstrateDown { .. }),
            "A12: must produce SubstrateDown — no Hydrated outcome on mid-stream failure"
        );
        // Note: write_count may be 3 (layers 1–3 wrote before failure), and
        // that is CORRECT: partial content-addressed layer writes are valid and
        // idempotent.  We do NOT assert write_count == 0.
    }

    // ── Integration proof via HttpBootCas ────────────────────────────────
    // Partial 200s then 5xx mid-stream → BootError::SubstrateDown,
    // run aborts fail-closed — no half-warmed box proceeds as if cold.
    let transport = MockCasTransport::new();
    // cold_hydrate does NOT call is_cached — it calls fetch_layer directly.
    // Layer 1: fetch GET → 200 (success), write PUT → 200 (success).
    transport.push(CasMethod::Get, "/v1/cas/", 200, b"layer1".to_vec());
    transport.push(CasMethod::Put, "/v1/cas/", 200, vec![]);
    // Layer 2: fetch GET → 503 (substrate down mid-stream — hard fail-closed).
    transport.push(CasMethod::Get, "/v1/cas/", 503, b"down".to_vec());

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport);
    let http_cas = HttpBootCas::new(client);
    let plan = fresh_plan(&[BLAKE3_KEY_A, "blake3:second-layer-key"]);

    let result = cold_hydrate(&http_cas, &plan);
    // A12 HARDENED: fail-closed ABORT (is_err/SubstrateDown), NOT Hydrated.
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
    // Write-back for public dep: PUT /v1/cas/_public/<blake3(data)> → 200.
    transport.push(CasMethod::Put, "/v1/cas/_public/", 200, vec![]);
    // Private dep: GET /v1/cas/acme/<blake3> → 200.
    transport.push(
        CasMethod::Get,
        "/v1/cas/acme/",
        200,
        b"private-artifact-bytes".to_vec(),
    );
    // Write-back for private dep: PUT /v1/cas/acme/<blake3(data)> → 200.
    transport.push(CasMethod::Put, "/v1/cas/acme/", 200, vec![]);

    // Public-dep key carries the `_public:` prefix as a convention so WP-2
    // can route to the public keyspace.
    const PUBLIC_KEY: &str =
        "_public:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e000000bb";
    const PRIVATE_KEY: &str =
        "acme-hmac:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e000000cc";

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport.clone());
    // Public routing is FAIL-SAFE OFF by default; the fabric enables it for
    // layers whose public provenance it has vouched for (the WP-8 plan-builder).
    // This test exercises that enabled path; the forged-prefix default-deny path
    // is proven in `a13_adversarial_forged_public_prefix_does_not_cross_tenant`.
    let cas = HttpBootCas::new(client).with_public_routing();
    let plan = fresh_plan(&[PUBLIC_KEY, PRIVATE_KEY]);

    // Both layers fetched+written; the routing is the load-bearing assertion.
    let _result = cold_hydrate(&cas, &plan);

    // Assert public-vs-private routing — use .expect() so the assertion is
    // LOAD-BEARING: if the call is missing, the test panics (not silently skips).
    // A13 HARDENED (cold-review finding): replace `if let Some(..)` with
    // `.expect(..)` so missing routing is a test failure, not a no-op.
    let calls = transport.calls_made();
    let public_call = calls.iter().find(|c| c.url.contains("_public")).expect(
        "A13: a CAS call for the public-dep layer must be made \
             (public key → _public keyspace routing)",
    );
    let private_call = calls.iter().find(|c| c.url.contains("/acme/")).expect(
        "A13: a CAS call for the private-artifact layer must be made \
             (private key → tenant namespace routing)",
    );

    // Public key resolves via _public, NOT under the tenant prefix.
    assert!(
        public_call.url.contains("_public"),
        "A13: public dep must use the _public keyspace; url={}",
        public_call.url
    );
    assert!(
        !public_call.url.contains("/acme/"),
        "A13: public dep must NOT be addressed under the tenant namespace \
         (intra-tenant dedup only — tense discipline); url={}",
        public_call.url
    );

    // Private artifact resolves under the tenant namespace, never _public.
    assert!(
        !private_call.url.contains("_public"),
        "A13: private artifact must NOT be addressed in the _public keyspace; url={}",
        private_call.url
    );
    assert!(
        private_call.url.contains("/acme/"),
        "A13: private artifact must use the tenant namespace; url={}",
        private_call.url
    );
}

/// A13-adversarial (SECURITY): a runner/job-FORGED `_public:` layer key must
/// NEVER reach the shared cross-tenant `_public` keyspace when the fabric has
/// not enabled public routing. Default-deny: `HttpBootCas::new` (no
/// `with_public_routing`) treats a `_public:` prefix as an opaque tenant key, so
/// every call routes to the tenant namespace (inert → miss → cold), not
/// `_public`.
///
/// This is the fail-safe that closes the `route_key` string-prefix provenance
/// gap until WP-8 lands typed, fabric-set public provenance at the plan-builder.
#[test]
fn a13_adversarial_forged_public_prefix_does_not_cross_tenant() {
    use corelink_runner::cas_http::{CasHttpClient, HttpBootCas};

    let transport = MockCasTransport::new();
    // With public routing OFF (default), the forged key routes to the TENANT
    // namespace — so the transport serves /v1/cas/acme/ for fetch + write-back.
    transport.push(
        CasMethod::Get,
        "/v1/cas/acme/",
        200,
        b"forged-bytes".to_vec(),
    );
    transport.push(CasMethod::Put, "/v1/cas/acme/", 200, vec![]);

    // A job-FORGED public key: it carries the `_public:` prefix, but the fabric
    // did NOT vouch for it (default-deny HttpBootCas — no with_public_routing()).
    const FORGED_PUBLIC_KEY: &str =
        "_public:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e000000bb";

    let client = CasHttpClient::new("https://cas.corelink.io", "acme", "pat", transport.clone());
    let cas = HttpBootCas::new(client); // default-DENY: no with_public_routing()
    let plan = fresh_plan(&[FORGED_PUBLIC_KEY]);

    let _result = cold_hydrate(&cas, &plan);

    let calls = transport.calls_made();
    assert!(
        !calls.is_empty(),
        "A13-adversarial: the forged-public layer must still be fetched (cold path)"
    );
    for c in &calls {
        // SECURITY: a forged prefix must NEVER reach the shared cross-tenant keyspace.
        assert!(
            !c.url.contains("/v1/cas/_public/"),
            "A13-adversarial: a FORGED `_public:` key must NEVER route to the shared \
             _public cross-tenant keyspace when public routing is off (fail-safe); url={}",
            c.url
        );
        // It routes to the tenant namespace instead (inert).
        assert!(
            c.url.contains("/v1/cas/acme/"),
            "A13-adversarial: a forged `_public:` key must route to the tenant \
             namespace (inert), not _public; url={}",
            c.url
        );
    }
}
