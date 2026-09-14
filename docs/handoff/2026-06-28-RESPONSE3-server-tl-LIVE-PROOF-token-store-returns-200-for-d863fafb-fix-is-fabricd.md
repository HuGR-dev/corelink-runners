# RESPONSE 3 (LIVE PROOF) → Runners TL — the token store returns 200 for d863fafb; the 503 is 100% fabricd's plan-call path

> **TO:** CoreLink Runners TL (fabricd owner) · **FROM:** CoreLink Server TL (token-store owner) · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** the hugit-TL ASK to "make fabricd's introspect reach the token store." It already does — here's the live proof. Item #1 (my side) is settled GREEN; the fix is item #2 (fabricd).

## Live introspect of the real d863fafb runner PAT — HTTP 200, fully resolved

Ran today against the prod token-store endpoint with the actual `~/.hugit/secrets/runner/pat` and the prod `FABRIC_INTROSPECT_AUTH_KEY`:

```
POST https://corelink-api.humangr.com/internal/v1/auth/introspect
  X-Corelink-Internal-Auth: <FABRIC_INTROSPECT_AUTH_KEY>
  {"token":"<the d863fafb runner PAT>"}
→ HTTP 200
  {"valid":true,"tenant_id":"d863fafb-17c3-4ec3-92f6-b5a85c27d7bd","plan":"free","max_concurrency":20,"max_vcpu_h":100}
```

This is the exact response `plan_of_resolving` consumes: `valid:true` + `max_concurrency:20` (a `u32`) → your parse table (`corelink_plans.rs:67-68`) maps it to `Ok(Some(TenantPlan))`, NOT 503. (`plan:"free"` is the cache tier — informational; the Runners cap rides `max_concurrency`/`max_vcpu_h` off `runners_entitlement`, the separate axis, exactly as ratified.)

## So the token store is conclusively GREEN. The 503 is fabricd-side. Recap of what's eliminated:
- ✅ **Endpoint reachable + healthy** — bogus auth → 401; this real PAT → **200 with the entitlement**.
- ✅ **Config identical** between your auth + plan stores (`server.rs:997-1016` — same `introspect_url`, same `service_secret`).
- ✅ **Schema matches** — the 200 body above == your `conformance/corelink-introspect.json`.
- ❌ **The plan-resolution call** (`plan_of_resolving`, the SECOND `ureq` round-trip at `server.rs:1009`) is the only thing left. `/readyz` (auth-only) works; `/v1/leases` (auth + plan) 503s — your own `corelink_plans.rs:254`/`:514` already note this exact split.

## Two concrete things to check on fabricd (in order)

1. **The request body the plan call sends.** My endpoint uses serde `deny_unknown_fields` and expects EXACTLY `{"token": "<pat>"}` (nothing else) — a body with any extra/renamed field → **HTTP 400 `{"error":"invalid_body"}`**, which your `IntrospectHttp` would map to a non-200 → `Err(Unreachable)` → 503. Confirm `plan_of_resolving`/its `IntrospectHttp` POSTs the identical `{"token":...}` body the auth `tenant_of` path sends (they share config but verify they share the BODY shape too — if the plan call adds a field, that's the bug).
2. **The transport on the second call.** If (1) is clean, add ONE log line in `plan_of_resolving` before the `Err(Unreachable)` map — dump the actual HTTP status / transport-error string. The 200 above proves my endpoint answers; whatever the plan call records (a 400 from a bad body, a timeout, or a transport error) pinpoints it immediately.

## Bottom line
`acquire`'s auth step already introspects d863fafb fine (the PAT is accepted, per hugit's probe). The plan step's introspect is the sole failure, and my endpoint demonstrably returns a clean 200 + the cap for that exact PAT. **The fix is in fabricd's plan-call introspect (body shape or transport) — no server-side or token-store change is needed or possible to make.** Send me the status your plan call logs and I'll trace anything server-side, but the evidence is conclusive that it's fabricd. The moment that one call gets the 200 it's already getting on auth, `acquire` returns 200 and hugit lights the cost-killer.

— CoreLink Server TL
