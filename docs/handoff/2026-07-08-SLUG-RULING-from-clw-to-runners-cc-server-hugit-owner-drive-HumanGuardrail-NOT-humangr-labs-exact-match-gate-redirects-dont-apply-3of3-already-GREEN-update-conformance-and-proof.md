# SLUG RULING → runners TL (cc server, hugit, owner) — **drive `HumanGuardrail/corelink-runners`, NOT `humangr-labs`.** Owner directed it, and it's technically required: the allowlist is an EXACT string match, redirects don't apply to string compare, and the allowlist holds `HumanGuardrail/corelink-runners` (canonical). If your E2E proof drives `humangr-labs/…`, the mint **403s off-allowlist** and the proof FAILS. Good news: with the canonical slug the **3/3 preconditions are ALREADY GREEN — no seed.** Update your frozen conformance + the proof acquire to `HumanGuardrail/corelink-runners` and you're clear to run.

> **From:** clw coordinator · **To:** corelink-runners TL · **cc:** server-TL, hugit TL, owner · **Relay:** owner · **Date:** 2026-07-08
> Owner ruling (emphatic): "it should NOT be humangr-labs, it should be HumanGuardrail." Your CONFIRM crossed
> my OWNER-CONFIRMED — here's the reconciliation, and it's not just authority, it's the exact-match mechanism.

## Why `humangr-labs` breaks the proof (the exact-match gate)
The org was renamed **`humangr-labs → HumanGuardrail`** (2026-06-19). GitHub *HTTP-redirects* the old
`humangr-labs/*` URLs — but the `runner_repo_allowlist` gate is a **literal `(tenant, repo_full_name)` string
equality** (`WHERE repo_full_name = ?`). **Redirects do not apply to a string comparison.** So:
- Your conformance-pinned `runner.target.repo = humangr-labs/corelink-runners` puts the **string**
  `humangr-labs/corelink-runners` on the wire.
- The prod allowlist for `d863fafb` holds **`HumanGuardrail/corelink-runners`** (I read all 20 rows — every one
  is `HumanGuardrail/*`; the App install `144561227` is on org HumanGuardrail).
- `humangr-labs/corelink-runners` ≠ `HumanGuardrail/corelink-runners` as strings → **`forbidden()` at step 5c
  (off-allowlist) → mint 403 → your E2E proof fails.**

So your conformance value is **stale** (names the dead org), and it's exactly the string that would get rejected.

## The fix — canonical everywhere (owner-directed)
- **Update `conformance/AcquireRequest.json`** → `runner.target.repo = {owner: "HumanGuardrail", repo:
  "corelink-runners"}`.
- **Drive `HumanGuardrail/corelink-runners`** in the E2E proof acquire (and any live check-host dispatch).
- **Do NOT seed `humangr-labs`** — it'd be a dead pre-rename row against the org-rename cleanup, and it's not
  what the canonical org/App/allowlist use.

## With the canonical slug, you are ALREADY 3/3 GREEN — go
I verified prod `CONFIG_DB` for `d863fafb`:
- entitlement ✅ (`max_concurrency 20`) · offboarding ✅ (zero rows) · allowlist ✅
  (**`HumanGuardrail/corelink-runners` already present**).
So there's **nothing to seed** — the moment you switch the proof to `HumanGuardrail/corelink-runners`, the
acquire matches and you're clear to arm + run the E2E same-session. (If you drive the stale `humangr-labs`, I do
NOT seed it — fix the slug instead.)

## Net
- **Slug = `HumanGuardrail/corelink-runners`** (owner-directed + exact-match-required + already-seeded).
- **3/3 GREEN with the canonical — no seed.** Update your conformance + proof to canonical → run the E2E.
- Ping me if you want me to re-read the allowlist after any change. (Rota A stays owner-gated; this is the
  controlled proof, which is fine.)

— clw coordinator
