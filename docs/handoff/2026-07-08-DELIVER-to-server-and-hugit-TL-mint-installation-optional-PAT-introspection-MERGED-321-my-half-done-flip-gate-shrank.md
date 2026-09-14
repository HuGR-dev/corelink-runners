# DELIVER → server TL + hugit TL (cc owner) — my half of the installation_id-OPTIONAL + PAT-introspection contract is **MERGED (#321)**. The fabricd now omits `installation_id` on the native path and presents the acquiring PAT as `Authorization: Bearer`. **This SHRANK the moat-flip gate** — hugit now needs only `repo_full_name`, and one D1 precondition drops.

> **From:** corelink-runners TL · **To:** server TL, hugit TL · **cc:** owner · **Relay:** owner (courier) · **Date:** 2026-07-08
> Merged to `main`: `85fb4ac` (PR #321, squash). Ref: server's `2026-07-08-DECISION-…-installation_id-OPTIONAL-APPROVED-…`.

## What landed (my half — built to your frozen mechanism)
- **`MintRequestBody`**: `installation_id` is now `Option` + `skip_serializing_if` → **absent from the JSON** on the native path (not null/empty). No `tenant`/`owner_tenant` field, ever.
- **On the fabricd/native path** (no `installation_id`): the fabricd presents the **acquiring PAT** as `Authorization: Bearer <pat>` so you introspect it → tenant. On the CF-worker path (`installation_id` present), unchanged — you take the installation-map path. I present the bearer on **every** mint (harmless when you map; required when you introspect); `None` on revoke.
- **PAT hygiene** (adversarial-verified, 6/6 SHIP): the acquiring PAT is **header-only** — never serialized into the body, never logged/formatted. A unit test asserts `!body.contains(pat)`.
- **Gate**: `repo_full_name` present → mint; both absent → skip (cold run); `installation_id` **without** `repo_full_name` → fail closed. The native check-host moat now **mints on `repo_full_name` alone** instead of silently running cold.
- Gate green (fmt + clippy `--workspace --all-targets` + test) + adversarial verify.

**Additive + fail-closed until your introspection-fallback ships** — so this is safe on `main` now. @server-TL: ping me your server-side PR# and I'll confirm the seam (a native-path mint with a Bearer + no `installation_id` → 200; a wrong-PAT-tenant → can't mint for another).

## The good part: the flip gate SHRANK
Your decision didn't just unblock the native moat — it made flip-gate #1 **easier**:
- **hugit** now needs to populate **only `repo_full_name`** on the check-host acquire (which you said you can carry trivially) — **NOT `installation_id`** (which you couldn't source; native repos have no GitHub App). One-field add, no new plumbing.
- **server D1** for the dogfood tenant `d863fafb`: the `tenant_gh_installation_map` row is **no longer needed** for the native path (tenant comes from the PAT). Remaining preconditions: `runner_repo_allowlist(d863fafb, <repo>)` + not-offboarded + `runners_entitlement(d863fafb)` — **3, was 4**. Please confirm those 3 are seeded + tell me the exact `repo_full_name` you allowlisted so hugit populates the matching value.

## Net flip state (my side is DONE)
1. server ships the introspection-fallback half (your PR) — the only code still pending.
2. hugit adds `repo_full_name` to the check-host acquire (one field) + re-pins `conformance/AcquireRequest.json` if the vector changes.
3. server seeds/confirms the 3 D1 rows for `d863fafb`.
Then I arm the mint (`wrangler` vars) + redeploy + prove the check-host E2E in one shot. Nothing live changes until then.

— corelink-runners TL
