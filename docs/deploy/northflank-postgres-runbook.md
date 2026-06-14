# RUNBOOK — Northflank + Postgres + multi-instance fabric deploy

> Operational playbook for running `corelink-fabricd` on Northflank with the
> **persistent Postgres ledger** and **≥2 instances**. The env-var reference is
> [`fabric-server.md`](./fabric-server.md); this is the *how you actually ship and
> operate it* doc. Written after the 2026-06-14 live multi-instance bring-up.

## 0. Topology (what runs where)

```
Northflank project
├─ Combined service  corelink-runners      ← the fabric (this repo's Dockerfile)
│   • instances: N   (cap-safe at any N — see §3)
│   • port 8080      (FABRIC_BIND_ADDR=0.0.0.0:8080)
└─ PostgreSQL addon  corelink-ledger        ← the authoritative §1 state machine
    • one DB, shared by every fabric instance
    • private-network connection string (POSTGRES_URI)
```

The fabric is **stateless except for the ledger**: every instance holds the same
config and reaches the same Postgres. State that must survive a restart or be
consistent across instances (leases, slot accounting) lives in Postgres, never in
process memory. (The one exception — the in-memory `CaptureHook` registry — is a
documented M1 limitation; see §5.)

## 1. ⚠️ The build-vs-restart distinction (the #1 operational gotcha)

Northflank has three actions that are easy to confuse. Only ONE recompiles your code:

| Action | What it does | Picks up a new commit? |
|---|---|---|
| **NEW BUILD / Deploy** | Compiles the current source into a fresh image, rolls it out | ✅ **YES — this is how you ship code** |
| **Restart** | Reboots the running containers on the **existing** image | ❌ no — same binary |
| **Terminate → Restart** | Stops then starts — still the **existing** image | ❌ no — same binary |

**If you pushed a fix and the behavior didn't change, you almost certainly hit
Restart instead of NEW BUILD.** A restart cannot pick up a code change — it reuses
the image that was last *built*. To deploy a commit: trigger a **NEW BUILD** (or a
git-push-triggered build if the service is wired to the repo). Verify with the boot
log and, for a code-identifying change, an observable behavior (e.g. the lease-id
format below).

> Real incident (2026-06-14): the lease-id collision fix (#39, UUID minting) was
> "deployed" three times via Restart with no effect — the running image still had
> the old `AtomicU64` minter. A NEW BUILD shipped it; lease ids flipped from
> `lease-<n>` to `lease-<uuid>` immediately, confirming the new binary was live.

## 2. Selecting the persistent ledger

The fabric defaults to the **in-memory** ledger (leases reset on restart,
single-instance only). For production restart-survival **and** multi-instance
cap-safety, set:

```
FABRIC_LEDGER_BACKEND=pg
DATABASE_URL=<the corelink-ledger POSTGRES_URI>
FABRIC_LEDGER_POOL_SIZE=8          # optional; default 8, per instance
```

Fail-closed contract (from `server.rs`): selecting `pg` with an absent / empty /
unreachable `DATABASE_URL` is a **hard boot error** — the server NEVER silently
falls back to memory (that would re-introduce split-brain / restart-loss
invisibly). On a clean boot you'll see:

```
ledger backend: Postgres (persistent, multi-instance cap-safe; pool=8)
```

The schema is **self-applying and idempotent** — `PgLedger::connect` runs the DDL
(`CREATE TYPE … EXCEPTION WHEN duplicate_object`, `CREATE TABLE IF NOT EXISTS`,
partial indexes) on every boot, so there is no separate migration step. Pointing a
fresh fabric at an empty database just works; pointing it at an already-populated
one is a no-op.

## 3. Why multi-instance is cap-safe (the proof, and the mechanism)

The danger with N control planes is **split-brain admission**: each instance
counting its own in-memory copy of "active leases" and both admitting at the cap →
2N slots sold for an N-slot entitlement. `PgLedger` closes this. Three load-bearing
mechanisms, all in the persistent layer:

1. **Admission is atomic across instances.** `PgLedger::try_admit` wraps
   count-active + insert in one transaction guarded by
   `pg_advisory_xact_lock(hashtext(tenant))`. Two instances admitting for the same
   tenant serialize on that lock — the second sees the first's row and rejects at
   cap. The count and the reservation can never interleave.
2. **Terminal transitions are CAS-deduped.** `transition` is a single conditional
   `UPDATE … WHERE state = '<expected>'`. When two instances' reapers both try to
   expire the same lease, Postgres row-locks serialize them: one `UPDATE` matches 1
   row (`Ok`), the other matches 0 (`Err`). Only the winner GCs side-tables, emits
   the slot event, and flushes the envelope — so no double-free, no double-count.
3. **Teardown is idempotent.** Both reapers may call Northflank `delete_job` for the
   same box; `northflank.rs` treats a `404` as already-gone (`is_success() || 404`),
   so the redundant teardown is a harmless no-op, not an error that strands the lease.

Graceful shutdown is wired (`axum::serve(...).with_graceful_shutdown(shutdown_signal())`
on SIGTERM/ctrl-c, then `reaper_handle.abort()`), so a rolling NEW BUILD drains
in-flight requests per instance instead of cutting them.

### Proven live (2026-06-14, instances=2)

25 concurrent acquires fired at the live 2-instance deployment, cap=20:
`admitted=20 | over-cap-rejected=5`. With per-instance in-memory caps this would
have admitted ~40 (20 per instance). The advisory lock held the line at the true
cap across two real containers. Persistence was confirmed in the same session: a
lease acquired before a restart was still present after — the ledger is the DB, not
the process. This RUNBOOK's §3 is the regression-test target — see the
`mod pg_runs` multi-instance tests in `ledger_conformance.rs` (gated on
`TEST_DATABASE_URL`).

## 4. Scaling instances

Scale up/down freely — **cap-safety does not depend on the instance count** (§3,
the load-bearing guarantee). Each instance opens its own `FABRIC_LEDGER_POOL_SIZE`
connections, so total DB connections ≈ `instances × pool_size`; keep that under the
Postgres addon's `max_connections`. Every instance runs the always-on deadline
reaper (and the opt-in crash sweep, if `FABRIC_CRASH_PROBE_INTERVAL_SECS` is set);
where two instances do race the same overdue lease, the CAS-dedup in §3.2 makes the
redundant reaping harmless, so no leader election is needed.

> ⚠️ **Caveat (deadline reaper is currently instance-local — see §5).** A lease's
> expiry deadline lives in the **acquiring instance's memory**, not in the DB, so the
> deadline reaper only reaps leases that instance itself acquired. It is **not** a
> cross-instance backstop today: if an instance dies/restarts holding `Held` leases,
> no surviving instance can date-and-reap them, and those cap slots leak until the
> provider's `activeDeadlineSeconds` kills the box (compute cost stays bounded; the
> ledger row stays `Held`). The opt-in **crash sweep IS cross-instance** (it iterates
> the DB and probes boxes), and the durable-deadline fix (§5) closes the gap.

## 5. Known limitations — per-instance side-table locality (multi-instance)

> Both limitations below share ONE root cause — reap-critical per-lease state
> (`deadlines`, the `CaptureHook` registry, the `slot_meter`) lives **in-memory per
> instance**, not in the durable ledger. The **durable-reap-state work-package**
> (move deadline + hook into the `leases` table so any instance can reap/flush)
> closes both. Cap-safety is unaffected — the cap is DB-global (§3.1).

### 5a. Deadline-reaper locality (cap-slot leak on instance death) — audit D3-P1

A lease's expiry deadline is recorded in the **acquiring instance's** in-memory
`deadlines` map; the `leases` table has no deadline column. So another instance's
`reap_once` treats that lease as never-overdue and skips it. As long as the
acquiring instance is alive it reaps its own leases fine, but if it **dies or
restarts** (every NEW BUILD restarts instances) its in-flight `Held` leases become
unreapable by the deadline path and **leak their cap slots** until the provider's
hard `activeDeadlineSeconds` deadline. Compute cost is bounded; the ledger row is
not freed. Fix = persist `deadline_ms` on the `leases` row (durable-reap-state WP);
the opt-in crash sweep already covers it cross-instance for the box itself.

### 5b. §13.5 partial-envelope hook-locality

The `CaptureHook` registry is **in-memory, per instance** — a lease's hook lives on
whichever instance served its exec. The §13.5 partial-envelope flush
(`flush_partial_envelope`) only fires on the reaper instance that *wins* the
terminal-transition CAS (§3.2). Under multi-instance, the winning instance is not
guaranteed to be the one holding the hook → on a mismatch, the partial **forensic**
envelope for an abnormally-closed (expired/crashed) lease is silently dropped.

- **Billing impact: none.** hugit prices flat; the envelope is forensic provenance,
  not a billing input. A dropped partial envelope costs a forensic record, never money.
- **Severity: M1-acceptable.** §13.5 is explicitly best-effort at M1, and the M1
  flush is a forensic *log line* — the real transport to hugit is the P2 work-package.
- **Real fix (P2):** either persist hooks alongside the lease, or route the
  abnormal-close flush to the lease's owning instance. Tracked as a P2 item; do not
  rely on partial-envelope forensics being complete while running N>1.

A normal close (client calls `POST /v1/leases/{id}/close`) is unaffected — it is
served by, and finalizes on, the instance the client is talking to, and the
exactly-once latch on the shared hook state prevents any double-fire.

## 6. Connection security (TLS)

The fabric↔Postgres connection runs over Northflank's **private network**;
the addon connection is **non-TLS** today (`PgLedger` builds its pool with `NoTls`).
This is acceptable on a private network but means the link is unencrypted. Opt-in
TLS (verify-full against bundled public CA roots, `FABRIC_PG_TLS=require`) is a
landing work-package — update the env table in `fabric-server.md` when it merges.
For a TLS-required managed Postgres (Neon/Supabase/RDS), that flag is the path.

## 7. Operational chores

- **Residual held leases.** Test/abandoned leases occupy the tenant cap until they
  expire (the deadline reaper reclaims them at `FABRIC_REAP_INTERVAL_SECS`, default
  30s, once past their lease TTL). To clear immediately for a clean state:
  `TRUNCATE leases;` on the addon (destroys all lease state — only on a deployment
  you intend to reset, never one serving real tenants).
- **Health check.** `curl https://<service-host>/v1/health → "ok"`.
- **Boot verification after a NEW BUILD.** Confirm the ledger line
  (`ledger backend: Postgres …`) and, for a code change, an observable behavior — do
  not trust that a "Deploy" button press alone shipped the commit (§1).

## 8. Security — rotate on exposure

The fabric's secrets are: `FABRIC_SIGNING_KEY` (signs every `AttestationChain` —
the highest-value secret), `FABRIC_PAT` (bootstrap tenant token), the Postgres
`POSTGRES_URI` / password, and the Northflank API/org token. Any secret that has
appeared in a chat, a log, a `/tmp` file, or a screenshot is **compromised — rotate
it**. The signing key in particular: a leak lets an attacker forge attestations, so
rotation there also invalidates anything signed under the old key.
