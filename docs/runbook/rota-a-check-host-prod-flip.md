# Runbook — Rota A (native CF check-exec) prod flip

> ## ✅ COMPLETED — HISTORICAL RECORD (annotated 2026-08-23)
>
> The rota-A flip described here was **completed 2026-07-08** (`ee5d245`:
> "deploy(fabricd): FLIP — arm the moat (CAS-cred mint + hydrate), rota-A fully live").
> Nothing below is pending work.
>
> The Northflank host named in the state section is also historical: the fabric moved to
> Cloudflare in the 2026-07 substrate flip (ADR-0008). The live deploy runbook is
> `docs/runbook/cloudflare-go-live.md`.


**Status 2026-07-07:** engineering DONE + merged (#310 core / #311 e2e / #312 fail-closed) and the CF
Containers runtime is **live-smoke PROVEN** (isolated worker: check-mode spawn → toolchain hydrate from
CAS → `/v1/exec` → CheckResult → teardown, all green). The flip is now a **config/deploy operation, not
an engineering risk.** This runbook lists the exact remaining steps. Steps marked 🔒 need **Northflank
access** (infra/owner) — the runners-TL session does not hold Northflank creds.

## What's already done (no action)
- ✅ Worker (`corelink-spawn-worker`, prod): check-host routes (`/v1/spawn mode:check`, `/v1/exec`),
  `EXEC_SERVER_AUTH_TOKEN` secret SET (verified), check-host image deployed (`@sha256:3b694…`).
- ✅ Worker cost/security: no unauth path spawns a container (`/v1/*` bearer-gated, `/webhook` HMAC+
  ratelimit); bots get cheap 401/404.
- ✅ fabricd is live + healthy on Northflank (`https://p01--corelink-runners--pmk6nf8xbcjb.code.run`,
  `/v1/health` → 200) — but on the PRE-rota-A binary until step 1.
- ✅ Composition proven: `cloud_exec::tests` (exec dispatch), `hybrid_flip_e2e` (check-host→CF provisioning
  through the real handler), `rota_a_check_host_exec_e2e` (full acquire→exec→CheckResult + fail-closed),
  spawn-Worker suite (100), + the live CF-runtime smoke.
- ✅ fabricd's HTTP client (`ureq`) is NOT edge-blocked by the prod worker (all UAs get the worker's 401,
  not a CF `1010`) — no UA-blocking latent bug for fabricd→worker `/v1/spawn`+`/v1/exec`.

## Step 1 🔒 — Rebuild + redeploy fabricd on the rota-A binary
The rota-A wiring (`HybridLeasedExec`, `with_paired_exec`) is a **binary** change — env alone does not add
it. fabricd builds from `main` via `crates/corelink-fabric-server/Dockerfile` (Northflank service
`corelink-fabricd`, `buildSource: git`, `projectBranch: main`).
- If Northflank **continuous-deploy on push** is ON, the #310–#312 merges already rebuilt it — confirm the
  live build SHA ≥ `03786c5` (the #312 merge).
- If OFF, trigger a manual build/deploy of `corelink-fabricd` from `main`.

## Step 2 🔒 — Ensure the CF box backend env on fabricd
For a check-host lease to route to Cloudflare, fabricd's box engine must be CF (or Hybrid). Per
`docs/deploy/fabric-server.md`, set on the `corelink-fabricd` Northflank service:
- `CLOUDFLARE_SPAWN_WORKER_URL` = the prod worker base URL (`https://corelink-spawn-worker.gmhelmold.workers.dev`).
- `CLOUDFLARE_SPAWN_AUTH_TOKEN` = **must byte-match the worker's `CLOUDFLARE_SPAWN_AUTH_TOKEN`** (the shared
  bearer; write-only on both — whoever provisioned it at CF go-live holds the value / it lives in fabricd's
  existing env). If fabricd already has it from a prior go-live, this is a no-op; confirm it's present.
- Backend selection (`cloud_exec::select_backend`): **both** CF + `NORTHFLANK_*` present ⇒ **Hybrid**
  (runner + check-host → CF, plain hermetic checks → Northflank). **CF only** ⇒ runner + check-host → CF,
  plain hermetic checks fail closed at spawn. The northflank-service.json template has `NORTHFLANK_*` but
  NOT the CF vars — so confirm the LIVE service has the CF vars (else it's Northflank-only = rota B, and
  check-host never reaches CF).

## Step 3 — Toolchain snapshot in the acquire tenant (#68, hugit-owned)
A real check-host lease hydrates its toolchain from CAS under the **fabric-authenticated tenant of the
acquiring PAT** (`leases.rs:741`, see `2026-07-07-REPLY-to-clw-and-hugit-TL-check-host-tenant-coordination…`).
So hugit snapshots its CI toolchain (recipe: clw #68 b-run, debian:12-slim) and **pushes to the tenant its
check leases acquire under**, then hands back the `toolchain_digest` (the snapshot `.root`). Hydrate dest is
`/toolchain` (Dockerfile `TOOLCHAIN_DIR`), exec cwd `/toolchain`, **PATH not auto-set** → the CheckDef
command must resolve its tools from `/toolchain`.

## Step 4 — Post-flip live smoke (proves the prod path, not just the runtime)
With fabricd on rota-A + CF env, and a toolchain digest `D` in tenant `T`:
1. Acquire a check-host lease against fabricd with a tenant-`T` PAT: `POST /v1/leases`
   `{image_digest:<pinned>, net_policy:"none", runner:null, toolchain_digest:"D", …}` → `200 Held`.
2. `POST /v1/leases/{id}/exec` `{check_def:{…, toolchain_ref:"D"}, tree_hash:…}` → `200` with a
   `CheckResult` (the check ran on the CF check-host — the moat). A CF `/v1/exec` failure → `503 fail_closed`
   (proven in `rota_a_check_host_exec_e2e`), never a fabricated result.
3. Confirm via billing/observability that the box ran on Cloudflare, not Northflank.
The **isolated-worker equivalent of this smoke is already GREEN** (2026-07-07) — step 4 just re-proves it on
the prod control plane.

## Rollback
Rota A is DEFAULT-OFF by construction: a check acquire WITHOUT `toolchain_digest` is a plain hermetic check
→ Northflank (rota B), byte-identical to today. To disable check-host-on-CF entirely, remove the CF vars
from fabricd (→ Northflank-only) or stop sending `toolchain_digest` on check acquires. No code change.

## Who does what
- 🔒 Steps 1 + 2 (Northflank rebuild + env): **owner / infra** (Northflank creds).
- Step 3 (toolchain snapshot): **hugit TL** (their toolchain + their tenant).
- Step 4 (post-flip smoke): **runners TL** can drive it given a tenant PAT + the digest.

— corelink-runners TL
