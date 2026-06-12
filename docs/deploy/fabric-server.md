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
| `NORTHFLANK_API_TOKEN` | — | Northflank API token.  When set (with the vars below), the cloud execution backend is activated. |
| `NORTHFLANK_PROJECT_ID` | — | Northflank project id. |
| `NORTHFLANK_TEAM_ID` | — | Northflank team/account id. |
| `FABRIC_DEV_UNSAFE` | — | Set to `1` to boot with the insecure well-known dev signing key (LOCAL USE ONLY — attestations are forgeable; also requires a loopback bind address). |

## Fail-closed notes

- **No `FABRIC_SIGNING_KEY` and no `FABRIC_DEV_UNSAFE=1`** → the process refuses to start.
- **`FABRIC_DEV_UNSAFE=1` with a non-loopback bind** → the process refuses to start.  The dev key is forgeable and must never serve external traffic.
- **No `NORTHFLANK_*` vars** → exec and provision stay on `NoBoxExec` / `NoBoxProvisioner`; every exec call returns 503.  The lease lifecycle still works; execution does not.
- **`InMemoryLedger`** — leases are not persisted across restarts.  This is M1 scope; the Postgres ledger (ratified decision #3) arrives later.

## Scope / known gaps

- **Envelope / §13 emission not wired** (CF-ENVELOPE-WIRE) — per-lease `CaptureHook`
  registration at acquire time is a separate work-package.  The envelope poll endpoints
  are mounted (routes exist) but return 404 until that wiring lands.  This binary serves
  the lease / exec / attestation path only.

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

Run (production — Northflank backend enabled):

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
