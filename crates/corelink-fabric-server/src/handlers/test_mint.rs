//! Dev/test-ONLY cred-ticket mint (`POST /v1/test/mint-cred-ticket`).
//!
//! ⚠️ SENSITIVE SURFACE — DEV/TEST ONLY. This endpoint mints a REAL, single-use,
//! lease-bound cred-ticket and stashes a per-job CAS PAT server-side, out-of-band
//! from the normal acquire+moat flow. It exists ONLY so the clw (CoreLink
//! Workspaces) TL can drive their `cred_ticket_redeems_against_the_real_fabric`
//! conformance journey against a real `Held` lease for a TEST tenant — there is no
//! other out-of-band mint (in production, cred-tickets are only injected into boxes
//! at acquire). A mistake here is a production credential-mint hole, so the surface
//! is triple-gated and OFF by default.
//!
//! ## Prod-inert by default (the load-bearing safety property)
//! The route is INERT unless `FABRIC_TEST_MINT_KEY` is set (non-empty). With it
//! unset — the PRODUCTION default — [`crate::app::AppState::test_mint`] is `None`
//! and every call returns `404`, indistinguishable from a non-existent route (no
//! enumeration oracle). An un-armed prod box therefore cannot mint ANYTHING here.
//! This is deliberately a NEW dedicated secret — NOT `FABRIC_ADMIN_KEY` (a
//! different, existing operator secret): arming the test mint is an explicit,
//! separate act.
//!
//! ## Gating (defense in depth)
//! 1. `FABRIC_TEST_MINT_KEY` present ⇒ route active; absent ⇒ `404`.
//! 2. The caller MUST present that key — `Authorization: Bearer <key>` OR
//!    `X-Fabric-Test-Mint-Key` — CONSTANT-TIME compared (reuses the one pinned
//!    [`constant_time_eq`]); wrong/absent ⇒ `401`.
//! 3. The CLAIMED tenant must be on the allowlist (`FABRIC_TEST_MINT_TENANTS`,
//!    default [`DEFAULT_TEST_MINT_TENANT`] = the full f0005 UUID); any other ⇒
//!    `400`. NOTE the precise scope of this bound: the allowlist gates the
//!    caller-CLAIMED `tenant` (used for the ledger record + the stashed
//!    `clw_tenant` LABEL). The ACTUAL scope of the minted CAS PAT is resolved
//!    SERVER-side from the `installation_id` / `acquiring_pat` the caller supplies
//!    (`runner_cas_mint`), NOT from the claimed tenant — and `MintedPat` does not
//!    return the resolved tenant, so this endpoint cannot cross-check them. In
//!    correct use the acquiring_pat resolves to the claimed (allowlisted) tenant,
//!    so the stash label is accurate. A caller who supplies a mismatched
//!    acquiring_pat (a real tenant they ALREADY hold) would get a PAT scoped to
//!    that real tenant under an f0005 LABEL — no privilege gain (they already held
//!    it), but the label would mislabel the scope (clw's list_refs would then 401).
//!    So: the allowlist bounds the LABEL/lease, not provably the PAT's real scope.
//!
//! ## Reuses the production mint VERBATIM
//! The ONLY new thing is this gated entry point + returning the trio to the authed
//! caller INSTEAD of injecting it into a box (contrast `runner_inject::
//! inject_cred_ticket_env`). Everything security-critical is the same code the
//! acquire path runs:
//!   - the lease-bound ticket ← the SAME [`CredTicketSigner`]
//!     (`FABRIC_CRED_TICKET_SECRET`);
//!   - the per-job CAS PAT ← the SAME [`crate::runner_cas_mint::CasPatMint`]
//!     (`CORELINK_RUNNER_MINT_URL` → corelink-server);
//!   - the server-side hold ← the SAME [`crate::app::AppState::stash_cred`].
//!
//! So the returned ticket redeems through the real
//! [`crate::handlers::cas_cred::redeem`] handler, single-use.
//!
//! ## Secret hygiene
//! The minted PAT is NEVER returned (only stashed) and NEVER logged. The returned
//! ticket is NEVER logged. The presented key and any acquiring PAT are redacted in
//! `Debug`. The only log line carries the lease id + tenant — never a secret.

use std::sync::Arc;

use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use corelink_fabric::{LeaseRecord, LeaseState, TenantId};
use corelink_runners_contracts::RunnerState;

use crate::app::AppState;
use crate::cred_ticket::StashedCred;
use crate::ingest_token::constant_time_eq;

/// The dev/test route path. A LOCAL const, deliberately NOT in the frozen
/// `corelink-fabric-api::paths` — it is not part of the product wire contract, it
/// is a dev/test knob that is inert in production.
pub(crate) const TEST_MINT_CRED_TICKET_PATH: &str = "/v1/test/mint-cred-ticket";

/// The dedicated header carrying the test-mint key (alternative to
/// `Authorization: Bearer`). Distinct from every other auth header on the fabric.
const TEST_MINT_HEADER: &str = "X-Fabric-Test-Mint-Key";

/// Default test tenant the mint is allowlisted for when `FABRIC_TEST_MINT_TENANTS`
/// is unset. clw's `cred_ticket_redeems_against_the_real_fabric` journey checks
/// `CLW_TENANT` = the FULL UUID for f0005 (NOT the short string `f0005`) — the
/// stashed cas_pat's tenant scope must equal this exact UUID or clw's list_refs
/// 401s. So the lease, the mint, and the stash all bind THIS value.
pub(crate) const DEFAULT_TEST_MINT_TENANT: &str = "00000000-0000-4000-8000-0000000f0005";

/// Short TTL for the test lease so it SELF-EXPIRES (the reaper reclaims it + the
/// recorded PAT is revoked at expiry). 10 minutes is ample for the clw journey.
const TEST_MINT_LEASE_TTL_MS: u64 = 600_000;

/// Composition-root config for the dev/test cred-ticket mint. `AppState.test_mint
/// == Some` IFF `FABRIC_TEST_MINT_KEY` was set at boot — the single arming switch
/// (absent ⇒ `None` ⇒ the route 404s and no mint is possible).
#[derive(Clone)]
pub(crate) struct TestMintConfig {
    /// The dedicated test-mint key the caller must present (constant-time compared).
    /// Held as `Arc<str>` and redacted in `Debug` (secret hygiene).
    pub(crate) key: Arc<str>,
    /// The tenant allowlist (default `[f0005]`). A request tenant not on it ⇒ 400,
    /// so the endpoint can never mint for a real customer tenant.
    pub(crate) tenants: Arc<Vec<String>>,
    /// The fabric's public base URL returned to the caller as `fabric_endpoint` —
    /// where clw redeems the ticket (`{fabric_endpoint}/v1/leases/{id}/cas-cred`).
    /// Sourced from `FABRIC_PUBLIC_BASE_URL`.
    pub(crate) fabric_endpoint: Option<String>,
}

impl std::fmt::Debug for TestMintConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestMintConfig")
            .field("key", &"***REDACTED***")
            .field("tenants", &self.tenants)
            .field("fabric_endpoint", &self.fabric_endpoint)
            .finish()
    }
}

impl TestMintConfig {
    /// Parse the config from the environment. `Ok(None)` (route inert) when
    /// `FABRIC_TEST_MINT_KEY` is absent/blank — the production default. `Some`
    /// arms the route.
    ///
    /// The key is validated with the SAME strength guard the mint auth-key and the
    /// C2c cred secret use ([`crate::runner_cas_mint::reject_weak_secret`]) — a
    /// dev-sentinel or trivially-short value fails boot loud, never arms a
    /// guessable secret.
    ///
    /// `FABRIC_TEST_MINT_TENANTS` is an OPTIONAL comma-separated allowlist; absent
    /// ⇒ `[f0005]` ([`DEFAULT_TEST_MINT_TENANT`]).
    pub(crate) fn from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Option<Self>> {
        let Some(key) = get("FABRIC_TEST_MINT_KEY")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            return Ok(None);
        };
        // A dev/default sentinel or too-short value must never arm a sensitive
        // credential-mint surface — fail boot loud (shared guard).
        crate::runner_cas_mint::reject_weak_secret("FABRIC_TEST_MINT_KEY", &key)?;

        let tenants: Vec<String> = match get("FABRIC_TEST_MINT_TENANTS") {
            Some(raw) => {
                let list: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if list.is_empty() {
                    vec![DEFAULT_TEST_MINT_TENANT.to_string()]
                } else {
                    list
                }
            }
            None => vec![DEFAULT_TEST_MINT_TENANT.to_string()],
        };

        let fabric_endpoint = get(crate::envelope_inject::FABRIC_PUBLIC_BASE_URL)
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());

        Ok(Some(Self {
            key: Arc::from(key),
            tenants: Arc::new(tenants),
            fabric_endpoint,
        }))
    }
}

/// `POST /v1/test/mint-cred-ticket` request body.
///
/// The tenant is allowlist-checked; `repo_full_name` is required by the mint
/// contract (allowlist-checked server-side against the resolved tenant);
/// `installation_id` / `acquiring_pat` are the two unforgeable tenant-resolution
/// inputs the CoreLink runner-mint accepts (installation-map vs PAT introspection).
// `Serialize` is derived ONLY under `cfg(test)` so the unit tests can build a
// request body as bytes; production NEVER serializes this input type (it is only
// ever deserialized off the wire), so the sensitive `acquiring_pat` field can
// never be serialized out of a prod build.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub struct TestMintRequest {
    /// The test tenant to mint for (must be on the allowlist). Default: `f0005`.
    #[serde(default)]
    pub tenant: Option<String>,
    /// The repo the per-job CAS PAT hydrates. Required by the mint contract.
    pub repo_full_name: String,
    /// OPTIONAL GitHub-App installation id (the installation-map tenant selector).
    #[serde(default)]
    pub installation_id: Option<String>,
    /// OPTIONAL acquiring PAT for the test tenant — presented to the mint as
    /// `Authorization: Bearer` so the server resolves the tenant by introspection
    /// on the native path. SENSITIVE (redacted in `Debug`); never logged.
    #[serde(default)]
    pub acquiring_pat: Option<String>,
}

impl std::fmt::Debug for TestMintRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestMintRequest")
            .field("tenant", &self.tenant)
            .field("repo_full_name", &self.repo_full_name)
            .field("installation_id", &self.installation_id)
            // The acquiring PAT is a sensitive bearer credential — never printed.
            .field(
                "acquiring_pat",
                &self.acquiring_pat.as_ref().map(|_| "***REDACTED***"),
            )
            .finish()
    }
}

/// `POST /v1/test/mint-cred-ticket` success body — the trio clw needs. NOT
/// `Debug`-derived: the ticket must never reach a log line via a stray `{:?}`.
#[derive(Serialize)]
struct TestMintResponse {
    /// The single-use `CLW_CRED_TICKET`, bound to `lease_id`.
    ticket: String,
    /// The `Held` lease the ticket redeems against.
    lease_id: String,
    /// The fabric base URL clw redeems against
    /// (`{fabric_endpoint}/v1/leases/{lease_id}/cas-cred`).
    fabric_endpoint: String,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Extract the presented test-mint key: the dedicated `X-Fabric-Test-Mint-Key`
/// header if set, else the `Authorization: Bearer <key>` value. Empty string when
/// neither is present (the constant-time compare then fails).
fn presented_key(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get(TEST_MINT_HEADER).and_then(|h| h.to_str().ok()) {
        return v.to_string();
    }
    if let Some(rest) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return rest.to_string();
    }
    String::new()
}

/// `POST /v1/test/mint-cred-ticket` — dev/test-only out-of-band cred-ticket mint.
pub(crate) async fn mint_cred_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    // Raw bytes — NOT `Json<TestMintRequest>`. A `Json` extractor parses the body
    // BEFORE this handler runs, so a malformed body returns 422 straight from the
    // extractor — before the arm gate below. That 422 (vs a 404 on a truly
    // non-existent path) is a weak enumeration oracle: it reveals the route is
    // registered even when disarmed. `Bytes` never rejects, so steps 1–2 own EVERY
    // disarmed/unauthed response and the body is parsed only in step 3.
    body: axum::body::Bytes,
) -> Response {
    // 1. GATE: the route is inert unless FABRIC_TEST_MINT_KEY armed the config.
    //    Absent ⇒ 404, indistinguishable from a non-existent route (no oracle) —
    //    for ANY body, well-formed or not, because this runs before the parse.
    let Some(cfg) = state.test_mint.as_ref() else {
        return err(StatusCode::NOT_FOUND, "no such route");
    };

    // 2. AUTH: constant-time compare the presented key. Wrong/absent ⇒ 401 — also
    //    before the parse, so an unauthenticated caller learns nothing from the
    //    body-validation path either (no 400/422 body-shape signal pre-auth).
    let presented = presented_key(&headers);
    if !constant_time_eq(cfg.key.as_bytes(), presented.as_bytes()) {
        return err(StatusCode::UNAUTHORIZED, "invalid test-mint key");
    }

    // 3. PARSE the body — reached ONLY on an armed route by an authed caller, so a
    //    malformed body (400) is never an enumeration/auth oracle for the inert
    //    surface (steps 1–2 already returned 404/401 for it).
    let req: TestMintRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return err(StatusCode::BAD_REQUEST, "invalid request body"),
    };

    // 4. TENANT ALLOWLIST (blast-radius bound): mint ONLY for an allowlisted test
    //    tenant. Default f0005. Any other tenant ⇒ 400 refuse.
    let tenant_str = req
        .tenant
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_TEST_MINT_TENANT)
        .to_string();
    if !cfg.tenants.iter().any(|t| t == &tenant_str) {
        return err(
            StatusCode::BAD_REQUEST,
            "tenant not permitted for test-mint (not on FABRIC_TEST_MINT_TENANTS)",
        );
    }
    let Ok(tenant) = TenantId::new(&tenant_str) else {
        return err(StatusCode::BAD_REQUEST, "invalid tenant id");
    };

    // The endpoint REUSES the production mint + signer verbatim; both must be
    // armed. Authed callers only reach this, so a plain 503 is safe (no oracle).
    let (Some(signer), Some(mint)) = (state.cred_signer.as_ref(), state.cas_pat_mint.as_ref())
    else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "test-mint requires the cred-ticket signer (FABRIC_CRED_TICKET_SECRET) \
             + the CAS PAT mint (CORELINK_RUNNER_MINT_*) to be armed",
        );
    };

    let now_ms = state.clock.now_ms();
    // N=1 ASSUMPTION (tracked N>1-flip follow-up). This mints a shard-AGNOSTIC id
    // (`mint_lease_id`), NOT the shard-targeted `mint_lease_id_for(shard, N)` the
    // acquire path uses. The stashed cred lives in THIS instance's memory, but the
    // ledger is shared (pg); at N>1 the Worker hash-routes the redeem
    // (`/v1/leases/{id}/cas-cred`) by `shard_of(id, N)`, which — for a shard-agnostic
    // id — can land on a DIFFERENT instance that has the lease Held in pg but no
    // stash → a spurious 410 on the first legit redemption. This is correct at N=1
    // (today's singleton, where every shard resolves to the one instance) and is
    // INERT until the owner-gated RAISE-N flip; at that flip this endpoint must
    // shard-target its lease id (and the Worker must stamp `X-Fabricd-Shard` on this
    // route) exactly like acquire, or be treated as N=1-only. Dev/test surface,
    // off-by-default, so this rides the same N>1 follow-up list as the reaper/list
    // shard-targeting — not a live-path gap.
    let lease_id = state.mint_lease_id();
    let deadline_ms = now_ms.saturating_add(TEST_MINT_LEASE_TTL_MS);

    // Create a short-TTL HELD lease via the SAME ledger admit/transition path the
    // acquire flow uses. A generous cap (this is a test tenant) so the test lease
    // never trips a concurrency reject.
    let rec = LeaseRecord {
        lease_id: lease_id.clone(),
        tenant: tenant.clone(),
        state: LeaseState::Pending,
        box_ref: format!("box:{lease_id}"),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        deadline_ms: Some(deadline_ms),
        billing_acquired_at_ms: None,
    };
    match state.ledger.try_admit(rec, u32::MAX) {
        Ok(true) => {}
        Ok(false) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "test lease admission refused",
            );
        }
        Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "lease ledger unavailable"),
    }
    if state
        .ledger
        .transition(&lease_id, RunnerState::Held, now_ms)
        .is_err()
    {
        let _ = state.ledger.remove(&lease_id);
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "lease ledger refused Pending->Held",
        );
    }

    // Mint the per-job CAS PAT via the SAME CoreLink runner-mint path acquire uses.
    // On failure: roll back the held lease (nothing is stashed yet) and fail — no
    // ticket is ever handed out without a stashed PAT behind it.
    let acquiring_pat = req.acquiring_pat.as_deref().unwrap_or("");
    let minted = match mint
        .mint(
            &req.repo_full_name,
            req.installation_id.as_deref(),
            acquiring_pat,
            &lease_id,
            deadline_ms,
            now_ms,
        )
        .await
    {
        Ok(m) => m,
        Err(e) => {
            let _ = state.ledger.remove(&lease_id);
            // `e` never carries the PAT (MintError redacts); log the failure class
            // only — never a secret.
            eprintln!(
                "test-mint: CAS PAT mint FAILED for lease {lease_id} (tenant {tenant_str}): {e} \
                 — no ticket issued (DEV/TEST endpoint)"
            );
            return err(StatusCode::SERVICE_UNAVAILABLE, "CAS PAT mint failed");
        }
    };

    // Mint the lease-bound ticket via the SAME signer, and stash the per-job PAT
    // server-side via the SAME stash the acquire path uses — so the returned
    // ticket redeems through the real cas_cred::redeem handler, single-use.
    let ticket = signer.ticket(&lease_id);
    let endpoint = state.clw_endpoint.as_deref().unwrap_or("");
    state.stash_cred(
        &lease_id,
        StashedCred {
            token: minted.token.clone(),
            endpoint: endpoint.to_string(),
            tenant: tenant_str.clone(),
        },
    );
    // Record the pat_id so the reaper REVOKES it when the test lease self-expires
    // (A7b: no per-job PAT is left un-revoked on any terminal path).
    state
        .pat_ids
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(lease_id.clone(), minted.pat_id.clone());

    // Forensic line — lease id + tenant ONLY; NEVER the ticket or the PAT.
    eprintln!(
        "test-mint: issued cred-ticket for lease {lease_id} (tenant {tenant_str}, ttl \
         {TEST_MINT_LEASE_TTL_MS}ms) — DEV/TEST endpoint"
    );

    let fabric_endpoint = cfg.fabric_endpoint.clone().unwrap_or_default();
    (
        StatusCode::OK,
        Json(TestMintResponse {
            ticket,
            lease_id,
            fabric_endpoint,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use corelink_fabric::{InMemoryLedger, LeaseLedger};
    use corelink_runners_contracts::RunnerState as CState;

    use crate::cred_ticket::CredTicketSigner;
    use crate::handlers::cas_cred;
    use crate::runner_cas_mint::MockMint;
    use crate::{StaticPlans, SystemClock};

    const KEY: &str = "test-mint-key-high-entropy-0123456";
    const SECRET: [u8; 32] = *b"cred-ticket-dev-secret-32-bytes!";
    /// The FULL UUID for the f0005 test tenant (clw checks CLW_TENANT against it).
    const F0005: &str = "00000000-0000-4000-8000-0000000f0005";

    /// A fully-armed AppState: cred signer + MockMint + a test-mint config with
    /// the given key + allowlist. `test_mint == None` when `key` is `None`.
    fn armed_state(key: Option<&str>, tenants: Vec<String>) -> AppState {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(SystemClock),
        )
        .with_cred_signer(Some(CredTicketSigner::new(SECRET)))
        .with_cas_pat_mint(Arc::new(MockMint::new()))
        .with_clw_endpoint(Some("https://cas.example".to_string()))
        .with_test_mint(key.map(|k| TestMintConfig {
            key: Arc::from(k),
            tenants: Arc::new(tenants),
            fabric_endpoint: Some("https://fabric.example.com".to_string()),
        }))
    }

    fn body(tenant: &str) -> TestMintRequest {
        TestMintRequest {
            tenant: Some(tenant.to_string()),
            repo_full_name: "HumanGuardrail/corelink-runners".to_string(),
            installation_id: Some("inst-1".to_string()),
            acquiring_pat: Some("pat-f0005".to_string()),
        }
    }

    fn hdrs(bearer: Option<&str>, dedicated: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(b) = bearer {
            h.insert(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {b}").parse().unwrap(),
            );
        }
        if let Some(d) = dedicated {
            h.insert(TEST_MINT_HEADER, d.parse().unwrap());
        }
        h
    }

    async fn call(state: &AppState, headers: HeaderMap, req: TestMintRequest) -> Response {
        let body = axum::body::Bytes::from(serde_json::to_vec(&req).unwrap());
        mint_cred_ticket(State(state.clone()), headers, body).await
    }

    /// Drive the handler with a RAW body (possibly not valid JSON) — for the
    /// enumeration-oracle tests that a malformed body must not distinguish the
    /// disarmed/unauthed surface from a non-existent route.
    async fn call_raw(state: &AppState, headers: HeaderMap, raw: &[u8]) -> Response {
        let body = axum::body::Bytes::copy_from_slice(raw);
        mint_cred_ticket(State(state.clone()), headers, body).await
    }

    #[tokio::test]
    async fn unset_key_is_404_inert() {
        // FABRIC_TEST_MINT_KEY unset ⇒ test_mint None ⇒ route 404, no mint possible.
        let state = armed_state(None, vec![]);
        let resp = call(&state, hdrs(Some(KEY), None), body(F0005)).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn wrong_or_absent_caller_key_is_401() {
        let state = armed_state(Some(KEY), vec![F0005.to_string()]);
        // Wrong key.
        assert_eq!(
            call(&state, hdrs(Some("wrong"), None), body(F0005))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        // No key at all.
        assert_eq!(
            call(&state, hdrs(None, None), body(F0005)).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn non_allowlisted_tenant_is_400() {
        let state = armed_state(Some(KEY), vec![F0005.to_string()]);
        // A real-looking customer tenant is refused — the blast-radius bound.
        assert_eq!(
            call(&state, hdrs(Some(KEY), None), body("acme"))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn disarmed_route_is_404_even_for_a_malformed_body() {
        // The enumeration-oracle fix: on the DISARMED route the arm gate runs BEFORE
        // the body is parsed, so a malformed/garbage body is 404 (indistinguishable
        // from a non-existent path) — NOT a 400/422 that reveals the route exists.
        let state = armed_state(None, vec![]);
        // Not even JSON, no key presented.
        assert_eq!(
            call_raw(&state, hdrs(None, None), b"}{ not json at all")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        // Empty body, WITH a would-be key — still 404 (disarmed owns every response).
        assert_eq!(
            call_raw(&state, hdrs(Some(KEY), None), b"").await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn armed_wrong_key_is_401_even_for_a_malformed_body() {
        // Auth also runs before the parse: a wrong-key caller always gets 401 and
        // never a body-shape signal (400/422), so the body-validation path is not an
        // oracle for an unauthenticated prober on the armed route either.
        let state = armed_state(Some(KEY), vec![F0005.to_string()]);
        assert_eq!(
            call_raw(&state, hdrs(Some("wrong"), None), b"}{ not json")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn armed_correct_key_malformed_body_is_400() {
        // Only an armed route + an authed caller ever reaches body validation, where
        // a malformed body is a plain 400 (no longer a pre-gate 422 from the extractor).
        let state = armed_state(Some(KEY), vec![F0005.to_string()]);
        assert_eq!(
            call_raw(&state, hdrs(Some(KEY), None), b"}{ not json")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn correct_key_and_tenant_returns_trio_and_round_trips_through_redeem() {
        let state = armed_state(Some(KEY), vec![F0005.to_string()]);

        // 200 with a well-formed trio, presented via the dedicated header.
        let resp = call(&state, hdrs(None, Some(KEY)), body(F0005)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let ticket = v["ticket"].as_str().expect("ticket string").to_string();
        let lease_id = v["lease_id"].as_str().expect("lease_id string").to_string();
        assert_eq!(v["fabric_endpoint"], "https://fabric.example.com");
        assert!(ticket.len() > 20, "ticket must be a real HMAC-base64 value");
        assert!(lease_id.starts_with("lease-"), "lease_id shape");

        // The lease must be HELD in the real ledger (redeem requires Held).
        let rec = state.ledger.get(&lease_id).unwrap().unwrap();
        assert!(
            matches!(rec.state, LeaseState::Wire(CState::Held)),
            "the minted test lease must be Held"
        );

        // Round-trip: redeem the returned ticket through the REAL cas_cred handler
        // → 200 yielding the stashed per-job PAT (MockMint's derived token).
        let redeem_resp = cas_cred::redeem(
            State(state.clone()),
            axum::extract::Path(lease_id.clone()),
            Json(cas_cred::CasCredRequest {
                ticket: ticket.clone(),
            }),
        )
        .await;
        assert_eq!(redeem_resp.status(), StatusCode::OK);
        let rbytes = axum::body::to_bytes(redeem_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let rv: serde_json::Value = serde_json::from_slice(&rbytes).unwrap();
        assert_eq!(
            rv["cas_pat"],
            MockMint::derived_token(&lease_id),
            "redeem must return the per-job PAT the test-mint stashed"
        );
        assert_eq!(
            rv["clw_tenant"], F0005,
            "the stashed cas_pat must be scoped to the FULL f0005 UUID (clw checks CLW_TENANT)"
        );

        // Single-use: a SECOND redeem of the same ticket → 410 (latch consumed).
        let second = cas_cred::redeem(
            State(state.clone()),
            axum::extract::Path(lease_id.clone()),
            Json(cas_cred::CasCredRequest { ticket }),
        )
        .await;
        assert_eq!(
            second.status(),
            StatusCode::GONE,
            "a second redemption must be 410 (single-use)"
        );
    }

    #[tokio::test]
    async fn default_tenant_is_f0005_when_body_omits_it() {
        let state = armed_state(Some(KEY), vec![DEFAULT_TEST_MINT_TENANT.to_string()]);
        let req = TestMintRequest {
            tenant: None, // omitted ⇒ defaults to f0005
            repo_full_name: "HumanGuardrail/corelink-runners".to_string(),
            installation_id: None,
            acquiring_pat: None,
        };
        let resp = call(&state, hdrs(Some(KEY), None), req).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn secrets_are_redacted_in_debug() {
        // The presented key must never leak via a TestMintConfig {:?}.
        let cfg = TestMintConfig {
            key: Arc::from("super-secret-test-mint-key"),
            tenants: Arc::new(vec![F0005.to_string()]),
            fabric_endpoint: None,
        };
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("super-secret-test-mint-key"),
            "the test-mint key must never appear in Debug"
        );
        assert!(dbg.contains("***REDACTED***"));

        // The acquiring PAT must never leak via a TestMintRequest {:?}.
        let req = TestMintRequest {
            tenant: Some("f0005".to_string()),
            repo_full_name: "o/r".to_string(),
            installation_id: None,
            acquiring_pat: Some("pat-super-secret-value".to_string()),
        };
        let rdbg = format!("{req:?}");
        assert!(
            !rdbg.contains("pat-super-secret-value"),
            "the acquiring PAT must never appear in Debug output"
        );
        assert!(rdbg.contains("***REDACTED***"));
    }

    #[test]
    fn from_env_default_off_when_key_absent() {
        let cfg = TestMintConfig::from_env(|_| None).expect("absent key must be Ok");
        assert!(cfg.is_none(), "no FABRIC_TEST_MINT_KEY ⇒ None (inert)");
    }

    #[test]
    fn from_env_rejects_a_weak_key() {
        // A dev sentinel must fail boot loud rather than arm the surface.
        let res =
            TestMintConfig::from_env(|k| (k == "FABRIC_TEST_MINT_KEY").then(|| "dev".to_string()));
        assert!(res.is_err(), "a dev-sentinel key must fail loud");
    }

    #[test]
    fn from_env_arms_with_default_tenant_allowlist() {
        let cfg = TestMintConfig::from_env(|k| match k {
            "FABRIC_TEST_MINT_KEY" => Some(KEY.to_string()),
            _ => None,
        })
        .expect("valid key must be Ok")
        .expect("armed");
        assert_eq!(cfg.tenants.as_slice(), &[F0005.to_string()]);
    }

    #[test]
    fn from_env_parses_custom_tenant_allowlist() {
        let cfg = TestMintConfig::from_env(|k| match k {
            "FABRIC_TEST_MINT_KEY" => Some(KEY.to_string()),
            "FABRIC_TEST_MINT_TENANTS" => Some(" f0005 , f0006 ".to_string()),
            _ => None,
        })
        .expect("valid key must be Ok")
        .expect("armed");
        assert_eq!(
            cfg.tenants.as_slice(),
            &["f0005".to_string(), "f0006".to_string()]
        );
    }
}
