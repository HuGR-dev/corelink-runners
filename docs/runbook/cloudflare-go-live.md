# Runbook — Cloudflare substrate go-live (flip the runner compute to Cloudflare Containers)

> **Living runbook** (not a dated handoff). Run this to take the runner compute
> substrate from "interim Northflank" to "Cloudflare Containers, co-located with
> R2" once the external gates clear (ADR-0008). It is the sibling of
> `docs/runbook/dogfood-go-live.md` (which brings COLD CI up on Northflank); this
> one captures the R2 co-location win (in-network CAS hydration, zero-egress).
> Owner-facing; steps you (or the runner TL) execute against the Cloudflare
> account / the deployed fabric.

## 0. What this proves and what is built vs gated

- **Proves (ADR-0008):** a queued GitHub Actions job → `acquire(runner)` → the
  fabric's `CloudflareEngine` HTTP-calls the **spawn-Worker** → a Container
  Durable Object `container.start`s the digest-pinned runner image → it registers
  as an ephemeral GH-Actions runner → runs the job → tears down. The win over
  Northflank: the container reads the R2-backed CAS **in-network** (zero-egress,
  fast cache-warm) instead of crossing the public internet.
- **Built + tested (Rust side, this repo):** `corelink-cloud-engine::CloudflareEngine<H: HttpTransport>`
  implements the `Engine` seam with the three fail-closed floors (isolation /
  X4 digest-pin / disk), auth, and exact spawn-Worker request shapes — proven
  against a mock transport, **no live account needed** (`crates/corelink-cloud-engine/src/cloudflare.rs`).
- **SKELETON / untested (Worker side):** `deploy/cloudflare/` (Worker + Container
  DO) is written, **not deployed, not run against a live account**. Pioneering
  (no documented precedent for GH-Actions runners on Cloudflare Containers).
  Expect first-deploy shakeout.
- **NOT YET WIRED (composition root):** the fabric-server binary's substrate
  selection currently resolves **only Northflank** (`cloud_backend_status` /
  `cloud_backend_from_env` in `crates/corelink-fabric-server/src/cloud_exec.rs`;
  the boot diagnostic in `main.rs` prints only `Northflank` / `NONE`). The
  ADR-0008 selection policy (Cloudflare-preferred-over-Northflank) is **the next
  build slice** — it must adapt the runner-direct, spawn-only `CloudflareEngine`
  onto the lease lifecycle. **§3 below cannot land until that slice ships.** Until
  then, setting `CLOUDFLARE_SPAWN_*` on the fabric is a no-op (the binary will
  still pick Northflank or fail closed). This is the first gate to clear.

## 1. Pre-flight gates — all owner / cross-TL, all must be true before flipping

| Gate | Owner | Done when |
|---|---|---|
| Composition-root selection slice landed (CF preferred over Northflank, per ADR-0008 §"Selection policy") | runner TL | the boot diagnostic in `main.rs` reports a `Cloudflare` branch (not just `Northflank` / `NONE`) |
| Cloudflare account: Workers **Paid** + **Containers** enabled; `wrangler` authed (`npx wrangler whoami`) | owner | account shows Containers GA; `wrangler` login resolves the account |
| Runner image (clw v0.1.1 baked, X4 floor, GH-Actions agent entrypoint) **pushed to the Cloudflare registry** and **digest-pinned** in `deploy/cloudflare/wrangler.jsonc` (`containers[].image` = `registry.cloudflare.com/<account>/corelink-runner@sha256:<digest>`) — same digest as `deploy/runner/Dockerfile`'s clw pin | owner | `wrangler.jsonc` `<FILL-AT-DEPLOY>` replaced with a real `@sha256:` digest |
| `CLOUDFLARE_SPAWN_AUTH_TOKEN` shared secret minted (held by both the fabric env and as a Worker secret) | owner | a strong random token exists; never logged (it is redacted in `CloudflareConfig`'s `Debug`) |
| **Isolation security sign-off** — Cloudflare's container/VM (gVisor-class) isolation reviewed against the untrusted-multi-tenant-CI threat model (vs the Firecracker-class bar) | owner + reviewer | explicit written sign-off; **do not serve real tenants without it** |
| **R2 co-location seam** — in-network CAS credentials/topology; add the `r2_buckets` binding to `wrangler.jsonc` (intentionally omitted in the skeleton) | Cache TL | R2 binding agreed + wired; the moat win is the whole point |
| **D-9 CAS-PAT mint** in-network / deployed (the per-job cache identity the container needs) | Server TL | mint Worker reachable from the container; `CLW_*` env resolvable |

If any row is open: do not proceed. Cold CI on Northflank (`dogfood-go-live.md`)
keeps the product up meanwhile — this flip is the moat-capturing upgrade, not a
prerequisite for shipping.

## 2. Step 1 — deploy the spawn-Worker (SKELETON — expect shakeout)

> The `deploy/cloudflare/` Worker is a **skeleton, never run live**. Budget for
> a first-deploy iteration loop (SDK API surface for `container.start({ env })`,
> the one-shot DO alarm vs an exiting container, the `image_digest`-assertion path
> — all flagged UNVERIFIED in `deploy/cloudflare/README.md` and `src/index.ts` TODOs).

```sh
cd deploy/cloudflare
npm install
# Confirm wrangler.jsonc containers[].image is digest-pinned (pre-flight gate) and
# matches deploy/runner/Dockerfile's clw pin. The image is bound HERE at deploy
# time, NOT per-spawn (README wrinkle #1): the per-spawn image_digest is an
# ASSERTION the Worker checks against this, not a pull directive. Refresh the
# image by redeploying with a new digest.
npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN   # paste the §1 shared secret
npx wrangler deploy
```

After deploy, note the Worker URL (e.g. `https://corelink-spawn-worker.<account>.workers.dev`)
— it is `CLOUDFLARE_SPAWN_WORKER_URL` in §3. Sanity-probe the auth surface: an
unauthenticated `POST /v1/spawn` must return `401` (per the frozen contract,
`docs/spec/cloudflare-spawn-worker-contract.md`).

## 3. Step 2 — flip the fabric to Cloudflare

> Requires the composition-root selection slice (§1 row 1). With it, the policy is
> automatic: **Cloudflare wins when `CLOUDFLARE_SPAWN_*` is present**; Northflank
> serves only when Cloudflare is absent (ADR-0008 selection policy: CF → Northflank → off).

Set on the deployed fabric env (env-var names are centralized in
`crates/corelink-cloud-engine/src/cloudflare.rs`):

- **Required (both, or the backend stays OFF):**
  - `CLOUDFLARE_SPAWN_WORKER_URL` — the §2 Worker base URL (no trailing slash; `/v1/...` is appended).
  - `CLOUDFLARE_SPAWN_AUTH_TOKEN` — the §1 shared secret (Bearer token; redacted in logs).
- **Optional tunables:**
  - `CLOUDFLARE_RUNNER_LABELS` — CSV, attached to every spawned container (e.g. `corelink-dogfood`). Absent ⇒ no labels.
  - `CLOUDFLARE_EXPIRY_MS` — per-container hard-kill backstop (defense-in-depth with the lease expiry). Absent/garbage ⇒ `DEFAULT_EXPIRY_MS` = `3_600_000` (1 h).
  - `CLOUDFLARE_RUNNER_STORAGE_MB` — the configured ephemeral disk the disk floor is asserted against; set to the real instance disk so a too-small instance fails CLOSED rather than ENOSPC mid-build. Absent/garbage ⇒ `DEFAULT_RUNNER_STORAGE_MB` = `20_480` (matches `standard-4`'s 20 GB).

**Confirm via the boot diagnostic.** Restart/redeploy the fabric and read its
startup log (`crates/corelink-fabric-server/src/main.rs`): it reports the backend
the wiring ACTUALLY resolved (never a token-only guess). Once the selection slice
exists, a `Cloudflare` line confirms the flip; a `Northflank` / `NONE` line means
`CLOUDFLARE_SPAWN_*` was not picked up (wrong env scope, empty value, or the slice
isn't deployed).

## 4. Step 3 — smoke a dogfood job

Dispatch the dogfood smoke exactly as in `dogfood-go-live.md` §2
(`dogfood-smoke.yml`, `workflow_dispatch`, `runs-on: corelink-dogfood`). The path
on Cloudflare:

queued job → `acquire` → `Engine::spawn` → `CloudflareEngine` `POST /v1/spawn`
(after the 3 fail-closed floors) → spawn-Worker → DO `container.start` (digest-pinned
image + JIT config + `CLW_*` env) → runner registers (`corelink-dogfood`) → job runs
**cache-warm from R2 in-network** → one-shot deregister → lease end fires `POST /v1/teardown`.

| Symptom | Likely cause | Fix |
|---|---|---|
| Acquire **503** (no Cloudflare branch in boot log) | composition-root selection slice not deployed, or `CLOUDFLARE_SPAWN_*` not picked up | confirm §1 row 1 landed; re-check the env scope + boot diagnostic (§3); both required vars set + non-empty |
| Spawn **401** at the Worker | `CLOUDFLARE_SPAWN_AUTH_TOKEN` mismatch (fabric env ≠ Worker secret) | re-`wrangler secret put` and re-set the fabric env to the SAME token |
| Spawn rejected, **image_digest mismatch** | the spawn request's `image_digest` ≠ the deploy-time-pinned `wrangler.jsonc` image (README wrinkle #1) | re-pin `wrangler.jsonc` to the runner image's real digest and `wrangler deploy`; keep it identical to `deploy/runner/Dockerfile` |
| Container starts but **runner never registers** | bad/expired JIT config, env not injected at `container.start({ env })` (UNVERIFIED skeleton path), or entrypoint not consuming `CORELINK_RUNNER_JITCONFIG` | check Worker logs (`wrangler tail`); verify the JIT config reaches the container env at start, not baked |
| Job **hangs / orphans** | one-shot lifecycle vs DO alarm not mapped ("container exited" → `/v1/status` `404`); `CLOUDFLARE_EXPIRY_MS` too low/high | validate the DO alarm against an exiting one-shot container; set `CLOUDFLARE_EXPIRY_MS` just above the longest CI job (it is the orphan-leak backstop, not a per-job timeout) |
| Job runs but **slow / R2 hydration fails** (egress, 403) | R2 co-location seam not wired (no `r2_buckets` binding in `wrangler.jsonc`), or `CLW_*` / D-9 mint creds unreachable in-network | Cache TL: confirm the R2 binding + in-network CAS creds; Server TL: confirm the D-9 mint is reachable from the container |

This path has **never run green E2E live** — the Worker is a skeleton and the
selection slice is pending. Treat the first green smoke as the real proof of the
GH-runner lifecycle fit on Cloudflare Containers (an ADR-0008 gated item).

## 5. Fallback — revert to Northflank (interim)

Cloudflare and Northflank are **mutually exclusive at the composition root**: when
`CLOUDFLARE_SPAWN_*` is present, Cloudflare wins (it is the default per ADR-0008).
To fall back:

1. **Unset** `CLOUDFLARE_SPAWN_WORKER_URL` and `CLOUDFLARE_SPAWN_AUTH_TOKEN` on the
   fabric env (clearing either disarms the Cloudflare backend — both are required).
2. Ensure the Northflank env is present (`NORTHFLANK_API_TOKEN` + `NORTHFLANK_PROJECT_ID`,
   per `dogfood-go-live.md`). The selection then falls back to `NorthflankEngine`.
3. Restart/redeploy; the boot diagnostic should report `Northflank` again.

If neither is wired ⇒ fail-closed (a runner lease is refused at admit; execs 503).
There is no silent default.

## 6. What's validated where (honesty ledger)

- **Runner / engine side (this repo):** BUILT + TESTED. `CloudflareEngine` floors,
  auth, fail-closed mapping, and exact spawn-Worker request shapes are proven
  against a mock `HttpTransport` (`crates/corelink-cloud-engine/src/cloudflare.rs`),
  no account needed.
- **Composition-root selection:** **NOT YET WIRED** — the next build slice (the
  binary resolves only Northflank today). Gate row 1.
- **Spawn-Worker + Container DO (`deploy/cloudflare/`):** SKELETON, **validated only
  live** (gated): `wrangler` deploy, the `container.start({ env })` SDK surface, the
  one-shot DO-alarm lifecycle, and the `image_digest` assertion are all UNVERIFIED
  until first deploy.
- **R2 co-location + isolation review + D-9 mint reachability:** **validated live,
  cross-TL gated** (Cache TL / owner+reviewer / Server TL respectively). The moat
  win (in-network R2 hydration) is real only once the R2 seam is wired.
- **Conformance vector** `conformance/cloudflare-spawn.json` (byte-identical drift
  tripwire between the Rust side and the Worker) is committed once both sides exist;
  until then, `docs/spec/cloudflare-spawn-worker-contract.md` IS the frozen contract.
