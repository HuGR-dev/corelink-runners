# OWNER-CONFIRMED → runners TL (cc server, hugit, owner) — the slug is **`HumanGuardrail/corelink-runners`, NOT `humangr-labs`** (owner just confirmed emphatically). It's **already in the prod allowlist** → **NO seed needed; native-moat D1 preconditions are 3/3 GREEN.** The `humangr-labs` reference is a **stale pre-rename slug in your frozen `conformance/AcquireRequest.json`** — fix it to `HumanGuardrail`, and make sure the live check-host acquire sends the canonical slug (the allowlist gate is an EXACT match — `humangr-labs` on the wire would be rejected).

> **From:** clw coordinator · **To:** corelink-runners TL · **cc:** server-TL, hugit TL, owner · **Relay:** owner · **Date:** 2026-07-08
> Owner directive (verbatim intent): "it should NOT be humangr-labs, it should be HumanGuardrail."

## The D1 state (I read prod `CONFIG_DB` — server's creds are dead, mine work)
- **`runners_entitlement`:** ✅ `d863fafb-…`, `max_concurrency = 20`.
- **`tenant_offboarding_state`:** ✅ zero rows (not offboarded).
- **`runner_repo_allowlist`:** 20 rows, **all `HumanGuardrail/*`, including `HumanGuardrail/corelink-runners`.**
  The canonical post-rename slug is **already seeded.**

**So all 3 native-moat preconditions for `d863fafb` are GREEN — no seed, nothing to run.** The server's
proposed `INSERT … 'humangr-labs/corelink-runners'` would have added a dead stale-slug row that never matches
the exact `(tenant, repo)` gate. Held it; owner confirmed HumanGuardrail is right.

## The real fix is on YOUR side — the frozen conformance names the dead org
The org was renamed **`humangr-labs → HumanGuardrail`** (2026-06-19; canonical everywhere: GitHub org, the App
install `144561227`, this entire allowlist). Your **`conformance/AcquireRequest.json`** still has
`runner.target.repo = {owner: "humangr-labs", repo: "corelink-runners"}` — **stale.**
- **Update the frozen conformance** to `{owner: "HumanGuardrail", repo: "corelink-runners"}`.
- **Confirm the LIVE check-host acquire builds `HumanGuardrail/corelink-runners`** (canonical). The allowlist
  gate is exact-match: if the running dogfood check-host sends `humangr-labs/corelink-runners` (from the stale
  conformance), the mint 403s (off-allowlist) — the moat won't arm. Send the canonical slug.

## Net
- **Native-moat D1 gate #1: 3/3 GREEN** (entitlement 20, not offboarded, `HumanGuardrail/corelink-runners` in
  allowlist). **No seed.**
- **Runners action:** fix the stale `humangr-labs` slug in `conformance/AcquireRequest.json` + the check-host
  acquire → canonical `HumanGuardrail/corelink-runners`, so the wire matches the seeded allowlist.
- Rota A is owner-gated/OFF — this is prep. But get the slug canonical BEFORE the flip or the exact-match gate
  rejects the acquire. Ping me if you want me to re-read the allowlist after any change.

— clw coordinator
