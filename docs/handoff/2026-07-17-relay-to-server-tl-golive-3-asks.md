# Relay → corelink-server TL — 3 go-live asks from the Runners fabric

**Date:** 2026-07-17 · **From:** corelink-runners TL · **Courier:** owner (gustavo@humangr.com)
**Context:** a go-live readiness audit of the 5 "not-fully-live" gaps found that 3 of them are
**cross-repo** — the fabric side is built + gate-green, but each needs a corelink-server action to
become real. None is urgent at today's single-tenant dogfood scale; all are prerequisites for the
**first external paying customer**. This doc is the single relay for all three. Reply inline or in a
server-side handoff doc; the owner carries it back.

---

## ASK 1 — Billing ingest: is `runner_slot_seconds` load-bearing? (unblocks arming the push)

**The fabric side:** the usage-push client is built + idempotent on both paths (fabricd Rust
`corelink_billing.rs`, spawn-worker TS `pushUsageEvent`). The server ingest
`POST /internal/v1/billing/usage` is already live + verified (your PR #473, `ad1eedbd-r1`:
`202 {accepted:1}`, re-POST `{deduped:1}`, bad-auth 401). Wire shape is now pinned by a committed
conformance vector on our side (`conformance/UsageEvent.json`, PR #385) — **please commit the
byte-identical vector on the server side** so the drift tripwire is symmetric (same convention as
`RunnerLease.json`).

**The open question (yours to settle):** is `runner_slot_seconds` **load-bearing for the live
product** (drives an invoice / a dashboard the customer sees / anti-abuse enforcement), or is it
**fully superseded by Stripe flat-concurrency** (COGS/reconciliation only)?
- If **load-bearing** → we must arm the spawn-worker push **at or before** first-customer onboarding
  (see the durability note below — pre-arm usage is otherwise **unrecoverable**).
- If **COGS-only** → arming is low-urgency; we still build a durable usage ledger our side (WP-F) so
  we can backfill whenever.

**Durability caveat you should know:** with the spawn-worker push OFF, a completed job's usage is
**computed never / persisted nowhere** (`index.ts:680` returns before `buildUsageEvent`; the
`jtenant:` tenant map is TTL'd 2h + deleted at completion; the reconciler reads the tenant-less
GitHub jobs API). So any real-customer job that runs **before** the push is armed loses its usage
history permanently. We are closing this our side with a durable pre-arm usage ledger (WP-F), but the
load-bearing answer decides whether that's sufficient or the push must be armed first.

---

## ASK 2 — Populate the per-tenant `max_vcpu_h` entitlement vector (arms the compute ceiling value)

**The fabric side:** the monthly vCPU-h compute ceiling is **built and armed** on live fabricd
(pg ledger + `FABRIC_RUNNER_VCPU=4` are on; `build_compute_gate` produces a live `ComputeGate`; the
`vcpu × ttl` reservation + atomic pg admit/terminal accrual are tested). The concurrency cap already
bounds per-tenant cost independently, so this is not a live hole — but the ceiling **value** on the
CoreLink auth path is sourced from the introspect `max_vcpu_h` entitlement, which is **0/disabled
until the server publishes it**. So the ceiling machinery runs but enforces nothing per-tenant.

**Ask:** publish `max_vcpu_h` in the introspect/entitlement response per tenant (the pricing ladder
values: caps 20–320, vCPU-h ceilings 100–2400). Until then the monthly wall — pricing's
"loss-impossible" guarantee — is non-binding. **Required before untrusted multi-tenant GA**, not
before dogfood.

---

## ASK 3 — Seed the first external customer's entitlement (unblocks the external-GA flip)

**The fabric side:** the external-customer path is built (App-token mint per installation, HMAC
webhook verify, per-tenant cap/COLD_REPO_CAP/atomic slots, env-0 cred-ticket, derived-tenant
billing, dead-letter recovery), and we've just added a Worker-side **installation allowlist** gate
(PR #387) so an un-entitled foreign repo is refused early (no spawn-claim/slot/orphan churn). The App
secrets are bound on the spawn-worker.

**What's missing is server-side onboarding state** — without it the mint 403s (fail-closed, safe,
but the customer's jobs never spawn). For the first customer, seed:
1. **installation → tenant** map (their GitHub App installation id → their CoreLink tenant).
2. `runners_entitlement.max_concurrency` (their purchased concurrency tier).
3. `runner_repo_allowlist` (the repos they're entitled to run).

Plus confirm the **webhook routing decision** (Option 1, your `REPLY:12-45`): the GitHub App's single
Webhook URL repoints to the spawn-worker `/webhook` — you confirmed this is free server-side. The
owner does the repoint; we need your go that the signup-worker doesn't need the delivery.

---

## Summary for the courier (owner)

| Ask | Server-TL action | Blocks | Urgency |
|---|---|---|---|
| 1 | Answer load-bearing Q + commit `UsageEvent.json` vector | arming billing push | before 1st paying customer |
| 2 | Publish `max_vcpu_h` per tenant | compute-ceiling value | before untrusted GA |
| 3 | Seed installation→tenant + entitlement + repo allowlist; confirm webhook repoint | external-GA flip | before 1st external customer |

None blocks dogfood. All three are "first real customer" gates. Fabric side is ready + gate-green on
each; these are the server-side halves.
