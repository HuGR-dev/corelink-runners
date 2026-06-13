# RUNBOOK — `corelink-fabricd` on Northflank

Deploy the fabric server as a long-running combined service on Northflank.
The fabric runs on Northflank **and** spawns its per-job microVMs on the same
provider — one plane, no cross-provider egress for the hot path.

---

## 1. Prerequisites

**1a. Connect GitHub to Northflank (owner step — do once).**

`humangr-labs/corelink-runners` is a private repo. Northflank must be granted
read access before it can pull source and build.

1. Log in to the Northflank dashboard as the `humangr` org owner.
2. Navigate to **Account settings → Integrations → GitHub**.
3. Click **Link GitHub account** and authorise `humangr-labs`.
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
date, but Northflank can version these.

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
LEASE=$(curl -s -X POST https://<HOST>/v1/leases/acquire \
  -H "Authorization: Bearer <PAT>" \
  -H "Content-Type: application/json" \
  -d '{"tenant":"humangr","ttl_secs":120}' \
  | jq -r '.lease_id')

echo "lease: $LEASE"

# Exec a no-op job (cloud backend: spawns a real Northflank job)
curl -s -X POST https://<HOST>/v1/leases/$LEASE/exec \
  -H "Authorization: Bearer <PAT>" \
  -H "Content-Type: application/json" \
  -d '{"steps":[]}'

# Close the lease
curl -s -X POST https://<HOST>/v1/leases/$LEASE/close \
  -H "Authorization: Bearer <PAT>"
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

**Envelope / §13 emission** routes are mounted but return 404 until
`CF-ENVELOPE-WIRE` lands (noted in `docs/deploy/fabric-server.md`). Do not
depend on those endpoints at this deployment stage.

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
