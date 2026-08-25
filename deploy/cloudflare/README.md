# CoreLink spawn-Worker + Container DO (Cloudflare substrate)

> **Status: LIVE — deployed to the gmhelmold CF account; autoscaler (/webhook) + /v1/* spawn
> surface in prod** (per the header declaration in `wrangler.jsonc`, which also carries the
> live runner-image digest pins and the prod-incident notes behind them).
> Pioneering (no documented precedent for GitHub-Actions runners on Cloudflare Containers).
> This is the Cloudflare side of the frozen seam in
> `docs/spec/cloudflare-spawn-worker-contract.md`; the Rust side (`CloudflareEngine`,
> `corelink-cloud-engine`) is built + tested and talks to this over HTTP. Per **ADR-0008**, Cloudflare
> Containers is the target compute substrate, co-located with the R2-backed CAS (the moat win).

## What this is

A Cloudflare **Worker** that routes the three frozen endpoints to a **Container Durable Object**
(`class extends Container`), which spawns a per-job **Cloudflare Container** running the digest-pinned
runner image (clw baked, GH-Actions agent as entrypoint):

- `POST /v1/spawn` → start a container with the JIT config + `CLW_*` env → `{ "handle": "<id>" }`
- `GET  /v1/status/{handle}` → `200` alive / `404` gone
- `POST /v1/teardown` → stop (idempotent) — ⚠️ always returns `204`, even for an unminted/bogus
  `handle` (mints a fresh, unrelated DO and destroys nothing); see
  [`docs/spec/cloudflare-spawn-worker-contract.md`](../../docs/spec/cloudflare-spawn-worker-contract.md#post-v1teardown)
  and [ADR-0010](../../docs/adr/0010-enumerable-runner-do-names.md) (PROPOSED, not implemented)

Auth: `Authorization: Bearer <CLOUDFLARE_SPAWN_AUTH_TOKEN>` (a Worker secret). Fabric-internal.

## Design notes / wrinkles surfaced while writing (READ before building further)

1. **Image is wrangler-bound, NOT per-spawn.** Cloudflare Containers pin ONE image per container
   class at **deploy time** (`wrangler.jsonc` → `containers[].image`). So the contract's per-spawn
   `image_digest` is an **assertion**, not a pull directive: the Worker MUST reject a spawn whose
   `image_digest` ≠ the deployed image (defense-in-depth; the real X4 pin lives in the wrangler config
   + the image push). This does not change the Rust side — it still sends `image_digest`; the Worker
   validates agreement. Refresh the pinned image by redeploying with a new digest.
2. **Env injection at runtime.** The runner needs per-job env (`CORELINK_RUNNER_JITCONFIG`, `CLW_*`).
   Cloudflare containers take env at `start({ env })` (runtime) — verify the exact API: the per-job
   JIT config MUST be injected at start, not baked. (UNVERIFIED against the live SDK — see TODOs in
   `src/index.ts`.)
3. **One-shot lifecycle.** A runner is ephemeral: register (JIT) → run one job → deregister → exit.
   The DO's default alarm manages container liveness; `expiry_ms` is the orphan-leak backstop. Map
   "container exited" → `/v1/status` `404`. Validate the DO alarm vs a one-shot container that exits.
4. **R2 co-location (the whole point).** The container reads the CAS from R2 in-network. The R2
   binding/credentials topology is a **Cache-TL coordination** item (ADR-0008 open decision) — not
   wired in this skeleton.

## Gates before this serves real tenants (ADR-0008)

- [ ] Live deploy: `wrangler` auth + push the digest-pinned runner image to Cloudflare's registry.
- [ ] **Isolation security review** for untrusted multi-tenant CI (CF container/VM model vs the
      Firecracker-class microVM bar) — **owner sign-off**.
- [ ] R2 co-location seam (in-network CAS creds) — **Cache TL**.
- [ ] Validate the 12 GiB RAM ceiling against the heaviest builds; confirm `standard-4` (4 vCPU /
      12 GiB / 20 GB disk).
- [ ] GH-runner lifecycle fit (registration, `git clone` egress, one-shot teardown) — live dogfood smoke.
- [x] Conformance vector `conformance/cloudflare-spawn.json` byte-identical with the Rust side
      (committed and pinned in `conformance/manifest.sha256`; enforced byte-exact against the
      Rust engine by `crates/corelink-cloud-engine/tests/cloudflare_conformance.rs`).

## Layout

- `wrangler.jsonc` — Worker + Durable Object + Container config (instance type, the pinned image).
- `src/index.ts` — the Worker (auth + routing) and the `RunnerContainer` DO class.
- `package.json` — `@cloudflare/containers` + `wrangler` (versions to pin at first real build).

## Deploy (when the account + gates are ready — NOT now)

```sh
cd deploy/cloudflare
npm install
# set the pinned runner image digest in wrangler.jsonc (matches deploy/runner/Dockerfile clw pin)
npx wrangler secret put CLOUDFLARE_SPAWN_AUTH_TOKEN
# REQUIRED before this Worker version deploys — see deploy-ordering note below.
# A mode==="check" /v1/spawn now fails CLOSED (503) if EXEC_SERVER_AUTH_TOKEN is
# unset, so this MUST be provisioned first or every check spawn 503s.
npx wrangler secret put EXEC_SERVER_AUTH_TOKEN
npx wrangler deploy
```

> **Deploy-ordering (breaking change) — provision `EXEC_SERVER_AUTH_TOKEN` FIRST.**
> As of this Worker version, `EXEC_SERVER_AUTH_TOKEN` is REQUIRED: a `mode==="check"`
> `/v1/spawn` fails closed with **503** when the secret is unset (there is no
> serve-unauthenticated back-compat default anymore). Because Worker secrets are
> read at request time, deploying this version WITHOUT first setting the secret
> will 503 **every** check spawn until it is provisioned. Order of operations:
> `wrangler secret put EXEC_SERVER_AUTH_TOKEN` **→** `wrangler deploy`. (Runner-mode
> spawns are unaffected — this gate is check-mode only.)
