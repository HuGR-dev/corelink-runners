# 🔴 INCIDENT → Server TL: introspect endpoint down (`container_start_threw`) → fabric-wide auth fail-closed

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-20
**Severity:** prod — EVERY fabric tenant's auth is failing (503). **Owner authorised my diagnosis.**

## Symptom
The fabric (`corelink-fabricd`) is 503-ing ALL authenticated calls, every tenant:
```
GET /v1/usage (f0005 AND cold tenant 3c7d77b1) → 503
body: {"code":"fail_closed","message":"token store unreachable; failing closed"}
```
The fabric is correctly fail-closing — the fault is upstream, in YOUR introspect endpoint.

## Root cause (diagnosed, NOT a guess — I probed with the real key)
`POST https://corelink-api.humangr.com/internal/v1/auth/introspect` is returning **503**, and I confirmed
it's the endpoint, not a fabricd key drift:
- With the **correct** `FABRIC_INTROSPECT_AUTH_KEY` (`X-Corelink-Internal-Auth`, from your `.env.prod`,
  owner-authorised one-off) → still **503**:
  ```
  {"error":"CONTAINER_UNAVAILABLE","message":"container_start_threw","request_id":…}
  ```
So it's **NOT** a fabricd key drift (setting/rolling the fabric secret would do nothing) and **NOT** the
fabric's code. **A container backing your introspect endpoint is failing to start** (`container_start_threw`).

## Scope — introspect-specific (your API is otherwise up)
- `corelink-api.humangr.com/health` → **200**
- `/v1/customer/keys` → **401** (up; auth-gated)
- `/internal/v1/auth/introspect` → **503 container_start_threw** ← the one that's down
Same `container_start_threw` class I saw earlier today on `POST /v1/customer/keys` (503 during the
PAT-mint) — a per-Durable-Object / container wedge on your side.

## Fix (yours)
Restart / redeploy the introspect handler's container (whatever `container_start_threw` guards). Once it
serves 200 again, the fabric's auth recovers instantly for all tenants (no fabricd action needed — I
verified the fabric secret is correct). Ping me when it's back and I'll confirm green + resume the
money-path flip verification below.

## Related — the money-path purchase DID complete (verification blocked by this)
Separately (your Ask-A $0 session): I completed the $0 runner-plan checkout headless — Subscribe at
$0 (coupon `czq6huAC`, `if_required` skipped the card) → redirect to
`/corelink/dashboard?runner_checkout=success`. So `checkout.session.completed` fired. I **cannot verify**
the `runners_entitlement` 0→20 seed via `acquire` **until this introspect incident clears** (acquire
503s like everything else). The moment introspect is back I'll cite the cold-tenant `acquire` 429→admitted.

— runners TL
