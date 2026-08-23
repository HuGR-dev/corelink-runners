# Runbook — Cloudflare substrate: live state + the WARM-moat flip (drop-day)

> **Living runbook** (not a dated handoff). REWRITTEN 2026-06-21 to match the
> **deployed reality**: the runner compute already runs on Cloudflare Containers
> via an **all-Cloudflare autoscaler** (the spawn-Worker's `/webhook`), proven live.
> The earlier version of this file described the pre-deploy skeleton + a
> fabric-driven selection slice; that is superseded — see §6 for the fabric path,
> which still exists but is NOT the deployed dogfood reality.
>
> The one thing left to capture the moat (cache-warm, recompute ≈ 0) is a single
> Worker secret. §2 is the precise drop-day procedure for it. Sibling runbook:
> `docs/runbook/dogfood-go-live.md` (cold CI on Northflank, the interim/fallback).

## 0. What is LIVE now vs the one remaining gate

**LIVE + PROVEN (on the `gmhelmold` CF account `6a1fc1c626fc2628823e60b9db01f5cd`, co-located with the R2 CAS):**
- **spawn-Worker** `https://corelink-spawn-worker.gmhelmold.workers.dev` — deployed.
  Auth surfaces verified: unauth `POST /v1/spawn` → `401`, any unauth route → `401`
  (the bearer gate); `/webhook` is HMAC-authed separately.
- **All-Cloudflare autoscaler** (`/webhook`): GitHub `workflow_job:queued` (HMAC verify) →
  mint one-shot JIT (`generate-jitconfig`) → `container.start` a digest-pinned runner image on a
  **Firecracker microVM**. **No Rust fabric in the loop.** Proven: a real `dogfood-smoke` job
  autoscaled onto `cf-runner-<uuid>`, ran to SUCCESS, self-deregistered (~20s, zero manual).
- **Hardening:** constant-time bearer + GitHub HMAC + native rate-limit (`WEBHOOK_LIMITER`, 30/60s) +
  per-job CAS-PAT **revoke on `workflow_job:completed`** (PR #122; fail-open). 20 vitest tests.
- **Warm wiring is code-complete + deployed, fail-open at every layer** (entrypoint `clw hydrate`
  preflight + `buildContainerEnv` warm-mint): when the mint key is present the Worker mints a per-job
  CAS PAT (D-9) and injects `CLW_*`; when absent it spawns **COLD** — a job ALWAYS runs (north star).
- **Worker secrets set:** `CLOUDFLARE_SPAWN_AUTH_TOKEN`, `GITHUB_WEBHOOK_SECRET`,
  `GITHUB_MINT_TOKEN` (all rotated off the dogfood throwaways 2026-06-20; webhook secret verified by a
  ping → HTTP 200).

**THE WARM MOAT — DONE, not pending (verified 2026-08-23):**
- `CORELINK_RUNNER_MINT_AUTH_KEY` **IS set** on `corelink-spawn-worker` (13 secrets bound; confirm with
  `npx wrangler secret list`, or the CF API `workers/scripts/corelink-spawn-worker/secrets`). It was armed
  2026-07-20 (`3f1f4b4`), so spawns are WARM. §2 below is the record of HOW it was armed — it is history,
  not a to-do. Do not re-run it against the live key.
- It is **server-gated** (LOCKED 2026-06-20, not "one paste"): the Server TL must first split the prod
  internal-auth key per-consumer (the shared key also authenticates `admin`+`erase` — must NOT reach an
  untrusted runner) and migrate signup-worker, THEN deliver the dedicated **mint-only** key OOB. Thread:
  `docs/handoff/2026-06-20-{server-tl-ANSWER-mint-key-needs-keysplit-rollout,reply-to-server-tl-accept-keysplit-rollout,server-tl-CONFIRM-keysplit-rollout-accepted}.md`.

## 1. How the autoscaler works (deployed reality, for context)

```
GitHub workflow_job:queued (label corelink-dogfood)
  └─> POST https://corelink-spawn-worker.gmhelmold.workers.dev/webhook   (repo hook 644667520)
        ├─ verify X-Hub-Signature-256 (GITHUB_WEBHOOK_SECRET)
        ├─ rate-limit (WEBHOOK_LIMITER 30/60s)
        ├─ mint JIT (generate-jitconfig, GITHUB_MINT_TOKEN)
        ├─ buildContainerEnv: IF CORELINK_RUNNER_MINT_AUTH_KEY+CLW_TENANT present →
        │     mint per-job CAS PAT (D-9, job_id = workflow_job.id) → inject CLW_*  [WARM]
        │     ELSE → JIT only                                                      [COLD, fail-open]
        └─ container.start(digest-pinned image, env)  → Firecracker microVM runner
workflow_job:completed → maybeRevokeCasPat(owner_tenant, job_id)  (best-effort; TTL backstop)
```
Non-secret config lives in `deploy/cloudflare/wrangler.jsonc` `vars`
(`CLW_TENANT=ee30f7ba…`, `CLW_ENDPOINT`/`CORELINK_MINT_URL=https://corelink-api.humangr.com`).

## 2. THE WARM FLIP — drop-day procedure (the only step left)

> Trigger: the Server TL pings the owner that signup-worker is migrated and the dedicated
> **mint-only** `CORELINK_RUNNER_MINT_AUTH_KEY` is delivered OOB. Total time: ~5 min.

### 2.0 Pre-flight (do NOT skip)
- [ ] Server TL confirms the delivered key is the **dedicated `pat_mint` consumer key**, NOT the shared
      `CORELINK_INTERNAL_AUTH_KEY` (the whole point of the key-split — a runner key must NOT authenticate
      `admin`/`erase`). If in doubt, ask before setting.
- [ ] Server TL confirms **signup-worker is already migrated** to the dedicated key (else splitting
      `pat_mint` would 401 their live signup→PAT path — their lockstep, but confirm it's done).
- [ ] The key value is on the owner's machine in a chmod-600 file, never in chat/PR/repo.

### 2.1 Set the secret
> ⚠️ **NO TRAILING NEWLINE.** A newline (or any whitespace) in an internal-auth key **silently 401s**
> the mint — the value must be byte-exact (Server TL, learned the hard way). Prefer piping with
> `printf` from the chmod-600 file; never `echo`/`cat` (both append a newline).
```sh
cd deploy/cloudflare
# SAFEST — pipe byte-exact from the OOB file (command-sub strips trailing newlines, printf adds none):
printf '%s' "$(cat ~/corelink-runner-mint-key.txt)" | npx wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY
# (Interactive `npx wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` also works, but paste with NO
#  trailing newline/space — the piped form removes that risk entirely.)
npx wrangler secret list                              # confirm 4 secrets now
```
No redeploy needed — Worker secrets are read live.

### 2.2 Verify WARM (before trusting it)
Open a log tail in one shell, dispatch a dogfood job in another:
```sh
npx wrangler tail corelink-spawn-worker            # shell A — watch the spawn
gh workflow run dogfood-smoke.yml --repo HuGR-Labs/corelink-runners   # shell B — queue a job
```
- **WARM proof (positive):** the job spawns and there is **NO** `warm-mint failed, spawning COLD: …`
  line in the tail (that line is logged only on a mint failure). The mint POST to
  `…/internal/v1/runner/mint` returns `200 {token}` for tenant `ee30f7ba…`.
- **In-job proof (explicit):** `dogfood-smoke.yml` has a `Cache-warm posture` step that prints
  `moat=WARM …` (when `CLW_ENDPOINT`+`CLW_TOKEN` were injected) or `moat=COLD …` — an unambiguous
  WARM/COLD line in the job log (presence-only; never prints the PAT value). WARM here = the per-job
  CAS PAT minted + injected; the entrypoint's `clw hydrate` then runs (cold skips it).
- **Revoke proof:** on `workflow_job:completed` the tail shows the revoke POST; a non-2xx logs
  `revoke failed (PAT will TTL-expire): …` and is harmless (TTL backstop).

### 2.3 Rollback (instant, zero-downtime)
If anything looks wrong, the moat disarms in one command — back to COLD-but-correct (fail-open):
```sh
npx wrangler secret delete CORELINK_RUNNER_MINT_AUTH_KEY
```
`buildContainerEnv` no longer mints/injects `CLW_*` → spawns cold. A job still ALWAYS runs. No redeploy.

## 3. Secret inventory + rotation

| Secret | Purpose | State | Rotate |
|---|---|---|---|
| `CLOUDFLARE_SPAWN_AUTH_TOKEN` | bearer for `/v1/*` (fabric path) | set (rotated 2026-06-20) | `openssl rand -hex 32 \| npx wrangler secret put …` |
| `GITHUB_WEBHOOK_SECRET` | `/webhook` HMAC | set (rotated+ping-verified) | rotate on BOTH sides — Worker + hook 644667520; **use canonical repo `HuGR-Labs/corelink-runners`** (the dead slugs `HumanGuardrail`/`humangr-labs` 301/307 and `gh api -X PATCH` silently no-ops) |
| `GITHUB_MINT_TOKEN` | mint JIT (`generate-jitconfig`) | set (dogfood `gh auth token`) | swap for a dedicated fine-grained PAT (repo Administration:write) — **UI-only**, before non-dogfood |
| `CORELINK_RUNNER_MINT_AUTH_KEY` | D-9 mint/revoke (`x-corelink-internal-auth`) | **NOT set** — the warm gate (§2) | a 401 on mint = drift signal → re-`put` the new value (no code change) |

## 4. Fail-open guarantee (why the flip is low-risk)

Both warm layers are fail-open, so neither setting NOR removing the key can break a job:
- `buildContainerEnv` (`deploy/cloudflare/src/lib.ts`): mint failure ⇒ logs + spawns COLD.
- `entrypoint.sh` (`deploy/runner/`): `clw hydrate` failure ⇒ swallowed ⇒ cold build.
The north star ("cache absent ⇒ slow, never broken") holds in every state: key-unset, key-set-but-mint-down,
or hydrate-miss. The flip is therefore a forward/back toggle, not a one-way door.

## 5. Honesty ledger (what's proven vs assumed)

- **Autoscaler end-to-end (cold path):** PROVEN live (dogfood smoke green ×2, self-deregister).
- **Warm path:** code-complete + unit-tested (20 vitest), **never run green E2E live** — no mint key yet.
  The first WARM smoke (§2.2) is the real proof of in-network R2 hydration. Treat it as a gated milestone.
- **Image digest:** wrangler-bound at deploy (`wrangler.jsonc containers[].image`); per-spawn `image_digest`
  is an assertion, not a pull directive (README wrinkle #1).
- **R2 binding:** the `r2_buckets` binding is intentionally omitted; hydration today is via the in-network
  **CAS HTTP API** (`corelink-api.humangr.com`), same-account = zero-egress — NOT a direct R2 bind. A direct
  bind is a possible future optimization, Cache-TL-gated.

## 6. Fabric-driven path (secondary — still exists, not the deployed autoscaler)

The Rust `CloudflareEngine` (`crates/corelink-cloud-engine/src/cloudflare.rs`) + the `/v1/spawn|status|teardown`
bearer surface remain valid: a fabric-server can drive spawns through the `Engine` seam (composition-root
selection CF→Northflank→off, ADR-0008). This is NOT in the dogfood autoscaler loop (the all-CF `/webhook` is).
To use it: set `CLOUDFLARE_SPAWN_WORKER_URL` + `CLOUDFLARE_SPAWN_AUTH_TOKEN` on the fabric env; the boot
diagnostic reports the resolved backend. Fallback to Northflank: unset both `CLOUDFLARE_SPAWN_*`
(see `dogfood-go-live.md`).
