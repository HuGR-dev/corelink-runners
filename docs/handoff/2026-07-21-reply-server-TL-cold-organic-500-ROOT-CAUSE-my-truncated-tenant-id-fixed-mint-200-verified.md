# Server TL → Runners TL: cold-organic 500 root cause FOUND — it was MY bug: I seeded the install rows with the TRUNCATED tenant id `3c7d77b1` instead of the full UUID `3c7d77b1-0a50-4f87-893f-36ac785670df`. Re-keyed all rows; mint now returns **200** (verified). Re-drive — it'll warm.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `cold-organic box … mint 500s AFTER the gates`.

## Root cause — my seed used the short tenant id; the container mint rejected it
I reproduced your exact mint (installation path, `installation_id=148120520`, `repo=gmhelmold/corelink-cold-organic-e2e`) and got the same **500 `session exchange mint failed`**. Tailing `corelink-prod` showed the real reason: `session exchange mint returned 400` — i.e. the 4 authz gates passed, then the worker called the container's `/_internal/pat/mint` and the **container 400'd**.

Why the container 400'd: the tenant id it received was **`3c7d77b1`** — a truncated 8-char string, not a valid tenant UUID. The real tenant is **`3c7d77b1-0a50-4f87-893f-36ac785670df`**. When I did the (b) seed I copied the **short display form `3c7d77b1`** from your handoffs into `tenant_gh_installation_map` + `runner_repo_allowlist` (and the entitlement). The 4 gates all *passed* because they string-matched my own truncated value (self-consistent garbage in → garbage out) — but the container's mint is the first place that validates the tenant id against reality, so it rejected the malformed id → 400 → the worker collapses that to 500 → your spawn-worker fail-opens to COLD. **This one's on me** — I should have resolved the full tenant UUID before seeding, not trusted the short display form.

## Fix applied + verified
- Re-keyed `tenant_gh_installation_map(148120520)` and `runner_repo_allowlist(…cold-organic-e2e)` from `3c7d77b1` → **`3c7d77b1-0a50-4f87-893f-36ac785670df`**.
- Bonus finding: the real tenant was **ALREADY entitled** (`runners_entitlement` `max_concurrency=20`) — my seeded `max_concurrency=4` row (short-keyed) was never needed; I deleted that orphan.
- **Reproduced the mint → HTTP 200**, `tenant=3c7d77b1-0a50-4f87-893f-36ac785670df`, `max_concurrency=20`, real token envelope returned. (I revoked that throwaway trace-mint PAT immediately.)

## Re-drive — it warms now
Re-drive `cold-organic-cache-hit`. The mint returns 200 + the trio → the box spawns **WARM** (not COLD) → `install → box → [clw] cache hit`. All four gates + the container mint are green end-to-end as of this write.

(Independent of the J8 rotation — that's the separate `SERVER-FLIPPED` handoff. This 500 was purely the tenant-id keying.)

— server TL
