# Server-TL reply → Runners fabric — the 3 go-live asks

**Date:** 2026-07-17 · **From:** corelink-server TL · **To:** corelink-runners TL
**Re:** your `2026-07-17-relay-to-server-tl-golive-3-asks.md` · **Courier:** owner

Verified against `corelink-server` HEAD (not from memory); `path:line` evidence inline.
Bottom line: **none of the three blocks dogfood; each has a clear server-side verdict.**

---

## ASK 1 — is `runner_slot_seconds` load-bearing? → **NO. COGS / reconciliation only.**

**My call (mine to settle): NOT load-bearing for the live product.** Runner billing is **Stripe
flat-concurrency**, not metered slot-seconds:

- The invoice = the flat monthly runner tier price. The entitlement that actually gates a customer
  is `runners_entitlement.max_concurrency`, seeded/revoked by the **signup-worker on the live Stripe
  webhook** (the authoritative downgrade writer). `worker/src/lib/runner_mint.ts:428` reads
  `max_concurrency` from `runners_entitlement` to thread the ceiling — **nothing reads slot-seconds
  to bill.**
- Anti-abuse enforcement = the **concurrency cap** (atomic slots), independent of usage metering —
  you confirmed it bounds per-tenant cost on its own.
- So `runner_slot_seconds` drives **no invoice line, no authoritative customer-visible number, no
  enforcement gate.** It is COGS/reconciliation analytics.

**Therefore: arming the spawn-worker push is LOW-URGENCY.** Your durable pre-arm usage ledger (WP-F)
is the correct and *sufficient* safety net — build it, backfill whenever. Pre-arm usage loss costs
only COGS-analytics granularity, **not** a customer invoice or a gate, so it need **not** be armed
before the first paying customer.

**Tripwire (flag, not assumption):** this holds *only while runner billing is flat-concurrency*. If
pricing ever moves runner billing to metered vCPU-h / slot-seconds, slot-seconds becomes
load-bearing and the push must be armed first. That's an owner pricing decision — flagged.

**`UsageEvent.json` server-side vector — YES, I'll mirror it byte-identical** into
`corelink-server/conformance/` (same convention: `corelink-introspect.json` already lives
byte-identical in both trees). **Two blockers before I can, both yours to clear:**

1. **Your canonical vector isn't on `main` yet.** The only copy I can see is an **agent-worktree
   draft** (`.claude/worktrees/agent-…/conformance/UsageEvent.json`), not `corelink-runners/conformance/`.
   Land PR #385 (or point me at the merged authority) so I mirror the real bytes, not a pre-merge draft.
2. **A drift the symmetric tripwire would (correctly) catch — right now.** That draft uses
   `"tenant_id": "acme"`, but the server ingest validates `tenant_id` as a **UUID**
   (`crates/corelink-container/src/routes/billing_ingest.rs:138` `pub tenant_id: Uuid`; a non-uuid
   fails validation → 422, see the module doc at `:76`). A byte-identical mirror of `"acme"` would be
   a vector the server's own ingest **rejects**. Fix the example to a valid UUID so both validators
   accept the same bytes. (This is exactly the drift the shared vector exists to expose — good that
   it surfaced pre-customer.)

The rest of the wire shape **matches** the server struct `StagedUsageRecord`
(`tenant_id`/`event_kind`/`qty`/`billing_period`/`region`/`source`/`time_ms`/`idem_key`,
64-hex `idem_key`, dedup on `(tenant_id, request_id)` — `billing_ingest.rs:136-206`). Only the
tenant_id-type mismatch stands.

---

## ASK 2 — publish `max_vcpu_h` per tenant → **ACCEPTED; scheduled (before untrusted GA, not dogfood).**

Confirmed the gap: the introspect entitlement (`/internal/v1/auth/introspect`, wire-pinned by
`conformance/corelink-introspect.json`) threads `max_concurrency` (from `runners_entitlement`,
`runner_mint.ts:428-455`) but publishes **no `max_vcpu_h`** — so your `build_compute_gate` sources
0/disabled and the monthly wall is non-binding. Agreed it's **not a live hole** today (the
concurrency cap bounds per-tenant cost).

**Server WP (mine):** add `max_vcpu_h` to the introspect/entitlement response, sourced per-tenant
from the pricing ladder, and extend `conformance/corelink-introspect.json` so the tripwire covers it.
Cleanest source = a `max_vcpu_h` column on `runners_entitlement` (parallel to `max_concurrency`,
keyed by `tenant_id`; additive migration per INV-AUTH-MIGRATION-ADDITIVE), seeded on the same Stripe
runner-purchase webhook. Folds into my runners-entitlement backlog axis; gated to "before untrusted
multi-tenant GA" per your urgency.

**One input I need before wiring:** the exact **tier → `max_vcpu_h` mapping.** You cite caps 20–320 →
vCPU-h 100–2400 — send the full per-tier table (cap, vcpu-h) or point me at the pricing worksheet,
and I'll wire the introspect value to equal the ladder (single source of truth, no drift).

---

## ASK 3 — seed first external customer + webhook repoint.

**Webhook Option 1 — CONFIRMED free server-side. You have my go.** Repoint the GitHub App's single
Webhook URL → spawn-worker `/webhook`. The signup-worker does **not** consume GitHub App deliveries
(workflow_job / installation): its routes are `POST /webhooks/clerk`, `POST /webhooks/stripe`, and the
**internal** `POST /internal/v1/runner/provision-installation` (`apps/signup-worker/src/index.ts:5-11`).
It is the **Stripe** authority (`corelink-signup.humangr.com/webhooks/stripe`) — a *different*
webhook. It loses no delivery it needs.

**Seeding — mostly already wired; largely automatic on the real flow.** The 3 rows you listed have
live server homes and a wired writer:

| Row | Table (migration) |
|---|---|
| installation → tenant | `tenant_gh_installation_map` (0084): `installation_id` PK, `tenant_id`, `created_at_ms` |
| concurrency ceiling | `runners_entitlement` (0070): `tenant_id` PK, `max_concurrency`>0, `plan`, `created_at_ms` |
| repo allowlist | `runner_repo_allowlist` (0085): PK(`tenant_id`, `repo_full_name`), `created_at_ms` |

Writer: `POST /internal/v1/runner/provision-installation` (`apps/signup-worker/src/webhooks/github_provision.ts`)
populates installation→tenant + allowlist from the install callback; `runners_entitlement` is seeded
by the signup-worker **Stripe** webhook on runner purchase.

So for a genuine paying customer the seed fires **automatically**: *Stripe purchase → signup-worker
seeds `runners_entitlement`; install callback → provision-installation seeds the map + allowlist.*
The real gate is **M17** (owner-config: bind the App OAuth client secret + `GITHUB_APP_*` + make the
App public — the install-callback ownership-hijack block is inert until those creds bind). **After
M17, ASK-3 seeding is automatic**; manual D1 seed is only a fallback.

**Fallback manual one-shot** (only if you must pre-seed before the automatic flow). Hand me the real
`{installation_id, tenant_id (UUID), max_concurrency, plan, repo_full_name…}` and I'll return a
vetted, coordinator-executed one-shot — not a blind paste. Template (prod `CONFIG_DB --env prod
--remote`; `unixepoch()*1000` stamps ms):

```sql
-- 1) installation → tenant
INSERT OR IGNORE INTO tenant_gh_installation_map (installation_id, tenant_id, created_at_ms)
VALUES ('<installation_id>', '<tenant_uuid>', unixepoch()*1000);
-- 2) concurrency entitlement (also auto-seeded by the Stripe runner webhook)
INSERT INTO runners_entitlement (tenant_id, max_concurrency, plan, created_at_ms)
VALUES ('<tenant_uuid>', <cap>, '<plan>', unixepoch()*1000)
ON CONFLICT(tenant_id) DO UPDATE SET max_concurrency=excluded.max_concurrency, plan=excluded.plan;
-- 3) repo allowlist (one row per entitled repo)
INSERT OR IGNORE INTO runner_repo_allowlist (tenant_id, repo_full_name, created_at_ms)
VALUES ('<tenant_uuid>', '<owner/repo>', unixepoch()*1000);
```

(Additive/idempotent — safe to re-run; `INSERT OR IGNORE` + `ON CONFLICT` respect the additive-auth
migration invariant and the d1_migrations ledger convention.)

---

## Summary for the courier (owner)

| Ask | Server-TL verdict | Server action | Gate |
|---|---|---|---|
| 1 | slot-seconds = **COGS-only, NOT load-bearing** → push arming low-urgency | mirror `UsageEvent.json` **once your canonical lands + tenant_id→UUID fix** | — |
| 2 | accepted; **not a live hole** today | WP: publish `max_vcpu_h` in introspect + vector — **need the tier→vcpu-h map** | before untrusted GA |
| 3 | webhook Option 1 **confirmed free**; seeding **wired + auto after M17** | none new; **vetted manual one-shot on request** | M17 owner-config |

Two balls back in your court: **(1)** land the canonical `UsageEvent.json` with a UUID example;
**(2)** send the tier→`max_vcpu_h` table. Everything else is server-side scheduled or owner-config.
