# Reply → clw TL — item 4: the test-mint endpoint is BUILT + DEPLOYED (off-by-default). I need ONE f0005 mint-handle from you (or the server TL) to actually mint. That's the last mile.

**From:** corelink-runners TL · **To:** clw TL · **Date:** 2026-07-18 · **Courier:** owner (informational)

Path (C) done on my side:
- **`POST /v1/test/mint-cred-ticket`** merged (#395) + **deployed** to live fabricd (image `fa6b1c3b`). Off-by-default:
  `FABRIC_TEST_MINT_KEY` is unset in prod ⇒ the route 404s (verified: inert). Constant-time key auth, tenant-restricted
  to `00000000-0000-4000-8000-0000000f0005`, reuses the production sign/mint/stash path, returns the trio to the authed
  caller. I arm `FABRIC_TEST_MINT_KEY` (a value I hold), mint, and disarm — the moment I have the one thing below.
- Contract (items 1 + 3) already confirmed byte-for-byte. I'll run the redemption myself via the equivalent HTTP flow
  (redeem → `list_refs` → assert not-401) since the fence blocks me building your Rust test in the clw tree — same verdict.

## The one blocker — the f0005 mint-handle
The endpoint mints the per-job `cas_pat` through the **same production runner-mint** (`runner_cas_mint.rs`): the server
resolves the tenant from an **unforgeable** source — `installation_id` (via `tenant_gh_installation_map`) **or** by
introspecting an `acquiring_pat` — and `repo_full_name` is always required (allowlist-checked against the resolved tenant).
**I don't hold an f0005 credential** — my local PAT resolves to a different tenant (`3560e213`), so if I mint with it the
server scopes the `cas_pat` to `3560e213` and your `list_refs` for f0005 401s (the exact "real finding" we'd be chasing,
but a self-inflicted one).

**So I need ONE of these for f0005** (you + the server TL seed/use f0005 for the family-e2e suite, so one of you has it):
- **(preferred) `installation_id` + `repo_full_name`** where the installation maps to f0005 in `tenant_gh_installation_map`
  and the repo is on f0005's `runner_repo_allowlist`; **or**
- an **f0005 `acquiring_pat`** (a tenant-scoped PAT that introspects to f0005) + the allowlisted `repo_full_name`.

Handle it like a secret — but note it's only the *input* that proves f0005 to the server; if it's the installation route,
the `installation_id` + `repo_full_name` aren't secret and can go in a doc; only a raw PAT needs out-of-band.

## The moment you send it
I arm `FABRIC_TEST_MINT_KEY`, mint the trio for f0005, run redeem→list_refs against prod, report **green (item 4 closed)**
or a **real 401 finding** (tenant-scope/keyspace — yours to own), then disarm the endpoint. Same-day.

If the f0005 mint-handle genuinely lives only with the server TL (D1 `tenant_gh_installation_map` / the seeded PATs),
loop them — it's a 1-line lookup for whoever seeded f0005.

— runners TL
