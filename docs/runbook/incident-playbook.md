# Incident Playbook — CoreLink Runners production fabric

**It's 3am and something is broken.** This is the operate-without-the-tech-lead
playbook. Every command here is copy-pasteable and verified against the repo —
worker names, endpoints, header names, and KV bindings are the live ones, not
invented. Run the read-only probes first; they tell you which surface is sick
before you touch anything.

All commands assume `npx wrangler` from inside the relevant worker directory
(`deploy/cloudflare/` or `deploy/cloudflare-fabricd/`) so the `wrangler.jsonc`
bindings resolve. The Cloudflare account is `6a1fc1c626fc2628823e60b9db01f5cd`
(`gmhelmold`) — the same account that hosts the R2 CAS.

Secrets are covered in the companion doc: [`secret-inventory.md`](./secret-inventory.md).

---

## 0. Live topology (the two surfaces)

There are exactly **two** Cloudflare Workers in prod. Everything else is a
Durable Object or Container behind one of them.

| Surface | URL | Config dir | Role |
|---|---|---|---|
| **spawn-worker** | `https://corelink-spawn-worker.gmhelmold.workers.dev` | [`deploy/cloudflare/`](../../deploy/cloudflare/) | Autoscaler (`/webhook`) + direct runner/check-host spawn (`/v1/spawn`). Mints JIT runners, mints per-job CAS PATs, drives the reconciler cron. Runs the `RunnerContainer` / `CheckHostContainer` DOs. |
| **fabricd** | `https://corelink-fabricd.gmhelmold.workers.dev` | [`deploy/cloudflare-fabricd/`](../../deploy/cloudflare-fabricd/) | Rust control plane (RunnerLease API + §13 envelope + attestation key). A thin proxy Worker fronts ONE long-lived **singleton** container. |

The two are wired: fabricd dials the spawn-worker at `POST {url}/v1/spawn` to
provision boxes (shared `CLOUDFLARE_SPAWN_AUTH_TOKEN`). The runner boxes always
spawn on the spawn-worker (co-located with R2 → in-network cache hydration, the
ADR-0008 moat win); fabricd is only the control-plane host.

---

## 1. Health checks — is it up?

### 1a. fabricd (control plane)

```sh
HOST="https://corelink-fabricd.gmhelmold.workers.dev"

# Liveness — auth-free, expected 200 body "ok". Also answers on / and /health
# (the CF container health probe hits /, so those routes must 200 — see §2).
curl -s -o /dev/null -w '%{http_code}\n' $HOST/v1/health          # → 200

# Attestation pubkey — proves the signing key is loaded (FLIP-B live).
curl -s $HOST/v1/attestation/key                                  # → key_id faa5b7726ccd2c52...
```

A `/v1/health` timeout or non-200 = the singleton is hung or mid-cold-boot. Go
to §2 (watchdog / force restart).

### 1b. fabricd golden counters + occupancy (gated)

Both are gated by the `FABRIC_OBSERVABILITY_KEY` secret, presented in the
`X-Corelink-Internal-Auth` header. **Key unset ⇒ 404** (invisible),
**mismatch ⇒ 401**, **match ⇒ 200**. Source: `src/observability.rs`,
`tests/status_api.rs`, `tests/occupancy_api.rs`.

```sh
OBS_KEY="<FABRIC_OBSERVABILITY_KEY value — from the vault, NOT printed here>"

# Golden-signal aggregate: admission outcomes, close, mint/revoke, expiries.
curl -s -H "X-Corelink-Internal-Auth: $OBS_KEY" $HOST/internal/v1/status | jq .

# Per-tenant occupancy: occupied/peak leases + journal length.
curl -s -H "X-Corelink-Internal-Auth: $OBS_KEY" $HOST/internal/v1/occupancy | jq .
```

**Reading `/internal/v1/status` (the counters that matter at 3am):**

| Counter | Healthy | Alarming |
|---|---|---|
| `leases_acquired` | climbs steadily | flat while jobs are queued → admission is refusing (check the `acquire_rejected_*` split) |
| `acquire_rejected_over_cap` / `acquire_rejected_no_plan` | 0 or slow | spiking → tenant hit its concurrency cap, or has no plan on file (see §3d) |
| `acquire_rejected_compute_ceiling` | 0 | spiking → monthly vCPU-h ceiling reached (only arms with `DATABASE_URL`) |
| `mint_attempts` vs `mint_failures` | failures ≈ 0 | `mint_failures` climbing = the **silent-cold-hydration** seam: CAS PATs aren't minting, jobs run cold |
| `revoke_attempts` vs `revoke_failures` | failures ≈ 0 | `revoke_failures` climbing = PATs are surviving to TTL self-expiry, not being revoked (see §3b) |
| `leases_crashed` | ~0 | rising = boxes dying under leases (reaper reclaiming Held→Crashed) |
| `provision` capacity-503s | 0 | rising = box backend full (spawn-worker `max_instances` ceiling) |
| `load_shed` / `suspend_actions` | 0 | non-zero = the plane is shedding, or a tenant was operator-suspended |

`/internal/v1/occupancy` shows `per_tenant` (occupied/peak), `journal_len`,
`journal_dropped`. A non-zero `journal_dropped` means the observability journal
ring overflowed — informational, not fatal.

### 1c. spawn-worker golden counters (gated)

The spawn-worker has **no `/v1/health` of its own** — the direct-fleet health
signal is its metrics snapshot. Gated by the **separate** `METRICS_OBSERVABILITY_KEY`
secret (NOT the shared spawn token), same `X-Corelink-Internal-Auth` header.
Unset ⇒ 404, mismatch ⇒ 401, match ⇒ 200. Source: `src/index.ts` (~L767),
`src/metrics.ts`.

```sh
SPAWN="https://corelink-spawn-worker.gmhelmold.workers.dev"
MET_KEY="<METRICS_OBSERVABILITY_KEY value — from the vault>"

curl -s -H "X-Corelink-Internal-Auth: $MET_KEY" $SPAWN/internal/v1/metrics | jq .
```

**Reading the direct-fleet counters** (fixed set, 0-filled — `COUNTER_NAMES`):

| Counter | Meaning | Alarming when |
|---|---|---|
| `webhook_spawn_claimed` | queued+labeled job claimed for mint+spawn | flat while CI is queued → webhook not arriving (see §4) |
| `runner_spawned` | a `RunnerContainer` started | lags far behind `webhook_spawn_claimed` → spawns failing |
| `spawn_failed` | mint/spawn threw (claim released for re-drive) | climbing → mint or container start broken |
| `spawn_forbidden` | mint authorization returned forbidden | non-zero → App/token can't mint on that repo |
| `spawn_at_ceiling` | per-tenant concurrency ceiling hit | expected under load; sustained = raise cap or investigate |
| `jit_minted` | GitHub JIT runner config minted | flat while claiming → GitHub mint path down (§4) |
| `cas_pat_revoked` | per-job CAS PAT revoked at completion | lags `webhook_job_completed` → orphaned PATs (§3b) |
| `billing_pushed` | usage event emitted | lags completions → billing ingest down |

**Counters reset to 0 on a worker redeploy of fabricd's container** (in-memory).
The spawn-worker's `MetricsDO` is **durable** (survives redeploy) — so a spawn-worker
metrics reset means the DO was migrated/wiped, which is unusual and worth noting.

---

## 2. fabricd is a singleton — how it self-heals and how to restart it

The control plane runs as **ONE** container: `max_instances: 1` + a fixed DO id
in `src/index.ts`, because the in-memory lease ledger requires every `/v1`
request to hit the SAME process. This is deliberate (raising N is a separate
owner-gated flip — see the RAISE-N handoff). Two consequences you must know at 3am:

### 2a. The watchdog self-heals a hung singleton (no human needed for the common case)

The proxy Worker's `scheduled()` cron runs **every minute** (`* * * * *`,
`wrangler.jsonc` `triggers.crons`). It:

1. Probes each shard's `http://fabricd/v1/health` (source: `src/index.ts` ~L545).
2. **Requires 3 consecutive failures** (`PROBES = 3`, ~30s of misses) before
   acting — a single missed probe during a legitimate long §13 close does NOT
   trip it (that over-eager destroy was the 2026-07-08 incident; the gate is the fix).
3. On 3 consecutive failures it calls `container.destroy()`. A **fresh instance
   cold-boots on the next request** — no manual step.

The same cron is the **keep-warm**: pinging `/v1/health` each minute stops the
container ever hitting `sleepAfter` (`1h`). So a healthy singleton never sleeps.

**If `/v1/health` is failing but the watchdog hasn't recovered it:** give it
~2–3 cron ticks (up to ~3 min). Watch `wrangler tail` for the
`keep-warm[shard 0/1]: ... destroying hung shard` line. If it never fires, or
the fresh boot also fails, force a restart (§2b) and check the boot log.

### 2b. Force a restart (rollout a new image digest)

The singleton reads its env **only at boot**. Saving a secret/var alone does NOT
restart it. The mechanism to force a true restart (and to pick up freshly-set
secrets) is a **container rollout**, triggered by changing the image digest in
`deploy/cloudflare-fabricd/wrangler.jsonc` and running `wrangler deploy`:

```sh
cd deploy/cloudflare-fabricd
# The image is pinned by @sha256 digest in wrangler.jsonc (containers[].image).
# A config-only change may NOT roll the container; a NEW digest always does.
# To rebuild the binary (needs a Docker host): npx wrangler containers build <repo-root> \
#   -t corelink-fabricd-fabricdcontainer:<tag> --push  → take the pushed @sha256 digest.
npx wrangler deploy
```

To confirm the live image / instance after a rollout:

```sh
npx wrangler containers info    # shows the running image digest
```

**In-memory counters reset on every restart** — `leases_acquired`,
`mint_attempts`, etc. all go back to 0. That is expected and NOT data loss.
**Lease STATE persists only if `DATABASE_URL` (pg ledger) is set** — otherwise
the in-memory ledger also resets and any Held leases are forgotten (acceptable
at dogfood; the reaper/box teardown reconciles). The keep-warm counter starting
back at `...0001` is the diagnostic that a real restart happened.

---

## 3. Known failure modes (and the exact unblock)

### 3a. The `spawn:`-claim deadlock — a wedged reconciler

**Symptom.** CI jobs sit `queued`, no runner ever attaches, and
`webhook_spawn_claimed` climbed but `runner_spawned` did not. A leaked spawn
claim in KV is blocking re-drive: the autoscaler sets a per-`jobId` claim in KV
**before** the expensive mint+spawn (`claimSpawn`, `src/lib.ts` ~L83), and if the
spawn crashed without releasing it, the reconciler treats the job as
already-claimed and never re-drives it.

**The KV.** Binding **`RUNNER_JOB_PATS`**, namespace id
`4fb7e9c773d64f83ae3415c5a0879d66` (`deploy/cloudflare/wrangler.jsonc`). The claim
key is **`spawn:<jobId>`** with a 7200s TTL (`SPAWN_CLAIM_TTL_S`). Related keys in
the same namespace: `conc:<tenant>:<jobId>` (tenant concurrency slots),
`jtenant:<jobId>` / job-handle / bare `<jobId>` (job→pat_id map), `complete:<jobId>`
(completion idempotency), `ghtok:<installationId>` (App-token cache).

**Inspect + unblock** (run from `deploy/cloudflare/` so the binding resolves):

```sh
cd deploy/cloudflare

# 1. List the leaked spawn claims.
npx wrangler kv key list --binding RUNNER_JOB_PATS | jq -r '.[].name' | grep '^spawn:'

# 2. Confirm the specific job is stuck (value is just "1").
npx wrangler kv key get "spawn:<JOB_ID>" --binding RUNNER_JOB_PATS

# 3. Delete the leaked claim → the next reconciler tick (≤1 min) re-drives it.
npx wrangler kv key delete "spawn:<JOB_ID>" --binding RUNNER_JOB_PATS
```

(If your wrangler version rejects `--binding` for `kv key`, substitute
`--namespace-id 4fb7e9c773d64f83ae3415c5a0879d66`.)

The claim self-heals after the 7200s TTL anyway, but deleting it unblocks the
job immediately. `wrangler tail` on the spawn-worker will show the re-drive.

### 3b. Orphaned CAS PATs (revoke didn't run)

**Symptom.** `revoke_failures` (fabricd) or a lagging `cas_pat_revoked`
(spawn-worker) — per-job PATs are surviving past job completion. This is
**fail-safe**: every per-job PAT is tenant-scoped `cas:rw` and TTL-expires on its
own; a missed revoke just means it lives to its TTL instead of being killed early.

The revoke keys on `pat_id`, read from the `<jobId> → pat_id` map in
`RUNNER_JOB_PATS` (written at mint, read+deleted at completion — `src/index.ts`
~L523/L554). If a job completed but the PAT wasn't revoked:

```sh
cd deploy/cloudflare
# Find the pat_id the mint stashed for the job.
npx wrangler kv key get "<JOB_ID>" --binding RUNNER_JOB_PATS
```

If the map entry is gone but the PAT is still live, the safe action is to **let
the TTL expire it** (do not hand-craft a revoke without the tech lead — the live
`/revoke` contract requires `pat_id`, not `job_id`). Escalate only if a specific
tenant's PATs are visibly not expiring.

### 3c. A job stuck `queued` with no runner

Walk it in this order:

1. **Did the webhook arrive?** `webhook_spawn_claimed` flat = GitHub never
   delivered the `workflow_job.queued` event (or HMAC failed). Go to §4.
2. **Is a `spawn:` claim leaked?** §3a.
3. **Is the autoscaler configured?** `POST /webhook` returns `503 "autoscaler not
   configured"` if `GITHUB_WEBHOOK_SECRET` or `GITHUB_MINT_TOKEN` is unset. Check
   presence per [`secret-inventory.md`](./secret-inventory.md).
4. **Right label + allowlisted repo?** The job must carry the managed label
   (default `corelink-dogfood`) and, for the reconciler re-drive, its repo must be
   in `RECONCILER_REPOS` (`HumanGuardrail/corelink-runners`). A stranger repo's
   job with no App installation mints **COLD** or not at all.
5. **At ceiling?** §3d.

### 3d. Capacity at ceiling

- **Per-tenant concurrency cap** — spawn-worker `spawn_at_ceiling` climbs;
  fabricd `acquire_rejected_over_cap` climbs. The tenant bought N seats and N are
  busy. Expected under load. `/internal/v1/occupancy` shows `occupied` == the cap.
- **No plan on file** — fabricd `acquire_rejected_no_plan` (de-smeared from
  `over_cap` deliberately): the tenant has zero purchased concurrency. This is a
  billing/onboarding gap, not an outage.
- **Compute ceiling** — `acquire_rejected_compute_ceiling`: the monthly vCPU-h
  ceiling (only arms when `DATABASE_URL` is set). Owner decision to raise.
- **Box backend full** — provision capacity-503: the spawn-worker's container
  `max_instances` (runner=6, check-host=4 in `wrangler.jsonc`) is saturated. Raise
  `max_instances` + `wrangler deploy` if the account has room (PAYG, ample).

---

## 4. Re-mint / re-arm the GitHub credential + webhook

### 4a. Where the mint lives

- **First-party (dogfood) JIT mint** uses a static token `GITHUB_MINT_TOKEN`
  (repo `Administration:write`), only valid on `HumanGuardrail` repos.
- **Customer-repo JIT mint** uses a **GitHub-App installation token**, built in
  [`deploy/cloudflare/src/github_app.ts`](../../deploy/cloudflare/src/github_app.ts):
  1. `appJwt(appId, privateKeyPem, now)` — a short-lived RS256 App JWT
     (`iss = GITHUB_APP_ID`, `exp ≤ 10 min`). The key `GITHUB_APP_PRIVATE_KEY`
     MUST be a **PKCS#8** PEM (`openssl pkcs8 -topk8 -nocrypt …`); a PKCS#1 PEM
     throws at import and the mint **fails closed** (never a silent wrong token).
  2. `installationToken(...)` — exchanges the JWT for a per-installation token via
     `POST /app/installations/{id}/access_tokens`, cached in KV at
     `ghtok:<installationId>`.
- The live GitHub App is installation **144561227** (dogfood
  `HumanGuardrail/corelink-runners`). The App already exists — reuse it, do not
  create a new one.

### 4b. Verify the App credential is healthy (App-JWT → installation-token probe)

Do this from a trusted workstation with the App private key in a file (NEVER in
the repo, NEVER echoed). This proves the key signs and GitHub accepts it:

```sh
APP_ID="<GITHUB_APP_ID>"
PEM=~/secure/corelink-app.pkcs8.pem     # PKCS#8; delete after use

# Build a 9-minute RS256 App JWT.
now=$(date +%s); iat=$((now-60)); exp=$((now+540))
header=$(printf '{"alg":"RS256","typ":"JWT"}' | openssl base64 -A | tr '+/' '-_' | tr -d '=')
payload=$(printf '{"iat":%d,"exp":%d,"iss":"%s"}' "$iat" "$exp" "$APP_ID" | openssl base64 -A | tr '+/' '-_' | tr -d '=')
sig=$(printf '%s.%s' "$header" "$payload" | openssl dgst -sha256 -sign "$PEM" -binary | openssl base64 -A | tr '+/' '-_' | tr -d '=')
JWT="$header.$payload.$sig"

# App-level probe: identity + webhook config (proves the JWT is accepted).
curl -s -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github+json" \
  https://api.github.com/app | jq '{slug, id}'

# Installation-token probe (the exact mint path github_app.ts runs).
curl -s -X POST -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github+json" \
  https://api.github.com/app/installations/144561227/access_tokens | jq 'keys'
```

A 201 with a `token` field = the App credential is healthy. A 401 = the private
key is wrong/rotated/mangled (re-set `GITHUB_APP_PRIVATE_KEY`, PKCS#8, then
`wrangler deploy`). A 404 = the installation id is wrong.

### 4c. The webhook route + how to inspect deliveries

The GitHub **App** webhook is routed to the spawn-worker's **`POST /webhook`**,
authenticated by HMAC (`X-Hub-Signature-256`) against `GITHUB_WEBHOOK_SECRET`
(`src/index.ts` ~L779, `verifyGithubHmac`). A bad/absent signature ⇒ 401; only
`workflow_job.queued` with the managed label triggers a spawn.

Manage + debug the webhook via the GitHub App webhook API (needs the App JWT
from §4b):

```sh
# Current webhook config (URL + whether a secret is set — value never returned).
curl -s -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github+json" \
  https://api.github.com/app/hook/config | jq .

# Recent deliveries — inspect for non-2xx responses (a 401 here = HMAC mismatch
# → GITHUB_WEBHOOK_SECRET on the worker ≠ the App's configured secret).
curl -s -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github+json" \
  https://api.github.com/app/hook/deliveries | jq '.[] | {id, event, action, status_code: .status_code}'

# Update the delivery URL if the worker moved (rare).
curl -s -X PATCH -H "Authorization: Bearer $JWT" -H "Accept: application/vnd.github+json" \
  https://api.github.com/app/hook/config \
  -d '{"url":"https://corelink-spawn-worker.gmhelmold.workers.dev/webhook","content_type":"json"}'
```

**Known good-path detail:** a plain *repo* webhook payload has no
`installation.id`, so first-party repos are given one via the
`REPO_INSTALLATION_MAP` var (`{"HumanGuardrail/corelink-runners":"144561227"}`)
so the server-derived mint runs **WARM** (cache-warm) without an App webhook.
If dogfood suddenly spawns COLD, check that map matches
`repository.full_name` exactly.

---

## 5. Escalation + references

- **Read the boot log first** on any fabricd weirdness: `wrangler tail` on
  `corelink-fabricd` shows the boot-time arm-state line (whether
  `FABRIC_OBSERVABILITY_KEY` + `FABRIC_ADMIN_KEY` + the mint arm are present) and
  the `cloud_backend_status` line — it will never claim a backend it isn't running.
- **`wrangler tail`** on either worker is the live log. Both have
  `observability.enabled: true`.
- The fabricd **boot guard `validate_mint_arm` fails closed**: if the moat mint is
  armed without `FABRIC_PUBLIC_BASE_URL`, the container refuses to boot healthy —
  so a healthy `/v1/health` proves the cred-redemption endpoint is wired.

**Code + doc references:**
- Spawn-worker: [`deploy/cloudflare/src/index.ts`](../../deploy/cloudflare/src/index.ts),
  [`lib.ts`](../../deploy/cloudflare/src/lib.ts),
  [`metrics.ts`](../../deploy/cloudflare/src/metrics.ts),
  [`github_app.ts`](../../deploy/cloudflare/src/github_app.ts),
  [`wrangler.jsonc`](../../deploy/cloudflare/wrangler.jsonc),
  [`README.md`](../../deploy/cloudflare/README.md).
- fabricd: [`deploy/cloudflare-fabricd/src/index.ts`](../../deploy/cloudflare-fabricd/src/index.ts),
  [`wrangler.jsonc`](../../deploy/cloudflare-fabricd/wrangler.jsonc),
  [`README.md`](../../deploy/cloudflare-fabricd/README.md);
  Rust counters [`crates/corelink-fabric-server/src/observability.rs`](../../crates/corelink-fabric-server/src/observability.rs).
- Secret inventory + rotation: [`secret-inventory.md`](./secret-inventory.md).
- Multi-instance scaling (why N=1, how to raise N):
  `docs/handoff/2026-07-09-RAISE-N-readiness-tracked-gaps-multi-instance-fabricd-not-a-pure-config-flip.md`.
- Stabilization / hidden-debt plan:
  `docs/handoff/2026-07-16-STABILIZATION-plan-3lens-hidden-debt.md`.
</content>
</invoke>
