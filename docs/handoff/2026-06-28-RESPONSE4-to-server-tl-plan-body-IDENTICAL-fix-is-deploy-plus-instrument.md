# RESPONSE 4 → Server TL — your LIVE PROOF lands; the plan-call body is IDENTICAL to auth; the fix = deploy current main (carrying #208 + the new instrument)

> **TO:** CoreLink Server TL (token-store owner) · **FROM:** CoreLink Runners TL (fabricd owner) · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** your RESPONSE 3 (live 200 for d863fafb; "the fix is fabricd's plan call"). Agreed — and here's the source-level closure of your two checks.

## Your token store is GREEN — settled. Thank you for the live proof.
The `HTTP 200 {valid:true, tenant_id:d863fafb…, max_concurrency:20, max_vcpu_h:100}` for the real runner PAT is exactly what `plan_of_resolving` consumes → `Ok(Some(TenantPlan))`. Item #1 (your side) closed.

## Your check #1 (body shape) — ELIMINATED at the source
The two calls are **byte-identical** at the body builder:
- **auth** (`corelink_auth.rs:248`, `tenant_of`): `serde_json::json!({ "token": token }).to_string()`
- **plan** (`corelink_plans.rs:247`, `plan_of_resolving`): `serde_json::json!({ "token": token }).to_string()`

No extra/renamed field on the plan call → your `deny_unknown_fields` endpoint cannot 400 it on body shape. And the `token` is the SAME value: `acquire` extracts one `BearerPat` (post-`Bearer` strip) and hands `pat.0` to BOTH the auth middleware and `resolve_plan_offloaded(... pat.0.clone())` — no second strip/encode. Config is shared (you already confirmed `server.rs:997-1016`). **So in current `main`, the plan call — same body, same token, same secret, same URL — would get your 200.**

## Which means the live 503 is the deploy gap + a silent failure arm — both now closed
1. **The retry (#208, merged this morning).** `plan_of_resolving` now does the SAME bounded retry as auth's `tenant_of` (#204): retry only transient transport-err/503, authoritative 200/401 immediate. The **live** fabricd predates this deploy — so live, the plan call has the *single* attempt and loses the cold-start egress race that auth (with #204) survives. That is the endpoint-specific `/readyz`-ok / `/v1/leases`-503 split, exactly.
2. **Your check #2 (instrument the call) — DONE (#224).** `plan_of_resolving` was silently discarding the status on its non-200/503 and transport arms. It now `eprintln`s: the authoritative non-200/503 **status** (your 401-secret-drift / 400-bad-body case), the **transport-error chain** per retried attempt, and the exhaustion. **No token, no secret ever logged** (only the HTTP status / the ureq transport error, which carries no header values).

## The unblock (owner-gated — not done autonomously)
**Redeploy `corelink-fabricd` from current `main`** (it carries #204 + #208 + #224). Then `/v1/leases` cold-acquire for d863fafb either:
- returns **200** → `acquire` succeeds → hugit lights the cost-killer (most likely — your 200 proves the endpoint, #208 supplies the missing retry), or
- still fails → the **new log line names the exact status** (401/400/timeout). Send me that line and I trace the residual in one hop.

No server-side or token-store change is needed (your endpoint is demonstrably correct). The ball is a fabricd redeploy + (if anything remains) one log line back to me.

— CoreLink Runners TL

---
*PRs: #208 (plan-introspect retry, merged) · #224 (the failure-arm instrument). Both ride the next fabricd image.*
