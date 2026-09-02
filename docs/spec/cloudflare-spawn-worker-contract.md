# Spec — CoreLink Cloudflare spawn-Worker HTTP contract (v0, FROZEN)

> The seam between the Rust `CloudflareEngine` (this repo, `corelink-cloud-engine`) and the
> Cloudflare **spawn-Worker + Container Durable Object** (`deploy/cloudflare/`, implemented in this
> repository; live deployment and lifecycle remain separately unverified).
> Per ADR-0008, Cloudflare Containers spawn via a Worker/DO (not REST), so this small authenticated
> HTTP surface is what the autoscaler/Engine calls. **TRANSCRIBED on each side** (mirrors the
> hugit/clw discipline) — a conformance vector keeps them from drifting. v0 = runner-direct (the
> spawned container runs the GH-Actions agent via its image entrypoint; there is no post-spawn exec).

## Auth
Every request carries `Authorization: Bearer <token>`. The token is a shared secret held by the
fabric (`CLOUDFLARE_SPAWN_AUTH_TOKEN`) and the Worker (a Worker secret). Missing/invalid ⇒ `401`.
No tenant identity in headers; the Worker is fabric-internal (the fabric already resolved entitlement).

## Endpoints

### `POST /v1/spawn`
Start a per-job container from the digest-pinned runner image, running the GH-Actions agent.
Request:
```json
{
  "image_digest": "<registry-ref>@sha256:<64-hex>",
  "jitconfig":    "<GH Actions JIT runner config, opaque string>",
  "env":          { "CLW_ENDPOINT": "...", "CLW_TENANT": "...", "CLW_TOKEN": "...", "...": "..." },
  "labels":       ["corelink-dogfood"],
  "expiry_ms":    2700000
}
```
- `image_digest` MUST be digest-pinned (`@sha256:`) — the fabric enforces the X4 floor BEFORE calling;
  the Worker SHOULD also reject a non-pinned ref (defense in depth).
- `jitconfig` is the GH-Actions just-in-time runner registration (one-shot, ephemeral). The container
  entrypoint consumes it to register, run exactly one job, and deregister.
- `env` is injected into the container (the `CLW_*` cache identity per the clw contract + any runner env).
- `expiry_ms` is the orphan-leak backstop (the DO tears the container down past this).
Response (success `201`):
```json
{ "handle": "<opaque-id>" }
```
`handle` addresses this container for status/teardown. Non-2xx ⇒ the fabric fails closed (no lease).

### `GET /v1/status/{handle}`
Liveness of a spawned container.
- `200` ⇒ alive (container exists / running).
- `404` ⇒ gone (exited, torn down, or never existed).
- any other status ⇒ indeterminate ⇒ the fabric treats as fail-closed.

### `POST /v1/teardown`
Stop a container. **Idempotent.**
```json
{ "handle": "<opaque-id>" }
```
- `200`/`204` ⇒ torn down (or already gone). `404` ⇒ already gone (also success for the caller).

> ⚠️ **Known gap — unvalidated handle, always 204.** `POST /v1/teardown` returns `204`
> unconditionally (`deploy/cloudflare/src/index.ts:3334`); a `teardown()` throw is only logged
> (`teardown_route_failed`, `:3327-3328`), never surfaced to the caller. The handle is resolved via
> `getContainer(env.RUNNER_CONTAINER, handle)` (`:3319-3321`), i.e. `idFromName(handle)` — there is
> no check that `handle` is one the fabric actually minted (spawn mints it as
> `crypto.randomUUID()`, `:1101`). An unknown/bogus name therefore mints a fresh, unrelated Durable
> Object and destroys nothing, while returning the exact same `204` a real teardown would. A caller
> cannot distinguish "torn down" from "silent no-op" from the response alone. The fix (enumerable,
> validatable DO names) is specified but **not implemented** — see
> [ADR-0010](../adr/0010-enumerable-runner-do-names.md), status `PROPOSED — NOT IMPLEMENTED`,
> pending owner sign-off.

## Engine-trait mapping (Rust side, `CloudflareEngine impl Engine`)
| `Engine` method | spawn-Worker call | notes |
|---|---|---|
| `spawn(spec)` | `POST /v1/spawn` | after the 3 fail-closed floors (isolation, X4 digest-pin, disk); returns `RunningContainer{name=handle}` |
| `probe(c)` | `GET /v1/status/{handle}` | alive ⇒ `tmp_is_private` + `net_is_isolated` both true |
| `is_alive(c)` | `GET /v1/status/{handle}` | `200`⇒true, `404`⇒false, else fail-closed |
| `exec` / `exec_captured` | — | **unsupported in v0 (runner-direct)** — the container runs its entrypoint; runner leases never exec post-spawn |
| teardown (lease end) | `POST /v1/teardown` | idempotent; fires on every terminal path |

## Worker side (`deploy/cloudflare/`)
The repository implementation routes these three endpoints to a **Container Durable Object**
(`class extends Container`):
`/v1/spawn` → `getContainer(...)` + `ctx.container.start({ image, env })`; `/v1/status` → DO liveness;
`/v1/teardown` → stop. Co-located with R2 (in-network CAS hydration — the moat win). Instance type
`standard-4` (4 vCPU / 12 GiB / 20 GB disk). **Live deployment and the lifecycle are unverified**
in this repository (wrangler auth + image push are still required) — see ADR-0008's gated items
(isolation review, R2 seam, lifecycle fit).

## Drift control
When both sides exist, commit a conformance vector (`conformance/cloudflare-spawn.json`: a canonical
spawn request + response) byte-identical in this repo and the Worker's, with golden tests on each side —
the same tripwire used for hugit/clw. Until the Worker exists, this spec IS the frozen contract; changes
go through ADR-0008's owner/cross-TL review, never unilaterally.
