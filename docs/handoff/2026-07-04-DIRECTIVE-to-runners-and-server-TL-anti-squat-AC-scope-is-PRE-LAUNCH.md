# DIRECTIVE → corelink-runners TL + corelink-server TL — OWNER RULING: the namespace-squat / AC-scope residual is PRE-LAUNCH (not a follow-up). Requirement + a design ask (the clean mechanism is non-obvious).

> **From:** clw coordinator · **Relay:** owner (Gustavo) · **Date:** 2026-07-04
> **Owner ruling, explicit:** the exact-key / anti-squat residual is **pre-launch**, not a follow-up. A stolen
> per-job runner PAT must NOT be able to create junk/unbounded AC keys under its tenant.

## The requirement (binary exit)
Even with deny-DELETE + AC create-only (no poison, no evict) confirmed, a stolen per-job PAT can today create
arbitrary NEW AC keys under its own tenant (namespace-squat). **Close it pre-launch:** a stolen per-job PAT can
create AC entries only within a **bounded, per-job scope** it can't exceed — no unbounded arbitrary-key creation.

## The honest difficulty (why this is a design ask, not a one-liner)
- AC keys are **opaque BLAKE3 hashes** (`clw/ref/runner/v1/`‖name) — **NOT prefix-scopable**; you can only
  exact-match specific keys.
- The **exact output name is NOT known at mint time** — the autoscaler webhook (`workflow_job.queued`) fires
  before the job runs, so the mint can't know which keys the job will write.
So the clean "exact-key allowlist" isn't achievable on the autoscaler path as-is. **Pick/propose the pre-launch
mechanism** — candidates (your call, you own the mint + keyspace):
1. **Per-job AC-write count cap** (simplest, server-side on the mint scope): the per-job PAT may create at most N
   AC keys (N = a small bound sufficient for a real job). Bounds the squat blast to N junk keys, gone at revoke.
2. **Per-job AC namespace** derived from the stable `jobId` (which the autoscaler HAS): the runner's clw writes
   under a `jobId`-scoped ref sub-namespace, the mint scopes the PAT to it. (Needs a clw ref-domain change — flag
   me if so; I own clw and will do it.)
3. Another mechanism you see that meets the bounded-scope exit.

## Ask
**Runners + Server: agree the mechanism + confirm it's in the pre-launch WP5/env-0 wave.** Recommend #1 (count
cap) as the pragmatic pre-launch closure — small, server-side, no keyspace change — unless you prefer #2. Reply
with the chosen mechanism; if it needs a clw change (#2), I build it. This closes the last runner-PAT residual
pre-launch, per the owner's no-loose-ends bar.
— clw coordinator
