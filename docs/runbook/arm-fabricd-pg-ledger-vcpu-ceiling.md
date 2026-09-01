# RUNBOOK — arm the durable pg ledger + vCPU ceiling on CF-fabricd (R1)

> Owner directive 2026-07-02 (`FABRIC_RUNNER_VCPU=4`). Deploy layer wired in #286.
> The durable path now has two gates: a non-empty `DATABASE_URL` **and** the exact
> Worker var `FABRIC_PG_DISABLED="0"`. Missing, blank, whitespace-padded, or any
> other value keeps the ledger in memory. `DATABASE_URL` alone never arms PG, and
> exact `"0"` is necessary but not sufficient operational authorization. This
> runbook is the controlled arm procedure once every preflight gate passes.

This is a production change, not a diagnostic technique. Do not deploy, restart,
delete/recreate a container, or alter a secret/flag merely to identify a fault.
The accountable owner must approve the fixed change, monitor, success threshold,
timeout, and rollback first. Canary flags and the canary deployment are outside
this runbook and must remain untouched.

## What this does
Moves the CF-fabricd DO singleton from the **in-memory** ledger to the durable **PgLedger**
(survives DO restart — no more lost lease state on the ~1-2 min recreate blip) and arms
`FABRIC_RUNNER_VCPU=4` so vCPU-hour accounting is durable. On the `corelink` auth path the ceiling
VALUE flows from the introspect `max_vcpu_h` entitlement per-acquire (0/unlimited until that vector
lands); arming now gives durable accounting and is ready for enforcement the moment the entitlement
vector ships. **Rust unchanged** — PgLedger is built + proven live (Northflank era).

## Prerequisite — a reachable Postgres
The container dials OUT to the DB over the public internet (`enableInternet = true`), so any
network-reachable Postgres works. Options:
- **Reuse the existing Northflank `corelink-ledger` addon** if it is still up (proven; schema already
  migrated). Its external connection string is the `DATABASE_URL`.
- **Stand up a fresh managed Postgres** (Neon / Supabase / Northflank). Free tier is enough at
  dogfood scale (singleton, low QPS). Use the **pooled** connection string if the provider offers one.
- TLS: a public-internet PG should use `require` (this is the default the Worker injects when
  `FABRIC_PG_TLS` is unset).

**Schema:** the PgLedger self-migrates on connect (the `leases` / accounting tables); a fresh DB needs
no manual DDL. (If reusing the Northflank addon, it is already migrated.)

Before touching the arm, prove from the provider or a trusted database client that
the endpoint accepts the intended role, TLS mode, and a session/direct connection,
and that the role can create the schema objects used by the self-migration. Do not
use transaction-mode pooling. Keep `FABRIC_PG_DISABLED` at the committed containment
value `"1"` during this validation: with that value, even a bound `DATABASE_URL`
cannot reach the container.

## Required go/no-go record

Before changing exact `"1"`, record all of the following in the incident/change
log. Any missing item is a NO-GO:

1. accountable owner and approved change window;
2. current deployed version/config and exact `FABRIC_PG_DISABLED="1"` containment;
3. fixed database endpoint, TLS mode, role/DDL capability, and provider headroom;
4. zero or explicitly drained/accepted in-flight lease blast radius;
5. live monitor queries for startup failures, provider resource use, PG connections,
   exporter attempts/failures, and `ledger_cross_instance_safe`;
6. success thresholds and a bounded observation interval; and
7. rollback prepared to restore exact `"1"` before any database-secret change.

D12 remains unresolved: existing evidence does not distinguish PgLedger startup
work from the jointly gated exporter as the source of resource burn. The arm must
therefore observe both paths and must not claim either attribution as fact.

## Arm procedure (once DATABASE_URL is in hand)
```bash
cd deploy/cloudflare-fabricd

# 1. Confirm containment is still explicit while preparing the database.
#    This must print the committed "1" line; stop if the result is ambiguous.
rg '"FABRIC_PG_DISABLED": "1"' wrangler.jsonc

# 2. Set the secret while the exact-0 arm is still CLOSED. The value is never echoed;
#    rotate/revoke it at the PG side if leaked.
npx wrangler secret put DATABASE_URL --name corelink-fabricd
#   (optional) override TLS if the PG can't do TLS:  npx wrangler secret put FABRIC_PG_TLS  -> "disable"

# 3. STOP unless the complete go/no-go record above is approved. Only then edit
#    wrangler.jsonc and change FABRIC_PG_DISABLED from "1" to the byte-exact
#    string "0". Do not delete the line: absence remains disabled.
rg '"FABRIC_PG_DISABLED": "0"' wrangler.jsonc

# 4. Controlled change only (never diagnosis): deploy the reviewed Worker config.
npx wrangler deploy --containers-rollout=none

# 5. Controlled change only: after checking the exact application id and blast
#    radius, recreate the singleton so it re-reads the exact-0 arm + secret.
#    Never use this delete/restart sequence to discover whether PG is causal.
#    (env-only changes are NOT applied by deploy alone — the DO reads envVars at START.)
npx wrangler containers list                      # find the fabricd app id
npx wrangler containers delete <fabricd-app-id>   # ~1-2 min fabricd outage (in-mem state is lost anyway)
npx wrangler deploy --containers-rollout=none      # DO recreates the container with pg env
```

## Verify (post-arm)
1. Re-open `wrangler.jsonc` and verify the arm remains byte-for-byte
   `"FABRIC_PG_DISABLED": "0"`.
2. `curl https://<fabricd>/v1/health` → 200 (container back up).
3. With the observability key, `GET /internal/v1/status` must return 200 and
   `ledger_cross_instance_safe: true`. Health 200 alone is insufficient: the
   in-memory fallback is also healthy.
4. Boot did NOT fail-closed: check `wrangler tail corelink-fabricd` for a clean start (no
   `FABRIC_RUNNER_VCPU ... requires ... postgres` bail, no `DATABASE_URL` connect error).
5. **Durability proof:** acquire a controlled test lease → recreate the container → the lease survives (was lost on
   in-memory). This is the concrete win.
6. Accounting armed: the compute-meter path is live (durable vCPU·ms). Enforcement stays at
   entitlement-`max_vcpu_h` (0/unlimited on corelink until the Server ships the entitlement vector).
7. During the bounded observation interval, compare provider resource use, PG
   connection/startup signals, and exporter attempt/failure signals against the
   predeclared thresholds. A 200 health response is not sufficient evidence.

If any monitor is unavailable, ambiguous, or crosses threshold, stop and execute
rollback. Do not add a restart/deploy experiment to the plan while observing.

## Rollback
Fail closed **before** changing the database secret:

1. Change the tracked `FABRIC_PG_DISABLED` value from `"0"` back to the exact
   string `"1"`, deploy, and verify the deployed config. Do not delete this line;
   keeping the explicit `"1"` makes containment auditable.
2. Recreate the singleton container so it starts without any PG-only env, then
   verify health and `ledger_cross_instance_safe: false`. This deliberately gives
   up restart-surviving leases, N>1, the vCPU ceiling, and durable billing export.
3. Only after the exact-1 containment deploy is effective may the owner delete,
   rotate, or repair `DATABASE_URL`. Removing the secret is optional defense in
   depth, not the primary rollback gate.

If PG is already preventing boot, the same order applies as an approved recovery:
restore exact `"1"` first, apply the reviewed rollback, verify the in-memory
posture, and only then manipulate the failing database credential. Repeated
delete/restart/deploy attempts are not diagnosis and are prohibited.

## Safety notes
- PG is inert unless **both** gates are present: non-empty `DATABASE_URL` and
  byte-exact `FABRIC_PG_DISABLED="0"`. The explicit containment value is `"1"`;
  unset or malformed values also fail closed and must never be used as an arm.
- Exact `"0"` only permits the jointly gated environment to reach the container;
  it does not prove database readiness, exporter safety, deployment success, or
  authorization. Those are separate preflight and post-change gates.
- Do NOT set `FABRIC_TENANT_MAX_VCPU_H` on the corelink path — it is a dead-knob there (boot guard
  `server.rs:818` fail-closes) ; the ceiling is entitlement-sourced.
- The recreate causes a brief fabricd outage; acceptable at dogfood (CI uses the autoscaler, not the
  fabricd; no live acquire consumers). Once on pg, restarts stop losing lease state.
