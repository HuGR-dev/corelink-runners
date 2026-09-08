# Deploying `corelink-fabricd` (M1 scope)

The production binary for the CoreLink Runners fabric server.
Wires the axum router, auth, lease lifecycle, signing key, and the cloud
execution backend (default-off).

## Required environment variables

| Variable | Description |
|---|---|
| `FABRIC_SIGNING_KEY` | Base64-encoded 32-byte ed25519 seed.  Required in production.  See below. |
| `FABRIC_PAT` | Bootstrap PAT → tenant mapping (the bootstrap token that can acquire leases). |
| `FABRIC_TENANT` | Tenant id for `FABRIC_PAT` (lowercase `[a-z0-9-]`). |
| `FABRIC_TENANT_MAX_CONCURRENCY` | Maximum concurrent leases for the bootstrap tenant (u32, ≥ 1).  **Required** — no default.  0 produces a server that authenticates but rejects every acquire. |

## Optional environment variables

| Variable | Default | Description |
|---|---|---|
| `FABRIC_BIND_ADDR` | `0.0.0.0:8080` | TCP address to listen on. |
| `FABRIC_TENANT_RATE_PER_MIN` | `120` | Acquire-request rate ceiling per minute for the bootstrap tenant (u32, ≥ 1). |
| `FABRIC_RUNNER_REPO_ALLOWLIST` | — (empty) | Comma-separated canonical runner targets the bootstrap tenant may target for a **runner** lease (Track-C C1): `repo:<owner>/<repo>` or `org:<org>` (case-insensitive). **Fail-closed:** unset/empty ⇒ the tenant may run NO runner leases; a runner acquire whose target is not listed is denied 400. Ignored by check/hermetic leases. Example: `repo:HuGR-Labs/corelink-runners,org:HuGR-Labs`. |
| `NORTHFLANK_API_TOKEN` | — | Northflank API token.  When set (with the vars below), the cloud execution backend is activated. |
| `NORTHFLANK_PROJECT_ID` | — | Northflank project id. |
| `NORTHFLANK_TEAM_ID` | — | Northflank team/account id. |
| `FABRIC_DEV_UNSAFE` | — | Set to `1` to boot with the insecure well-known dev signing key (LOCAL USE ONLY — attestations are forgeable; also requires a loopback bind address). |
| `FABRIC_LEDGER_BACKEND` | `memory` | Lease ledger backend: `memory` (in-memory, leases reset on restart, single-instance) or `pg`/`postgres` (persistent, multi-instance cap-safe).  Any other value is a hard boot error. |
| `DATABASE_URL` | — | Postgres connection URL.  **Required + non-empty** when `FABRIC_LEDGER_BACKEND=pg` — selecting `pg` without a reachable `DATABASE_URL` is a hard boot error (NEVER a silent fallback to memory).  Ignored for the `memory` backend. |
| `FABRIC_LEDGER_POOL_SIZE` | `8` | Postgres connection-pool size (`usize`, ≥ 1).  `0` or unparseable is a hard boot error.  Ignored for the `memory` backend. |

### Self-serve auth/cap backend (CoreLink introspect — the M1 prod path)

| Variable | Default | Description |
|---|---|---|
| `FABRIC_AUTH_BACKEND` | `static` | `corelink` selects the multi-tenant CoreLink-introspect backend (auth + per-tenant cap + vCPU-h ceiling from the live entitlement vector). `static` uses the single bootstrap `FABRIC_PAT`/`FABRIC_TENANT`. |
| `CORELINK_INTROSPECT_URL` | — | The introspect endpoint, e.g. `https://corelink-api.humangr.com/internal/v1/auth/introspect`. **Required** when `FABRIC_AUTH_BACKEND=corelink`. |
| `FABRIC_INTROSPECT_AUTH_KEY` | — | Dedicated `x-corelink-internal-auth` value for introspect — **NEVER** the shared `CORELINK_INTERNAL_AUTH_KEY`. **Required** when `FABRIC_AUTH_BACKEND=corelink`. |

### Cloudflare box-spawn backend (prod runner substrate — ADR-0008)

| Variable | Default | Description |
|---|---|---|
| `CLOUDFLARE_SPAWN_WORKER_URL` | — | The spawn-Worker base URL. Cloudflare requires this URL and all three distinct control tokens below. Partial configuration is rejected. With Cloudflare configuration absent, Northflank is selected if configured; otherwise `NoBox` serves the lease lifecycle and exec returns 503. |
| `CLOUDFLARE_SPAWN_AUTH_TOKEN` | — | Bearer token for `POST /v1/spawn` (must match the Worker's spawn token). |
| `CLOUDFLARE_EXEC_AUTH_TOKEN` | — | Bearer token for `POST /v1/exec`; required with the other two tokens for a valid Cloudflare engine configuration. |
| `CLOUDFLARE_LIFECYCLE_AUTH_TOKEN` | — | Bearer token for status, teardown, egress cutoff, and suspension control; required with the other two tokens. |

### Billing usage-push (corelink-billing ingest — off the admission path)

| Variable | Default | Description |
|---|---|---|
| `BILLING_INGEST_URL` | — | corelink-billing ingest, e.g. `https://corelink-api.humangr.com/internal/v1/billing/usage`. Absent ⇒ no push (fail-open). |
| `BILLING_INGEST_AUTH_KEY` | — | Dedicated ingest `x-corelink-internal-auth` (NEVER the shared key, NEVER the introspect/mint key). |
| `BILLING_REGION` | — | 3-char region stamped on each event (e.g. `iad`). |
| `FABRIC_BILLING_PUSH_INTERVAL_SECS` | `30` | Flush-driver interval for the buffered usage-push. |

> NOTE (topology): on the all-Cloudflare runner path, the per-completed-job
> `runner_slot_seconds` event also originates in the CF spawn-Worker itself
> (`deploy/cloudflare`), so billing remains captured even when fabricd is not the
> direct runner path. The fabricd push above covers the lease-API path's billing.

## Fail-closed notes

- **No `FABRIC_SIGNING_KEY` and no `FABRIC_DEV_UNSAFE=1`** → the process refuses to start.
- **`FABRIC_DEV_UNSAFE=1` with a non-loopback bind** → the process refuses to start.  The dev key is forgeable and must never serve external traffic.
- **No `NORTHFLANK_*` vars** → exec and provision stay on `NoBoxExec` / `NoBoxProvisioner`; every exec call returns 503. The lease lifecycle remains available; execution does not.
- **Ledger backend selection** — the default `memory` ledger does not persist leases across restarts and is single-instance only.  For production restart-survival + multi-instance cap-safety, set `FABRIC_LEDGER_BACKEND=pg` and provide `DATABASE_URL`.  Selecting `pg` with an absent/empty/unreachable `DATABASE_URL` is a hard boot error — the server NEVER silently falls back to memory (that would re-introduce split-brain / restart-loss invisibly).

## Scope / status

- **Envelope / §13 emission — WIRED** (WP-ENVELOPE-WIRE landed). `build_app_and_state`
  layers ONE shared `HookRegistry` onto the HTTP handlers (`app_full`) and the returned
  `state`; the acquire success path registers a per-lease `CaptureHook`, so the envelope
  poll/ingest endpoints (`/v1/leases/{id}/envelope/{events,meta,ingest}` + the close
  terminal-observe) expose the §13 surface for every acquired lease — integration-contract v1.2.0 §13.

## Generating a signing key

```sh
head -c 32 /dev/urandom | base64
```

Store the result as `FABRIC_SIGNING_KEY`.  Keep it secret; it is the key that signs every `AttestationChain` this fabric produces.

## Docker example

Build:

```sh
docker build -t corelink-fabricd:latest \
  -f crates/corelink-fabric-server/Dockerfile .
```

Run (Northflank box backend — the DEV/interim substrate; prod boxes spawn on
Cloudflare, see "Production deploy — option (b)" below):

```sh
docker run --rm \
  -e FABRIC_SIGNING_KEY="$(head -c 32 /dev/urandom | base64)" \
  -e FABRIC_PAT="your-bootstrap-pat" \
  -e FABRIC_TENANT="your-tenant" \
  -e NORTHFLANK_API_TOKEN="..." \
  -e NORTHFLANK_PROJECT_ID="..." \
  -e NORTHFLANK_TEAM_ID="..." \
  -p 8080:8080 \
  corelink-fabricd:latest
```

Run (local dev — insecure dev key, no cloud backend):

```sh
docker run --rm \
  -e FABRIC_DEV_UNSAFE=1 \
  -e FABRIC_PAT="dev-pat" \
  -e FABRIC_TENANT="dev" \
  -p 8080:8080 \
  corelink-fabricd:latest
```

## Health check

```sh
curl http://localhost:8080/v1/health
# → "ok"
```

## Production deploy — option (b): fabricd as the prod control plane

Owner decision 2026-06-25 (gap #1 → b): the planned deployment places
`corelink-fabricd` in the prod control-plane role. That plan would light up the
killer (direct CoreLink per-job attested cost) AND the M1 self-serve direct front door;
the CF spawn-Worker remains the GitHub-Actions autoscaler, while fabricd supplies
the `RunnerLease` + §13 + attestation front door. Engineering work was completed
and tested; the remaining work is deployment and secret provisioning.

### Prod env profile (self-serve + CF boxes + billing + attestation)

```sh
FABRIC_AUTH_BACKEND=corelink
CORELINK_INTROSPECT_URL=https://corelink-api.humangr.com/internal/v1/auth/introspect
FABRIC_INTROSPECT_AUTH_KEY=<dedicated introspect key>          # secret
FABRIC_SIGNING_KEY=<base64 32-byte ed25519 seed>              # secret — prod attestation key
CLOUDFLARE_SPAWN_WORKER_URL=<spawn-worker base url>
CLOUDFLARE_SPAWN_AUTH_TOKEN=<matches the Worker spawn secret>  # secret
CLOUDFLARE_EXEC_AUTH_TOKEN=<dedicated Worker exec secret>     # secret
CLOUDFLARE_LIFECYCLE_AUTH_TOKEN=<dedicated Worker lifecycle secret> # secret
BILLING_INGEST_URL=https://corelink-api.humangr.com/internal/v1/billing/usage
BILLING_INGEST_AUTH_KEY=<dedicated ingest key>               # secret
BILLING_REGION=<3-char colo, e.g. iad>
FABRIC_BIND_ADDR=0.0.0.0:8080
# Ledger: `memory` is fine for the single-instance dogfood bring-up; set
# FABRIC_LEDGER_BACKEND=pg + DATABASE_URL for restart-survival / multi-instance.
```

### ⚠️ OPEN sub-decision (owner) — deployment host for fabricd-the-container

The Dockerfile remains host-agnostic (any container platform). The runner BOXES
are planned for Cloudflare (ADR-0008) via the spawn-Worker; that decision stands.
But fabricd itself — a long-lived HTTP control plane — requires a host. Options:

- **CF Container (long-lived) fronted by a Worker** — keeps everything on
  Cloudflare; needs an external Postgres (Hyperdrive/managed) if you want the
  `pg` ledger (the `memory` ledger needs none for a single instance).
- **A small managed host (Fly / a VM)** for fabricd, with CF as the box engine —
  natural for the `pg` ledger + always-on, but adds a non-CF surface.

For the FIRST bring-up (checkpoint A) the `memory` ledger + a single instance is
enough to light up the killer; the durable/cross-instance path is M1 scale
hardening, not a checkpoint-A blocker. **This host choice is the owner's call.**

### Checkpoints (each unblocks the killer's render path independently)

- **(A) reachable + `CORELINK_URL` set** — after fabricd deployment with the env
  above; `curl $CORELINK_URL/v1/health → ok`; a lease acquire with a real
  tenant PAT returns `200 Held`. Configure the direct CoreLink CLI/SDK with the
  URL and tenant PAT.
- **(B) §13 ingest available** — acquire a lease, confirm
  `GET /v1/leases/{id}/envelope/meta` returns 200 (not 404) and the box can ingest;
  per-job metrics auto-stamp each land. (Code already wired — this confirms the
  deployed path, with no additional implementation.)
- **(C) attestation key published + enforcement on** — `GET /v1/attestation/key`
  returns the prod pubkey; the CoreLink CLI/SDK verifier enforces the key (closes
  the P0 verdict-forgery window).
- **(D) direct smoke** — a CoreLink CLI/SDK run renders the attested per-job cost
  on the supported usage/attestation surface and records the `spend_proof`.

(A)+(B) alone make per-PR cost real. Owner-gated steps: the host bring-up, the
`FABRIC_SIGNING_KEY` generation (`head -c 32 /dev/urandom | base64`), and the
secret provisioning.
