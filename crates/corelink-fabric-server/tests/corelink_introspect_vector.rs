//! Consumer golden for the RATIFIED `corelink-introspect.json` conformance
//! vector (auth/billing drift tripwire).
//!
//! The contracts crate proves the vector's BYTES (SHA-256 + manifest
//! membership). This suite proves the CONSUMER: it feeds each of the four
//! ratified introspect response cases through the real `CoreLinkTokenStore`
//! (tenant resolution) and `CoreLinkPlanStore` (cap resolution) via the
//! `FakeIntrospect` double, and asserts the resolved BEHAVIOR — the tenant id
//! and the cap/None — not merely `is_ok`. If corelink-server's ratified shape
//! and our parsers ever drift, this breaks alongside the byte goldens.
//!
//! ## §B tracked semantic (does NOT affect this vector's shape)
//!
//! corelink-server currently derives `max_concurrency` from the CACHE tier —
//! and its entitlement store is a stub returning `false`, so the field is
//! ABSENT for every tenant today (cases 2 and 3 here carry no cap, matching).
//! Whether the cap ultimately rides the Cache tier or a separate Runners
//! subscription is an owner/product decision tracked SEPARATELY; the wire shape
//! frozen in `corelink-introspect.json` is invariant under that decision. Case
//! 1 pins the shape for the day a cap DOES appear (`max_concurrency:40`).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use corelink_fabric_server::app::{PlanSource, PlanSourceError};
use corelink_fabric_server::auth::{TokenStore, TokenStoreError};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectBody, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::corelink_plans::CoreLinkPlanStore;

// ── Test double (mirrors the FakeIntrospect pattern in corelink_auth/plans) ──

/// A scripted [`IntrospectHttp`] double returning a fixed `{status, body}`.
struct FakeIntrospect {
    status: u16,
    body: String,
    last_call: Mutex<Option<(String, String, String)>>,
}

impl FakeIntrospect {
    fn ok(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            last_call: Mutex::new(None),
        }
    }
}

impl IntrospectHttp for FakeIntrospect {
    fn post(
        &self,
        url: &str,
        auth_header_value: &str,
        json_body: &str,
    ) -> anyhow::Result<IntrospectResponse> {
        *self.last_call.lock().unwrap() = Some((
            url.to_string(),
            auth_header_value.to_string(),
            json_body.to_string(),
        ));
        Ok(IntrospectResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn cfg() -> CoreLinkAuthConfig {
    CoreLinkAuthConfig {
        introspect_url: "https://corelink-api.example/internal/v1/auth/introspect".to_string(),
        service_secret: "s3cr3t".to_string(),
        timeout: Duration::from_secs(2),
    }
}

/// Path of the workspace-root `conformance/<name>` vector — two levels up from
/// this crate's manifest dir (`crates/corelink-fabric-server`).
fn vector_path(name: &str) -> std::path::PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("workspace root not found");
    workspace.join("conformance").join(name)
}

/// Load + parse the 4-case ratified vector into a JSON array.
fn load_cases() -> Vec<serde_json::Value> {
    let raw = std::fs::read_to_string(vector_path("corelink-introspect.json"))
        .expect("cannot read corelink-introspect.json conformance vector");
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(&raw).expect("conformance vector must be a JSON array");
    assert_eq!(arr.len(), 4, "ratified vector must carry exactly 4 cases");
    arr
}

/// Feed a single case's object (re-serialized to a body string) through a
/// `CoreLinkTokenStore` and return the resolved tenant outcome.
fn resolve_tenant(case: &serde_json::Value) -> Result<Option<String>, TokenStoreError> {
    let body = serde_json::to_string(case).expect("case must re-serialize");
    let store = CoreLinkTokenStore::new(FakeIntrospect::ok(200, &body), cfg());
    store
        .tenant_of("pat-token")
        .map(|opt| opt.map(|t| t.as_str().to_string()))
}

/// Feed a single case's object through a `CoreLinkPlanStore` and return the
/// resolved cap (as `Option<max_concurrency>`). The passed tenant is the one
/// auth already resolved; for the `valid:false` case we pass a placeholder
/// (it is never read — the store short-circuits on `valid:false`).
fn resolve_cap(
    case: &serde_json::Value,
    resolved_tenant: &str,
) -> Result<Option<u32>, PlanSourceError> {
    use corelink_fabric::TenantId;
    let body = serde_json::to_string(case).expect("case must re-serialize");
    let store = CoreLinkPlanStore::new(FakeIntrospect::ok(200, &body), cfg());
    let tenant = TenantId::new(resolved_tenant).expect("placeholder tenant must be valid");
    store
        .plan_of_resolving(&tenant, "pat-token")
        .map(|opt| opt.map(|plan| plan.max_concurrency))
}

// ── The 4-case table ─────────────────────────────────────────────────────────

/// Case 1 — pro, cap 40: tenant 1111… resolves; cap is `Some(40)`.
#[test]
fn case_1_pro_resolves_tenant_and_cap_40() {
    let cases = load_cases();
    let case = &cases[0];

    let tenant = resolve_tenant(case)
        .expect("reachable")
        .expect("Some tenant");
    assert_eq!(tenant, "11111111-1111-4111-8111-111111111111");

    let cap = resolve_cap(case, &tenant).expect("reachable");
    assert_eq!(cap, Some(40), "pro plan pins max_concurrency 40");
}

/// Case 2 — solo, no cap: tenant 2222… resolves; cap is `None`
/// (authenticated, uncapped — the honest M1 state).
#[test]
fn case_2_solo_resolves_tenant_uncapped() {
    let cases = load_cases();
    let case = &cases[1];

    let tenant = resolve_tenant(case)
        .expect("reachable")
        .expect("Some tenant");
    assert_eq!(tenant, "22222222-2222-4222-8222-222222222222");

    let cap = resolve_cap(case, &tenant).expect("reachable");
    assert_eq!(cap, None, "solo (no max_concurrency) → Ok(None)");
}

/// Case 3 — enterprise, no cap: tenant 3333… resolves; cap is `None`.
#[test]
fn case_3_enterprise_resolves_tenant_uncapped() {
    let cases = load_cases();
    let case = &cases[2];

    let tenant = resolve_tenant(case)
        .expect("reachable")
        .expect("Some tenant");
    assert_eq!(tenant, "33333333-3333-4333-8333-333333333333");

    let cap = resolve_cap(case, &tenant).expect("reachable");
    assert_eq!(cap, None, "enterprise (no max_concurrency) → Ok(None)");
}

/// Case 4 — valid:false: no tenant (`Ok(None)`); no plan (`Ok(None)`).
/// The body is fed through both stores too (case 4 included).
#[test]
fn case_4_invalid_resolves_no_tenant_no_cap() {
    let cases = load_cases();
    let case = &cases[3];

    let tenant = resolve_tenant(case).expect("reachable");
    assert_eq!(tenant, None, "valid:false → Ok(None) tenant");

    // The plan store short-circuits on valid:false before reading the tenant,
    // so the placeholder passed here is never consulted.
    let cap = resolve_cap(case, "11111111-1111-4111-8111-111111111111").expect("reachable");
    assert_eq!(cap, None, "valid:false → Ok(None) plan");
}

// ── Typed drift tripwire ───────────────────────────────────────────────────────

/// The ratified `corelink-introspect.json` vector parses under the strict
/// typed lens ([`IntrospectBody`] + `deny_unknown_fields`) AND re-serializes
/// byte-identically — the same drift tripwire the hugit-side
/// `RunnerLease`/`FenceManifest`/`IntentMetrics` goldens carry, now around the
/// corelink-server auth/billing seam vector. A field added or renamed in
/// corelink-server's frozen shape breaks `deny_unknown_fields` here; any
/// whitespace/order drift breaks the byte-exact compare. The vector's BYTES
/// (and its frozen sha) are unchanged — this only adds the typed wall.
#[test]
fn introspect_vector_typed_strict_and_byte_exact() {
    // Byte-exact whole-array round-trip: parse the committed file through the
    // typed array, re-serialize pretty with the committed trailing newline,
    // compare WITHOUT trim so any drift breaks here.
    let raw = std::fs::read_to_string(vector_path("corelink-introspect.json"))
        .expect("cannot read corelink-introspect.json conformance vector");
    let parsed: Vec<IntrospectBody> = serde_json::from_str(&raw)
        .expect("ratified introspect vector must parse under deny_unknown_fields");
    assert_eq!(parsed.len(), 4, "vector carries exactly 4 cases");
    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&parsed).expect("typed introspect array must re-serialize")
    );
    assert_eq!(
        raw, re,
        "corelink-introspect.json typed round-trip is not byte-exact"
    );

    // Per-case shape pins (the strict lens preserves the honest optionals).
    assert!(parsed[0].valid);
    assert_eq!(
        parsed[0].tenant_id.as_deref(),
        Some("11111111-1111-4111-8111-111111111111")
    );
    assert_eq!(parsed[0].max_concurrency, Some(40), "pro case pins the cap");
    assert_eq!(parsed[1].max_concurrency, None, "solo: no cap field");
    assert_eq!(parsed[2].max_concurrency, None, "enterprise: no cap field");
    assert!(!parsed[3].valid);
    assert_eq!(parsed[3].tenant_id, None, "valid:false carries only `valid`");

    // An UNKNOWN field is a hard parse error under deny_unknown_fields.
    let mut tampered: serde_json::Value =
        serde_json::from_str(&raw).expect("vector re-parses as Value");
    tampered[0]
        .as_object_mut()
        .expect("case 0 is an object")
        .insert("surprise".to_string(), serde_json::json!(1));
    assert!(
        serde_json::from_value::<Vec<IntrospectBody>>(tampered).is_err(),
        "an unknown introspect field must fail the typed tripwire"
    );
}
