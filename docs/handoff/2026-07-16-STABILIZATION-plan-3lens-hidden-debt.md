# Stabilization plan — "100% firm, self-running" + 3-lens hidden-debt ledger

**Date:** 2026-07-16 · **Author:** runners TL · **Trigger:** owner — "como estabilizar de vez +
3 agents óticas diferentes procurando débitos ocultos." · **Status:** PROPOSED — awaiting owner
go/scope before fan-out.

Three adversarial auditors ran code-grounded, read-only (Lens A runtime-fragility/SPOF · Lens B
security/secret-hygiene · Lens C correctness/coverage). Findings deduped across lenses, ranked by
`probability × blast-radius × how-hidden`, and tagged **LIVE-TODAY** / **MULTI-TENANT-GATE** /
**INERT-owner-gated**. Inert-by-design N>1 machinery was verified correct and NOT inflated.

**Headline:** the security core is solid (all auditors independently confirmed: auth fail-closed to
503/404, constant-time compares, HMAC verified both sides, zero secret in any log/error path, boot
guards, digest-pinned Dockerfiles). What's missing is not correctness-in-the-small — it's the
operational envelope that makes it survive failure and multi-tenancy WITHOUT a human babysitting it.

---

## TIER 1 — LIVE-TODAY: stabilize the current path first

These bite the live dogfood/first-party path now, or the system's availability.

**F1 [CRITICAL] fabricd is a single-instance availability SPOF, ~90–110s hang-detection.**
`deploy/cloudflare-fabricd/wrangler.jsonc` (`max_instances:1`) + `src/index.ts:545-623`. Every `/v1/*`
routes to ONE container; a hang is caught only on the 1-min cron → 3 probes (~30s) → destroy → cold
boot. pg buys durability, NOT availability. Redeploy = a cold blip (no overlap at N=1). *Compounding
(F1b MED):* the proxy→container fetch has NO per-request timeout (`src/index.ts:460-542`) so a partial
hang piles up client stalls before the watchdog fires; *(F1c MED)* the watchdog can `destroy()` a
still-cold-booting container (30s probe budget < cold Rust+pg boot under load) → boot loop.
**Fix:** (now, cheap) add `AbortSignal.timeout` on the proxied fetch + a boot-grace exemption in the
watchdog + faster probe. (owner-gated) flip N≥2 shards — routing is built+inert, needs
`FABRIC_NUM_SHARDS`+`max_instances` raised together on pg.

**F2 [HIGH] Per-job credential lifecycle is leaky (4 facets, one root).**
- mint-before-start: `buildContainerEnv` mints the `cas:rw` PAT (`index.ts:660`) but the
  `jobId→patId` revoke-key is written only AFTER a successful start (`:520-528`); a start failure
  (e.g. F3 cap) orphans a live PAT to its 2h TTL, and the 60s reconciler re-mints a fresh orphan each
  tick.
- env-0 defeated on the Worker path (Lens B HIGH): the cred-ticket is MULTI-USE for
  `CRED_TICKET_TTL_S=7200` and `CLW_CRED_TICKET`+`CLW_FABRIC_ENDPOINT` are injected into the untrusted
  container env (`lib.ts:376-386,439-441`), so in-box untrusted code can redeem the live read-write
  CAS PAT repeatedly — the docstring still claims single-use (`lib.ts:318-320`). fabricd side is
  correctly single-use; the Worker dropped it to feed two clw processes. Blast radius = same-tenant.
- cred-stash never wiped at completion + redeem has no lease-liveness check (`index.ts:224-234`,
  completion `:823-871`) → the ticket→cred mapping outlives the job to TTL.
- revoke is fail-open (`index.ts:549-565`) — a failed revoke leaves a live PAT until TTL.
**Fix:** register the revoke-key AT mint time (not post-start) + revoke on the spawn-failure catch;
wipe `CRED_STASH` on `workflow_job:completed` + gate redeem on lease-liveness; Worker redeem
single-use or boot-nonce-bound; shorten per-job PAT TTL so every fail-open window is minutes not 2h.

**F3 [HIGH] Capacity ceiling mismatch + crash-reclaim OFF → thrash and paid-slot pinning.**
Admission ceiling = entitlement 20 (`lib.ts:519`) but `RunnerContainer max_instances:6`
(`deploy/cloudflare/wrangler.jsonc`, CheckHost:4). Between 7–20 concurrent jobs the fairness gate
admits, the CF class cap rejects the start → retry-exhaust → claim release → 60s reconciler re-drive →
same wall = multi-minute silent thrash (+ F2 PAT churn). Separately, `surface_crashes` is opt-in and
`FABRIC_CRASH_PROBE_INTERVAL_SECS` is unset+unforwarded (`reaper.rs:633-655`) so a mid-flight box
death pins a BILLABLE slot until the hard deadline; `FABRIC_BILLING_EXPORT_INTERVAL_SECS` is also
unset so a watchdog `destroy()` drops ≤30s of buffered usage events.
**Fix:** clamp admission to `min(entitlement, class max_instances)` AND raise `max_instances` to a
sane ceiling; surface an explicit at-capacity 503 (no orphan-and-retry); wire + forward
`FABRIC_CRASH_PROBE_INTERVAL_SECS` + `FABRIC_BILLING_EXPORT_INTERVAL_SECS` in the proxy envVars.

**F4 [HIGH] The live `/webhook` queued orchestration has ZERO route-level test.**
`index.ts:779-943` is driven by no test — `index.test.ts` covers only pure `./lib` fns, the DO SDK is
`vi.mock`'d. Unproven: claim-before-spawn, claim-release-on-failure, env-0 stash→inject, forbidden/
at-ceiling short-circuit. Any wiring regression ships green (this is the Lens-E class that produced
the 2026-07-09 `token`/`token_plaintext` cold-boot bug). Also untested at HTTP layer: the cred-ticket
redemption route `POST /v1/leases/{id}/cas-cred` key-mapping (`index.ts:954-979`), and the TS side of
`conformance/cloudflare-spawn.json` (enforced Rust-only today — the drift tripwire is one-sided).
**Fix:** integration test posting a signed `queued` body asserting claim+mint+spawn+metrics; a
cred-cred route test (200/401/410/404 + body keys); a TS golden test binding `SpawnBody` to the vector.

**F5 [MED] No alerting (nobody watches the golden counters) + one counter under-counts.**
Both surfaces expose golden signals but nothing observes them → a failure is invisible until a
customer complains. `webhook_job_completed` can permanently under-count: bumped fire-and-forget in
`waitUntil` AFTER `claimCompletion` records the dedup key (`index.ts:862,870`) — a worker kill between
them loses the count AND dedups the redelivery.
**Fix (Pillar 2):** an email canary/alert cron reading `/internal/v1/status` + `/internal/v1/metrics`
+ `/v1/health`, alerting on mint/spawn failures, capacity-503, health-down; bump the completion
counter before/atomically-with the claim.

---

## TIER 2 — MULTI-TENANT-GATE: must land before the first real external PAYING customer

Dormant under single-tenant dogfood; bite the moment a 2nd tenant exists — and we just wired the
external `runs-on: corelink` path, so this milestone is now reachable.

**F6 [HIGH] Billing mis-attribution on a `jtenant:` KV-miss.** `index.ts:615`
`derivedTenant ?? env.CLW_TENANT` — a warm multi-tenant job whose `jtenant:${jobId}` entry TTL-expired
(7200s) or missed bills the customer's `runner_slot_seconds` to `CLW_TENANT` (dogfood in prod); revoke
has the same fallback (`:558`). This is EXACTLY what `reconcileCompletedJobBilling` was hardened to
never do (emits 0 instead, `lib.ts:929`) — but the LIVE path does the opposite and no test covers the
miss branch. **Fix:** drop the `?? CLW_TENANT` fallback — no derived tenant ⇒ no push (mirror the
reconciler's I2 rule).

**F7 [HIGH] Per-tenant concurrency ceiling — the SKU's whole point — is bypassed on cold spawns.**
`index.ts:668` gates only when `mint.tenant && mint.maxConcurrency != null`; a cold spawn (no
installation_id / repo not mapped — the dominant plain-webhook case) never calls `acquireTenantSlot`
→ unlimited parallel runners. And `acquireTenantSlot` is itself fail-open + non-atomic
(`lib.ts:516,522`) so even warm it only advises. Concurrency is the billing model; today it isn't
enforced. **Fix:** back the slot with a Durable Object atomic count; enforce a repo-scoped fallback
cap on the cold path too.

**F8 [MED] No recovery for a stuck external-tenant job.** The 60s reconciler re-drive and the
spawn-claim release-on-kill both scan ONLY `RECONCILER_REPOS` (single dogfood repo). GitHub fires
`queued` once; a transient spawn failure OR a platform-killed `waitUntil` that leaks a `spawn:` claim
(the documented 2026-07-05 deadlock) = a permanently-queued job with no recovery for up to 2h, for any
customer repo. **Fix:** derive the reconciler/allowlist from the onboarded-installation set; DO-back
the spawn-claim so release is guaranteed; shorten the claim TTL vs the pat-map TTL.

---

## TIER 3 — INERT / owner-gated: track, fix at activation, do NOT build now

Verified correct-while-inert; each is gated on a future flip. Listed so none is rediscovered cold.
- **Label split-brain TS↔Rust** (`lib.ts:652` vs `webhook.rs:516`): the gates DON'T mirror
  (`[corelink,self-hosted]` and `corelink-standard-4` → TS serves, Rust refuses). Dormant — fabricd
  autoscaler unarmed. Fix at RAISE-N: one shared decision-table conformance vector, golden-tested both
  sides (the "mirror" CLAUDE.md claims but nothing enforces). Includes the Rust `RESERVED_LABELS`
  safeguard gap (`webhook.rs:844` accepts `corelink-builder` in the CSV → builder-pool hijack).
- **Size-resolver split-brain** (`size.rs` vs hardwired `instance_type:standard-4`): on ladder
  activation `corelink-standard-8` mints a JIT but gets a 4-vCPU box. Fix in the 3-seam ladder flip.
- **PINNED_IMAGE_DIGEST inert** (`/v1/spawn` accepts any `@sha256:` ref) — arm at runner-fleet activation.
- **N>1 round-robin cursors** per-isolate — seed from a hash/DO counter when N>1 goes live.
- **ALLOW_LEGACY_PAT_ENV=1** escape hatch injects raw `CLW_TOKEN` — cheap win NOW: add a boot
  assertion refusing it when a prod marker is set (mirror fabricd's boot guard).
- **Egress/IMDS denylist inert** (`enableInternet=true`, exact-host only, no CIDR) — substrate
  assumption (CF Containers expose no IMDS); enforce at network layer OR stop shipping it as assurance.

---

## The wave plan (6 pillars → WPs) — for owner approval

Sequenced by "what most takes you off babysitting duty." Each WP is disjoint (module/crate/config),
SOTA-DoD: invariants stated, completeness criteria, gate-green (tsc/vitest or cargo fmt/clippy/test),
lead cold-validates each diff before merge, no self-report merges.

| Wave | Pillar | WPs | Closes |
|---|---|---|---|
| **W1 — resilience** | 1 | proxy per-request timeout + watchdog boot-grace + faster probe; clamp admission to class cap + explicit 503; wire crash-probe + billing-export envVars | F1(hardening), F3 |
| **W2 — alerting** | 2 | email canary/alert cron Worker (reads both counter surfaces + health); fix completion-counter ordering | F5 |
| **W3 — credential lifecycle** | 5 | revoke-key at mint-time + revoke-on-spawn-fail; wipe stash + lease-liveness on redeem; Worker single-use/nonce ticket; shorten PAT TTL; `ALLOW_LEGACY_PAT_ENV` prod-refusal guard | F2, T3-legacy |
| **W4 — live-path tests** | 4 | route-level `/webhook` queued integration test; cred-cred route test; TS conformance golden | F4 |
| **W5 — CI/CD** | 3 | deploy pipeline on merge; fabricd image build WITHOUT local Docker (the hang that blocked go-live); continuous synthetic canary (corelink-smoke as monitor) | Pillar 3/4 |
| **W6 — runbook** | 6 | consolidate incident playbook (health-check, fabricd restart, spawn-claim-deadlock unblock, re-mint, secret inventory + rotation, delete the ~/Downloads App key) | Pillar 6 |
| **W7 — multi-tenant gate** (before 1st external paying customer) | — | drop `CLW_TENANT` billing fallback; DO-backed atomic concurrency slot + cold-path cap; reconciler allowlist from installations | F6, F7, F8 |

**Owner decisions needed before fan-out:**
1. **Scope:** W1–W6 now (stabilize the LIVE path)? W7 same push, or gated to "before first external
   paying customer"?
2. **fabricd N≥2:** harden the singleton window now (cheap, no cost) and defer N≥2 to real volume
   [rec], OR arm N≥2 now (raises always-on container cost)?
3. **Capacity:** raise `RunnerContainer max_instances` (6→?) — cost knob; I clamp admission regardless.
4. **Email alerting:** which send provider (Resend/SendGrid/SES) + API key + destination address
   (owner said channel = email).

Nothing is fanned out until this is approved.
