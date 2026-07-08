# RUNBOOK — `corelink-fabricd` on Northflank

Deploy the fabric server as a long-running combined service on Northflank.
The fabric runs on Northflank **and** spawns its per-job microVMs on the same
provider — one plane, no cross-provider egress for the hot path.

---

## 1. Prerequisites

**1a. Connect GitHub to Northflank (owner step — do once).**

`HumanGuardrail/corelink-runners` is a private repo. Northflank must be granted
read access before it can pull source and build.

1. Log in to the Northflank dashboard as the `humangr` org owner.
2. Navigate to **Account settings → Integrations → GitHub**.
3. Click **Link GitHub account** and authorise `HumanGuardrail`.
4. Note the resulting **VCS link ID** (a UUID shown in the integration list).
   You will need it in step 3.

Without this step the build will fail with a VCS-access error; there is no
workaround via the API alone.

**1b. Have the Northflank Org API token.**

The token must be scoped to the `humangr` team. Keep it in a password manager
or a secrets vault — it is used in step 4 and must never be committed to git.

---

## 2. Generate the signing key

```sh
head -c 32 /dev/urandom | base64
```

This is `FABRIC_SIGNING_KEY`. Every `AttestationChain` the fabric issues is
signed with this key. **Treat it as a root secret:**

- Store it in Northflank's secret store (see step 3), not in the JSON template
  or in git.
- Rotate it only during a planned maintenance window; rotation invalidates all
  live attestations.

---

## 3. Set environment variables and secrets

Open **Northflank → project `corelink-runners` → Secrets** (or use the
`/v1/teams/humangr/projects/corelink-runners/secrets` API) and create the
following entries **before** creating the service.

| Variable | Kind | Value |
|---|---|---|
| `FABRIC_SIGNING_KEY` | **SECRET** | Output of step 2 |
| `FABRIC_PAT` | **SECRET** | Bootstrap PAT mapped to `FABRIC_TENANT` |
| `NORTHFLANK_API_TOKEN` | **SECRET** | Northflank Org API token (step 1b) |
| `FABRIC_TENANT` | plain | Lowercase `[a-z0-9-]` tenant id, e.g. `humangr` |
| `FABRIC_TENANT_MAX_CONCURRENCY` | plain | Integer >= 1, e.g. `10` |
| `FABRIC_TENANT_RATE_PER_MIN` | plain | Default `120`; adjust as needed |
| `NORTHFLANK_TEAM_ID` | plain | `humangr` |
| `NORTHFLANK_PROJECT_ID` | plain | `corelink-runners` |

`FABRIC_BIND_ADDR` (default `0.0.0.0:8080`) and `FABRIC_REAP_INTERVAL_SECS`
(default `30`) are included in the template for explicitness; omit if the
defaults are acceptable.

Do **not** set `FABRIC_DEV_UNSAFE` — it is banned in production (the server
refuses to start if a non-loopback bind address is combined with the dev key).

---

## 4. Create the service

Fill the `<FILL: ...>` placeholders in `deploy/northflank-service.json` (do
this on a local copy — never commit filled-in secrets). Then POST the spec:

```sh
curl -s -X POST \
  "https://api.northflank.com/v1/teams/humangr/projects/corelink-runners/services" \
  -H "Authorization: Bearer <NORTHFLANK_API_TOKEN>" \
  -H "Content-Type: application/json" \
  -d @deploy/northflank-service.json
```

The endpoint is **team-scoped** (the API token is an Org token bound to the
`humangr` team). Confirm the exact field names for `billing`, `buildSettings`,
and `vcsData` against the [Northflank API reference](https://northflank.com/docs/v1/api)
at deploy time — the spec above uses the documented names as of the authoring
date, but Northflank can version these. Also confirm at deploy time:
`runtimeEnvironment` (the env-injection key for service environment variables)
and the `ports[].public: true` ingress shape (the path for exposing the HTTP
port publicly) — both are asserted in the template but should be validated
against the live Northflank API docs before submitting.

On success the response contains the service ID. Save it; you will need it for
status polling and teardown.

---

## 5. Verify

**5a. Poll build + deploy status.**

```sh
curl -s \
  "https://api.northflank.com/v1/teams/humangr/projects/corelink-runners/services/corelink-fabricd" \
  -H "Authorization: Bearer <NORTHFLANK_API_TOKEN>" \
  | jq '.data.deployment.status'
```

Wait until the status is `running` (typically 2–5 min for the first build).

**5b. Health check.**

```sh
curl -s https://<assigned-northflank-domain>/v1/health
# Expected: 200 OK  body: "ok"
```

The public domain is shown on the service detail page under the `http` port.

**5c. Smoke test — acquire → exec → close (bootstrap PAT).**

This mirrors the E2E sequence proven locally. Replace `<PAT>` with
`FABRIC_PAT` and `<HOST>` with the public domain.

```sh
# Acquire a lease
# URL: POST /v1/leases  (no /acquire suffix)
# Tenant is derived from the PAT — do NOT send `tenant` or `ttl_secs`
# All fields in the body are required (deny_unknown_fields)
LEASE_RESP=$(curl -s -X POST https://<HOST>/v1/leases \
  -H "Authorization: Bearer <PAT>" \
  -H "Content-Type: application/json" \
  -d '{
    "image_digest": "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc",
    "net_policy": "isolated",
    "tmp_root": "/work/tmp",
    "expiry_ms": 120000
  }')

# Response shape: {"lease": {..., "lease_id": "..."}, "exec_endpoint": "..."}
LEASE=$(echo "$LEASE_RESP" | jq -r '.lease.lease_id')
echo "lease: $LEASE"

# Exec a job
# URL: POST /v1/leases/<LEASE_ID>/exec
# Body uses CheckDef + tree_hash (deny_unknown_fields); no `steps` field
curl -s -X POST https://<HOST>/v1/leases/$LEASE/exec \
  -H "Authorization: Bearer <PAT>" \
  -H "Content-Type: application/json" \
  -d '{
    "check_def": {
      "def_digest": "sha256:def000000000000000000000000000000000000000000000000000000000000000",
      "command": "echo smoke-ok",
      "inputs": [],
      "toolchain_ref": "toolchain:alpine",
      "env_manifest": "",
      "glob_set": []
    },
    "tree_hash": "sha256:tree000000000000000000000000000000000000000000000000000000000000000"
  }'

# Close the lease
# `status` field is required — a bodyless close fails deserialization
curl -s -X POST https://<HOST>/v1/leases/$LEASE/close \
  -H "Authorization: Bearer <PAT>" \
  -H "Content-Type: application/json" \
  -d '{"status":"succeeded"}'
```

A 200 on all three calls confirms the full path: auth → lease lifecycle →
cloud exec → teardown.

---

## 6. Operational notes

**One-provider elegance.** The fabric service and the per-job compute it
spawns are both on Northflank. No cross-provider hop on the hot path; egress
posture follows ADR-0003.

**Fail-closed posture.**
- No `FABRIC_SIGNING_KEY` → process refuses to start. No signed attestations
  are possible without it.
- No `NORTHFLANK_*` vars → exec/provision fall back to `NoBoxExec` /
  `NoBoxProvisioner`; every exec returns 503. The lease lifecycle still works;
  execution does not.

**Single-instance constraint (M1).** The ledger is `InMemoryLedger` — leases
are not persisted and reset on restart. Do **not** scale `deployment.instances`
beyond `1` until a shared (Postgres) ledger lands (ratified decision #3, M1+).
Scaling to >1 will produce split-brain lease state silently.

**Cost shape.** The `corelink-fabricd` service itself runs on `nf-compute-20`
(always-on, small fixed cost). Per-job microVMs are billed only when running —
consistent with the CoreLink concurrency pricing model (flat concurrency seats,
not per-minute).

**Envelope / §13 emission** routes are LIVE (envelope-wire landed, PR #30). The
per-lease `CaptureHook` is registered at acquire on the Held path, so
`POST /v1/leases/{id}/envelope/events` and `GET /v1/leases/{id}/envelope/meta`
serve on the real exec path. The §13.2 ack is driven through the lease `close`
body. The `IntentMetrics` §13.4 conformance vector is live on both sides (#5).

---

## 7. Rollback / teardown

To delete the service (destructive — stops all running leases):

```sh
curl -s -X DELETE \
  "https://api.northflank.com/v1/teams/humangr/projects/corelink-runners/services/corelink-fabricd" \
  -H "Authorization: Bearer <NORTHFLANK_API_TOKEN>"
```

Confirm the field name and HTTP verb against the Northflank API docs before
executing. A soft rollback (redeploy previous image) is preferred where
possible; use the Northflank dashboard rollback button on the service detail
page.

---

## 8. Deploy gotchas (learned on the 2026-06-13 go-live)

The fabric went LIVE on Northflank end-to-end on 2026-06-13 (acquire → real
microVM → exit 0 → signed attestation → teardown, provider verified clean).
These are the traps we hit; document-once so the next deploy is painless.

**Live service facts (the deployed instance):**

| Field | Value |
|---|---|
| Org | `human-guardrail` |
| Team | `humangr` |
| Service | `corelink-runners` |
| Public host | `p01--corelink-runners--pmk6nf8xbcjb.code.run` |
| Plan | `nf-compute-50` |
| Instances | `1` — **NEVER >1** until a shared ledger lands (split-brain; see §6) |

**8a. Dockerfile is NOT at the repo root.** It lives at
`crates/corelink-fabric-server/Dockerfile`. In the Northflank build settings:

- **Dockerfile location = `/crates/corelink-fabric-server/Dockerfile`**
- **Build context = `/`** (repo root — the build needs the whole workspace)

A root-relative Dockerfile path will fail the build with a "file not found".

**8b. Env var changes need a real container restart.** Saving an env-var change
in the Northflank UI does **not** by itself reload the running container — the
pod keeps the OLD environment until it is recreated. Use **Terminate** (it
recreates the pod) to force a true restart.

- Diagnostic: the in-memory lease counter resets to `...0001` only on a true
  restart. If a fresh acquire does not start at `...0001` after you changed an
  env var, the container did **not** pick up the change yet.

**8c. The killer trap — a typo'd env KEY fails silently to NoBox.** The required
cloud var is the **singular** `NORTHFLANK_PROJECT_ID`. A plural typo
(`NORTHFLANK_PROJECTS_ID`) leaves `NORTHFLANK_PROJECT_ID` unset, so the backend
falls back to `NoBoxExec` and **every exec returns 503 "no execution backend
attached"** — while the lease lifecycle keeps working, masking the cause.

- Now self-diagnosing: the boot log names the missing var on a partial cloud
  config (`cloud_backend_status`, PR #33) — it will never claim "Northflank"
  while silently running NoBox. Check the boot log first.

**8d. The two-stage rollout that worked.** Bring the service up fail-closed,
then arm cloud exec — so a misconfig surfaces as an honest 503, never a
silent-wrong exec:

- **STAGE 1 — static backend.** Set the 7 `FABRIC_*` vars
  (`FABRIC_SIGNING_KEY`, `FABRIC_PAT`, `FABRIC_TENANT`,
  `FABRIC_TENANT_MAX_CONCURRENCY`, `FABRIC_TENANT_RATE_PER_MIN`, plus the
  explicit `FABRIC_BIND_ADDR` / `FABRIC_REAP_INTERVAL_SECS` if not defaulting).
  Health is `200`; the lease lifecycle works; exec is `503` (fail-closed, no
  cloud creds — expected).
- **STAGE 2 — arm cloud exec.** Add `NORTHFLANK_API_TOKEN`,
  `NORTHFLANK_TEAM_ID=humangr`, `NORTHFLANK_PROJECT_ID=corelink-runners`, then
  **restart** (§8b). Exec now provisions a real microVM and returns `200`.

**8e. Smoke test.** The acquire → exec → close sequence in §5c is the exact one
we ran live (`POST /v1/leases`, `POST /v1/leases/{id}/exec`,
`POST /v1/leases/{id}/close {"status":"succeeded"}`). DTOs and URLs verified
against `corelink-fabric-api` — no drift.
