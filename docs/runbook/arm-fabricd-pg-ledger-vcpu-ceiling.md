# RUNBOOK — arm the durable pg ledger + vCPU ceiling on CF-fabricd (R1)

> Owner directive 2026-07-02 (`FABRIC_RUNNER_VCPU=4`). Deploy layer wired in #286
> (gated on `DATABASE_URL`). This runbook is the arm procedure once a Postgres URL exists.

## What this does
Moves the CF-fabricd DO singleton from the **in-memory** ledger to the durable **PgLedger**
(survives DO restart — no more lost lease state on the ~1-2 min recreate blip) and arms
`FABRIC_RUNNER_VCPU=4` so vCPU-hour accounting is durable. On the `corelink` auth path the ceiling
VALUE flows from the introspect `max_vcpu_h` entitlement per-acquire (0/unlimited until that vector
lands); arming now gives durable accounting and is ready for enforcement the moment the entitlement
vector ships. **Rust unchanged** — PgLedger is built + proven live (Northflank era).

## Prerequisite — a reachable Postgres (the ONE owner input)
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

## Arm procedure (once DATABASE_URL is in hand)
```bash
cd deploy/cloudflare-fabricd

# 1. Set the secret (never echoed; rotate/revoke at the PG side if leaked).
npx wrangler secret put DATABASE_URL --name corelink-fabricd
#   (optional) override TLS if the PG can't do TLS:  npx wrangler secret put FABRIC_PG_TLS  -> "disable"

# 2. Deploy the Worker code (Docker-free — image is the managed-registry ref).
npx wrangler deploy --containers-rollout=none

# 3. Recreate the singleton container so it re-reads envVars WITH the new secret.
#    (env-only changes are NOT applied by deploy alone — the DO reads envVars at START.)
npx wrangler containers list                      # find the fabricd app id
npx wrangler containers delete <fabricd-app-id>   # ~1-2 min fabricd outage (in-mem state is lost anyway)
npx wrangler deploy --containers-rollout=none      # DO recreates the container with pg env
```

## Verify (post-arm)
1. `curl https://<fabricd>/v1/health` → 200 (container back up).
2. Boot did NOT fail-closed: check `wrangler tail corelink-fabricd` for a clean start (no
   `FABRIC_RUNNER_VCPU ... requires ... postgres` bail, no `DATABASE_URL` connect error).
3. **Durability proof:** acquire a lease → recreate the container → the lease survives (was lost on
   in-memory). This is the concrete win.
4. Accounting armed: the compute-meter path is live (durable vCPU·ms). Enforcement stays at
   entitlement-`max_vcpu_h` (0/unlimited on corelink until the Server ships the entitlement vector).

## Rollback
Remove the secret (`wrangler secret delete DATABASE_URL --name corelink-fabricd`) + recreate the
container → falls straight back to in-memory, no ceiling (the gated-on-`DATABASE_URL` design). Zero
code revert needed.

## Safety notes
- The Worker change (#286) is **inert until `DATABASE_URL` is set** — deploying it changes nothing
  until the secret exists.
- Do NOT set `FABRIC_TENANT_MAX_VCPU_H` on the corelink path — it is a dead-knob there (boot guard
  `server.rs:818` fail-closes) ; the ceiling is entitlement-sourced.
- The recreate causes a brief fabricd outage; acceptable at dogfood (CI uses the autoscaler, not the
  fabricd; no live acquire consumers). Once on pg, restarts stop losing lease state.
