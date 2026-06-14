//! De-risk the `FABRIC_AUTH_BACKEND=corelink` flip: prove the THREE admission
//! arms end-to-end against a MOCK corelink introspect, BEFORE the real
//! PAT/entitlement exists, by driving the real `CoreLinkPlanStore::plan_of_resolving`
//! seam into the real `CapGate` decision.
//!
//! This complements — does NOT duplicate — the existing coverage:
//!   * `src/corelink_plans.rs` mod tests: the store's status→outcome mapping in
//!     isolation (no CapGate).
//!   * `tests/corelink_plans.rs`: the same arms through the HTTP acquire handler
//!     (CapGate via `oneshot`), but with INLINE bodies and only a SINGLE admit
//!     under cap — it never SATURATES to N to prove the reject-AT-N boundary.
//!   * `tests/corelink_introspect_vector.rs`: the 4 ratified cases resolve the
//!     cap, but never drive the CapGate admission DECISION.
//!
//! The GAP this file fills: the cap-enforcement BOUNDARY (admit while under N,
//! reject AT N) decided by the real `CapGate` over a real ledger, with the
//! `valid:true+cap` arm parsing the REAL frozen `corelink-introspect.json`
//! vector bytes — so Arm 1 doubles as a parse-conformance check against the
//! frozen `bfb38e28…` shape. No HTTP server, no new dependency: the
//! `IntrospectHttp` trait seam is the injection point (a canned `FakeIntrospect`).

use std::path::Path;
use std::time::Duration;

use corelink_fabric::{
    CapDecision, CapGate, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, RateWindow,
    TenantId, TenantPlan,
};
use corelink_fabric_server::app::{PlanSource, PlanSourceError};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::corelink_plans::CoreLinkPlanStore;
use corelink_runners_contracts::RunnerState;

const NOW_MS: u64 = 1_717_000_000_000;

// ── MOCK introspect seam (the trait boundary; no real HTTP, no new dep) ──────

/// A scripted [`IntrospectHttp`] double: returns a canned `{status, body}` or a
/// transport error — WITHOUT a socket. This IS the mock corelink introspect the
/// real `CoreLinkPlanStore` talks to.
struct FakeIntrospect {
    scripted: Option<IntrospectResponse>,
}

impl FakeIntrospect {
    fn ok(status: u16, body: &str) -> Self {
        Self {
            scripted: Some(IntrospectResponse {
                status,
                body: body.to_string(),
            }),
        }
    }

    /// A network-layer failure (DNS/TLS/connect/timeout): the transport returns
    /// `Err`, the store must map it to `Unreachable`.
    fn transport_error() -> Self {
        Self { scripted: None }
    }
}

impl IntrospectHttp for FakeIntrospect {
    fn post(&self, _url: &str, _auth: &str, _body: &str) -> anyhow::Result<IntrospectResponse> {
        match &self.scripted {
            Some(r) => Ok(IntrospectResponse {
                status: r.status,
                body: r.body.clone(),
            }),
            None => Err(anyhow::anyhow!("simulated transport error")),
        }
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

fn tenant() -> TenantId {
    // The tenant auth already resolved (the cap rides the PASSED tenant; the
    // store never re-parses tenant_id for the plan).
    TenantId::new("11111111-1111-4111-8111-111111111111").expect("valid tenant id")
}

/// Build the store over a mock introspect and resolve the plan for the passed
/// tenant + PAT — the production `plan_of_resolving` seam under test.
fn resolve(introspect: FakeIntrospect) -> Result<Option<TenantPlan>, PlanSourceError> {
    let store = CoreLinkPlanStore::new(introspect, cfg());
    store.plan_of_resolving(&tenant(), "pat-acme")
}

/// A `Held` lease for `tenant` — counts as one active (occupied) slot.
fn held(lease_id: &str, t: &TenantId) -> LeaseRecord {
    LeaseRecord {
        lease_id: lease_id.to_string(),
        tenant: t.clone(),
        state: LeaseState::Wire(RunnerState::Held),
        box_ref: format!("box-{lease_id}"),
        created_at_ms: NOW_MS,
        updated_at_ms: NOW_MS,
        deadline_ms: None,
    }
}

/// The single source of truth for Arm 1's canned body: case 1 of the REAL
/// frozen `corelink-introspect.json` vector (pro, `max_concurrency:40`),
/// re-serialized to a one-object body string. Reading it from the committed
/// file (not an inline literal) is what makes Arm 1 a parse-conformance check
/// against the frozen `bfb38e28…` shape.
fn frozen_valid_cap_body() -> (String, u32) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("workspace root not found");
    let raw = std::fs::read_to_string(
        workspace
            .join("conformance")
            .join("corelink-introspect.json"),
    )
    .expect("cannot read corelink-introspect.json conformance vector");
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(&raw).expect("conformance vector must be a JSON array");
    let case1 = &arr[0];
    // Pin the vector really IS the valid:true+cap case we expect, so a future
    // re-order of the vector cannot silently weaken this arm.
    assert_eq!(case1.get("valid").and_then(|v| v.as_bool()), Some(true));
    let cap = case1
        .get("max_concurrency")
        .and_then(serde_json::Value::as_u64)
        .expect("case 1 of the frozen vector must carry max_concurrency") as u32;
    let tenant_id = case1.get("tenant_id").and_then(|v| v.as_str()).unwrap();
    assert_eq!(
        tenant_id, "11111111-1111-4111-8111-111111111111",
        "Arm 1's frozen case must key the tenant this harness passes"
    );
    (
        serde_json::to_string(case1).expect("case re-serializes"),
        cap,
    )
}

// ── ARM 1 — valid:true + max_concurrency=N: admit under N, REJECT at N ────────

/// The cap is enforced FROM the introspect field, end-to-end:
/// `plan_of_resolving` parses the REAL frozen vector body (`valid:true`,
/// `max_concurrency:40`) into a plan, and the real `CapGate` admits while the
/// ledger holds < N active leases and rejects once it holds N. This is the
/// cap-enforcement BOUNDARY the existing handler test does not exercise (it only
/// admits a single lease under cap), AND a parse-conformance check (the body is
/// the frozen `bfb38e28…` case-1 bytes).
#[test]
fn arm1_valid_with_cap_enforces_boundary_admit_under_n_reject_at_n() {
    let (body, cap) = frozen_valid_cap_body();
    assert_eq!(cap, 40, "the frozen pro case pins max_concurrency 40");

    // The plan resolved from the MOCK introspect carries the frozen cap.
    let plan = resolve(FakeIntrospect::ok(200, &body))
        .expect("reachable")
        .expect("valid:true + cap → Some(plan)");
    assert_eq!(plan.tenant, tenant(), "cap rides the PASSED tenant");
    assert_eq!(
        plan.max_concurrency, cap,
        "cap is enforced FROM the introspect field, not a default"
    );

    let gate = CapGate;
    let window = RateWindow::new();

    // Drive the ledger from 0 active up to the cap: every slot strictly under N
    // ADMITS; the very next attempt (ledger holding exactly N) REJECTS over-cap.
    let mut ledger = InMemoryLedger::new();
    for i in 0..cap {
        // With `i` active leases (i < N), admission is granted.
        assert_eq!(
            gate.check(&ledger, &plan, NOW_MS, &window),
            CapDecision::Admit,
            "with {i} active < cap {cap}, acquire must be admitted",
        );
        // Occupy the slot the admission just granted.
        ledger
            .put(held(&format!("l-{i}"), &plan.tenant))
            .expect("put held lease");
    }

    // The ledger now holds exactly N (= cap) active leases: the gate REJECTS.
    assert_eq!(
        gate.check(&ledger, &plan, NOW_MS, &window),
        CapDecision::RejectOverCap,
        "at exactly cap {cap} active leases, acquire must be rejected over-cap",
    );
}

// ── ARM 2 — no cap / valid:false → Ok(None) → over-cap reject (fail-closed) ───

/// valid:true but NO `max_concurrency` (a cache-only / uncapped tenant) →
/// `Ok(None)`. A `None` plan is NOT a default allowance: with no plan on file
/// there is no cap to gate against, so admission is fail-closed rejected. We
/// prove the "no plan" → reject mapping by gating against a ZERO-cap plan (the
/// CapGate's encoding of "no purchased slots": cap 0 admits nothing), even on a
/// completely EMPTY ledger.
#[test]
fn arm2_valid_without_cap_is_none_then_fail_closed_reject() {
    let out = resolve(FakeIntrospect::ok(
        200,
        r#"{"valid":true,"tenant_id":"11111111-1111-4111-8111-111111111111"}"#,
    ))
    .expect("reachable — uncapped is NOT an error");
    assert!(
        out.is_none(),
        "valid:true + no max_concurrency → Ok(None) (no Runners entitlement)"
    );

    // No plan ⇒ zero purchased slots ⇒ the gate rejects even an EMPTY ledger.
    assert_eq!(
        no_plan_admission_decision(),
        CapDecision::RejectOverCap,
        "no plan must be a fail-closed reject, NEVER a default allowance",
    );
}

/// valid:false → `Ok(None)` → the SAME fail-closed reject as arm 2's uncapped
/// case (authoritative "no plan" answer; no Runners entitlement).
#[test]
fn arm2_valid_false_is_none_then_fail_closed_reject() {
    let out = resolve(FakeIntrospect::ok(200, r#"{"valid":false}"#)).expect("reachable");
    assert!(
        out.is_none(),
        "valid:false → Ok(None) (authoritative no plan)"
    );

    assert_eq!(
        no_plan_admission_decision(),
        CapDecision::RejectOverCap,
        "no plan must be a fail-closed reject, NEVER a default allowance",
    );
}

/// The admission a `None` plan yields: the acquire path has no plan to gate
/// against, which is the zero-purchased-slots fail-closed reject. Encoded as a
/// cap-0 plan over an empty ledger (the gate admits nothing at cap 0) — this is
/// what the handler returns as `over_cap` for a no-plan tenant.
fn no_plan_admission_decision() -> CapDecision {
    let zero_slots = TenantPlan {
        tenant: tenant(),
        max_concurrency: 0,
        rate_ceiling_per_min: 0,
    };
    CapGate.check(
        &InMemoryLedger::new(),
        &zero_slots,
        NOW_MS,
        &RateWindow::new(),
    )
}

// ── ARM 3 — transport ↯ / 503 / malformed → Err(Unreachable) → 503 fail-closed ─

/// A transport error (the network layer fails) → `Err(Unreachable)`: the cap
/// question is unanswerable, so the acquire path 503s fail-closed — NEVER a
/// false no-plan reject that would 0-slot a legitimate tenant.
#[test]
fn arm3_transport_error_is_unreachable_not_false_no_plan() {
    assert_eq!(
        resolve(FakeIntrospect::transport_error()),
        Err(PlanSourceError::Unreachable),
        "transport error must be Unreachable (503), not a false Ok(None) reject",
    );
}

/// A 503 from the introspect endpoint → `Err(Unreachable)` → 503 fail-closed.
#[test]
fn arm3_status_503_is_unreachable() {
    assert_eq!(
        resolve(FakeIntrospect::ok(503, "")),
        Err(PlanSourceError::Unreachable),
    );
}

/// A malformed 200 body (unparseable JSON) → `Err(Unreachable)`. A transient
/// glitch returning garbage must surface as 503, never a silent Ok(None) that
/// would 0-slot a legit tenant.
#[test]
fn arm3_malformed_body_is_unreachable() {
    assert_eq!(
        resolve(FakeIntrospect::ok(200, "not json at all")),
        Err(PlanSourceError::Unreachable),
    );
}

/// A 200 whose body is valid JSON but OMITS the authoritative `valid` field is
/// equally indeterminate → `Err(Unreachable)` (can't determine intent → 503,
/// never a false no-plan reject).
#[test]
fn arm3_missing_valid_field_is_unreachable() {
    assert_eq!(
        resolve(FakeIntrospect::ok(200, r#"{"tenant_id":"x"}"#)),
        Err(PlanSourceError::Unreachable),
    );
}
