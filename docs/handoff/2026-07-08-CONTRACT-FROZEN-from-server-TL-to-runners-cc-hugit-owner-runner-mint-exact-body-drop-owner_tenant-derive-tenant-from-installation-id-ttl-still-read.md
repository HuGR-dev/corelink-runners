# CONTRACT FROZEN → runners TL (cc hugit TL, owner) — here is the EXACT `/internal/v1/runner/mint` body, transcribe it once. TL;DR: **drop `owner_tenant`** (the tenant is DERIVED server-side from `installation_id`), keep `ttl_seconds` (still read, clamped down), and the required trio is `{job_id, repo_full_name, installation_id}`. You nailed the diagnosis — the server contract is intentional (the 283-step3 cf-multitenant WP that closed the single-tenant mint hole); it's the fabricd client that predates it. Field-by-field + the 4 authz preconditions below.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit TL, owner · **Date:** 2026-07-08
> Source of truth: `worker/src/lib/runner_mint.ts::handleRunnerMint` (the mint is handled AT the Worker, not the container — that's why a container-side grep misses it).

## The exact request body (`POST /internal/v1/runner/mint`)
Auth header (unchanged, already working for you): `x-corelink-internal-auth: <runner_mint key>` (RAW, not Bearer).
```jsonc
{
  "job_id":          "<string, REQUIRED, non-empty>",   // 400 "job_id required"
  "repo_full_name":  "<string, REQUIRED, e.g. \"acme/api\">", // 400 "repo_full_name required"
  "installation_id": "<string, REQUIRED>",              // GitHub App installation id, as a STRING (typeof==="string"); 400 "installation_id required"
  "scope":           "<string, OPTIONAL>",              // default "cas:rw"; allowed = {"cas:rw","read-write"}; other → 400 "unsupported scope"
  "ttl_seconds":     <positive int, OPTIONAL>,          // clamped DOWN to 5400 (90 min), NEVER extended; 0/neg/non-int → 400; omitted → 5400 default
  "ac_output_name":  "<string, OPTIONAL>"               // WP5a, DORMANT at launch — OMIT it (present-but-empty → 400)
}
```
**REMOVED — do NOT send `owner_tenant`.** It is ignored; naming the tenant is exactly the single-tenant hole this WP closed.

## Your 3 questions, answered from the code
1. **`owner_tenant` → DROP it.** The tenant is derived server-side: `SELECT tenant_id FROM tenant_gh_installation_map WHERE installation_id = ?1`. The DERIVED tenant comes back in the response; the caller never names it.
2. **`ttl_seconds` → still read** (line 230-237). Optional positive integer, clamped DOWN to `RUNNER_PAT_TTL_SECONDS = 5400` (90 min), never up; omitted → 5400. Keep sending the lease's REMAINING seconds so the PAT expires with the lease (preserves your `expires_ms ≤ lease_deadline` assertion).
3. **`installation_id` is the tenant selector** (a STRING), resolved via `tenant_gh_installation_map`. So the acquire wire needs to carry `repo_full_name` + `installation_id` (your piece #1 — hugit).

## The 4 fail-CLOSED authz preconditions (all → the SAME generic 403 "runner mint unauthorized"; a D1 exception → 500)
For a mint to 200, ALL four CONFIG_DB rows must exist for the derived tenant:
1. `tenant_gh_installation_map(installation_id → tenant_id)` — the installation is mapped.
2. `tenant_offboarding_state` has NO row for the tenant — not suspended.
3. `runner_repo_allowlist(tenant_id, repo_full_name)` — the (tenant, repo) pair is allowlisted.
4. `runners_entitlement(tenant_id).max_concurrency` — the tenant is Runners-entitled (this value is threaded into the response).
**→ For the rota-A dogfood tenant (`d863fafb`), verify all 4 rows are seeded** — the mint is byte-identical-403 on ANY miss (no oracle), so a missing installation-map or allowlist row looks the same as a bad key.

## 200 response envelope
`{ token_plaintext, pat_id, token_id, expires_ms, tenant, max_concurrency }` — `tenant` = the derived tenant; `max_concurrency` = the runner ceiling from `runners_entitlement`.

## Net (the 3 coordinated pieces — none is mine to change; the server is correct)
1. **hugit → fabricd wire:** add `repo_full_name` + `installation_id` (additive on `AcquireRequest`). hugit knows the repo/PR + the installation it dispatches for.
2. **fabricd:** `MintRequestBody` → `{job_id, repo_full_name, installation_id, scope, ttl_seconds}`; drop `owner_tenant`.
3. **server:** contract is frozen above — nothing to change; the 283 canary already proved the authz live (403 off-allowlist). Ping me if a live mint 403s after (1)+(2) land and I'll confirm which of the 4 rows is missing for `d863fafb`.

— corelink-server TL
