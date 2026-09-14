# DEEP scenario / story / journey catalog — the exhaustive map (v1, growing)

**Date:** 2026-07-17 · **Lead:** runners TL · **Why:** the first decomposition (3 broad agents) was
SHALLOW (AP-4 oversized WP). Depth = every JOURNEY × every COMBINATION × every FAILURE-INJECTION point
× every ORDERING/RACE — each cell a named scenario with cited evidence. This catalog is the map that
each narrow, deep WP exhausts. It is meant to GROW (the completeness-critic wave adds what's missing).

## The combination axes (the cartesian space every journey is crossed against)
- **Actor/repo:** first-party dogfood (in REPO_INSTALLATION_MAP) · unmapped first-party (App-path) ·
  external-org (App install) · unknown/unauthorized repo.
- **Tenant:** single (dogfood d863fafb) · second tenant · at-plan-ceiling tenant · no-plan tenant ·
  suspended tenant · compute-ceiling-hit tenant.
- **Label:** `corelink` · `corelink-dogfood` · `corelink-standard-{2,4,8}` · `corelink-<arbitrary>` ·
  RESERVED `corelink-builder` · `[corelink, self-hosted]` · `[corelink, gpu]` (non-passthrough) ·
  `[corelink-builder, corelink]` · unknown-only · empty · duplicate labels · case variants.
- **Mint mode:** cold (no mint key) · cold (no installation) · warm env-0 (stash+ticket) · warm legacy
  (CLW_TOKEN, non-prod) · legacy-refused-in-prod · forbidden (403) · transient (5xx→fail-open cold).
- **Concurrency state:** under per-key cap · exactly AT per-key cap · AT fleet cap (under per-key) ·
  idempotent re-admit (same jobId) · N-way race (concurrent acquires) · post-expiry re-acquire.
- **Lifecycle stage:** queued → claimed → minted → stashed → spawned → started → in_progress →
  completed → torn-down → revoked → wiped → billed. (A scenario can be cut at ANY stage.)
- **Delivery:** first delivery · queued redelivery · completed redelivery · in_progress event ·
  out-of-order (completed before queued) · lost webhook (no delivery) · leaked spawn-claim.
- **Failure injection point (× EACH lifecycle stage):** mint 403 · mint 5xx · mint timeout · stash
  DO-throw · JIT `generate-jitconfig` 404/5xx/timeout · container start fail (each of the retry
  attempts) · GitHub API 5xx/rate-limit/abuse · KV get/put/list error · DO unavailable · introspect
  down · revoke 4xx/5xx · teardown destroy() throw · billing ingest 5xx · Resend down · pg down.

## Spawn-worker JOURNEY FAMILIES (each = one deep WP; crossed against the axes above)
- **SJ-1 Cold first-party happy** — unmapped-key deploy: webhook(queued,corelink) → claim → mint COLD
  (no CLW_*) → JIT → spawn → started → completed → teardown → (no revoke: cold). Variations: each label;
  redelivery no-op; teardown-throw swallowed.
- **SJ-2 Warm env-0 first-party happy (the LIVE product path)** — webhook → REPO_INSTALLATION_MAP →
  mint WARM → CAS-PAT minted → stash → ticket injected (NEVER CLW_TOKEN) → spawn → clw redeem #1 (boot
  hydrate) → clw redeem #2 (job run) → completed → revoke(pat_id) + CRED_STASH wipe + teardown + bill.
  Cross: assert the raw PAT is in NEITHER container env NOR any log; the multi-use redeem serves both;
  post-completion redeem ⇒ 404 (F2-3 wipe).
- **SJ-3 Warm App-path (external-LIKE)** — App webhook (installation.id present, repo NOT in map) →
  App-installation-token mint → env-0 → … Cross: prove the App token path (not the static token),
  server-derived tenant.
- **SJ-4 Forbidden** — mint 403 → hard-deny → claim release → spawn_forbidden → NO JIT, NO container,
  NO orphan. Cross: does a 403 ever leak? does the claim always release?
- **SJ-5 At-ceiling** — slot at per-key cap ⇒ refuse + release + spawn_at_ceiling; AND at fleet cap
  (under per-key) ⇒ over_fleet_cap refuse. Cross: cold-repo cap; the min(entitlement,FLEET) clamp;
  idempotent re-admit doesn't consume a 2nd slot.
- **SJ-6 Spawn-failure → recovery** — container start fails (each retry attempt exhausts) → claim
  release + spawn_failed + F2 PAT revoke (not orphaned) + F8 dead-letter record (if warm) → reconciler
  warm-retry → recovered (delete record) OR giveup at MAX. Cross: cold job (no record); already-claimed
  (leave); TTL-expiry; the reconciler bump/claim/drive ordering.
- **SJ-7 Redelivery / idempotency** — queued redelivered (claim present ⇒ no double spawn) · completed
  redelivered (counter no-op via done-claim, but revoke/teardown/wipe re-run idempotently) · out-of-order.
- **SJ-8 Concurrency race** — N concurrent `runs-on: corelink` for the SAME tenant → the atomic slot
  admits EXACTLY min(cap,fleet), rejects the rest; NO double-count; NO thrash. (E4 live + a DO-sim unit.)
- **SJ-9 Burst / rate-limit** — many webhooks in <60s → WEBHOOK_LIMITER caps per-repo bucket; ignored
  events don't count; a leaked-secret flood is bounded.
- **SJ-10 Reconciler GitHub-scan** — a LOST webhook (no delivery) leaves a queued+labeled+runnerless job
  → the scan (RECONCILER_REPOS) redrives it (family label) → recovered; a leaked spawn-claim is cleared.
- **SJ-11 cred-cred route FSM** — every DO status (200 cred / 401 bad ticket / 410 expired / 404 no
  lease) × the exact body key-rename (token→cas_pat, endpoint→clw_endpoint, tenant→clw_tenant) × no raw
  StashedCred key leaks × malformed/oversized body ⇒ 400 not 500.
- **SJ-12 /v1/spawn + /v1/exec direct** (fabricd→worker provision) — mode:runner vs mode:check routing;
  image-digest pin assertion; auth; malformed body ⇒ 400.
- **SJ-13 Multi-tenant** (X4 for the real tenant) — 2 tenants concurrent → per-tenant slot isolation,
  correct billing attribution, one busy tenant doesn't starve the other (per-repo rate key).

## fabricd JOURNEY FAMILIES (each = one deep WP)
- **FJ-1 Lease FSM full** — acquire → Held → Closed; Held→Expired(deadline); Held→Crashed(dead box);
  cancel; every rejection reason; double-acquire; revoke coupling. **FJ-2 Admission reject-mode** (every
  arm + counter). **FJ-3 Admission queue-mode** (every arm + the byte-identity where claimed). **FJ-4
  Moat mint/revoke** — cred-ticket single-use (410 after first), cas_cred lease-liveness, mint-arm boot
  guard fail-closed on partial, token_plaintext, revoke by pat_id. **FJ-5 §13 envelope FSM** — collector,
  CaptureHook, JobClose ack (30s), tier-2/3 fallback + durable checkpoint. **FJ-6 Attestation** — sign,
  intent_metrics_sig, pubkey-only, key_id. **FJ-7 Reaper** — expire + crash surfacing + counters. **FJ-8
  Counters 3-way parity** — every field one seam, snapshot parity. **FJ-9 N>1 routing** — FNV-1a TS↔Rust
  identity, mint rejection-sampling, cap-guard fail-closed on non-pg N>1, durable suspend. **FJ-10
  Conformance + auth** — vector round-trip, tamper reject, deny_unknown_fields, fail-closed auth.

## CHAOS scenarios (E5, live, I run — each a named experiment with a timed-recovery artifact)
- **CH-1** kill fabricd container mid-lease → time watchdog detect+destroy+cold-boot; prove no boot-loop
  (boot-grace) + ledger survives (pg). **CH-2** inject a real spawn failure → F8 dead-letter recovers.
  **CH-3** dependency down: GitHub 5xx / introspect-down / KV-error / Resend-down → prove fail-closed
  (security) vs fail-open (fairness/billing) per north-star, each. **CH-4** force a REAL canary breach →
  prove it DETECTS + EMAILS. **CH-5** proxy per-request timeout: a wedged upstream → 503 fast. **CH-6**
  deploy rollback. **CH-7** cold-boot under concurrent load (boot-grace + timeout together).

## The deep decomposition (waves of NARROW WPs — sweet-spot, no oversized WP)
- **Wave-0 baseline** (running): the 3 broad agents — a floor + gap discovery.
- **Wave-1 spawn journeys**: SJ-1..SJ-7, SJ-11, SJ-12 — ONE agent PER journey family, each EXHAUSTING
  its variation matrix (author real tests, cite each cell). ~9 narrow deep agents.
- **Wave-2 fabricd journeys**: FJ-1..FJ-10 — ONE agent per family. ~10 narrow deep agents.
- **Wave-3 live STRESS**: SJ-8, SJ-9, SJ-13, B* — I run, 50-100+ concurrency, evidence = counters/runs.
- **Wave-4 CHAOS**: CH-1..CH-7 — I run, sequential, timed-recovery artifacts.
- **Wave-5 external/multi-tenant**: SJ-3 live (unmapped repo) + D1/D2.
- **Wave-6 COMPLETENESS CRITIC**: an agent whose ONLY job is "what journey/combination/failure is
  MISSING from this catalog + the evidence ledger?" → feeds the next round until it comes back empty.

## Evidence rule (unchanged, enforced)
No cell "validated" without a cited artifact. Every below-grade cell is listed honestly (fabricable-
later vs X4). The lead cold-validates each agent's evidence (re-runs a sample) before accepting.
