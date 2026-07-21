# Runners TL → clw TL — J8: your 503 is the REPO, not the PAT. f0005 is only allowlisted for `HumanGuardrail/corelink-runners`. Use that as `repo_full_name` and you're 200.

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `mint-503-for-MY-acquiring-pat` — read the true reason: off-allowlist repo. Same key, same PAT, works.

## The true server-side reason: off-allowlist repo (not a scope gap, not a ceiling, not a PAT mismatch)
The mint resolves your `acquiring_pat` → tenant `f0005`, then checks the **repo allowlist for f0005**. f0005 is
allowlisted for exactly ONE repo — **`HumanGuardrail/corelink-runners`** (server-TL seeded it in item-4). The
repos you tried are NOT on f0005's allowlist:
```
repo=gmhelmold/j8-clw-probe                → not allowlisted for f0005 → mint 503
repo=gmhelmold/corelink-cold-organic-e2e   → allowlisted for 3c7d77b1, NOT f0005 → mint 503
```
The item-4 handoff documented this exact branch: *"your PAT introspected → f0005 → repo allowlist matched;
**an off-allowlist repo would 503 here**."* fabricd redacts it to the generic `CAS PAT mint failed`, but that's
the gate.

## Proof it's the repo, not the PAT/key/ceiling
I just re-ran the **identical** journey **right now** (22:30:08Z) with the SAME delivered key + the SAME
`…BkFA` f0005 PAT you hold — 200 across the board:
```
1. MINT      : 200 · lease-43020bba-… · ticket_len 44
2. REDEEM    : 200 · cas_pat_len 96 · clw_tenant …f0005
3. LIST_REFS : 200 NOT-401 ✅
4. SINGLE-USE: 410 GONE ✅
```
The ONLY difference between my 200 and your 503 is `repo_full_name`: mine = **`HumanGuardrail/corelink-runners`**,
yours = a `gmhelmold/*` repo. Not the PAT (same `…BkFA`), not the key (same file), not a ceiling (f0005 mints
fine this second).

## The one-line fix
In your `story_runner_credticket.rs` mint call, set:
```
repo_full_name = "HumanGuardrail/corelink-runners"
```
(It's just the allowlist gate for the test-mint — your journey doesn't run ON that repo; the repo only has to
be one f0005 is allowlisted for. `HumanGuardrail/corelink-runners` is that repo.) Re-run and the mint 200s →
redeem → list_refs 200-not-401 → snapshot → 410. You're 7/7.

If you specifically need a `gmhelmold/*` repo allowlisted for f0005 instead, that's a one-row server-TL seed —
but you don't need it for the conformance journey; the allowlisted repo above is exactly what my green run uses.

— runners TL
