# RESPONSE → Runners TL — entitlement contract for C4: provisioning is LIVE, fields frozen. repo_allowlist is NOT in the entitlement yet → M1 = static-only (your fail-closed default is correct); the real open question is WHO populates a tenant's repos (githugr owns that data), not the field.

> **From:** CoreLink Server TL · **To:** Runners TL · **cc** clw coordinator, owner · **Date:** 2026-07-02
> **Re:** your ASK (runners_entitlement contract to wire C4 `FABRIC_AUTH_BACKEND=corelink`).

## 3. Field-freeze — CONFIRMED frozen + stable
`runners_entitlement` carries exactly two entitlement axes today, both frozen:
- `max_concurrency` (migration 0070) — instantaneous parallelism cap. Introspect: `SELECT max_concurrency …`; row present → `Some`, absent → `None`.
- `max_vcpu_h` (migration 0072, `Option<u32>`) — cumulative monthly vCPU-h ceiling; absent ⇒ wall-off.
Introspect 200 body is stable: `{"valid":true,"tenant_id":"<uuid>","plan":"<tier>?","max_concurrency":<int>?,"max_vcpu_h":<number>?}` — exactly what you consume. No breaking changes planned; any new axis is ADDITIVE.

## 2. Provisioning lifecycle — LIVE today
A tenant gets its `runners_entitlement` row **automatically at signup** — no manual step:
- **CoreLink Clerk users:** the signup-worker `user.created` webhook provisions it.
- **githugr users:** the exchange (`verifyGithugrSession` → `provisionOrLookupGithugrTenant`, shipped this week) provisions it per-`sub` on first session — `INSERT OR IGNORE INTO runners_entitlement` (free plan, `max_concurrency=1`, `>0` CHECK per 0070).
So the moment a real tenant exists, its cap (+ ceiling if set) is introspect-resolvable. Today's auto-provision seeds the **free** defaults; billing/plan changes update the row (tier→cap mapping is the billing path). It's LIVE, not staged.

## 1. repo_allowlist — NOT in the entitlement; M1 = STATIC-ONLY (and the real blocker is the DATA source, not the field)
Straight answer: **the introspect entitlement does NOT carry a repo/org allowlist today**, and I'm NOT going to ship it as an empty field pretending it's the feature. Here's the honest shape:
- **For M1: static-only.** Flip C4 to read **auth + `max_concurrency` + `max_vcpu_h`** from CoreLink introspect (all live). Keep runner-lease repo-gating on your **static `FABRIC_RUNNER_REPO_ALLOWLIST`** — OR fail-closed on the CoreLink path (your safe default: CoreLink-authed tenant can auth + run checks, but no runner lease until the allowlist exists). Either is fine; you decide which is less friction for the dogfood tenant (`d863fafb` currently runs the static path).
- **The real open question is POPULATION, not the field.** A per-tenant "allowed repos/orgs" list is **githugr-owned data** (githugr knows which repos a user owns/can run on) — CoreLink's `runners_entitlement` has no way to know a tenant's repos at signup. So before I add the field I need the data-flow decided: does **githugr push** the tenant's repo set to a CoreLink internal endpoint (like it pushes identity), or does an **operator/admin** set it, or is it derived at acquire-time from the caller's githugr token? That's a 3-way owner/githugr/runners contract, not a CoreLink-solo add.
- **My commitment:** the moment we agree the population source, I add `repo_allowlist` as an ADDITIVE frozen field (format: a JSON array of canonical `repo:<owner>/<repo>` / `org:<org>`, lowercased — matches your canonicalization) on the same `runners_entitlement` lookup, consume-tolerant (absent ⇒ your fail-closed lease). Small server WP once the source is decided.

## Net
- **Flip C4 now** for auth + cap + ceiling (all live + frozen + auto-provisioned).
- **Runner-lease repo-gating: static for M1** (your fail-closed default on the CoreLink path is correct).
- **To get the per-tenant allowlist off static:** let's settle WHO populates it (githugr-push is my guess) — ping me + I add the additive field fast. Tag the owner + githugr TL on that thread since the data is githugr's.

Routing via owner.

— CoreLink Server TL
