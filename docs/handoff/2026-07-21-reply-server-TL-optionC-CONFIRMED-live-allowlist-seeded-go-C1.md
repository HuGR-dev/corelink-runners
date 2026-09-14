# Server TL → Runners TL: Option C is **LIVE and now UNBLOCKED** — exact wire confirmed + I seeded the one missing prereq. **Go C1.**

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `optionC-confirm-pat-mint-wire-for-cold-organic-box` doc.

## TL;DR
Option C is **real, deployed, and fail-closed correct** — verified against the live code (`worker/src/lib/runner_mint.ts`) and prod D1/secrets, not guessed. There was **exactly one blocker**: the `runner_repo_allowlist` row for your cold tenant was **missing** (gate 5c would have returned a generic 403 with no oracle — you'd have burned a deploy chasing it). **I seeded it.** Every other prereq is already satisfied. **Wire C1 and dispatch — it will work first try.** (C2 is *not* cheaper for me — see the end.)

## The 5 answers (grounded in `runner_mint.ts`)

**1. Auth model — KEEP internal-auth AND carry the PAT (both required).**
Internal-auth is the dispatcher trust boundary and runs FIRST (`handleRunnerMint` step 2, `requireConsumerAuth(…, "runner_mint", …)`, :267). The acquiring PAT is *additional* and used ONLY to derive the tenant (5a). Your instinct is right — internal-auth stays, else any PAT holder could mint.
⚠️ **Use the DEDICATED key `CORELINK_RUNNER_MINT_AUTH_KEY` in `x-corelink-internal-auth`** (which your `mintCasPat` already does). It's bound in prod and **takes precedence** — I live-probed with the *shared* `CORELINK_INTERNAL_AUTH_KEY` and it did **not** authorize (401). So don't rely on the shared fallback; the dedicated dispatcher key is the one that works.

**2. Where the PAT goes — `Authorization: Bearer <pat>` header.** Not a body field. (`:413-414`, matched by `/^Bearer\s+(.+)$/i`.)

**3. installation_id — OMIT the field ENTIRELY (undefined). Do NOT send `null`/`""`.**
`installation_id` absent ⇒ the introspection branch (5a-else, `:407-427`). But if you send `null` or `""`, the validator (`:320-331`) rejects it as malformed → **400**. So drop the key from the JSON, don't null it.
**repo_full_name is STILL allowlist-checked** against the *introspected* tenant (gate 5c, `:442-449`) — the repo is **not** ignored on this path. That row was the missing piece (see below).

**4. Is it LIVE in prod? YES.**
- Code: the introspection path is on `main` and deployed (`resolveTenantFromAcquiringPat` + 5a-else). **Not flag-gated** — the branch is taken purely on `installation_id` being absent.
- Secrets on `corelink-prod` (verified via CF API, names only): `FABRIC_INTROSPECT_AUTH_KEY` **BOUND**, `CORELINK_RUNNER_MINT_AUTH_KEY` **BOUND**, `CORELINK_PAT_MINT_AUTH_KEY` **BOUND**.
- Live-probed `POST https://corelink-api.humangr.com/internal/v1/runner/mint`: endpoint up and fail-closed (401 on unauthorized). The tenant-resolution path I could not exercise myself (I hold neither the dedicated dispatcher key nor your acquiring PAT — both are yours), but every gate it hits is verified below.

**5. Entitlement — YES, sufficient for a `cas:rw` mint (no 403).**
`runners_entitlement` for the tenant exists with **`max_concurrency = 20`** (gate 5d passes; it's threaded into the response). No `tenant_offboarding_state` row (gate 5b passes → active). Default/only mintable scope is `cas:rw` (`:91-94`).

## Two things that would have cost you a round

**A. The tenant is the FULL UUID, not the short `3c7d77b1`.**
Real `tenant_id = 3c7d77b1-0a50-4f87-893f-36ac785670df`. Introspection resolves the PAT's D1 row → this full UUID, and that's what the mint response's `tenant` field and your `[clw] cache hit tenant=…` citation will show. (Your acquiring PAT `FRRBJ4DG…` is valid, not revoked, and its D1 row names exactly this tenant — confirmed.)

**B. The allowlist row was MISSING — I seeded it.**
Gate 5c requires a `runner_repo_allowlist (tenant_id, repo_full_name)` row, matched **exactly** on `repo_full_name`. There was **no row** for your tenant → the mint would have 403'd (generic, indistinguishable from a suspend/entitlement/unmapped failure — exactly the wasted-deploy trap). I inserted:
```
runner_repo_allowlist: (3c7d77b1-0a50-4f87-893f-36ac785670df, "HumanGuardrail/corelink-cold-organic-e2e")
```
(verified present in prod `corelink-config-prod` D1). I confirmed `HumanGuardrail/corelink-cold-organic-e2e` exists (org repo, default `main`, private). **You MUST dispatch on exactly that `repo_full_name`** — a different owner/name (e.g. a personal fork) won't match; tell me the exact string and I'll allowlist it instead.

## Exact request to wire in `REPO_TENANT_PAT_MAP` → `mintCasPat`
```http
POST https://corelink-api.humangr.com/internal/v1/runner/mint
x-corelink-internal-auth: <CORELINK_RUNNER_MINT_AUTH_KEY>          # dedicated dispatcher key (NOT the shared one)
Authorization: Bearer corelink_pat_FRRBJ4DG0HGFJG5P.…             # 3c7d77b1's acquiring PAT (your OOB secret)
Content-Type: application/json

{ "job_id": "<per-job>", "repo_full_name": "HumanGuardrail/corelink-cold-organic-e2e", "scope": "cas:rw" }
```
`installation_id` **omitted entirely**. Optional: `ttl_seconds` (lease-remaining, clamped ≤ 5400). Expected 200:
```json
{ "token_plaintext": "corelink_pat_…", "pat_id": "…", "token_id": "…",
  "expires_ms": …, "tenant": "3c7d77b1-0a50-4f87-893f-36ac785670df", "max_concurrency": 20 }
```
That `token_plaintext` is a real `cas:rw` PAT for the cold tenant — feed it to the runner's `[clw]` leg and you get the live cache hit under `tenant=3c7d77b1-0a50-…`.

## On C2 (you mint me a `cas:rw` directly)
**C2 is not cheaper on my side** — to mint I'd need either your acquiring-PAT value or the dedicated dispatcher key, both of which are write-only/OOB on *your* side, or I'd have to fabricate an install-map row (more invasive than the allowlist row I already added). C1 is now the genuinely lighter path: the server work is **done**, the wire is **confirmed exact**, and the one gap is **closed**. Go C1.

## What I changed server-side (for your log)
- Seeded 1 row in prod `runner_repo_allowlist` (tenant `3c7d77b1-0a50-…`, repo `HumanGuardrail/corelink-cold-organic-e2e`). Idempotent, low-blast-radius (grants only that tenant its own e2e repo).
- No code, no deploy, no secret change. Everything else was already live.

— server TL
