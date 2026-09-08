# Deploying `corelink-fabricd` on Cloudflare Containers (gap-#1 option b)

The Rust control plane (RunnerLease API · §13 envelope · attestation key) runs as
ONE singleton CF Container, fronted by a thin proxy Worker. The container stays
available while real activity is recent; after 5m without a real request it may
scale to zero and cold-start on the next request. The minute cron is an
activity-gated watchdog: it probes only after a recent real request and refuses
to wake an idle or uncertain shard. The runner BOXES still spawn on
`../cloudflare` (the spawn-Worker); this is only the control-plane host. Env
matrix + checkpoints: `docs/deploy/fabric-server.md`.

> **Operational safety:** deploy, container delete/restart, image rollout, secret
> mutation, and arm-state changes are recovery/change actions, not diagnostic
> probes. Diagnose read-only first. Use those actions only with an accountable
> owner, a fixed preflight, active monitoring, a declared rollback, and an
> approved change window. This document does not authorize a live mutation.

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
npx wrangler secret put FABRIC_SIGNING_KEY        < ~/.corelink/secrets/fabric-signing-key-prod
npx wrangler secret put FABRIC_INTROSPECT_AUTH_KEY < ~/.corelink/secrets/fabric-introspect-key
npx wrangler secret put BILLING_INGEST_AUTH_KEY    < ~/.corelink/secrets/billing-ingest-key
npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN < ~/.corelink/secrets/cf-spawn-token
npx wrangler secret put CLOUDFLARE_EXEC_AUTH_TOKEN < ~/.corelink/secrets/cf-exec-token
npx wrangler secret put CLOUDFLARE_LIFECYCLE_AUTH_TOKEN < ~/.corelink/secrets/cf-lifecycle-token

# 2. Deploy (builds + pushes the image, creates the Worker + container + DO + cron).
npm run deploy
```

The prod signing key was generated 2026-06-25 (32-byte ed25519, fingerprint
`9f54d5ee`); its PUBLIC half — `key_id faa5b7726ccd2c52`,
`pubkey_b64 Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=` — is what
`GET /v1/attestation/key` will serve and what the CoreLink CLI/SDK verifier pins. Setting a
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

## Emergency admission freeze

Set the non-secret Worker var `FABRIC_ADMISSION_PAUSED=1` to pause new work at
the proxy edge. The Worker returns `503` with `Retry-After: 60` before looking
up or waking a container for these routes:

- `POST /v1/leases` (new lease acquire)
- `POST /webhooks/github` workflow_job.queued and other non-completion events
  (autoscaler-driven new lease acquire)
- `POST /v1/test/mint-cred-ticket` (dev/test ticket mint)

The default is fail-open only when the binding is absent or exactly `0`. Any
other value, including a malformed or whitespace-padded value, is treated as
paused. A `workflow_job.completed` webhook is parsed at the edge and still
forwards to Rust so credential revocation and runner teardown can complete.
Health, attestation, lease reads, existing lease execution and credential
redemption, cancel/teardown, and close continue through the normal proxy path
so already-issued leases can drain. Set the var back to exactly `0` to resume
admissions; applying the change still requires the normal owner-approved
Worker rollout.

## Smoke (checkpoint A/B/C)
```sh
HOST="https://corelink-fabricd.<account-subdomain>.workers.dev"   # printed by deploy
curl -s $HOST/v1/health                       # → ok
curl -s $HOST/v1/attestation/key              # → key_id faa5b7726ccd2c52 (the prod pubkey)
# acquire with a real tenant PAT → 200 Held; GET .../envelope/meta → 200 (not 404)
```

These application-route calls are wake-capable. Use them only while validating
an approved rollout or an already-active service. During containment or an idle
scale-to-zero check, use provider control-plane reads and do not call `/`,
`/health`, `/v1/health`, `/v1/usage`, or internal status routes.

Use `$HOST` as the CoreLink fabric URL for the direct CLI/SDK smoke. Set
`CORELINK_URL=$HOST` and provide the tenant credential through `CORELINK_PAT`;
there is no external-project handoff.

## Container health probe (historical behavior; not current-state evidence)

CF Containers probes the default port on `/`. In the 2026-07-08 incident,
fabricd served only `/v1/*`; the resulting 404 coincided with `healthy:0` and a
reverted rollout. `app.rs` therefore keeps `/` and `/health` as auth-free
`200 "ok"` routes (same liveness surface as `/v1/health`). That dated observation
explains why the routes exist; it is not evidence about the current deployment.
A 200 proves only that the responding process is live. It does not prove the PG
ledger, billing exporter, mint path, image identity, or end-to-end traffic.

## Single-flight singleton — fragility, mitigations, scaling path

The control plane was designed as ONE container (`max_instances: 1` + a fixed DO id
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
  runtime stalls. The configured **`standard-2` provider shape is 1 vCPU / 6 GiB /
  12 GB**; keep the `FABRIC_PROVISION_MAX_INFLIGHT` gate and the observed health
  probe evidence tied to that shape. **Verified 2026-07-08:**
  health stayed `200` across all 30 polls (0.4–1.0s) through a 32s close.

**Close latency — diagnosed, NOT a bug (2026-07-08).** A raw close (e.g. `curl`)
takes ~32s, but that is the **§13.2 JobClose ack window** (`ack_timeout`, hardcoded
`Duration::from_secs(30)` at `leases.rs:964`): every off-box/agent lease registers
a §13 CaptureHook at acquire, and the close blocks up to 30s (fail-closed) waiting
for the client's **JobClose ack**. A non-acking test client waits the full 30s; a
REAL acking client (the direct CoreLink CLI/SDK path — proven metrics round-trip) collapses the
window to ~0 and the close returns in **~2.7s** (teardown + attestation + 3 pg
writes). So the close is fast for real traffic — the "slow close" was a
non-acking-test artifact, not pg latency. The pg work itself is ~2.7s; no offload
needed at current scale. The real requirement — the plane staying UP during any
long ack-wait — is handled by `close_ack_gate`, which bounds concurrent ack-waits;
the 2026-07-08 probe kept health at `200` across 30 polls during a 32s close.

**Scaling path (not yet done):** the singleton was required only by the in-memory
ledger. The **pg ledger has two gates**: a non-empty `DATABASE_URL` secret **and**
the byte-exact Worker var `FABRIC_PG_DISABLED="0"`. Only then does the Worker
forward `FABRIC_LEDGER_BACKEND=pg`, the URL, `FABRIC_RUNNER_VCPU=4`, and the
pg-only billing export, giving cross-instance cap-safety via the advisory lock.
Unset, blank, whitespace-padded, or any other value of `FABRIC_PG_DISABLED`
fails closed to the in-memory ledger even when the URL secret remains bound.

The current containment posture is `FABRIC_PG_DISABLED="1"`, so the configured
path is deliberately in-memory (single-instance, state lost on restart).
`DATABASE_URL` alone never arms PG. Exact `FABRIC_PG_DISABLED="0"` is necessary
but not sufficient authorization to rearm: the database/TLS/role preflight must
pass on a fixed configuration, the owner must approve the change, monitors and a
rollback to exact `"1"` must already be ready, and post-change status must prove
`ledger_cross_instance_safe: true`. Do not raise `FABRIC_NUM_SHARDS` or
`max_instances` on health alone. D12 still leaves attribution between PgLedger
startup work and the PG-gated exporter unresolved, so monitor both paths and do
not describe either one as the established source of resource burn. See
`docs/runbook/arm-fabricd-pg-ledger-vcpu-ceiling.md` for the ordered procedure.

## Boxes (checkpoint B+ — when wiring real per-job metrics)

Until a box backend is wired the lease/§13/attestation surface remains pending live verification, while `exec`
returns 503 (no box backend) — fail-closed, exactly as the dress-rehearsal showed.

⚠️ **The substrate you wire decides which lease KINDS run — this trap previously
broke prod once (#195).** `CloudflareEngine` v0 remains **runner-only by design** (ADR-0007:
the spawn-Worker's only container is the GitHub-Actions runner image). So:

- **Cloudflare ONLY** (`CLOUDFLARE_SPAWN_WORKER_URL` var + `CLOUDFLARE_SPAWN_AUTH_TOKEN`
  secret) → **runner** leases spawn on Cloudflare, but a **check-exec** lease
  (`allow_egress=false`) **fails closed at spawn** (#198). The direct CoreLink
  check/exec path therefore requires the second substrate; Cloudflare-only is a
  runner-only configuration. Wiring CF alone and routing check/exec work to it is
  the #195 regression.

- **Rota B — BOTH substrates** (`CLOUDFLARE_SPAWN_*` **and** `NORTHFLANK_API_TOKEN`
  + `NORTHFLANK_PROJECT_ID`) → the composition selects the **Hybrid** backend
  (`select_backend(true,true)`): **runner→Cloudflare** (the R2-co-located moat),
  **check-exec→Northflank**. This is the supported dual-backend configuration for
  the two CoreLink lease kinds. Set all four and redeploy.

```bash
# wrangler.jsonc vars:  CLOUDFLARE_SPAWN_WORKER_URL, NORTHFLANK_PROJECT_ID
# (+ NORTHFLANK_RUNNER_* tuning as needed; see docs/deploy/fabric-server.md)
npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN       < ~/.corelink/secrets/cf-spawn-token
npx wrangler secret put CLOUDFLARE_EXEC_AUTH_TOKEN        < ~/.corelink/secrets/cf-exec-token
npx wrangler secret put CLOUDFLARE_LIFECYCLE_AUTH_TOKEN   < ~/.corelink/secrets/cf-lifecycle-token
npx wrangler secret put NORTHFLANK_API_TOKEN          < <northflank token, OOB>
# envVars are read at container start. Applying them requires an owner-approved
# rollout with preflight, monitoring, and rollback; never delete/restart/deploy
# merely to diagnose whether a variable is present.
```

After the env becomes active, smoke BOTH kinds before promoting the host: a runner
acquire → 200 Held + a CF `/v1/spawn` fired; a check acquire → 200 Held + provisioned on
Northflank (not Cloudflare). The `tests/hybrid_flip_e2e.rs` e2e pins this routing offline;
the live smoke confirms the real backends. **Never claim boxes work off the boot log alone
— prove an end-to-end spawn of each kind** (the #195 lesson: "substrate wired" ≠ spawn works).

## Historical status snapshot (not a current production assertion)

On 2026-07-09, a bounded validation at
`https://corelink-fabricd.gmhelmold.workers.dev` recorded the moat path working.
That snapshot used the #332 cred-redemption-fix binary, tag
`golive-20260709-credredemption`. The canonical configured image reference is
the `containers[0].image` value in [`wrangler.jsonc`](./wrangler.jsonc); this
README intentionally does not duplicate a digest that can become stale. It adds the
`validate_mint_arm` boot guard: a successful boot checks that
`FABRIC_PUBLIC_BASE_URL` is wired when mint is armed (the earlier `cb6fca46…`
moat-fix binary minted a
real PAT the box could never redeem — deployment audit wf_63a2b814). `/v1/health → 200 ok`,
`/v1/attestation/key
→ key_id faa5b7726ccd2c52` (prod key). The per-job CAS PAT mint was proven REAL (a hydrating
check-host acquire went 503→200 across the `token_plaintext` response-parse fix — a mint-armed
transition a cold-run could never produce), and the attested-cost `intent_metrics_sig` rides the
close. CF-native: introspect auth + CF spawn-Worker box backend, **no Northflank**.
None of those dated results establishes the current image, arm state, liveness,
or end-to-end behavior; re-establish each claim with current, read-only evidence.

⚠️ **Two deployment bugs the earlier "proven" reads MISSED** (both fixed): (1) `index.ts` didn't
forward the mint/cred/emit vars into the CONTAINER (only the Worker saw them) → mint OFF → cold-run
200 masked it; (2) `MintResponseBody` read `token` but the server sends `token_plaintext` →
fail-closed on every real mint. Lesson: a 200 on a hydrating acquire does NOT prove a mint — only a
mint-armed 503→200 transition (or a server-side mint-request log) does.

The historical Northflank cutover record is retained in dated handoffs. Current
operation is the Cloudflare fabric directly; validate it with the CoreLink CLI/SDK
and the public API, then re-pin the returned attestation key if the approved
CoreLink deployment changes it.

**Controlled-change note:** to rebuild the binary — temp-copy `crates/corelink-fabric-server/Dockerfile` to the repo
root, `npx wrangler containers build <repo-root> -t corelink-fabricd-fabricdcontainer:<tag> --push`
(the root `.dockerignore` keeps the context small), pin the returned `@sha256` digest in
`wrangler.jsonc`, `wrangler deploy` (a NEW digest forces the container rollout; a config-only change
does not). This is an owner-approved release procedure, never a diagnostic step.
Docker daemon required for the build only.
