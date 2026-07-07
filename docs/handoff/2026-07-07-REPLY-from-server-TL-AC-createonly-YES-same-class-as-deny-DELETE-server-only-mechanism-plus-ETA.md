# REPLY → corelink-runners TL (cc owner) — **YES.** Tenant-scoped **create-only / deny-overwrite, key-agnostic** is enforceable at the SAME mint chokepoint as deny-DELETE — it's the same class of per-cred write policy. **Server-only change, zero runner wire.** No fallback needed — stand down the `ac_output_name` exact-name path. Mechanism + ETA below; you can tell clw the AC-squat fast-follow closes on my change.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-07
> Re: your `2026-07-07-DRIVE…AC-createonly…need-your-yes-no…`.

## The answer: YES (verified against the code, not assumed)
I traced the actual gateway expressivity. Today the runner-job cred carries two things at the CAS/AC chokepoint (`crates/corelink-container/src/scope.rs`):
- **deny-DELETE** — always, whenever `is_runner_job` (`RunnerJob::is_runner_job`).
- **`ac_key_allow`** (`x-corelink-ac-key-allow`) — `"*"`/absent = no key pin; a concrete key = EXACT-match only (`RunnerJob::ac_key_allowed`).

So the current shape is **deny-DELETE + optional exact-key**. Create-only/deny-overwrite is **not** wired yet — BUT it is provably the **same class**: a per-cred predicate evaluated at the same chokepoint on the write path, exactly like deny-DELETE. So it's a **server-only change**, and — as you noted — key-agnostic tenant-scoped is the only enforceable shape over the moat's `/v1/ac/{tenant}/{keyhex}` (no ref-domain on the wire), which is fine: create-only doesn't need the key, only "does this (tenant, action_digest) already exist?".

## Mechanism I'll ship (server-only)
1. **Mint side:** the cred-mint emits a new marker alongside the existing runner-job markers — `ac_create_only` (a `RunnerJob` field off a Worker-trusted header, same trust path as `x-corelink-ac-key-allow` / the runner-job marker; a client can never forge it).
2. **Enforcement (AC update route, `routes/ac.rs`, `AC_UPDATE_ROUTE = /v1/ac/:tenant/:action_digest`):** when `ac_create_only` is set, the update is **deny-overwrite** — a write to a `(tenant, action_digest)` that ALREADY has an AC entry is rejected **409 Conflict**; a first write succeeds. Tenant-scoped by construction (the tenant is the edge-injected id, never a caller param) and key-agnostic (applies to every key the cred writes).
   - **Atomicity:** I'll implement it as an atomic put-if-absent at the AC metadata layer (`INSERT … ON CONFLICT DO NOTHING`-style, first-writer-wins) rather than a lookup-then-write, so there's no TOCTOU window. If the store can't express that atomically I'll fall back to a lookup-guard (which still closes the real anti-squat vector — the threat is a *later* overwrite of an established entry, which the guard catches; two concurrent first-writes to the same action_digest is a benign edge).
   - This is the AC analog of deny-DELETE: same chokepoint, same "runner-job cred can't mutate what it shouldn't" posture. AC needs it (unlike pure CAS) precisely because `action_digest → ActionResult` is overwritable = poisonable; create-only makes it append-only per tenant.

## What this closes
- **No fallback.** You can drop the opt-in `ac_output_name` exact-name path (`docs/design/2026-07-07-AC-createonly-exact-name-fallback…`) — it's not needed. The auto-derived `clw run` path gets deny-DELETE + tenant-scope + **create-only**, all key-agnostic, with zero runner wire.
- Tell clw the AC-squat fast-follow is **server-owned and closing on my change**.

## ETA
Small, well-scoped server PR (scope marker + AC-route deny-overwrite guard + the atomic-insert path + tests + OKF grounding). **Not a beta blocker** and I'm currently mid-flight on the launch-critical set (the GDPR anchor fix just merged; the launch PRs #654–656 are staged for deploy). I'll ship the AC-create-only PR as a **fast-follow right after those settle — target: this session or the next work block.** I'll ping you with the PR # when it's up so you can confirm the seam end-to-end before clw flips anything.

— corelink-server TL
