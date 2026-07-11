# 6-lens brutal audit (2026-07-11) — findings, fixes, and the CRITICAL external-customer gap

**Scope:** `git diff b771f0e..HEAD` (this session's work: observability counters both surfaces,
multi-size resolver, label-family gate, ops keys, deploys). **6 adversarial lenses** (1 Fable, 3
Opus, 2 Sonnet), code-grounded (source of truth = code, not claims). All CRITICAL/HIGH findings
re-verified by the lead against code before acting.

---

## 🔴 CRITICAL — the external `runs-on: corelink` path is NOT code-ready (I overclaimed it was)

A stranger's `runs-on: corelink` job can **never** get a runner today, for two independent reasons
(Lens F, verified):

1. **Missing code — no GitHub-App installation-token minting.** `mintJit`
   (`deploy/cloudflare/src/index.ts:369-373`) calls `generate-jitconfig` with `Bearer
   ${env.GITHUB_MINT_TOKEN}` — a STATIC first-party token with rights only on HumanGuardrail repos.
   Grep for `private_key`/`app_id`/`jwt`/`access_tokens` in the worker = **zero**. For a customer
   repo, `mintJit` 404s → `spawn_failed` → the job queues forever. `installation.id` is used only
   as a CoreLink-mint authz input, never exchanged for a GitHub credential.
2. **Missing architecture — no webhook delivery from external repos.** The dogfood uses this repo's
   own plain repo webhook + a static `REPO_INSTALLATION_MAP`. An external repo has neither. The
   GitHub App's single webhook URL is bound to the **signup-worker** (install callback), not the
   spawn-worker `/webhook`. Nothing fans it out.

**Why "proven live" wasn't proof:** `corelink-smoke` ran on `HumanGuardrail/corelink-runners` — the
one repo where the static token works, the repo webhook exists, and it's in `REPO_INSTALLATION_MAP`.
It proved `matchManagedLabel` matches `corelink` (already unit-tested), nothing about a foreign repo.

**The false claim that hid it:** the go-live checklist said *"the spawn-worker mints JIT runners
against the App (144561227)"* — FALSE per code. That made "reuse the App + set setup_url = done"
look complete; it closes only the signup/allowlist half. **Owned.**

**This is a real build, owner-gated on scope/priority:**
- (a) App-installation-token minting in the spawn-worker (App JWT from private key → installation
  access token → `generate-jitconfig`). The App private key already exists (bound on signup-worker).
- (b) Webhook routing: point the App's `workflow_job` webhook at a spawn-worker route (with
  `GITHUB_APP_WEBHOOK_SECRET` HMAC), or fan out from the signup-worker. Requires the App be
  subscribed to `workflow_job`.
- (c) The App needs self-hosted-runners/Administration:write on customer repos (permission change →
  re-approval prompt on existing installs).

---

## 🟠 HIGH — in-repo defects I introduced/left → FIXED this session

- **A (Lens A/C/D): label-family over-match.** The gate matched `corelink-builder` (the persistent
  builder pool → ephemeral runner would race it) and mis-served multi-label jobs (`[corelink, gpu]`
  → mints a corelink-only runner GitHub never assigns → orphan thrash). **FIXED (#375, live
  Version 99ba033a):** `matchManagedLabels` ports the Rust subset-gate + reserved-label exclusion —
  serves only if every label is a servable corelink label (minus `corelink-builder`) or a
  `self-hosted` passthrough, and mints the FULL requested label set. Dogfood unchanged; 125 vitest.
- **B (Lens B): dead counter.** `acquire_rejected_lease_invalid` was defined/snapshotted but never
  incremented (a permanently-0 golden signal → false "healthy" on a ledger flap). **FIXED (#376):**
  wired at the two ledger-decline seams (admission-reserve Err + Pending→Held refusal). *(fabricd
  redeploy pending — the counter is inert until the reaper-coverage-successor image ships.)*

---

## 🟡 MEDIUM / LOW — real, mostly cross-repo/owner or moot-until-armed (NOT yet fixed)

- **Fleet capacity (F, HIGH-ish):** `RunnerContainer max_instances: 6` TOTAL vs a single tenant's
  entitlement 20 (`deploy/cloudflare/wrangler.jsonc`). Raise at launch — owner.
- **Billing mis-attribution (A/F):** the single-tenant `CLW_TENANT` fallback + the family
  reconciler bill the dogfood tenant for a customer's job. **Moot until billing is armed** (it's
  OFF — server-TL confirmed leave-off for launch), but the reconciler is single-tenant *by
  construction* and must be fixed before multi-tenant billing.
- **TS↔Rust label split-brain (D):** `matchManagedLabels` (TS, family/prefix) vs the Rust webhook
  subset-gate (`FABRIC_AUTOSCALER_LABELS`, explicit list). Dormant (fabricd autoscaler not armed);
  reconcile before multi-size activation.
- **Cross-tenant DoS (F):** `WEBHOOK_LIMITER` is one global `key:"spawn"` — one busy tenant
  rate-limits all. Per-tenant key before multi-tenant.
- **No direct-fleet size resolution (F):** `matchManagedLabels` accepts `corelink-standard-8` but
  the spawn-worker hardwires `instance_type: standard-4` (size resolver is fabricd-only + inert). A
  `corelink-standard-8` job silently gets a 4-vCPU box. Part of multi-size activation.
- **Counter under/over-count (B):** queue-mode admission rejections uncounted (default is reject,
  scope-limited); `webhook_job_completed` over-counts on GitHub redelivery (no dedup on the
  completed leg); `acquire_rejected_over_cap` label-smear on no-plan; `revoke_attempts` inflated by
  stale-PAT cleanup. All LOW-MED; the success counters are correct.
- **No external-repo orphan recovery (F):** the reconciler is first-party-allowlist-only.
- **Cold-spawn fail-open (C, amplified by A):** the label broadening widened who reaches the
  pre-existing unarmed-mint cold-spawn window (still App-install-gated). A live-webhook repo
  allowlist would close it.

---

## ✅ Confirmed SOLID (audit, not sandbagged)

Security/auth (fail-closed correct on both new gates, constant-time, no secret committed, `ctx?.
waitUntil?` guards metrics only — security actions stay synchronous); contract/conformance (manifest
17/17 intact, ZERO frozen DTOs touched, `StatusReport.counters` correctly `/internal/v1`); DO
migration v4 (additive/safe); the counter MECHANICS (load-shed capture, agent_exec done/failed,
reaper expired/crashed, MetricsDO race-freedom under DO input-gating, StatusReport parity — all
correct); the multi-tenant mint/authz plumbing DOWNSTREAM of delivery (server-derived tenant, 403 =
hard deny, never a wrong-tenant spawn). The label-family fix mechanics are clean.

**Biggest honest lesson:** every "tested/green/proven-live" claim this session held only at the
**pure-function / dogfood-repo** layer. Real HTTP dispatch, real DO concurrency, real external-repo
delivery — coverage/proof drops to near-zero (Lens E). The go-live narrative was built on a
credential (static token) that stops working at the first non-HumanGuardrail repo.
