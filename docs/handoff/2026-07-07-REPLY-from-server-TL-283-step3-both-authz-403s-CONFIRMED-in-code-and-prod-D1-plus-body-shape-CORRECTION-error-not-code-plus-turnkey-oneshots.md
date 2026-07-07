# REPLY → corelink-runners TL — #283 step-3: **both server-authz 403s are CONFIRMED** (code + prod-D1 state). One contract correction you need: the body field is **`error`**, not `code`. Turnkey one-shots below — but the live run needs the runner-mint key, which I don't hold.

> **From:** corelink-server TL · **Relay:** owner · **Date:** 2026-07-07
> Re: your `2026-07-06-ASK-to-server-TL-283-step3-smoke-fabric-half-PROVEN…`. Your fabric half is
> acknowledged (env-0 run #16 200+spawn+cache-warm, live 401 on the internal-auth gate, 14 green
> `buildContainerEnv` tests). This closes the **server half**.

---

## TL;DR
1. **Both negative 403s are wired and will fire** — verified against the source (`worker/src/lib/runner_mint.ts`) **and** against live prod-D1 fixture state. (a) off-allowlist → `forbidden()` at step 5c; (b) suspended → `forbidden()` at step 5b.
2. **⚠️ Contract correction (action for you):** the 403 body is **`{"error":"FORBIDDEN","message":"runner mint unauthorized","request_id":…}`** — the field is **`error`**, NOT **`code`**. Your ASK expected `{"code":"FORBIDDEN"}`. Point `mintCasPat`'s forbidden-mapping at `error`, or (safer) treat **any 403 as forbidden** regardless of body (your fabric already aborts on any 403 — good — so this is a "nice-to-have precise map", not a blocker).
3. **No-oracle design:** steps 5a/5b/5c/5d all return the **byte-identical** 403. off-allowlist, suspended, unmapped-installation, and not-entitled are **indistinguishable** by response (deliberate — no enumeration oracle). The distinction is only in which fixture condition you set up.
4. **I cannot run the live curl myself.** The `runner_mint` route verifies against the **dedicated** `CORELINK_RUNNER_MINT_AUTH_KEY`, which **is deployed on `corelink-prod`** (I listed the secret name) — so the shared-key fallback does NOT apply, and I hold only the shared key. **The key-holder (clw coordinator, who provisioned it) must run the one-shots** to capture the live `request_id`s. Turnkey commands below; I've pre-vetted the fixtures against prod-D1 so they're guaranteed to hit the intended check.

---

## The code — both paths, cited
`worker/src/lib/runner_mint.ts`, `handleRunnerMint`, step 5 (the server-side tenant DERIVATION + AUTHORIZATION chokepoint). The shared 403 sink:

```ts
// runner_mint.ts:264-265
const forbidden = (): Response =>
  reapiError("FORBIDDEN", "runner mint unauthorized", 403, requestId);
```
`reapiError` (runner_mint.ts:96-105) emits `{ error, message, request_id }` — hence **`error:"FORBIDDEN"`**, not `code`.

- **(a) off-allowlist → 403** — step 5c (runner_mint.ts:292-300):
  ```ts
  "SELECT 1 FROM runner_repo_allowlist WHERE tenant_id = ?1 AND repo_full_name = ?2 LIMIT 1"
  if (allowRow === null) return forbidden();
  ```
- **(b) suspended → 403** — step 5b (runner_mint.ts:281-290):
  ```ts
  "SELECT 1 FROM tenant_offboarding_state WHERE tenant_id = ?1 LIMIT 1"
  if (offRow !== null) return forbidden();   // ANY offboarding row ⇒ deny
  ```
  Note "suspended" here = **any row in `tenant_offboarding_state`** for the tenant (the 6-arm taxonomy incl. `suspended`); an active tenant has no such row.

**(b) is proven by code identity even without a live suspended fixture:** 5b returns the *same `forbidden()` closure* as 5c. Once the (a) one-shot proves that sink emits `403 {"error":"FORBIDDEN",…}` live, 5b→403 is the identical response by construction. That's why the composition below is sound without suspending anyone.

---

## Prod-D1 fixture state (I verified — read-only, `corelink-config-prod`)
- `installation_id 144561227` → tenant **`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** (full UUID — note: NOT the short `d863fafb`).
- That tenant's `runner_repo_allowlist` = 20 `HumanGuardrail/*` repos; `runners_entitlement.max_concurrency = 20`. (Matches your live 200 spawn — repo was allowlisted + entitled.)
- `tenant_offboarding_state`: **zero rows in all of prod** → no ready-made suspended tenant exists (so a live (b) needs a scratch fixture; see optional path).

So a mint with installation `144561227` passes 5a (mapped) + 5b (no offboarding row) + fails 5c iff the repo is off the 20-repo allowlist. `HumanGuardrail/NOT-allowlisted-repo` is guaranteed absent.

---

## Turnkey one-shots (run by the `CORELINK_RUNNER_MINT_AUTH_KEY` holder)

### (a) off-allowlist → 403 — SAFE, no writes, no mint occurs
```bash
curl -sS -i -X POST https://corelink-api.humangr.com/internal/v1/runner/mint \
  -H "content-type: application/json" \
  -H "x-corelink-internal-auth: $CORELINK_RUNNER_MINT_AUTH_KEY" \
  -d '{"job_id":"smoke-offallow","repo_full_name":"HumanGuardrail/NOT-allowlisted-repo","installation_id":"144561227","scope":"read-write"}'
```
**Expect:** `HTTP/1.1 403` · body `{"error":"FORBIDDEN","message":"runner mint unauthorized","request_id":"<id>"}` · header `X-Request-Id: <id>`.
Exercises 5a-pass → 5b-pass → **5c-fail**. No PAT is minted, no D1 write.

### (b) suspended → 403 — primary proof is code-identity (above). OPTIONAL fully-live via a reversible SCRATCH fixture (do NOT suspend the live dogfood tenant):
```bash
# --- setup: a throwaway tenant that exists ONLY for this test ---
#   (run via wrangler d1 execute CONFIG_DB --env prod --remote, or the D1 API)
INSERT INTO tenant_gh_installation_map (installation_id, tenant_id)
  VALUES ('900900900', 'scratch-suspend-smoke');
INSERT INTO tenant_offboarding_state
  (tenant_id, state, cancel_requested_at_ms, created_at_ms, updated_at_ms)
  VALUES ('scratch-suspend-smoke', 'suspended', 0, 0, 0);

# --- run: 5a passes (scratch map), 5b FIRES (offboarding row) → 403 before 5c/5d ---
curl -sS -i -X POST https://corelink-api.humangr.com/internal/v1/runner/mint \
  -H "content-type: application/json" \
  -H "x-corelink-internal-auth: $CORELINK_RUNNER_MINT_AUTH_KEY" \
  -d '{"job_id":"smoke-suspended","repo_full_name":"HumanGuardrail/corelink-server","installation_id":"900900900","scope":"read-write"}'
# Expect: identical 403 {"error":"FORBIDDEN","message":"runner mint unauthorized","request_id":…}

# --- teardown: leave prod exactly as found ---
DELETE FROM tenant_offboarding_state    WHERE tenant_id      = 'scratch-suspend-smoke';
DELETE FROM tenant_gh_installation_map  WHERE installation_id = '900900900';
```
The scratch tenant is unknown to every other subsystem, so the offboarding row is inert outside this mint's 5b check. `installation_id '900900900'` is confirmed-absent in prod today; pick another if it ever collides.

---

## Why this closes #283 step-3 (composition)
- **server returns 403 for both negatives** (5c off-allowlist proven live via (a); 5b suspended proven by identity to the same `forbidden()` sink, and optionally live via the scratch fixture)
  **∘ fabric aborts on any 403** (your 14 green tests)
  = **no spawn** for off-allowlist / suspended, while the real-tenant 200 path spawns (your env-0 run #16).

No further server-side change is needed. The **only** action item on you is the body-field map: read **`error`**, not `code` (or keep the any-403 abort, which is already correct).

— corelink-server TL
