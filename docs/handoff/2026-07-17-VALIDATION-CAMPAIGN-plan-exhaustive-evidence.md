# Exhaustive Validation Campaign — every atom proven, with evidence

**Date:** 2026-07-17 · **Lead:** runners TL (techlead skill) · **Trigger:** owner — "cada molécula e
átomo validado em todas as direções e combinações possíveis, com evidência." · **Status:** PREP —
awaiting owner go on the 3 risk/scope decisions before fan-out.

## The honest framing (rigor compact)
"Every combination" is combinatorially infinite; a real tech lead scopes it to a **complete,
prioritized, evidence-backed matrix** — every FEATURE × every DIRECTION (happy / edge / adversarial /
failure) × the load-bearing COMBINATIONS — and is explicit about the line between **fabricable**
(I can produce evidence) and **X4-blocked** (needs a real external repo / 2nd tenant / CoreLink PAT /
hugit dispatch — not fabricable by me). No cell is marked "validated" without a cited artifact
(run ID, counter delta, log line, HTTP response, test name). Nothing is generalized from a dogfood
smoke to an external customer (the Lens-E lesson).

## Evidence grades (every cell gets one)
- **E0 unit** — pure-function test (vitest/cargo). - **E1 route/integration** — handler driven with
  mocked infra. - **E2 live-probe** — real endpoint response. - **E3 live-smoke** — one real happy-path
  run. - **E4 live-STRESS** — real load / concurrency / burst. - **E5 CHAOS** — real failure injected +
  recovery proven. - **X4** — needs a real external input; documented, not fabricated.
The campaign's job: push every atom to the **highest grade its nature allows**, and record the artifact.

## The validation surface (the "atoms") — 6 domains

**A. Correctness — exhaustive path/combination coverage** (parallelizable: static audit + authored tests)
- A1 webhook gate: every action (queued/completed/in_progress/other) × every label set (bare `corelink`,
  `corelink-<size>`, RESERVED `corelink-builder`, multi-label, `self-hosted` passthrough, unknown, empty)
  × warm/cold × forbidden/at-ceiling/success. A2 credential lifecycle FSM: mint→stash→redeem×N→wipe;
  spawn-fail→revoke; completion→revoke+wipe; legacy-guard; env-0-armed vs cold; every branch of
  buildContainerEnv. A3 concurrency slot: under/at/over per-key, under/at/over fleet, warm/cold key,
  idempotent-retry, expiry-prune, release, race-ordering. A4 dead-letter: record/first-only/cold-skip,
  retry/giveup/recovered/already-claimed/missing/TTL. A5 GitHub-scan reconciler: detect/redrive/claim-leak/
  family. A6 cred-cred route: 200/401/410/404 + key-rename + malformed. A7 conformance vectors: every
  wire type both sides. A8 fabricd Rust control plane in full: leases, admission reject+queue modes,
  moat mint/revoke, attestation, all counters, reaper (expire/crash), §13 envelope FSM, size resolver,
  N>1 routing (inert) — audit the 659+ suite for GAPS, add the missing cells. A9 counter accuracy:
  every golden signal increments at exactly the right seam, no over/under-count.

**B. Live STRESS — concurrency / burst / capacity** (SEQUENTIAL on live infra; E4)
- B1 concurrency: fire N simultaneous `runs-on: corelink` jobs → prove the atomic slot admits ≤cap,
  rejects >cap, fleet cap holds, NO thrash (counter evidence). B2 burst/rate-limit: rapid webhooks →
  prove WEBHOOK_LIMITER + per-repo bucketing. B3 redelivery/idempotency: replay queued+completed →
  prove no double spawn/bill/count (claim dedup). B4 capacity ceiling: drive to fleet cap → prove a
  clean at-ceiling refusal, not orphan-thrash.

**C. CHAOS — real failure + recovery** (SEQUENTIAL, DISRUPTIVE on live infra; E5)
- C1 fabricd kill: destroy the singleton mid-flight → time the watchdog recover (prove the boot-grace
  doesn't boot-loop). C2 spawn-failure→F8: inject a real spawn failure → prove the dead-letter records +
  the reconciler warm-recovers. C3 dependency failure: GitHub 5xx / introspect-down / Resend-down /
  KV-error → prove fail-closed (security) vs fail-open (fairness/billing) per the documented north-star.
  C4 canary REAL alert: force a genuine breach (e.g. break a monitored surface) → prove the canary
  DETECTS + EMAILS (not just the transport curl). C5 deploy rollback: prove a bad deploy can be rolled
  back. C6 cold-boot under load: prove the watchdog boot-grace + per-request timeout under a real cold
  boot.

**D. External / multi-tenant journeys** (X4-BLOCKED — documented, owner-gated)
- D1 real EXTERNAL repo (non-HumanGuardrail) installs the App + dispatches → the ONLY real proof of the
  customer path (mechanism proven; journey never run). D2 multi-tenant: 2+ real tenants → concurrency
  isolation, billing attribution, fairness. D3 moat E2E: real `[clw] cache hit` (needs a CoreLink PAT
  or hugit dispatch). D4 billing: a real usage event end-to-end (armed OFF by owner/server-TL).

**E. Security — adversarial** (parallelizable audit + targeted live; E1/E2 + live)
- E1 auth fail-closed: fuzz bad/missing/malformed auth on EVERY gate (fabricd /internal + admin, spawn
  /internal, cred-cred, webhook HMAC) → prove 401/404, never fail-open. E2 secret non-leak: no secret in
  any log / error body / response / container env dump (grep + live probe). E3 HMAC replay/timing. E4
  untrusted-env envelope: the cred-ticket exposure bound, egress inertness. E5 X4 supply-chain: every
  FROM @sha256-pinned, digest match, no unpinned pull.

**F. Evidence ledger** — the deliverable: a matrix (atom × grade × artifact), plus a ranked list of every
cell that STAYS below its achievable grade (the honest residual), each labeled fabricable-later vs X4.

## Wave structure (techlead go/no-go)
- **Disjoint & parallel** (fan out to worktree/read-only agents): A (correctness audit + authored
  tests, sliced by module), E-audit (security static). Different files/modules → conflict-free.
- **SEQUENTIAL & lead-run** (shared LIVE infra — CANNOT parallelize; chaos on one surface perturbs all):
  B (stress), C (chaos). I run/orchestrate these one at a time, capturing evidence, restoring state
  between each.
- **X4-documented** (no fan-out closes these): D — I write the exact runbook + inputs needed, owner/
  external-gated.

## DoD (per WP) — SOTA
Every WP: (1) enumerates its atoms as an explicit checklist; (2) pushes each to its highest achievable
grade; (3) cites the artifact per cell; (4) reports every below-grade cell honestly (no fake-green, no
tautology — a real gap noted beats a green lie); (5) gate stays green (no test weakened to pass). The
lead COLD-VALIDATES every agent's evidence (re-runs a sample, inspects artifacts) before accepting.

## OWNER DECISIONS (block fan-out)
1. **Chaos on the LIVE dogfood infra?** C-waves cause brief real outages (fabricd kill → ~watchdog
   window) + real spawn cost (load). Dogfood is internal (no paying customers). [rec: YES — it's the
   only way to prove resilience with evidence; dogfood is low-stakes; I restore state after each.]
2. **Load/stress scale + cost budget.** B1 spins N real containers (cost). How far: to the fleet cap
   (20) to prove rejection, or beyond? [rec: to cap + a bit over, to prove the clean refusal — bounded
   spend.]
3. **X4-blocked (D): accept as documented residual, or provide inputs?** Can you create a throwaway
   EXTERNAL repo (D1) + a 2nd test tenant (D2)? If not, they stay documented-gaps with the exact steps.
