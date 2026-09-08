# Secret-rotation checklist — CoreLink Runners (Northflank fallback fabric)

> **Scope: FALLBACK / ALTERNATIVE PATH.** The canonical production edge is the
> Cloudflare `fabricd` + spawn-Worker deployment. This checklist covers secrets
> mounted on the Northflank `corelink-runners` fallback service only. For the
> canonical Worker secret inventory and rotation procedure, use
> `docs/runbook/secret-inventory.md` and `docs/runbook/cloudflare-go-live.md`.

> Per-variable runbook for rotating every live secret in `corelink-fabricd`.
> Source of truth for variable semantics: `crates/corelink-fabric-server/src/server.rs`
> and `crates/corelink-fabric-server/src/cloud_exec.rs`.
> Apply step and the NEW-BUILD gotcha are in §9.

---

## Quick-reference table

| Variable | Zero-downtime? | Highest blast-radius concern |
|---|---|---|
| `FABRIC_SIGNING_KEY` | No — consumer re-fetch required | Forged attestations |
| `FABRIC_PAT` | No — client token update required | Tenant impersonation |
| `DATABASE_URL` (Postgres password) | No — atomic ledger reconnect | Full lease-state access |
| `NORTHFLANK_API_TOKEN` | Yes (rolling) | Provision/teardown/exec of all boxes |
| `NORTHFLANK_PROJECT_ID` | Not a secret; see note | Mismatch breaks cloud backend |
| `FABRIC_INTROSPECT_AUTH_KEY` | Yes (rolling) | Bypass per-acquire auth check |
| `FABRIC_ADMIN_KEY` | Yes (rolling) | Unlimited tenant onboarding |
| `FABRIC_OBSERVABILITY_KEY` | Yes (rolling) | Internal occupancy data leak |

---

## 1. `FABRIC_SIGNING_KEY` — fabric attestation signing key

**What it is.** A 32-byte secret used by `FabricSigner` to produce every
`AttestationChain` the fabric emits. It also seeds the **ingest-scoped HMAC
key** that is injected into sandboxed boxes in place of the tenant PAT — derived
deterministically via `SHA-256("corelink-ingest-secret:v1:" ++ signing_key)`.
Rotating the signing key rotates the ingest secret implicitly.

**Blast-radius if leaked.** An attacker with this key can forge valid
`AttestationChain` payloads — any claim about what ran inside a box, including
supply-chain provenance and fence results, becomes spoofable. This is the
highest-value secret in the fabric. A leaked ingest key lets a box exfiltrate
a write-capable scoped token for the duration of any lease; however a box that
learns the _signing_ key can re-derive the ingest key, so treat the signing key
as giving both capabilities.

**Generate a new value.**

```bash
openssl rand -base64 32
# Example output (never reuse): oJ3kL8yZ2m7N1qP5tX0wA6vR4sF9eB/GhUiWcYdOE=
```

The value must be exactly 32 bytes after base64-decode. `server.rs`
`config_from_env` rejects anything that decodes to a length != 32 at boot.
Trailing newlines are stripped automatically (secret-mount safety), but double-
check that the value you paste carries no embedded whitespace.

**Where to set it in Northflank.**
Northflank project → `corelink-runners` service → Environment → Secret
environment variables → `FABRIC_SIGNING_KEY`. Set the new value; do NOT save
yet — update `DATABASE_URL` in the same save if you are rotating both together.

**Order and coordination (NOT zero-downtime).**

1. Generate the new key offline (`openssl rand -base64 32`).
2. The published attestation-verification key at `GET /v1/attestation/key`
   changes on the next boot. Any configured CoreLink verifier that has cached
   the old key will reject attestations signed with the new key until it
   re-fetches.
3. Notify configured CoreLink verifiers before rotating. Agree on a
   coordination window: new value set → NEW BUILD → verifiers re-fetch.
4. Set the new value in Northflank env, trigger a NEW BUILD (see §9).
5. Immediately after boot: `curl https://<service-host>/v1/attestation/key`
   to confirm the published key changed.
6. Confirm upstream consumers re-fetched the new key and attestation
   verification is green.

**The ingest-key side-effect.** Any in-flight leases whose boxes received the
old ingest token will present tokens that the new key rejects. Drain or
terminate in-flight leases before rotating if continuity matters.

---

## 2. `FABRIC_PAT` — bootstrap tenant bearer token (static-auth mode)

**What it is.** The single PAT (Personal Access Token) that `StaticTokenStore`
accepts on the `Authorization: Bearer <token>` header in
`FABRIC_AUTH_BACKEND=static` mode. It maps to `FABRIC_TENANT` (the bootstrap
tenant id). Required and non-empty in static mode; ignored in `corelink` mode.

**Blast-radius if leaked.** An attacker with this token can call any
authenticated fabric endpoint as the bootstrap tenant: acquire leases, exec
commands in boxes, close leases, and exhaust the concurrency cap. They cannot
read the signing key from API responses (it is never returned), but they can
trigger unlimited Northflank job spawns (direct cost impact).

**Generate a new value.**

```bash
# A URL-safe random token; 32 bytes of entropy is sufficient.
openssl rand -base64 32 | tr -d '/+=' | cut -c1-43
# Or use your preferred PAT format (e.g. "clr_" prefix for clarity).
```

**Where to set it in Northflank.**
Same service → Environment → `FABRIC_PAT`. Set the new value.

**Order and coordination (NOT zero-downtime in static mode).**

1. Generate new token.
2. Update every configured client (for example, a CI script) to use the new
   token BEFORE the NEW BUILD, or coordinate a cutover window:
   - option A (rolling): clients update token, then deploy. Brief window where
     the old token is still live; new token rejected until deploy.
   - option B (atomic): set new value in Northflank, trigger NEW BUILD, then
     update clients. Brief window where old clients get 401.
3. In `corelink` auth mode (`FABRIC_AUTH_BACKEND=corelink`) this variable is
   ignored entirely — skip.

---

## 3. `DATABASE_URL` — Postgres connection URL (ledger backend)

**What it is.** The full Postgres connection string including username and
password, consumed by `PgLedger::connect` when `FABRIC_LEDGER_BACKEND=pg`.
Northflank surfaces this as the addon's `POSTGRES_URI`. Required and non-empty
when the pg backend is selected; hard boot error if absent (no silent fallback
to memory — see `server.rs` fail-closed comment).

**Blast-radius if leaked.** Full read/write access to the `leases`,
`billing_events`, and `lease_envelopes` tables — the authoritative state
machine for every lease, concurrency cap, and billing record. An attacker can
read or destroy all tenant lease state, forge billing records, and wipe the
ledger.

**Generate a new value (Postgres password rotation).**

Northflank manages the addon password. To rotate:

1. Northflank project → `corelink-ledger` addon → Settings → Reset credentials
   (or rotate password). Northflank generates a new password.
2. Copy the new `POSTGRES_URI` from the addon's connection-string panel.

**Where to set it in Northflank.**
Service `corelink-runners` → Environment → `DATABASE_URL`. Paste the new URI.

**Order and coordination (NOT zero-downtime — ledger reconnect).**

1. Rotate the Postgres password in the addon (step above).
2. The old `DATABASE_URL` is immediately invalid after the Northflank rotation.
   The fabric's pool will begin returning connection errors on the next query.
3. Update `DATABASE_URL` in the service env **in the same Northflank save** as
   the credential reset (or as fast as possible after) to minimize the window.
4. Trigger a NEW BUILD — `PgLedger::connect` re-establishes the pool with the
   new URI. Until the build completes, the running instance will fail Postgres
   queries (leases cannot be admitted, held leases can still be closed if the
   pool has cached connections).
5. After the NEW BUILD boot, confirm the ledger log line:
   `ledger backend: Postgres (persistent, multi-instance cap-safe; pool=…)`.

**Multi-instance note.** With N instances running, all will lose their pools
simultaneously on a credential rotation. A rolling restart alone is not
sufficient to recover — all instances need a NEW BUILD with the new URI.

---

## 4. `NORTHFLANK_API_TOKEN` — Northflank platform API token

**What it is.** The bearer token passed to the Northflank API for every
box-lifecycle call: `NorthflankEngine::spawn` (job create), `exec_captured`
(exec), `delete_job` (teardown), and `is_alive` (liveness probe). Read by
`NorthflankConfig::from_env` in `cloud_exec.rs`. Both `NORTHFLANK_API_TOKEN`
and `NORTHFLANK_PROJECT_ID` must be non-empty for `cloud_backend_from_env` to
wire the Northflank backend; if either is absent the fabric falls back to
`NoBoxExec` / `NoBoxProvisioner` (fail-closed, no exec).

**Blast-radius if leaked.** Full control over the Northflank project: create,
exec into, and delete any job in the project; read job logs and metadata. An
attacker can spawn arbitrary workloads on your account, incur compute costs,
and access any data that lands inside a running box.

**Generate a new value.**

Northflank → Account → API tokens → Create new token (project-scoped,
minimum permissions: job:create, job:delete, job:exec, job:read). Copy the
token on creation — it is shown only once.

**Where to set it in Northflank.**
Service `corelink-runners` → Environment → `NORTHFLANK_API_TOKEN`. Set the new
value.

**Order and coordination (zero-downtime rolling).**

1. Create the new token in Northflank with the same project scopes as the
   existing token.
2. Set `NORTHFLANK_API_TOKEN` to the new value in the service env.
3. Trigger a NEW BUILD. The new instances boot with the new token; in-flight
   exec calls on old instances use the old token until those instances drain
   (graceful shutdown via SIGTERM).
4. Revoke the old token in Northflank after all old instances have terminated.
5. Verify with `corelink smoke` (see §9) that exec still works.

---

## 5. `NORTHFLANK_PROJECT_ID` — Northflank project identifier

**What it is.** Not a secret in the cryptographic sense — it is the Northflank
project slug (e.g. `corelink-runners`) that scopes every API call. Both this
and `NORTHFLANK_API_TOKEN` are required for the cloud backend to be wired
(see `cloud_backend_status` in `cloud_exec.rs`). A mismatch between the token's
project scope and this ID causes all API calls to 403/404.

**Blast-radius if leaked.** Low on its own — it names the project but confers
no capability without the API token.

**If you need to change it** (project rename or migration):

1. Update `NORTHFLANK_PROJECT_ID` in the service env.
2. Trigger a NEW BUILD.
3. Verify boot log: `cloud backend: Northflank (wired)` (not `partial` or
   `off`).

---

## 6. `FABRIC_INTROSPECT_AUTH_KEY` — CoreLink introspection service secret

**What it is.** The shared secret passed as `Authorization: Bearer <key>` to
the CoreLink introspection endpoint (`CORELINK_INTROSPECT_URL`) when
`FABRIC_AUTH_BACKEND=corelink`. Used by both `CoreLinkTokenStore` (per-request
auth) and `CoreLinkPlanStore` (per-acquire concurrency cap). Required and
non-empty in `corelink` mode; ignored in `static` mode.

**Blast-radius if leaked.** An attacker can call the CoreLink introspection
endpoint as the fabric service — read token-to-tenant mappings and concurrency
caps for any tenant whose token they know. This is an internal service-to-
service credential; exposure is bounded by what the introspect endpoint returns.

**Generate a new value.**

```bash
openssl rand -base64 32
```

Coordinate with the CoreLink Cache team — both sides must rotate simultaneously
(the Cache endpoint and this fabric config). This is a cross-repo change.

**Where to set it in Northflank.**
Service `corelink-runners` → Environment → `FABRIC_INTROSPECT_AUTH_KEY`.

**Order and coordination (zero-downtime if coordinated).**

1. Agree on new value with the CoreLink Cache team.
2. CoreLink Cache team deploys the new accepted key on their side first.
3. Set `FABRIC_INTROSPECT_AUTH_KEY` to the new value and trigger a NEW BUILD.
4. CoreLink Cache team revokes the old key after this fabric's build is live.

---

## 7. `FABRIC_ADMIN_KEY` — tenant-onboarding admin endpoint secret

**What it is.** The secret gating `POST /internal/v1/admin/tenants` (the
operator onboarding route, WP-C). Default-off: absent/empty → the route returns
404. When set, requests must present a matching `X-Corelink-Internal-Auth`
header. Only wired in `static` auth mode.

**Blast-radius if leaked.** An attacker can POST to the admin endpoint and
onboard arbitrary tenants with arbitrary concurrency caps — bypassing all
billing controls. In the current seed (static mode, bootstrap tenant), this is
partially mitigated by the fact that the ledger still enforces per-tenant caps;
however, an attacker can create unlimited tenants and saturate the Northflank
project with spawned boxes (cost impact).

**Generate a new value.**

```bash
openssl rand -base64 32
```

**Where to set it in Northflank.**
Service `corelink-runners` → Environment → `FABRIC_ADMIN_KEY`.

**Order and coordination (zero-downtime rolling).**

1. Generate new key.
2. Update any internal tooling or scripts that POST to the admin endpoint to
   use the new key.
3. Set `FABRIC_ADMIN_KEY` to the new value in the service env, trigger a NEW
   BUILD.
4. The old key is rejected on the next request after the new build is live.
   No in-flight tenant state is affected — the key only gates the POST endpoint.

---

## 8. `FABRIC_OBSERVABILITY_KEY` — internal occupancy endpoint secret

**What it is.** The secret gating `GET /internal/v1/occupancy`
(WP-OCCUPANCY-API). Default-off: absent/empty → the route returns 404. When
set, requests must present a matching `X-Corelink-Internal-Auth` header.
Independent of `FABRIC_ADMIN_KEY`.

**Blast-radius if leaked.** Read-only occupancy data (active lease counts per
tenant). No write capability; no credentials exposed. Low severity, but still a
data-leak of internal concurrency telemetry.

**Generate a new value.**

```bash
openssl rand -base64 32
```

**Where to set it in Northflank.**
Service `corelink-runners` → Environment → `FABRIC_OBSERVABILITY_KEY`.

**Order and coordination (zero-downtime rolling).**

1. Generate new key.
2. Update any monitoring/dashboarding scripts that poll `/internal/v1/occupancy`.
3. Set the new value in Northflank env, trigger a NEW BUILD.
4. Old key rejected after the build is live.

---

## 9. Apply step — NEW BUILD required (not Restart)

**Every secret rotation that touches Northflank env vars requires a NEW BUILD.**
A Restart (or Terminate + Restart) reboots the containers on the existing image
with the existing env — it does NOT pick up env changes that were saved after
the last build. This is the #1 operational gotcha (documented in
`docs/deploy/northflank-postgres-runbook.md` §1 with a real 2026-06-14
incident).

**Rotation apply checklist:**

1. Set the new secret value(s) in Northflank → Service → Environment →
   Save. (For atomic multi-secret rotations, set all changed values in one
   save before triggering the build.)
2. Northflank → Service → **New Build** (or push a commit to trigger a
   git-linked build). Confirm a build job starts — do NOT click Restart.
3. Watch the build log to completion.
4. After rollout, inspect the boot log for:
   - Signing key: `signer: Ed25519 (…)` — the fingerprint should change.
   - Ledger: `ledger backend: Postgres (persistent, multi-instance cap-safe; pool=…)`
   - Cloud backend: `cloud backend: Northflank (wired)` (not `partial` or `off`)
5. Run `corelink smoke` against the live endpoint to confirm the fabric still
   serves: acquire a lease, exec a no-op command, close the lease, verify the
   attestation chain. A green smoke confirms all secrets are accepted and the
   execution path is end-to-end live.
6. For `FABRIC_SIGNING_KEY` rotations: verify `GET /v1/attestation/key`
   returns the new public key, and confirm every configured CoreLink verifier
   has re-fetched it.
7. Revoke or delete the old secret value at its source (old Northflank API
   token, old Postgres password) after you have confirmed the new build is
   healthy. Do NOT revoke before the build is live — there is a brief window
   where the running instance still needs the old credential.

---

## Rotation risk summary

| Variable | Zero-downtime? | Notes |
|---|---|---|
| `FABRIC_SIGNING_KEY` | **No** — brief service degradation + verifier re-fetch | Highest priority; coordinate configured CoreLink verifiers before rotating |
| `FABRIC_PAT` | **No** — client cutover window | Coordinate client update with build |
| `DATABASE_URL` | **No** — pool reconnect gap between addon reset and NEW BUILD | Minimize the credential-reset → NEW BUILD window |
| `NORTHFLANK_API_TOKEN` | **Yes** — rolling (old instances drain, new boot with new token) | Revoke old token only after old instances terminate |
| `NORTHFLANK_PROJECT_ID` | Not a secret; update freely with NEW BUILD | |
| `FABRIC_INTROSPECT_AUTH_KEY` | **Yes** — if CoreLink Cache deploys new key first | Cross-repo coordination required |
| `FABRIC_ADMIN_KEY` | **Yes** — rolling | Update tooling before or immediately after build |
| `FABRIC_OBSERVABILITY_KEY` | **Yes** — rolling | Update monitoring scripts before or immediately after build |
