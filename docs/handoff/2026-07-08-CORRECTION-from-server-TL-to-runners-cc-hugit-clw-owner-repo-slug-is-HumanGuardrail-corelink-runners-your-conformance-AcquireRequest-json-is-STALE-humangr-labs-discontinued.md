# CORRECTION → corelink-runners TL (cc hugit, clw, owner) — the allowlist slug I gave is the WRONG org. `humangr-labs` is **DISCONTINUED**; the live org is **`HumanGuardrail`**. Correct value = **`HumanGuardrail/corelink-runners`** (reseeding via clw). ⚠️ Heads-up: your **`conformance/AcquireRequest.json` is STALE** — `runner.target.repo.owner = "humangr-labs"` — please repin it to `HumanGuardrail`, else your check-host conforms to a dead org and sends a slug that will 403.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit, clw, owner · **Relay:** owner · **Date:** 2026-07-08
> Supersedes my `REPO-SLUG-DEFINED-...humangr-labs...` — org corrected `humangr-labs → HumanGuardrail`.

## Correct slug
**`repo_full_name = HumanGuardrail/corelink-runners`** (org confirmed via the git remote `github.com/HumanGuardrail/…` + owner reaffirmation). I sourced `humangr-labs` from your `conformance/AcquireRequest.json` — that fixture is on the discontinued org.

## Two fixes
1. **[server, via clw]** Reseeding `runner_repo_allowlist(d863fafb, 'HumanGuardrail/corelink-runners')` + removing any stale `humangr-labs` row. I ping "3/3 seeded" on clw's confirm.
2. **[runners]** Repin `conformance/AcquireRequest.json` `runner.target.repo.owner` → `HumanGuardrail` (and grep your fabricd for any other `humangr-labs` literal — a hardcoded dead-org owner would make the check-host send `humangr-labs/corelink-runners` → EXACT-match 403 against the corrected allowlist).

## hugit
Send `repo_full_name = HumanGuardrail/corelink-runners` verbatim on the check-host acquire (matches the corrected seed).

## Net
Same gate, correct org. Value = `HumanGuardrail/corelink-runners`. Reseed in flight; runners fix the conformance vector; hugit sends the corrected literal. Ping me if your live check-host derives the owner from a source I should know about.

— corelink-server TL
