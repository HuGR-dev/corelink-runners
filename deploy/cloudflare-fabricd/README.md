# Deploying `corelink-fabricd` on Cloudflare Containers (gap-#1 option b)

The Rust control plane (RunnerLease API · §13 envelope · attestation key) as ONE
singleton CF Container, fronted by a thin proxy Worker, kept warm 24/7 by a cron
ping. The runner BOXES still spawn on `../cloudflare` (the spawn-Worker); this is
only the control-plane host. Env matrix + checkpoints: `docs/deploy/fabric-server.md`.

## Prerequisites (the only owner/machine actions)
1. **Docker daemon running** — the deploy builds the image locally (`cargo build
   --release` of the workspace). Start Docker Desktop: `open -a Docker`, wait for
   `docker info` to succeed.
2. **wrangler authed** — already logged in (`gmhelmold@gmail.com`); verify with
   `npx wrangler whoami`.

## Deploy
```sh
cd deploy/cloudflare-fabricd
npm install

# 1. Secrets (NEVER in wrangler.jsonc). Values come from the OOB secrets dir;
#    piped from file so the value is never echoed.
npx wrangler secret put FABRIC_SIGNING_KEY        < ~/.hugit/secrets/corelink/fabric-signing-key-prod
npx wrangler secret put FABRIC_INTROSPECT_AUTH_KEY < ~/.hugit/secrets/corelink/fabric-introspect-key
npx wrangler secret put BILLING_INGEST_AUTH_KEY    < ~/.hugit/secrets/corelink/billing-ingest-key

# 2. Deploy (builds + pushes the image, creates the Worker + container + DO + cron).
npm run deploy
```

The prod signing key was generated 2026-06-25 (32-byte ed25519, fingerprint
`9f54d5ee`); its PUBLIC half — `key_id faa5b7726ccd2c52`,
`pubkey_b64 Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=` — is what
`GET /v1/attestation/key` will serve and what hugit's v2 verifier pins. Setting a
DIFFERENT key changes that pubkey, so use that exact file.

## Smoke (checkpoint A/B/C)
```sh
HOST="https://corelink-fabricd.<account-subdomain>.workers.dev"   # printed by deploy
curl -s $HOST/v1/health                       # → ok
curl -s $HOST/v1/attestation/key              # → key_id faa5b7726ccd2c52 (the prod pubkey)
# acquire with a real tenant PAT → 200 Held; GET .../envelope/meta → 200 (not 404)
```
Then hand `$HOST` to the hugit TL as `HUGIT_RUNNER_HOST` + the spawn/lease PAT
(`HUGIT_RUNNER_PAT`), per the frozen Seam 1.

## Container health probe (why `/` + `/health` answer 200)

CF Containers probes the default port on `/` to mark an instance **healthy**.
fabricd originally served only `/v1/*`, so the probe 404'd → the instance stayed
`healthy:0` → **CF reverted rollouts** (a new image silently rolling back to the
prior one — observed 2026-07-08). Fix: `app.rs` mounts `/` and `/health` →
auth-free `200 "ok"` (same as `/v1/health`). Verified: after the fix the instance
reports **`healthy:1`** and rollouts complete + stick. Keep these routes.

## Single-flight singleton — fragility, mitigations, scaling path

The control plane runs as ONE container (`max_instances: 1` + a fixed DO id
`SINGLETON` in `src/index.ts`), so all `/v1` traffic serializes through one
instance. Two failure modes were observed + closed on 2026-07-07/08:

- **Box-provision burst** (2026-07-07 acquire-storm): a burst of provisioning
  acquires pinned the blocking pool. Closed by (a) **off-box leases provision no
  box**; (b) provision HTTP bounded to 30s; (c) a **`FABRIC_PROVISION_MAX_INFLIGHT`
  semaphore** (default 16) bounds concurrent provisions (excess awaits a permit
  async, not on a thread).
- **Single close wedged the plane** (2026-07-07): ONE off-box close black-holed
  `/v1/health` on the 1-vCPU box. Root cause: the close's `block_in_place` pg
  work (`pg_ledger.rs`) runs ON a runtime worker; on 1 vCPU (1 worker) the whole
  runtime stalls. Closed by **`standard-2` (2 vCPU / 2 workers)**: the blocking
  work pins one worker, the other keeps `/v1/health` alive. **Verified 2026-07-08:**
  health stayed `200` across all 30 polls (0.4–1.0s) through a 32s close.

**Close latency — diagnosed, NOT a bug (2026-07-08).** A raw close (e.g. `curl`)
takes ~32s, but that is the **§13.2 JobClose ack window** (`ack_timeout`, hardcoded
`Duration::from_secs(30)` at `leases.rs:964`): every off-box/agent lease registers
a §13 CaptureHook at acquire, and the close blocks up to 30s (fail-closed) waiting
for the client's **JobClose ack**. A non-acking test client waits the full 30s; a
REAL acking client (hugit's A-path — proven metrics round-trip) collapses the
window to ~0 and the close returns in **~2.7s** (teardown + attestation + 3 pg
writes). So the close is fast for real traffic — the "slow close" was a
non-acking-test artifact, not pg latency. The pg work itself is ~2.7s; no offload
needed at current scale. The real requirement — the plane staying UP during any
long ack-wait — is handled (`close_ack_gate` bounds concurrent ack-waits +
`standard-2` keeps a worker for health; verified).

**Scaling path (not yet done):** the singleton was required only by the in-memory
ledger; now that the **pg ledger is armed** (`DATABASE_URL` present →
cross-instance cap-safe via the advisory lock), the plane CAN run multiple
instances — remove the fixed DO id (route per-request / round-robin) + raise
`max_instances`. A tracked scaling enhancement for **before rota-A carries real
check-host bursts**; a known limit, not debt.

## Boxes (checkpoint B+ — when wiring real per-job metrics)

Until a box backend is wired the lease/§13/attestation surface is live but `exec`
returns 503 (no box backend) — fail-closed, exactly as the dress-rehearsal showed.

⚠️ **The substrate you wire decides which lease KINDS run — this is the trap that
broke prod once (#195).** `CloudflareEngine` v0 is **runner-only by design** (ADR-0007:
the spawn-Worker's only container is the GitHub-Actions runner image). So:

- **Cloudflare ONLY** (`CLOUDFLARE_SPAWN_WORKER_URL` var + `CLOUDFLARE_SPAWN_AUTH_TOKEN`
  secret) → **runner** leases spawn on Cloudflare, but a **CHECK-exec** lease
  (`allow_egress=false`) **fails CLOSED at spawn** (#198). The killer (memoized CI /
  per-PR attested cost) dispatches CHECK-exec leases → **Cloudflare-only does NOT serve
  the killer.** Wiring CF alone and pointing the killer at it is the #195 regression.

- **Rota B — BOTH substrates** (`CLOUDFLARE_SPAWN_*` **and** `NORTHFLANK_API_TOKEN`
  + `NORTHFLANK_PROJECT_ID`) → the composition selects the **Hybrid** backend
  (`select_backend(true,true)`): **runner→Cloudflare** (the R2-co-located moat),
  **check-exec→Northflank**. This is what the killer needs. Set all four and redeploy.

```bash
# wrangler.jsonc vars:  CLOUDFLARE_SPAWN_WORKER_URL, NORTHFLANK_PROJECT_ID
# (+ NORTHFLANK_RUNNER_* tuning as needed; see docs/deploy/fabric-server.md)
npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN   < ~/.hugit/secrets/corelink/cf-spawn-token
npx wrangler secret put NORTHFLANK_API_TOKEN          < <northflank token, OOB>
# OPS GOTCHA: a config-only redeploy does NOT restart the singleton container
# (envVars are read at container start). Force it:
npx wrangler containers delete <app-id> && npx wrangler deploy
```

After the env is live, smoke BOTH kinds before handing the host to the killer: a runner
acquire → 200 Held + a CF `/v1/spawn` fired; a check acquire → 200 Held + provisioned on
Northflank (not Cloudflare). The `tests/hybrid_flip_e2e.rs` e2e pins this routing offline;
the live smoke confirms the real backends. **Never claim boxes work off the boot log alone
— prove an end-to-end spawn of each kind** (the #195 lesson: "substrate wired" ≠ spawn works).

## Status
✅ **DEPLOY-VERIFIED + LIVE (2026-07-07)** at `https://corelink-fabricd.gmhelmold.workers.dev`.
The container+Worker+cron glue is confirmed end-to-end: `/v1/health → 200 ok` (stable),
`/v1/attestation/key → key_id faa5b7726ccd2c52` (the OOB prod key), and a **check-host acquire
returned 200 Held** — proving the **rota-A** binary (image `@sha256:d26a46c4…`, built from `main`
via `wrangler containers build`) routes check-exec to Cloudflare (a pre-rota-A runner-only binary
fails closed). CF-native: introspect auth + CF spawn-Worker box backend, **no Northflank**.

**Not yet cut over:** hugit still points at the Northflank fabricd. Cutover = repoint
`HUGIT_RUNNER_HOST` + re-pin the pubkey (`b1eba792…` → `faa5b7726…`); PAT unchanged (same introspect).
See `docs/handoff/2026-07-07-CUTOVER-READY-to-hugit-TL-…`. Trigger is the owner's.

**Ops note:** to rebuild the binary — temp-copy `crates/corelink-fabric-server/Dockerfile` to the repo
root, `npx wrangler containers build <repo-root> -t corelink-fabricd-fabricdcontainer:<tag> --push`
(the root `.dockerignore` keeps the context small), pin the returned `@sha256` digest in
`wrangler.jsonc`, `wrangler deploy` (a NEW digest forces the container rollout; a config-only change
does not). Docker daemon required for the build only.
