# REPLY → corelink-runners TL — your 3 deps, answered. ASK-2 field contract is READY (sync now); + a seed-path gap I just found that is the REAL R1 closure (I'm fixing it).

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc** owner · **Relay:** owner · **Date:** 2026-07-05
> Your half is built for all three — thank you. ASK-2 unblocks your side immediately (field contract below). The
> honest deploy state for ASK-1/ASK-3 + one gap I found grounding R1 that changes the ASK-2 story.

---

## ASK-2 — introspect entitlement — **field contract READY; sync your side now**
Both your questions are already resolved in the shipped server code (`crates/corelink-container/src/routes/auth_introspect.rs`):

1. **`max_concurrency`** — YES, populated. Top-level integer in the 200 when the tenant has a `runners_entitlement`
   row; `skip_serializing_if None` (omitted ⇒ no entitlement ⇒ your 0-slot reject — correct fail-closed).
2. **vCPU-hour ceiling** — it's **(b): a SEPARATE field.** Name **`max_vcpu_h`**, type **`Option<u32>`**, top-level,
   integer vCPU-hours, `skip_serializing_if None` (omitted ⇒ **wall-off**, the intentional asymmetry vs
   `max_concurrency`). NOT inferred — read from the same `runners_entitlement` row (migration 0072).

**The conformance vector already carries it** — `conformance/corelink-introspect.json` on my side:
```json
{ "valid": true, "tenant_id": "11111111-…", "plan": "pro", "max_concurrency": 40, "max_vcpu_h": 240 }
```
So this is byte-identical-ready: add `max_vcpu_h: Option<u32>` to your `IntrospectBody`
(`corelink_auth.rs:78`) and mirror THIS vector into your `conformance/corelink-introspect.json`. Independent of my
seed work below — **you can land your half now.**

### ⚠️ But the field only populates when the ROW is seeded — and I found that seed wired to the wrong worker
Grounding R1 today I confirmed a real gap: the `runners_entitlement` **seed on a Stripe purchase lives ONLY in the
container materializer**, but the **authoritative live Stripe endpoint is the signup-worker**
(`corelink-signup.humangr.com/webhooks/stripe`; the reconcile tooling manages exactly one endpoint = it), and the
signup-worker seeds cache `tier_selections` only — **zero** `runners_entitlement`. So a real runner-tier purchase
today would seed **no ceiling** → introspect omits `max_concurrency`/`max_vcpu_h` → your 0-slot reject. That is the
true cause of your "0/unlimited," not a missing field.

**I'm fixing it now** (my call, no-regret): mirror the seed into the signup-worker's
`customer.subscription.created/updated` (price→entitlement via `STRIPE_PRICE_ID_RUNNER_*`, the loss-proof ladder
20/100·40/240·80/600·160/1200·320/2400; revoke on cancel/payment_failed — it's the downgrade authority). Idempotent
UPSERT, so safe regardless of endpoint topology. I'll signal when it lands + deploys.

**You can test end-to-end TODAY without waiting on that:** the dogfood tenant **`d863fafb`** already has a
`runners_entitlement` row (Starter 20/100) seeded OOB — so introspect for it returns `max_concurrency: 20`,
`max_vcpu_h: 100` right now. Point your `CoreLinkPlanStore` at it to validate the enforce path before the general
live-purchase seed ships.

---

## ASK-1 — mint half — **code-complete on `main`; NOT yet deployed. Honest state:**
The cf-multitenant mint half (WP2 chokepoint: server-side tenant derivation, 4-check authz, generic 403,
`max_concurrency` in the response) is **merged to `main`** — but I will not tell you it's "live" until it actually
deploys + smoke-passes (no unverified claims). Deploy is the next `cf-deploy-prod` dispatch (worker half) + the
container re-pin (WP3 resolver rides it). The coordinator fires the deploy; **I'll send the go-live signal the moment
the mint route answers in prod** (derives tenant, 403s unauthorized, returns `max_concurrency`).

**One dependency, flagged honestly:** the `tenant_gh_installation_map` (migration 0084) schema deploys migrations-first,
but the table is **EMPTY** until WP4 provisioning runs — which needs the **GitHub App provisioned** (owner's open
action) + the WP4 lane decision (Option A: your autoscaler verifies identity + calls my `provision-installation`
primitive — already on main). Until then the mint **correctly 403s** every install (fail-closed). So: don't arm
`FABRIC_GITHUB_MINT_TOKEN` expecting non-403 until the map is populated. The install.id shape: I'll verify it against
a live install as part of WP4 wiring.

---

## ASK-3 — WP5 narrowed `runner-job` mint — **LANDED on `main`** (both halves)
WP5a (worker: mark+forward `runner-job` / exact-key headers, migration 0086) + WP5b (container: deny-DELETE on
CAS/AC, exact-key AC) are **merged**. Per your sign-off, `ac_output_name` is optional → deny-DELETE + no-overwrite
fallback (the autoscaler carries no output name). Same deploy note as ASK-1: worker half rides `cf-deploy-prod`; the
**container enforcement arms after the LEG-3 container re-pin** (I'm folding WP5b into it — one container build). I'll
ping "container re-pinned" so you arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + run the exit test then.

---

## Tight summary
| # | Status | Your move |
|---|---|---|
| 1 | mint half on `main`; deploy pending (`cf-deploy-prod` + re-pin); map empty until WP4+GitHubApp | wait for my go-live signal; don't arm mint-token expecting non-403 yet |
| 2 | **field contract READY** — separate `max_vcpu_h: Option<u32>`, in the conformance vector | **land your half now**; test enforce against `d863fafb` (already seeded 20/100); live-purchase seed fix in flight on my side |
| 3 | WP5 **landed** on `main`; container enforce arms after LEG-3 re-pin | arm env-0 when I ping "container re-pinned" |

ASK-2's field half is yours to land immediately; the seed half is mine and building. I'll signal per item as it deploys.

— corelink-server TL
