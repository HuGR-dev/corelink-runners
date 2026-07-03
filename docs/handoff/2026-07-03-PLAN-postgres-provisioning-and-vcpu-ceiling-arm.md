# PLAN — Provision Postgres for the CF `corelink-fabricd` and arm the vCPU-h compute ceiling

**Date:** 2026-07-03
**Status:** PLAN ONLY — owner-executable. No production change, no `wrangler deploy`,
no DB provisioning was performed. Read-only code investigation + this doc.
**Scope:** give `corelink-fabricd` (the singleton CF Container control plane) a durable,
cross-instance-safe Postgres ledger, and turn on the vCPU-h compute-accounting ceiling
(`FABRIC_RUNNER_VCPU=4`).

---

## 0. Why (the fail-closed coupling, already decided)

The prod fabricd today runs the **in-memory** ledger (`deploy/cloudflare-fabricd/wrangler.jsonc`:
`standard-1`, `max_instances 1`; no `FABRIC_LEDGER_BACKEND`/`DATABASE_URL` in
`src/index.ts` `envVars`). Two facts force Postgres before the ceiling can be armed:

- Arming the ceiling needs **`FABRIC_RUNNER_VCPU>0` AND `FABRIC_LEDGER_BACKEND=pg`**.
  The boot **fails closed** if the ceiling is armed on a non-Postgres backend —
  `crates/corelink-fabric-server/src/server.rs:757-765` (`runner_vcpu.is_some() &&
  ledger_backend != Postgres` → `bail!`). Rationale in the code: the admit is a
  Σ-read-then-reserve and only `PgLedger`'s `pg_advisory_xact_lock(tenant)` makes that
  pair atomic *across instances*; an in-memory ledger would let a tenant fanning across
  instances reach ~2× the ceiling.
- Selecting `pg` makes `DATABASE_URL` a **hard requirement** — there is deliberately no
  silent fallback to memory (`server.rs:575-589`).

So: no Postgres ⇒ no ceiling. This plan provisions the DB first, then flips both env
vars in one deploy.

---

## 1. How fabricd connects to Postgres (code-cited)

### Connection env var — name + format

- **`DATABASE_URL`** — the one and only connection string the server reads.
  Read + trimmed + required-non-empty iff `pg` is selected: `server.rs:575-589`
  (`get("DATABASE_URL")`). Passed verbatim into the pool builder as
  `deadpool_postgres::Config.url` — `crates/corelink-fabric/src/pg_ledger.rs:332-333`
  (`cfg.url = Some(database_url.to_string())`). Format is a standard libpq/tokio-postgres
  URL: `postgres://USER:PASSWORD@HOST:PORT/DBNAME` (query params like `?sslmode=…` are
  **not** how TLS is selected here — see below).

### Backend selector + pool size

- **`FABRIC_LEDGER_BACKEND`** — `memory` (default) | `pg` | `postgres`; any other value is
  a hard boot error: `server.rs:558-571`.
- **`FABRIC_LEDGER_POOL_SIZE`** — optional `usize`, default `8`; `0`/unparseable → error:
  `server.rs:592-606`. The pool has a hard **floor of 4** and reserves 2 connections for
  the releasing terminal transition (`pg_ledger.rs:360-371`), so a small value is safe but
  the deploy guidance is `pool_size ≥ 2 × peak_concurrent_tenant_ops` (`pg_ledger.rs:349-350`).

### Transport security (TLS) — REQUIRED over the public internet

- **`FABRIC_PG_TLS`** — `disable` (default → plaintext `NoTls`) | `require`
  (verify-full rustls). Resolver: `pg_ledger.rs:93-108`; wired at
  `server.rs:608-612`. TLS branch in `connect`: `pg_ledger.rs:376-384`.
- `require` = **verify-full**: full server-cert chain validation + SNI hostname check,
  **no bypass anywhere** (`pg_ledger.rs:110-137`, doc-comment `:38-54`). Trust anchors are
  the **bundled `webpki_roots` Mozilla public-CA set**, NOT the OS store — so a Postgres
  behind a **private/internal CA is out of scope for M1** (documented non-goal). Any managed
  Postgres over the public internet (Neon/Supabase/RDS) chains to a public CA and passes.
- Since fabricd → Postgres crosses the **public internet** (see §2), **`FABRIC_PG_TLS=require`
  is mandatory** for this deployment. (Plaintext `disable` is only for the original
  private-network layout.)

### Schema bootstrap / migration — AUTOMATIC, no separate tool

There is **no migration step to run**. `PgLedger::connect` applies the full schema
**idempotently on every boot** via one batched, transactional DDL script —
`pg_ledger.rs:386-395` (`batch_execute("BEGIN; {DDL} COMMIT;")`), fail-closed (DDL error →
`Err`, no half-open ledger). The DDL (`pg_ledger.rs:139-185`) is all
`CREATE TABLE/TYPE/INDEX IF NOT EXISTS` + `ALTER TABLE … ADD COLUMN IF NOT EXISTS`:

- `leases` table + the ceiling columns (`box_vcpu_count`, `accrual_period_key`,
  `reserved_vcpu_ms`, `accrued_at_ms`, `billing_acquired_at_ms`) — the compute-accounting
  state lives here.
- `compute_accrual (tenant, period_key, accrued_vcpu_ms)` — the durable per-(tenant,period)
  accrued vCPU·ms, the terminal half of the ceiling invariant.
- `lease_state` enum.

**Owner obligation:** the DB **role in `DATABASE_URL` must have `CREATE` privilege** on its
schema (needs `CREATE TYPE` + `CREATE TABLE`). On Neon/Supabase the default owner role has
this. A read-only or restricted role fails the first-boot DDL (fail-closed → container won't
serve). `TENANT_PLANS_DDL` (`pg_ledger.rs:206+`) is a frozen anchor NOT applied by `connect`
yet — irrelevant to this plan.

---

## 2. Reachability — where can a Container DO reach Postgres?

`corelink-fabricd`'s Rust process runs **inside a Cloudflare Container** (a Linux process
under the `FabricdContainer` Durable Object), making a **raw `tokio-postgres` TCP+TLS**
connection. Options:

| Option | Reachable from the container? | Verdict |
|---|---|---|
| **Managed external Postgres over public internet + TLS** (Neon / Supabase / RDS) | **Yes** — outbound TCP:5432 from the container to a public host; `FABRIC_PG_TLS=require` gives verify-full against the public-CA cert the provider serves. | **RECOMMENDED.** |
| **Cloudflare Hyperdrive** | **No.** Hyperdrive is exposed as a **Worker binding** (a connection string resolvable only inside the Worker isolate's runtime). The fabricd DB client runs in the **container process**, not the Worker isolate, so it cannot consume a Hyperdrive binding. Not usable without re-architecting the DB path through the proxy Worker. | Rejected for this deployment. |
| **Postgres co-located in the same container / a sidecar** | Ephemeral + single-instance; defeats the entire cross-instance cap-safety purpose. No durability across container restart/redeploy. | Rejected. |
| **Self-hosted Postgres on a small VM (Fly/EC2)** over public internet + TLS | Works (same shape as managed), but you own patching, backups, TLS cert lifecycle. | Fallback only. |

### Recommendation: **Neon** (serverless Postgres, public-CA TLS)

One line: **Neon serverless Postgres over the public internet with `FABRIC_PG_TLS=require`
(verify-full) — a single `DATABASE_URL` secret, scale-to-zero fits the low-traffic
singleton, and its cert chains to a public CA so the hermetic `webpki_roots` trust anchor
verifies without any OS trust-store dependency.**

Notes:
- Use Neon's **pooled** connection string (PgBouncer endpoint, host `…-pooler.…`) so the
  singleton's small pool (`FABRIC_LEDGER_POOL_SIZE`, floor 4) plus scale-to-zero wakeups
  don't exhaust direct connections.
- Supabase is an equivalent second choice (also public-CA TLS, also a single URL); pick
  Neon for the cleaner scale-to-zero + pooler story. RDS works but adds AWS surface + a VPC
  egress decision — heavier than needed for a `standard-1` singleton.
- `max_instances 1` + a **shared** external Postgres is exactly what makes cross-instance
  accounting correct: even if instances scale >1 later, the advisory-lock admit
  (`pg_ledger.rs:609`, `SELECT pg_advisory_xact_lock(hashtext($1))`) serializes the
  Σ-read+reserve on the ONE shared DB.

---

## 3. Owner-executable steps (exact)

> Pre-req: Docker daemon up (the deploy builds the image locally) and `wrangler` authed —
> per `deploy/cloudflare-fabricd/README.md`. All commands from `deploy/cloudflare-fabricd/`.

### Step 1 — Provision the database (owner, Neon)
1. Create a Neon project (region close to the fabricd colo; billing region is `iad`, so
   US-East is a good match).
2. Create a database + a role **with `CREATE` privilege** (default owner role is fine).
3. Copy the **pooled** connection string:
   `postgres://USER:PASSWORD@ep-xxxx-pooler.us-east-2.aws.neon.tech/DBNAME`
   (no `?sslmode=` needed — TLS is selected by `FABRIC_PG_TLS`, not the URL).

### Step 2 — Store the connection string as a Worker secret
```sh
cd deploy/cloudflare-fabricd
# The container reads env DATABASE_URL; the proxy Worker forwards it via envVars (Step 3).
printf '%s' 'postgres://USER:PASSWORD@ep-xxxx-pooler.us-east-2.aws.neon.tech/DBNAME' \
  | npx wrangler secret put DATABASE_URL
```
Secret name is **`DATABASE_URL`** (matches what the container env expects after Step 3).

### Step 3 — Wire the env into the container (`src/index.ts` + `Env`)
The container gets its env from `FabricdContainer`'s `this.envVars` block
(`src/index.ts:45-73`). Secrets/vars flow **Worker `Env` → `envVars` → container process**.
Add the four keys (this is a code edit to `src/index.ts`, reviewed like any change):

- Extend `interface Env` with `DATABASE_URL: string;` (secret binding).
- In `this.envVars`, add:
  ```
  FABRIC_LEDGER_BACKEND: "pg",
  DATABASE_URL:          env.DATABASE_URL,
  FABRIC_PG_TLS:         "require",
  FABRIC_RUNNER_VCPU:    "4",
  ```
  Optionally `FABRIC_LEDGER_POOL_SIZE: "8"` (default is already 8).

`wrangler.jsonc` needs **no** change for these (they are container envVars injected by the
DO, not Worker `vars`); `max_instances 1` and `standard-1` stay as-is. `standard-4` is the
runner box; `FABRIC_RUNNER_VCPU=4` describes the runner-box vCPU multiplier, not the
fabricd container's size — leave the container `standard-1`.

### Step 4 — Do NOT set `FABRIC_TENANT_MAX_VCPU_H` (fail-closed on this path)
Prod fabricd uses `FABRIC_AUTH_BACKEND: "corelink"` (`src/index.ts:46`). On the CoreLink
auth path, setting `FABRIC_TENANT_MAX_VCPU_H` is a **hard boot error**
(`server.rs:818-825`) — the per-tenant ceiling is sourced from the CoreLink introspect
`max_vcpu_h` entitlement, not this env. Leave it unset.

### Step 5 — Deploy (forces a container restart — envVars are read at start)
```sh
npm install
npm run deploy
# OPS GOTCHA (README): a config-only redeploy does NOT restart the singleton container —
# envVars are read at container START. Force a fresh start:
npx wrangler containers delete <app-id> && npx wrangler deploy
```

### Step 6 — Verify (schema auto-applies on first boot)
```sh
HOST="https://corelink-fabricd.<account-subdomain>.workers.dev"
curl -s $HOST/v1/health            # → ok  (means connect() + DDL succeeded, else fail-closed)
# In Neon SQL: \dt  → leases, compute_accrual present; \dT → lease_state enum present.
# Acquire a runner lease with a real tenant PAT → 200 Held; after terminal, a
# compute_accrual row exists for (tenant, period_key).
```
A boot failure (bad `DATABASE_URL`, no-CREATE role, TLS mismatch) is **fail-closed**: the
container won't serve `/v1/health`. Check `wrangler tail` for the `PgLedger:` error.

---

## 4. Readiness checklist + what ONLY the owner can provide

### Owner-only inputs (no one else can supply these)
- [ ] **A Postgres account + database** (Neon recommended) — owner's cloud account/billing.
- [ ] **The `DATABASE_URL` connection string**, with a role that has **`CREATE`** privilege
      (needed for first-boot DDL). Pooled endpoint preferred.
- [ ] Confirm the provider serves a **public-CA** TLS cert (Neon/Supabase do) — a
      private-CA Postgres is an M1 non-goal and will fail verify-full.

### Readiness checklist (in order)
- [ ] DB provisioned; pooled `DATABASE_URL` in hand; role has `CREATE`.
- [ ] `wrangler secret put DATABASE_URL` done on `corelink-fabricd`.
- [ ] `src/index.ts` edited: `Env.DATABASE_URL` + the 4 envVars
      (`FABRIC_LEDGER_BACKEND=pg`, `DATABASE_URL`, `FABRIC_PG_TLS=require`,
      `FABRIC_RUNNER_VCPU=4`); reviewed + merged.
- [ ] `FABRIC_TENANT_MAX_VCPU_H` is **NOT** set (would fail-closed on the corelink path).
- [ ] Docker up + `wrangler` authed.
- [ ] Deploy with a forced container restart (`containers delete` + `deploy`).
- [ ] `/v1/health → ok`; `leases` + `compute_accrual` tables exist; a lease acquire
      writes a `compute_accrual` row at terminal.

### Cross-repo dependency to flag (NOT a blocker for provisioning, but for real ENFORCEMENT)
- On the **corelink** auth path, arming `FABRIC_RUNNER_VCPU=4` turns compute **accounting**
  ON (durable accrual + Σ-reserved), but the **enforced per-tenant ceiling** is the
  introspect **`max_vcpu_h`** entitlement. Per the code note (`server.rs:775-777`), CoreLink
  keeps the documented **default `0` (= disabled/unlimited per tenant) until the
  entitlement vector lands**. So until CoreLink's `/internal/v1/auth/introspect` returns a
  non-zero `max_vcpu_h`, the ceiling **accrues but does not reject**. Wiring that entitlement
  is a corelink-server (Server-TL) item, tracked separately — flag it, don't block this DB
  provisioning on it.

---

## Appendix — code citations

- `FABRIC_LEDGER_BACKEND` parse: `crates/corelink-fabric-server/src/server.rs:558-571`
- `DATABASE_URL` read (required iff pg): `server.rs:575-589`
- `FABRIC_LEDGER_POOL_SIZE`: `server.rs:592-606`
- `FABRIC_PG_TLS` resolve/wire: `pg_ledger.rs:93-108`, `server.rs:608-612`
- ceiling-needs-pg fail-closed guard: `server.rs:757-765`
- `FABRIC_RUNNER_VCPU` parse: `server.rs:687-704`
- `FABRIC_TENANT_MAX_VCPU_H` dead-knob guard (corelink path): `server.rs:818-825`
- `PgLedger::connect` (pool + TLS branch): `pg_ledger.rs:327-384`
- schema auto-apply on boot (idempotent DDL, fail-closed): `pg_ledger.rs:386-395`
- DDL (`leases` + ceiling columns + `compute_accrual` + `lease_state`): `pg_ledger.rs:139-185`
- advisory-lock admit (cross-instance atomicity): `pg_ledger.rs:609`
- container envVars injection: `deploy/cloudflare-fabricd/src/index.ts:45-73`
- `corelink` auth backend in prod: `src/index.ts:46`
- current wrangler (standard-1, max_instances 1, in-memory): `deploy/cloudflare-fabricd/wrangler.jsonc`
