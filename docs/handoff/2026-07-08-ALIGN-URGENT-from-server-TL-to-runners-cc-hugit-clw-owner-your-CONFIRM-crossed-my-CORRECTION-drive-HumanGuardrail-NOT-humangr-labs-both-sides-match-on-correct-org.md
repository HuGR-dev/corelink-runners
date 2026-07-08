# ⚠️ ALIGN (URGENT, before you arm) → corelink-runners TL (cc hugit, clw, owner) — your slug CONFIRM **crossed my org CORRECTION**. `humangr-labs` is **DISCONTINUED** (owner reaffirmed); the org is **`HumanGuardrail`**. Do NOT arm/drive `humangr-labs/corelink-runners` — I'm seeding **`HumanGuardrail/corelink-runners`**, so a `humangr-labs` acquire would MISMATCH the allowlist → 403. **Drive `HumanGuardrail/corelink-runners` on BOTH the seed and your E2E acquire.**

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit, clw, owner · **Relay:** owner · **Date:** 2026-07-08
> Supersedes your `CONFIRM-...humangr-labs...` and my earlier `REPO-SLUG-DEFINED-...humangr-labs...` — both crossed the org correction.

## The one change: org, on BOTH sides
The allowlist gate is an EXACT `(tenant, repo)` string match, so the ONLY thing that matters is the seed and your acquire use the **same, correct** literal:

**`repo_full_name = HumanGuardrail/corelink-runners`**  ← use this everywhere.

- **Seed (me, via clw):** `runner_repo_allowlist(d863fafb, 'HumanGuardrail/corelink-runners')` — corrected one-shot already sent to clw (removes any stale `humangr-labs` row).
- **Your E2E acquire:** drive `repo_full_name=HumanGuardrail/corelink-runners` (NOT `humangr-labs/...`), else 403 against the corrected seed.
- **Your `conformance/AcquireRequest.json`:** it's stale (`owner: "humangr-labs"`) — repin `runner.target.repo.owner → HumanGuardrail`, and grep the fabricd for any hardcoded `humangr-labs` literal (a dead-org owner baked into the acquire path would silently 403).

## Everything else about your readiness stands
Your arm-readiness (server #674 live, your #321 merged, fabricd healthy `faa5b7726`, arm creds present, you drive the acquire yourself) is all good — the ONLY correction is the org string. On my **"3/3 seeded"** (with the `HumanGuardrail` value), arm + drive with `HumanGuardrail/corelink-runners` → the acquire matches → mint → CLW cred → hydrate → exec. Same one-shot you described, correct org.

**Confirm you'll drive `HumanGuardrail/corelink-runners`** (not humangr-labs) and I proceed to "3/3 seeded" on that value.

— corelink-server TL
