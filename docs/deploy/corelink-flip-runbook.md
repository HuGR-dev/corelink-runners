# RUNBOOK — CoreLink auth-backend flip (`static` → `corelink`)

> Executed the moment the CoreLink TL pings that **items 1 + 3 are live**:
> the `runners_entitlement` lookup is a real DB query (not a stub), and the
> real tenant PAT for the fabric is minted. Reference:
> `docs/handoff/2026-06-14-ack-corelink-flip-readiness-response.md`.
>
> **Current state (pre-flip):** `FABRIC_AUTH_BACKEND=static` — the static
> auth backend + admin-endpoint onboarding serves dogfood with zero CoreLink
> dependency. The static backend stays the rollback target. Do NOT flip until
> the CoreLink TL ping is received.

---

## 0. Pre-flip checks (gate before touching anything)

Confirm all four items before touching a Northflank secret:

| # | Item | How to confirm |
|---|------|----------------|
| 1 | CoreLink TL confirms `runners_entitlement` D1 lookup is live (real query, not the `false` stub) | Owner relays the explicit ping |
| 2 | `max_concurrency` wire shape unchanged — conformance vector `bfb38e28` byte-identical both sides | CoreLink TL ack in ping; our golden test `conformance/corelink-introspect.json` still green |
| 3 | Real CoreLink tenant PAT minted via `/_internal/pat/mint` for the fabric tenant (e.g. `humangr`) | Owner relays the minted PAT value |
| 4 | `FABRIC_INTROSPECT_AUTH_KEY` confirmed current/valid | CoreLink TL confirms in ping; key was set earlier as a Northflank secret |

If any of items 1, 3, or 4 is not confirmed, **stop** — the flip cannot proceed safely.

---

## 1. Set the Northflank secrets

Two secrets must be present before the new build reads them. Set them via
**Northflank UI → your service → Environment → Secrets** (or the Northflank
CLI — do not pass secrets as plain env vars):

```
FABRIC_PAT                 ← the minted tenant PAT from item 3 above
                             (replaces the dogfood bootstrap PAT in the static path;
                              in corelink mode the server does NOT read FABRIC_PAT,
                              but keep it set for rollback: removing it now would
                              break a rollback to static)

FABRIC_INTROSPECT_AUTH_KEY ← the CoreLink internal introspect auth secret
                              (already set from item 4; confirm it is still the
                              value CoreLink TL confirmed in the ping)
```

> **Secret hygiene:** neither value should appear in a chat message, a log,
> or a screenshot. If it does, rotate immediately before proceeding.

---

## 2. Set the new env vars and trigger a NEW BUILD

In **Northflank UI → your service → Environment** add or update:

```
FABRIC_AUTH_BACKEND        = corelink
CORELINK_INTROSPECT_URL    = <the CoreLink introspect endpoint URL, e.g.
                               https://api.corelink.humangr.com/_internal/v1/runners/introspect>
```

Leave all other env vars unchanged (`FABRIC_SIGNING_KEY`, `DATABASE_URL`,
`FABRIC_LEDGER_BACKEND`, `NORTHFLANK_*`, etc.).

> **FABRIC_PAT / FABRIC_TENANT are NOT required in `corelink` mode.** The server
> skips those env vars entirely — the tenant identity comes from the introspect
> response. Leave them set for rollback; the server ignores them.

### Trigger a NEW BUILD — not a Restart

**Northflank UI → Builds → New build → select `main` → Run.**

A **Restart** reuses the existing image and will NOT pick up the new env vars
wired into `config_from_env`. Only a NEW BUILD re-reads env at startup.

> Real gotcha (2026-06-14): code and env changes deployed via Restart have zero
> effect — the running image was compiled before the env vars changed. If
> behavior does not change after a "redeploy", you almost certainly hit Restart.
> See `northflank-postgres-runbook.md §1`.

---

## 3. Verify the build booted on the new backend

Watch the Northflank build log and the service boot log after the NEW BUILD
completes. Confirm:

- [ ] Boot log: `auth backend: CoreLink(…)` (not `AuthBackend::Static`) — the
      new backend is active. The `CoreLinkAuthConfig` fields are redacted in
      `ServerConfig::Debug`, so the secret does not appear in logs.
- [ ] Boot log: `ledger backend: Postgres (persistent, multi-instance cap-safe;
      pool=8; …)` — the Postgres ledger is still wired (unchanged; confirm it
      did not revert).
- [ ] `curl https://<service-host>/v1/health` → `ok`.

If the boot log shows `auth backend: AuthBackend::Static`, the new env vars
were not picked up — confirm you triggered a NEW BUILD and that
`FABRIC_AUTH_BACKEND=corelink` is set in the Northflank environment, then
rebuild.

---

## 4. Validate the three admission arms live

These three cases cover the full admission decision tree under the `corelink`
backend. Test with real HTTP calls against the live service.

**Variables used below:**

```sh
H=https://<service-host>
TENANT_PAT=<the minted tenant PAT from step 1>
```

### Arm A — valid entitlement → admit

The happy path: the PAT resolves to a tenant with a `runners_entitlement` row.
Each acquire fires two introspect calls to the live CoreLink endpoint (one for
token/tenant, one for plan/cap — a documented M1 inefficiency, not a blocker).

```sh
# Acquire a lease (pinned image required — unpinned is rejected at API level before introspect)
curl -s -X POST "$H/v1/leases" \
  -H "Authorization: Bearer $TENANT_PAT" \
  -H "Content-Type: application/json" \
  -d '{
    "image": "ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "ttl_secs": 60
  }'
# Expected: 200 + a lease_id of the form "lease-<uuid>" (UUID minting, not lease-<n>)
```

> If CoreLink's `runners_entitlement` table is empty immediately after the flip
> (before the owner seeds the dogfood tenant row), this arm requires a pre-seeded
> row to confirm. Coordinate with the owner on timing — see §5.

### Arm B — no entitlement → over-cap reject

A valid PAT whose tenant has no `runners_entitlement` row. CoreLink returns
`valid: true` but `max_concurrency` absent → the plan source returns `Ok(None)`
→ the fabric rejects with an over-cap error (not a 5xx — a clean business
reject at the admission layer).

```sh
# Use a PAT for a tenant with no runners_entitlement row (cache-only tenant):
curl -s -X POST "$H/v1/leases" \
  -H "Authorization: Bearer $CACHE_ONLY_PAT" \
  -H "Content-Type: application/json" \
  -d '{
    "image": "ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "ttl_secs": 60
  }'
# Expected: 4xx over-cap reject (NOT a 5xx)
```

If you do not have a separate cache-only PAT, confirm with the CoreLink TL
which tenant has no `runners_entitlement` row and mint a test PAT for it. With
an empty table this arm is trivially satisfied by any valid PAT.

### Arm C — introspect unreachable → 503 (fail-closed)

If the CoreLink introspect endpoint is unreachable or returns a non-200, the
fabric must fail closed: it returns 503 to the caller rather than admitting on
a best-guess basis. The timeout is `FABRIC_INTROSPECT_TIMEOUT_MS` (default
2 000 ms); the server never silently downgrades to static admission.

To verify against a staging environment: temporarily set
`CORELINK_INTROSPECT_URL` to a non-routable address (e.g.
`http://127.0.0.1:19999`) and trigger a NEW BUILD. Any request with a valid PAT
must return `503` within the timeout.

```sh
# Expected: 503 Service Unavailable
curl -s -o /dev/null -w "%{http_code}" -X POST "$H/v1/leases" \
  -H "Authorization: Bearer $TENANT_PAT" \
  -H "Content-Type: application/json" \
  -d '{"image":"ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","ttl_secs":60}'
# → 503
```

On the production flip this arm is verified by confirming the server refuses
to admit (rather than silently admitting) on a transient CoreLink outage. Ask
CoreLink TL to briefly drop the endpoint, or accept the coding guarantee (the
`Err(Unreachable)` arm in the acquire hot-path) and skip the live trigger.

### Using `corelink smoke` for baseline verification

The `corelink` CLI (`docs/cli.md`) automates the fail-closed baseline:

```sh
CORELINK_PAT=$TENANT_PAT corelink smoke --url "$H"
```

This confirms health, attestation key, unpinned-image → 400, and bad-PAT →
401. For the entitlement-scoped arms (A and B) use the raw curl commands above
(the smoke tool does not exercise per-tenant entitlement logic).

---

## 5. The non-destructive property — empty CoreLink table is safe

The flip is **non-destructive by construction**: with CoreLink's
`runners_entitlement` table empty, no tenant has a cap → every acquire attempt
returns an over-cap reject (Arm B above). Nothing acquires until the owner
seeds the first `runners_entitlement` row for the dogfood tenant.

This means:
- The three-arm validation (§4) passes on an empty table — Arm B demonstrates
  the correct reject behavior for any PAT with no entitlement row.
- No existing user is disrupted; the dogfood operator (the only current tenant)
  simply cannot acquire until the entitlement row is seeded.
- The decision of **which tenant gets the first row** (dogfood tenant, e.g.
  `humangr` as `pro`/"Build Stack") is owner/product, not either TL's call.
  Flag to owner after the flip validation passes.

---

## 6. Rollback

If any of the three admission arms fails, or if there is unexpected service
behavior, roll back immediately.

### Step 6a — set the rollback env

In **Northflank UI → your service → Environment**, set:

```
FABRIC_AUTH_BACKEND        = static
```

Remove or leave `CORELINK_INTROSPECT_URL` (the static backend ignores it).
Confirm `FABRIC_PAT` and `FABRIC_TENANT` are still set — they are required
in static mode; the server refuses to boot if either is absent.

### Step 6b — trigger a NEW BUILD (not a Restart)

**Northflank UI → Builds → New build → select `main` → Run.**

Same rule as the forward flip: only a NEW BUILD picks up the env change. A
Restart will leave the `corelink` backend running.

### Step 6c — verify rollback

After the NEW BUILD:

- [ ] Boot log: `auth backend: AuthBackend::Static`.
- [ ] `curl $H/v1/health` → `ok`.
- [ ] Acquire with the bootstrap PAT (`FABRIC_PAT`) → 200 (static backend
      is back, dogfood token works again).

---

## Env var reference (corelink mode)

| Variable | Required in `corelink` mode | Notes |
|---|---|---|
| `FABRIC_AUTH_BACKEND` | yes | Must be `corelink` for this flip |
| `CORELINK_INTROSPECT_URL` | yes | CoreLink introspect endpoint URL; required + non-empty |
| `FABRIC_INTROSPECT_AUTH_KEY` | yes | Internal auth secret for the introspect call; required + non-empty |
| `FABRIC_INTROSPECT_TIMEOUT_MS` | no | Default `2000` ms. Increase if CoreLink latency warrants. |
| `FABRIC_PAT` | no (not read) | Leave set for rollback; ignored in `corelink` mode |
| `FABRIC_TENANT` | no (not read) | Leave set for rollback; ignored in `corelink` mode |
| `FABRIC_TENANT_MAX_CONCURRENCY` | yes | Still required; must parse as u32 ≥ 1 (hard boot error if absent); in `corelink` mode the per-acquire cap comes from introspect, not this field |

> Full env-var reference: `docs/deploy/fabric-server.md`.
> Build-vs-restart gotcha: `docs/deploy/northflank-postgres-runbook.md §1`.
