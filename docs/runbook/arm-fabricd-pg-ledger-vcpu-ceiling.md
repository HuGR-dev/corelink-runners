# RUNBOOK — arm the durable pg ledger + vCPU ceiling on CF-fabricd (R1)

> Owner directive 2026-07-02 (`FABRIC_RUNNER_VCPU=4`). Deploy layer wired in #286.
> The durable path now has two gates: a non-empty `DATABASE_URL` **and** the exact
> Worker var `FABRIC_PG_DISABLED="0"`. Missing, blank, whitespace-padded, or any
> other value keeps the ledger in memory. This runbook is the arm procedure once
> a Postgres URL exists.

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

# 3. Only after the fixed database configuration passed its connection/permission
#    checks, edit wrangler.jsonc and change FABRIC_PG_DISABLED from "1" to the
#    byte-exact string "0". Do not delete the line: absence remains disabled.
rg '"FABRIC_PG_DISABLED": "0"' wrangler.jsonc

# 4. Deploy the Worker config (Docker-free — image is the managed-registry ref).
npx wrangler deploy --containers-rollout=none

# 5. Recreate the singleton container so it re-reads the exact-0 arm + secret.
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

If PG is already preventing boot, the same order applies: deploy exact `"1"`
first, recreate the container, prove the in-memory service is healthy, and only
then manipulate the failing database credential.

## Safety notes
- PG is inert unless **both** gates are present: non-empty `DATABASE_URL` and
  byte-exact `FABRIC_PG_DISABLED="0"`. The explicit containment value is `"1"`;
  unset or malformed values also fail closed and must never be used as an arm.
- Do NOT set `FABRIC_TENANT_MAX_VCPU_H` on the corelink path — it is a dead-knob there (boot guard
  `server.rs:818` fail-closes) ; the ceiling is entitlement-sourced.
- The recreate causes a brief fabricd outage; acceptable at dogfood (CI uses the autoscaler, not the
  fabricd; no live acquire consumers). Once on pg, restarts stop losing lease state.
