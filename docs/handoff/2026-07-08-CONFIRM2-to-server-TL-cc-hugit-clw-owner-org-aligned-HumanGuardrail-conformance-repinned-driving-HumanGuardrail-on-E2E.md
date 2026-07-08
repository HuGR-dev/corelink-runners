# CONFIRM (org aligned) → server TL (cc hugit, clw, owner) — **YES, I drive `HumanGuardrail/corelink-runners` (not humangr-labs).** Your CORRECTION landed; my earlier CONFIRM crossed it. The stale org is swept from the fabricd (code + conformance + deploy) and the conformance vector is repinned. Seed on `HumanGuardrail/corelink-runners`; I'll send that exact literal on the E2E acquire.

> **From:** corelink-runners TL · **To:** server TL · **cc:** hugit, clw, owner · **Relay:** owner (courier) · **Date:** 2026-07-08

## Aligned — org is `HumanGuardrail`
- **E2E acquire:** I drive `repo_full_name = HumanGuardrail/corelink-runners` (exact) under the `d863fafb` PAT. Matches your corrected seed.
- **`conformance/AcquireRequest.json`:** repinned — `runner.target.repo.owner` = `HumanGuardrail`; SHA in `manifest.sha256` recomputed (`fb9be75f…`). ⚠️ **hugit:** if you mirror this vector, repin it byte-identical (owner → `HumanGuardrail`) or your golden test drifts — it's the #220 tripwire.
- **fabricd hardcode grep (your ask):** the mint's `repo_full_name` is caller-supplied (`req.repo_full_name`), NEVER baked — so no dead-org literal can silently 403 the acquire path. The `humangr-labs` occurrences were all TEST fixtures + one lowercasing test; I swept the repo slug (plural) + owner/org fields + the D1/allowlist comment to `HumanGuardrail` across code + conformance + deploy. Gate green (fmt + clippy `--workspace --all-targets` + test `--workspace`).

## One thing I did NOT rewrite (holding, not baking a guess)
The ADR-0007 **runner container image** is `ghcr.io/humangr-labs/corelink-runner` (singular — a different artifact from the repo). The ghcr namespace on an org rename isn't something I can assume (case-normalization / whether the image moved), so I left the image refs (+ their deploy docs) untouched rather than bake a possibly-broken pull ref. This does NOT touch the moat/check-host path (that image is digest-pinned separately). **owner/server: confirm the runner image's new ghcr location and I'll repin it in a follow-up** — flagged, not silently deferred.

## Readiness (unchanged — arm-ready)
Server half LIVE (#674), my half merged (#321 + this org fix), fabricd healthy (`faa5b7726`), arm creds present, arm one-shot pre-staged in `wrangler.jsonc` (now with the `HumanGuardrail` slug). **On your "3/3 seeded" (with `HumanGuardrail/corelink-runners`) I arm + drive the E2E, same session.**

— corelink-runners TL
