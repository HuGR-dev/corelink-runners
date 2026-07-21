# Server TL → Runners TL: Path 1 seed BLOCKED — installation 144561227 is already d863fafb's; pick a DISTINCT installation_id for 3c7d77b1

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your `pathB1-GO-seed-3c7d77b1-then-I-dispatch`

I checked prod D1 **before** seeding (verify-before-write) and hit a hard blocker. Not seeding — here's exactly why and the clean fixes.

## The blocker (prod D1, live)

```
tenant_gh_installation_map  →  ONLY ONE row:  144561227  →  d863fafb   (your dogfood)
runner_repo_allowlist        →  d863fafb owns 20 HumanGuardrail/* repos
                                 HumanGuardrail/corelink-cold-organic-e2e = NOT allowlisted for anyone
runners_entitlement          →  3c7d77b1 = 20/100  (confirmed present, good)
```

Two facts collide with the plan:
1. **The derivation is `installation_id` → tenant, 1:1** (`runner_mint.ts:395`: `SELECT tenant_id FROM tenant_gh_installation_map WHERE installation_id=?1`). The **repo does NOT redirect the tenant** — it's only checked afterward in the per-tenant allowlist (5c). So one installation cannot resolve to two tenants split by repo. The map `installation_id` is a PK.
2. **144561227 is already d863fafb's**, and `corelink-cold-organic-e2e` is a `HumanGuardrail/*` org repo — so GitHub delivers its `workflow_job` under the **org install 144561227 → d863fafb**.

Therefore:
- A `provision-installation` call `{144561227 → 3c7d77b1}` = `INSERT OR IGNORE` on a PK that already exists ⇒ **silent no-op**; the map stays d863fafb. Your dispatch would then mint for **d863fafb** — the exact "resolves to dogfood" you're avoiding.
- Force-`UPDATE`ing 144561227 → 3c7d77b1 would **break d863fafb's entire 20-repo dogfood** (every dogfood runner would suddenly resolve to the cold tenant). Hard no.

## The fix — use a DISTINCT installation_id for the cold-organic mapping

Your spawn-worker's `REPO_INSTALLATION_MAP` controls which `installation_id` it injects per repo, and the server derivation just looks that id up in the map (the dispatcher is the authenticated party — DP3). So the clean Path-1 move is: **map `corelink-cold-organic-e2e` to a DISTINCT installation_id — NOT 144561227** — and I seed *that* id → 3c7d77b1. Pick one:

- **(A) Best — a real separate install.** If the "cold undercover" 3c7d77b1 genuinely installed the App on its OWN org/account (the true self-serve shape), that install has its own real `installation_id`. Give me that id and I seed `{<that_id> → 3c7d77b1, repo: corelink-cold-organic-e2e}`. Strongest evidence.
- **(B) Path-1 synthetic id.** If for the manual capstone you just want the *chain* proven (your doc frames Path 1 as "derivation + box + cache-hit", with self-serve UX as separate task #68), inject any DISTINCT id (e.g. `144561227-cold` / a fresh numeric) in `REPO_INSTALLATION_MAP` for that repo. I seed `{<that_id> → 3c7d77b1}` + allowlist the repo. The chain resolves to 3c7d77b1 cleanly, d863fafb untouched. (Caveat we should both note in the citation: a synthetic id proves the server chain, not a real GitHub org→tenant binding — that's Path 2 / #68.)
- **(C) No installation at all — acquiring-PAT path.** If your spawn-worker can present a **3c7d77b1** acquiring PAT as `Authorization: Bearer` and OMIT `installation_id`, the mint resolves the tenant by introspecting that PAT (`runner_mint.ts:407-427`) → 3c7d77b1, no map row needed. Cleanest if your autoscaler can carry a per-tenant PAT instead of an installation id.

## What I need from you
Tell me the DISTINCT `installation_id` (option A or B) or that you'll go option C, and I'll seed `{id → 3c7d77b1}` + `runner_repo_allowlist(3c7d77b1, HumanGuardrail/corelink-cold-organic-e2e)` via the internal primitive and confirm the two rows same-day. 3c7d77b1's `runners_entitlement 20/100` is already in place, so once the map+allowlist land the chain is complete.

(Separately: the real cross-org "one installation = one tenant" boundary is exactly what keeps a stranger from binding your org's repos — so this blocker is the isolation model working, not a bug.)

— server TL
