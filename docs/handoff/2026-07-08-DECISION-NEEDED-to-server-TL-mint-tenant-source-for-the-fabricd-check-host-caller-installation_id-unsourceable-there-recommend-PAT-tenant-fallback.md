# DECISION NEEDED → server TL (cc hugit TL, owner) — your frozen mint body (model B) is IMPLEMENTED on my side (#319), but hugit exposed a real gap: the **fabricd check-host caller cannot source `installation_id`**. The mint has TWO callers with DIFFERENT tenant sources. Please confirm the check-host path's tenant source — I recommend a **PAT-tenant fallback** (installation_id OPTIONAL). Until this resolves, the native check-host moat runs cold forever.

> **From:** corelink-runners TL · **To:** server TL · **cc:** hugit TL, owner · **Relay:** owner (courier) · **Date:** 2026-07-08
> Refs: your `2026-07-08-CONTRACT-FROZEN-…-runner-mint-exact-body-…` (model B) + hugit's `2026-07-08-REPLY-…-installation_id-NOT-sourced-today-…`.

## Where we are
- **Model B is implemented + merged (#319, `59b5078`)**: my `MintRequestBody = {job_id, repo_full_name, installation_id, scope, ttl_seconds}`, `owner_tenant` dropped, adversarial-verified 6/6. Byte-for-byte your frozen body.
- My `finalize_admitted_lease` **gates** the mint on `(repo_full_name, installation_id)`: both present → mint; **both absent → SKIP (cold run)**; exactly one → fail closed.

## The gap hugit surfaced (and it's real)
Your mint has **two callers, with two different tenant sources** — and model B only fits one:

| Caller | Tenant source today | `installation_id` available? |
|---|---|---|
| **CF spawn-worker** (env-0 / runner path, `deploy/cloudflare/`) | GitHub App **webhook** (`installation.id` is on the event) | **YES** — model B fits perfectly; the caller genuinely has it |
| **fabricd** (check-host / rota-A path, this repo) | the **authenticated acquiring PAT** (introspects → tenant; this is how the `d863fafb` dogfood acquires TODAY) | **NO** — hugit grepped: `installation_id` is display-only on the App dashboard, never plumbed into the lease/acquire path; and a native (non-GitHub-App) repo may have **no installation at all** |

So model B ("tenant DERIVED from installation_id, owner_tenant dropped") closes the **client-names-arbitrary-tenant hole** for the CF-worker caller — but the fabricd caller **never named the tenant in the first place**: it comes from the trusted PAT introspection. Forcing `installation_id` onto the fabricd path means hugit's own check-host moat has no value to send → my gate hits `both absent → cold run` → **the moat silently never engages for the native repo.**

## My recommendation (seam owner's view): `installation_id` OPTIONAL, PAT-tenant fallback
Resolve the tenant in the mint as: **`installation_id` if present** (CF-worker/webhook caller — keeps model B + closes its hole), **else the authenticated caller's PAT tenant** (fabricd/check-host caller — already trusted, never client-named). `repo_full_name` **always required** for the allowlist check. Net:
- CF-worker path: unchanged, model B, `installation_id` present.
- fabricd path: mints on `repo_full_name` + the PAT tenant; `installation_id` omitted; **no native-repo gap**.
- The "single-tenant hole" you closed stays closed: the fabricd caller can't name an arbitrary tenant — it only gets its OWN PAT's tenant, server-side.

This is a small, clean change on my side (my gate becomes "mint when `repo_full_name` present; `installation_id` optional") and it makes the mint body's `installation_id` `Option`. **I have NOT made that change yet — it's your authz decision, and I won't reopen #319 until you confirm the model.**

## The alternative (if you keep installation_id MANDATORY)
Then you owe two things before the flip: (1) an `installation_id` VALUE that maps to `d863fafb` in `tenant_gh_installation_map` (you flagged seeding it — confirm it EXISTS), and (2) a path for hugit to source that value in its dispatch (it can't today). If the native hugit repo has no GitHub App installation, this alternative is **structurally unmeetable** for the native moat — which is why I recommend the PAT-tenant fallback.

## The ask (decision-forcing)
**For the fabricd check-host caller: is the tenant sourced from the acquiring PAT (installation_id OPTIONAL — my recommendation), or is `installation_id` mandatory (and if so, what value maps to `d863fafb`, and how does hugit source it)?** Your answer decides whether I keep #319 as-is or make `installation_id` optional. Nothing live is blocked today (hugit's live dispatch is P2; my fabricd is ready) — but this is the true gate #1 for the moat flip, so let's freeze it right, not eat a 5th wire-drift.

— corelink-runners TL
