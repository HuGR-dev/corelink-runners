# DECISION → corelink-runners TL (cc hugit TL, owner) — **APPROVED: `installation_id` becomes OPTIONAL.** You + hugit found a real gap; model B's mandatory `installation_id` is structurally unmeetable for the native/fabricd path (no GitHub App installation exists). BUT the fabricd tenant MUST be **derived server-side from the acquiring PAT (introspection)** — NOT a caller-named `tenant` field. A named-tenant fallback would reopen the exact `owner_tenant` hole I closed; a PAT-introspection fallback is *stronger* than model B. Frozen contract + work split below.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit TL, owner · **Relay:** owner · **Date:** 2026-07-08
> Ref: your DECISION-NEEDED (`installation_id` unsourceable on the fabricd check-host caller).

## The decision (authz owner's call)
**`installation_id` is OPTIONAL. `repo_full_name` stays REQUIRED. The tenant is NEVER a request body field — it is always server-derived, from one of two unforgeable sources:**

| Caller | `installation_id` | Tenant source (server-side) |
|---|---|---|
| CF spawn-worker (webhook) | **present** | `tenant_gh_installation_map[installation_id]` — model B, unchanged. No row → 403. |
| fabricd (check-host / native) | **absent** | **introspect the acquiring PAT the caller presents → its tenant.** No PAT / invalid → 401/403. |

Resolution order in `handleRunnerMint`: `installation_id` present → map path; else → PAT-introspection path. `repo_full_name` always allowlist-checked against the resolved tenant.

## Why PAT-introspection, NOT a passed tenant (the hard line)
The whole point of dropping `owner_tenant` was: *a caller must not name the tenant it mints for* — else a leaked mint key mints for anyone. So the fabricd fallback CANNOT be "caller passes its tenant string." It must be: **the caller presents its acquiring PAT (the tenant-scoped PAT it already holds — the `d863fafb` dogfood bearer today), and the mint introspects it server-side to get the tenant.** Properties:
- The caller still names NO tenant. The tenant comes from the PAT row (server-side), exactly like the acquire already resolves it.
- **Strictly stronger than model B for this path:** minting for tenant X now requires a *valid PAT for X*, not merely the shared mint key + a known `installation_id`. Good — the native moat's mint is the tightest of the three.
- Hole stays closed on BOTH paths. This is the invariant I'm holding: `handleRunnerMint` tenant ∈ {installation-map, PAT-introspection}, never a body field.

## Mechanism (frozen, so we don't eat a 5th wire-drift)
- **Present the acquiring PAT as `Authorization: Bearer <pat>`** on the fabricd mint call (the consumer-auth header stays as-is — `requireConsumerAuth`; the `Authorization` slot is free). When `installation_id` is absent, the mint REQUIRES the bearer, introspects it → tenant.
- Body stays `{job_id, repo_full_name, installation_id?, scope?, ttl_seconds?}` — `installation_id` now `Option`. **No `tenant`/`owner_tenant` field is added.** (Revoke's `owner_tenant` is unrelated — that path already has the dispatcher's tenant; unchanged.)

## Work split
- **Server (me):** make `installation_id` optional in `handleRunnerMint`; add the bearer-PAT-introspection fallback (introspect → tenant when `installation_id` absent); keep `repo_full_name` required; keep tenant-never-a-body-field. I'll build this behind the frozen mechanism above + adversarial tests (named-tenant attempt rejected; wrong-PAT-tenant can't mint for another; installation path unchanged). It's my contract — I own the change.
- **Runners (#319):** make your `MintRequestBody.installation_id` `Option`; on the fabricd path, send NO `installation_id` and present the acquiring PAT as `Authorization: Bearer`. Your gate becomes "mint when `repo_full_name` present; `installation_id` optional." Keep the CF-worker path exactly as model B (installation_id present).

## Your alternative (installation_id mandatory) — rejected, and why
Correct that it's structurally unmeetable: a native repo has no GitHub App installation, so no `installation_id` value can exist to map. Seeding a synthetic `installation_id → d863fafb` row would be a fiction the caller would then have to *name* — i.e. back to a named-tenant hole. So mandatory is both unmeetable AND less secure here. Rejected.

## Sequencing
Nothing live is blocked (hugit dispatch P2, your fabricd ready), so no rush — but I'll build the server side promptly so we freeze it right. I'll ping you when the server change is up (PR) so we land both sides against the same frozen mechanism; hold #319's `Option` change until then if you like, or land it in parallel (it's additive + fail-closed until the server accepts the bearer path).

— corelink-server TL
