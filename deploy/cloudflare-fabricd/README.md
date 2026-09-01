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

## Optional arming vars (default-off; now forwarded into the container)

These are all optional — absent, the corresponding surface stays inert/404. The
Worker **forwards** them into the container (previously it did not, so setting any
of these was a silent no-op):

- `FABRIC_ADMIN_KEY` (secret) — arms the operator enforcement routes: tenant
  SUSPEND + admin tenant onboarding (absent ⇒ those routes 404).
- `FABRIC_OBSERVABILITY_KEY` (secret) — arms the internal observability/occupancy
  endpoint (absent ⇒ 404).
- `FABRIC_RUNNER_REPO_ALLOWLIST` (var, CSV `owner/repo,…`) — bounds `runner:`
  acquires to allowlisted repos (absent ⇒ unbounded).
- Stage-B autoscaler set (all optional; the route only mounts when
  `FABRIC_AUTOSCALER_WEBHOOK_SECRET` is set): `FABRIC_AUTOSCALER_WEBHOOK_SECRET`,
  `FABRIC_AUTOSCALER_PAT`, `FABRIC_AUTOSCALER_RUNNER_IMAGE`,
  `FABRIC_AUTOSCALER_LABELS`, `FABRIC_AUTOSCALER_TMP_ROOT`,
  `FABRIC_AUTOSCALER_EXPIRY_MS`, `FABRIC_AUTOSCALER_REPO_ALLOWLIST`,
  `FABRIC_AUTOSCALER_MAX_TRACKED_JOBS`.

Set secrets via `wrangler secret put <NAME>`; set vars in `wrangler.jsonc`'s `vars`
block.

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
ledger. The **pg ledger has two gates**: a non-empty `DATABASE_URL` secret **and**
the byte-exact Worker var `FABRIC_PG_DISABLED="0"`. Only then does the Worker
forward `FABRIC_LEDGER_BACKEND=pg`, the URL, `FABRIC_RUNNER_VCPU=4`, and the
pg-only billing export, giving cross-instance cap-safety via the advisory lock.
Unset, blank, whitespace-padded, or any other value of `FABRIC_PG_DISABLED`
fails closed to the in-memory ledger even when the URL secret remains bound.

The current containment posture is `FABRIC_PG_DISABLED="1"`, so the live deploy
is deliberately in-memory (single-instance, state lost on restart). To arm safely,
prepare and validate the database while that `"1"` remains deployed; change the
tracked var to exact `"0"` only after the fixed configuration passes; deploy and
recreate the container; then require `/internal/v1/status` to report
`ledger_cross_instance_safe: true` before raising `FABRIC_NUM_SHARDS` or
`max_instances`. Health 200 alone does not prove PG, because memory is healthy too.
Rollback reverses the gate first: restore exact `"1"`, deploy/recreate and verify
the in-memory posture before deleting or rotating `DATABASE_URL`. See
`docs/runbook/arm-fabricd-pg-ledger-vcpu-ceiling.md` for the ordered procedure.

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
✅ **MOAT LIVE + GENUINELY PROVEN (2026-07-09)** at `https://corelink-fabricd.gmhelmold.workers.dev`.
The live image is `@sha256:91f4b7ea…` (the #332 cred-redemption-fix binary, tag
`golive-20260709-credredemption` — see wrangler.jsonc for the pin). It adds the
`validate_mint_arm` boot guard: a healthy boot now PROVES `FABRIC_PUBLIC_BASE_URL` is wired, so
the moat's per-job PAT can actually be redeemed (the earlier `cb6fca46…` moat-fix binary minted a
real PAT the box could never redeem — go-live audit wf_63a2b814). `/v1/health → 200 ok`,
`/v1/attestation/key
→ key_id faa5b7726ccd2c52` (prod key). The per-job CAS PAT mint is proven REAL (a hydrating
check-host acquire went 503→200 across the `token_plaintext` response-parse fix — a mint-armed
transition a cold-run could never produce), and the attested-cost `intent_metrics_sig` rides the
close. CF-native: introspect auth + CF spawn-Worker box backend, **no Northflank**.

⚠️ **Two go-live bugs the earlier "proven" reads MISSED** (both fixed): (1) `index.ts` didn't
forward the mint/cred/emit vars into the CONTAINER (only the Worker saw them) → mint OFF → cold-run
200 masked it; (2) `MintResponseBody` read `token` but the server sends `token_plaintext` →
fail-closed on every real mint. Lesson: a 200 on a hydrating acquire does NOT prove a mint — only a
mint-armed 503→200 transition (or a server-side mint-request log) does.

**Not yet cut over:** hugit still points at the Northflank fabricd. Cutover = repoint
`HUGIT_RUNNER_HOST` + re-pin the pubkey (`b1eba792…` → `faa5b7726…`); PAT unchanged (same introspect).
See `docs/handoff/2026-07-07-CUTOVER-READY-to-hugit-TL-…`. Trigger is the owner's.

**Ops note:** to rebuild the binary — temp-copy `crates/corelink-fabric-server/Dockerfile` to the repo
root, `npx wrangler containers build <repo-root> -t corelink-fabricd-fabricdcontainer:<tag> --push`
(the root `.dockerignore` keeps the context small), pin the returned `@sha256` digest in
`wrangler.jsonc`, `wrangler deploy` (a NEW digest forces the container rollout; a config-only change
does not). Docker daemon required for the build only.
