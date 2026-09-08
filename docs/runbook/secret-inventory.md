# Secret Inventory — CoreLink Runners production fabric

Every secret across the two prod Workers, what it authenticates, and how to
**check its presence without ever printing a value**. Companion to
[`incident-playbook.md`](./incident-playbook.md).

> **No secret values live in this repo, and none can be read back.** Cloudflare
> Worker secrets are write-only: `wrangler secret list` returns **names only**,
> `wrangler secret put` reads from stdin. There is no `wrangler secret get`.
> This inventory lists NAMES and SURFACES, never values.

## How to check presence (names only, no values)

```sh
# spawn-worker secrets:
npx wrangler secret list --name corelink-spawn-worker

# fabricd secrets:
npx wrangler secret list --name corelink-fabricd
```

`--name <worker>` targets the deployed Worker by its `wrangler.jsonc` `name`, so
you can run this from any directory. The output is a JSON array of `{name, type}`
— compare it against the "Expected on" column below. **Non-secret `vars`**
(URLs, tenant ids, shard count) live in cleartext in each `wrangler.jsonc` `vars`
block and are not secrets.

---

## spawn-worker (`corelink-spawn-worker`) — `deploy/cloudflare/`

| Secret | Authenticates / enables | Fail-closed behavior when absent | Notes |
|---|---|---|---|
| `CLOUDFLARE_SPAWN_AUTH_TOKEN` | Inbound `POST /v1/spawn` from fabricd. **Must byte-match** fabricd's `CLOUDFLARE_SPAWN_AUTH_TOKEN`. | Missing/mismatch ⇒ 401 on every spawn. | Shared control-credential across both surfaces. Rotate on BOTH workers together. |
| `CLOUDFLARE_EXEC_AUTH_TOKEN` | Inbound `POST /v1/exec` from fabricd. Must byte-match the fabricd exec token. | Missing, mismatched or overlapping domain tokens ⇒ 401. | Rotate matching copies on both Workers; keep distinct from spawn and lifecycle. |
| `CLOUDFLARE_LIFECYCLE_AUTH_TOKEN` | Inbound status, teardown, egress-cutoff and tenant-suspension control from fabricd. Must byte-match the fabricd lifecycle token. | Missing, mismatched or overlapping domain tokens ⇒ 401. | Rotate matching copies on both Workers; keep distinct from spawn and exec. |
| `EXEC_SERVER_AUTH_TOKEN` | The check-host exec-server; injected into the check-host container at spawn, presented on `/v1/exec`. | Unset ⇒ a `mode:"check"` spawn **fails closed 503** (O7 hardening). | Only needed once check-host is live-flipped. |
| `GITHUB_WEBHOOK_SECRET` | HMAC (`X-Hub-Signature-256`) verify on `POST /webhook`. | Absent ⇒ `/webhook` returns `503 "autoscaler not configured"`. | Must equal the GitHub App's configured webhook secret (verify via `/app/hook/config`). |
| `GITHUB_MINT_TOKEN` | First-party JIT runner mint (`generate-jitconfig`), `Administration:write` on `HuGR-Labs` repos. | Absent ⇒ `/webhook` 503. | Dogfood path. Customer repos use the App path instead. |
| `GITHUB_APP_ID` | The App's numeric id — the App-JWT `iss` (`github_app.ts`). | Absent ⇒ App path inert; every mint falls back to `GITHUB_MINT_TOKEN`. | Pairs with the private key below. May be a `var` or a secret. |
| `GITHUB_APP_PRIVATE_KEY` | Signs the RS256 App JWT → installation-token mint for **customer** repos. **PKCS#8 PEM required.** | Absent ⇒ App path inert (default-safe). A PKCS#1 PEM ⇒ mint **fails closed** (throws at import). | **Hygiene flag — see below.** The root credential for minting on any installed repo. |
| `CORELINK_RUNNER_MINT_AUTH_KEY` | `x-corelink-internal-auth` for the D-9 per-job CAS PAT mint (cache-warm). | Absent ⇒ runner spawns **COLD** (no cache-warm), fail-open. | Delivered by the Server TL. |
| `BILLING_INGEST_AUTH_KEY` | `x-corelink-internal-auth` for the billing usage push to corelink-billing. | Absent ⇒ no usage-push (jobs still run). | Dedicated key — never the shared spawn token, never the mint key. |
| `METRICS_OBSERVABILITY_KEY` | Gates `GET /internal/v1/metrics` (`X-Corelink-Internal-Auth`). | Absent ⇒ the metrics route **404s** (invisible). | Ops-READ key, deliberately separate from the spawn-CONTROL token so it can rotate independently. |
| `FLEET_BUSY_READ_KEY` | Gates `GET /internal/v1/fleet/busy` (`X-Corelink-Internal-Auth`) — the pre-roll deploy gate's authority on "is a box executing customer work". | Absent ⇒ the route **404s** (invisible) ⇒ `deploy-spawn-worker.yml` refuses to roll unless dispatched with `force: true`. | Ops-READ key, separate from BOTH the spawn-CONTROL token and `METRICS_OBSERVABILITY_KEY`. The **same value** must be bound as the GH Actions secret `FLEET_BUSY_READ_KEY` in `HuGR-Labs/corelink-runners`; rotate both together. |

Non-secret `vars` here (not secrets): `CLW_TENANT`, `CLW_ENDPOINT`,
`CORELINK_MINT_URL`, `RECONCILER_REPOS`, `REPO_INSTALLATION_MAP`,
`SPAWN_WORKER_PUBLIC_URL`. KV binding `RUNNER_JOB_PATS`
(id `4fb7e9c773d64f83ae3415c5a0879d66`).

---

## fabricd (`corelink-fabricd`) — `deploy/cloudflare-fabricd/`

| Secret | Authenticates / enables | Fail-closed behavior when absent | Notes |
|---|---|---|---|
| `FABRIC_SIGNING_KEY` | Signs every `AttestationChain` the fabric issues (`/v1/attestation/key` serves the pubkey). | **Process refuses to start** without it — no signed attestations possible. | **Root secret.** Rotating it invalidates all live attestations — planned window only. Prod key id `faa5b7726ccd2c52`. |
| `FABRIC_INTROSPECT_AUTH_KEY` | `x-corelink-internal-auth` for PAT introspection against CoreLink (tenant derivation on acquire). | Absent ⇒ tenant introspection fails ⇒ acquires rejected. | |
| `BILLING_INGEST_AUTH_KEY` | `x-corelink-internal-auth` for the fabricd billing usage push. | Absent ⇒ no usage-push. | Same role as the spawn-worker's, separate value. |
| `CLOUDFLARE_SPAWN_AUTH_TOKEN` | Outbound auth to the spawn-worker's `/v1/spawn` (box provisioning). | Absent ⇒ box backend inert. **Must match** the spawn-worker's copy. | Rotate on BOTH workers together. |
| `CLOUDFLARE_EXEC_AUTH_TOKEN` | Outbound auth to the spawn-worker's `/v1/exec`. | Missing or partial scoped-token set ⇒ Cloudflare engine configuration is rejected before transport. | Dedicated exec-control token; never reuse spawn or lifecycle. |
| `CLOUDFLARE_LIFECYCLE_AUTH_TOKEN` | Outbound auth to status, teardown, egress-cutoff, and tenant-suspension control routes. | Missing or partial scoped-token set ⇒ Cloudflare engine configuration is rejected before transport. | Dedicated lifecycle-control token; never reuse spawn or exec. |
| `CORELINK_RUNNER_MINT_AUTH_KEY` | `x-corelink-internal-auth` for the moat per-job CAS PAT mint (env-0 C2c). | Part of the mint-arm trio; boot guard `validate_mint_arm` fails closed if the arm is partial. | Armed together with `FABRIC_CRED_TICKET_SECRET` + the mint URL var + `FABRIC_PUBLIC_BASE_URL`. |
| `FABRIC_CRED_TICKET_SECRET` | HMAC for the env-0 cred-ticket (the single-use `CLW_CRED_TICKET` injected instead of the raw PAT). | Part of the mint-arm trio (see above). | Keeps the CAS PAT out of the untrusted container env. |
| `DATABASE_URL` | Postgres connection for the durable `PgLedger` (persists lease state; arms the vCPU-h ceiling). | Absent ⇒ in-memory ledger (lease state resets on restart), ceiling cannot arm. | Required before raising `FABRIC_NUM_SHARDS`/`max_instances` > 1. Pair with `FABRIC_PG_TLS=require`. |
| `FABRIC_GITHUB_MINT_TOKEN` | PAT-broker JIT mint (preferred over the App path). `Administration:write`. | Absent ⇒ falls back to the `FABRIC_GITHUB_APP_*` trio. | ADR-0007 Stage-A runner broker. |
| `FABRIC_GITHUB_APP_ID` | Runner-broker App id (fallback mint). | All-absent ⇒ App path not wired; a `runner:` acquire without either mechanism is rejected. | Trio below. |
| `FABRIC_GITHUB_APP_INSTALLATION_ID` | Runner-broker App installation id. | (as above) | |
| `FABRIC_GITHUB_APP_PRIVATE_KEY_B64` | Base64 of the runner-broker App PEM (the `_B64` form survives env-var UIs). | (as above) | |
| `FABRIC_ADMIN_KEY` | Arms operator enforcement routes: tenant SUSPEND + admin onboarding. | Absent ⇒ those routes **404** (inert). | |
| `FABRIC_OBSERVABILITY_KEY` | Gates `GET /internal/v1/status` + `/internal/v1/occupancy` (`X-Corelink-Internal-Auth`). | Absent ⇒ those routes **404**. | The ops-READ key for the control plane. |
| `FABRIC_AUTOSCALER_WEBHOOK_SECRET` | Mounts the Stage-B autoscaler route (default-off). | Absent ⇒ the autoscaler route does not mount. | Pairs with `FABRIC_AUTOSCALER_PAT`; other `FABRIC_AUTOSCALER_*` are vars. |
| `FABRIC_AUTOSCALER_PAT` | The PAT the Stage-B autoscaler uses to mint runners. | Inert unless the webhook secret above is set. | |

Non-secret `vars` here (not secrets): `FABRIC_NUM_SHARDS`,
`CORELINK_INTROSPECT_URL`, `BILLING_INGEST_URL`, `BILLING_REGION`,
`CLOUDFLARE_SPAWN_WORKER_URL`, `FABRIC_PUBLIC_BASE_URL`, `CLW_ENDPOINT`,
`CORELINK_RUNNER_MINT_URL`, `FABRIC_EMIT_INTENT_METRICS_SIG`,
`FABRIC_ADMISSION_PAUSED` (absent/exact `0` fail-open; other values pause new
lease/admission/mint routes at the Worker edge with `503` + `Retry-After`).

## Repository, CI, and runtime surfaces

The drift checker also inventories names used outside the two production
Workers. This keeps CI credentials, env-0 hand-off names, and Cloudflare
bindings visible without ever recording a value.

| Name | Surface / contract |
|---|---|
| `COLD_ORGANIC_TENANT_PAT` | Spawn-worker tenant PAT selected by `REPO_TENANT_PAT_MAP`; absent mapping is rejected. |
| `CORELINK_CF_ACCESS_CLIENT_ID` | Fabricd/runner CF Access service-token client id; forwarded only when configured. |
| `CORELINK_CF_ACCESS_CLIENT_SECRET` | Fabricd/runner CF Access service-token client secret; forwarded only when configured. |
| `FABRIC_TEST_MINT_KEY` | Opt-in test-mint arming key; absent keeps the route 404/inert. |
| `FABRIC_GITHUB_APP_PRIVATE_KEY` | Optional fabricd GitHub-App PEM fallback credential for runner minting. |
| `FABRIC_PAT` | Fabric-side PAT name used by the local/CI operator tooling. |
| `PINNED_IMAGE_DIGEST` | Spawn-worker image admission `var`; absent leaves the optional pin disarmed. |
| `FABRIC_COMPUTE_TERMINAL_AUTHORITY` | Spawn-worker compute terminal-receipt authority expected from `FABRIC_COMPUTE_URL`; mismatches fail closed. |
| `FABRIC_COMPUTE_TERMINAL_PUBLIC_KEY` | Spawn-worker Ed25519 public key used to verify authenticated compute terminal receipts; missing or invalid configuration fails closed. |
| `FABRIC_COMPUTE_TERMINAL_RECEIPT_VERSION` | Spawn-worker compute terminal-receipt schema/version expected from `FABRIC_COMPUTE_URL`; mismatches fail closed. |
| `FABRIC_COMPUTE_TERMINAL_KEY_ID` | Spawn-worker compute terminal-receipt signing key id expected from `FABRIC_COMPUTE_URL`; missing or mismatched configuration fails closed. |
| `FABRIC_CREDENTIAL_ISSUER_AUTH_KEY` | Fabricd credential-issuer authorization key forwarded to the container; absent leaves issuer authorization disabled. |
| `CORELINK_ADMIN_KEY` | Local operator alias for the fabric admin key; value is never logged. |
| `CLOUDFLARE_API_TOKEN` | CI/API fallback token for Cloudflare deploy and container operations. |
| `CLOUDFLARE_CONTAINERS_API_TOKEN` | Preferred CI/API token for Cloudflare container operations. |
| `GITHUB_TOKEN` | GitHub Actions job token used by repository automation. |
| `GITHUB_RECONCILER_TOKEN` | Reconciler credential used by the repository automation path. |
| `GITHUB_WEBHOOK_REPO_SECRET` | Repository webhook HMAC secret used by the reconciler ingress. |
| `NPM_TOKEN` | Optional npm publish credential in the release workflow. |
| `PYPI_TOKEN` | Optional PyPI publish credential in the release workflow. |
| `RESEND_API_KEY` | Canary notification credential; absent makes notifications a no-op. |
| `CORELINK_PAT` | GitHub Actions integration PAT for CoreLink operations. |
| `CORELINK_PROBE_PAT` | Probe credential used by CI smoke/latency checks. |
| `CLW_TOKEN` | Legacy/raw env-0 token name; production injection uses the ticket below. |
| `CLW_CRED_TICKET` | Single-use env-0 credential ticket redeemed inside the trusted runner. |
| `CLW_LEASE_ID` | Lease binding paired with `CLW_CRED_TICKET` during redemption. |
| `CLW_FABRIC_ENDPOINT` | Fabric endpoint binding paired with env-0 runner credentials. |
| `TOOLCHAIN_DIGEST` | Check-host toolchain manifest digest; absent makes check-host startup fail closed. |
| `RUNNER_JOB_PATS` | Spawn-worker KV binding for per-job PAT state. |
| `CRED_STASH` | Spawn-worker Durable Object binding for single-use credential tickets. |
| `CONCURRENCY_SLOTS` | Spawn-worker Durable Object binding for fleet slot admission. |
| `WEBHOOK_LIMITER` | Spawn-worker rate-limit binding for webhook and diagnostic routes. |
| `METRICS` | Metrics binding used by the worker observability surface. |
| `CANARY_KV` | Canary Worker KV binding for probe state. |
| `FABRICD` | Fabricd service/container binding used by deployment configuration. |
| `FABRICD_SVC` | Canary service binding to `corelink-fabricd`. |
| `SPAWN_SVC` | Canary service binding to `corelink-spawn-worker`. |
| `CHECK_HOST_CONTAINER` | Spawn-worker Durable Object binding for check-host leases. |
| `RUNNER_CONTAINER` | Spawn-worker Durable Object binding for runner leases. |

> The `FABRIC_*` secrets and vars are set on the **Worker**, but the singleton
> **container** only sees what `wrangler.jsonc` explicitly forwards into its
> `envVars`, and reads them **only at boot**. Setting a secret is not enough — a
> container rollout (new image digest + `wrangler deploy`) is what makes fabricd
> pick it up. See the playbook §2b.

---

## Rotation checklist

Rotate on a **compromise**, on **staff departure**, or on a **scheduled cadence**
(quarterly for ops keys; the two root secrets on their own careful cadence).

1. **Generate** the new value out-of-band (password manager / vault). For
   symmetric keys: `head -c 32 /dev/urandom | base64`.
2. **Set** it — never echo it, never commit it:
   ```sh
   npx wrangler secret put <NAME> --name <corelink-spawn-worker|corelink-fabricd>
   # (paste the value at the prompt, or pipe from a vault: `vault read ... | wrangler secret put ...`)
   ```
3. **Rotate each control domain on both surfaces.** The spawn, exec and
   lifecycle tokens must each match between fabricd and the spawn Worker.
   Rotate the corresponding client and server copies together; the three
   domains rotate independently of one another and must remain distinct.
   `BILLING_INGEST_AUTH_KEY` must match its CoreLink counterpart.
4. **Roll the container** (fabricd only): the singleton reads env at boot, so
   after `wrangler secret put` you MUST trigger a rollout (new image digest +
   `wrangler deploy`) — see the playbook §2b. The spawn-worker picks up secrets
   on its next `wrangler deploy`.
5. **Verify presence** (never value): `wrangler secret list --name <worker>`.
6. **Verify function** with the read-only probes in the playbook (§1 health,
   §4b App-JWT probe). A 401 after rotation = the two ends drifted; re-sync.
7. **Special cases:**
   - `FABRIC_SIGNING_KEY` — rotation invalidates all live attestations; do it only
     in a planned window and re-publish the new `key_id` to hugit's verifier.
   - `GITHUB_APP_PRIVATE_KEY` / `FABRIC_GITHUB_APP_PRIVATE_KEY_B64` — regenerate the
     key in the GitHub App settings, re-encode PKCS#8 (`openssl pkcs8 -topk8
     -nocrypt`), set the new value, then **delete every local copy of the old and
     new `.pem`** (see hygiene below).
   - The env-0 mint trio (`CORELINK_RUNNER_MINT_AUTH_KEY`,
     `FABRIC_CRED_TICKET_SECRET`) must stay armed together with
     `FABRIC_PUBLIC_BASE_URL` or the fabricd boot guard fails closed.

---

## Hygiene action — REQUIRED (owner)

> **Audited 2026-08-23.** The item below had been open since 2026-07-16 with no
> record of whether anyone had acted on it. It had not been acted on. What was
> actually on disk, verified that day:
>
> | file | mode found | mode now |
> |---|---|---|
> | `~/Downloads/corelink-app.pk8.pem` | `600` | `600` |
> | `~/Downloads/corelink-runners-fleet.2026-06-15.private-key.pem` | **`644`** | `600` |
> | `~/Downloads/corelink-runners.2026-07-13.private-key.pem` | **`644`** | `600` |
>
> Two of the three App private keys were **world-readable for ~2 months**, on the
> machine that also runs the self-hosted CI runners (i.e. the box that executes
> workflow code). Permissions were tightened to `600` immediately; that removes
> the ongoing exposure but does **not** undo any read that already happened.
> `~/Downloads/githugr-clerk-pubkey.pem` is a PUBLIC key and is irrelevant here.
>
> **Step 1 confirmed:** `GITHUB_APP_PRIVATE_KEY` IS bound on `corelink-spawn-worker`
> (checked via the CF API secrets listing), so the running fabric does not read the
> local files — they are redundant copies and deleting them breaks nothing.
> ⚠️ But CF secrets are write-only: once the local copy is gone the value cannot be
> read back, only regenerated.
>
> **Step 3 (rotation): the owner decided NOT to rotate** on 2026-08-23, having been
> shown the `644`/2-month exposure and told plainly that deleting the file does not
> invalidate the key — anyone who copied it in that window still holds a working
> credential. Recorded here as a **conscious accepted risk, not an open action.**
> Revisit if the App's blast radius grows (more repos, more installs).
>
> **Step 2 (deletion) remains with the owner** and is the only step left.

**A GitHub-App private key was downloaded to `~/Downloads` and is now bound to the
running App.** A `.pem` in `~/Downloads` is a root credential sitting in a
world-readable, backup-synced, easily-leaked location — it can mint installation
tokens for every repo the App is installed on.

**Action for the owner:**

1. Confirm the key is set on the Worker (`wrangler secret list --name
   corelink-spawn-worker` shows `GITHUB_APP_PRIVATE_KEY`).
2. **Delete the local copy:**
   ```sh
   # Verify it's the App key first (do NOT cat the contents into logs), then:
   rm -P ~/Downloads/*.pem        # -P overwrites before unlinking (macOS)
   ```
   Also purge it from any clipboard manager, editor "recent files", and Downloads
   backup/sync.
3. If there is any doubt the key was exposed, **rotate it** (regenerate in the
   GitHub App settings → re-encode PKCS#8 → `wrangler secret put` → `wrangler
   deploy`) per the checklist above.
4. Going forward, keep App keys in the vault only; pull to a `~/secure/` file with
   `chmod 600` for the duration of a probe, then `rm -P` it.

---

## References

- Incident playbook: [`incident-playbook.md`](./incident-playbook.md).
- Configs: [`deploy/cloudflare/wrangler.jsonc`](../../deploy/cloudflare/wrangler.jsonc),
  [`deploy/cloudflare-fabricd/wrangler.jsonc`](../../deploy/cloudflare-fabricd/wrangler.jsonc).
- GitHub App mint: [`deploy/cloudflare/src/github_app.ts`](../../deploy/cloudflare/src/github_app.ts).
- Stabilization / hidden-debt plan:
  `docs/handoff/2026-07-16-STABILIZATION-plan-3lens-hidden-debt.md`.
