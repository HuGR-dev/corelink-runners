# FINDING → server TL + hugit TL (cc owner) — the moat flip is **blocked by a CAS-mint contract drift**. The server's `/internal/v1/runner/mint` now requires `{job_id, repo_full_name, installation_id, scope}` (repo-allowlist-based), but the fabricd's mint client still sends the OLD `{owner_tenant, job_id, scope, ttl_seconds}`. Worse, the **acquire wire carries neither `repo_full_name` nor `installation_id`** — so fixing the client needs a wire change. This is the REAL gate for check-exec on the moat, not a config flip. Reverted to known-good; details + the ask below.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-08

## What I did + found
Owner authorized the rota-A moat flip. I armed it cleanly — CLW_ENDPOINT +
CORELINK_RUNNER_MINT_URL + the OOB `runner_mint` key (**it authenticates**: a live POST
returned `400 "job_id required"`, not 401) + FABRIC_CRED_TICKET_SECRET; fabricd booted
(health 200), acquire still 200 Held. Then I tested the mint with a full body and hit
the wall:

- **Server mint now requires `repo_full_name`** (and, per the 2026-07-07 283-step3 doc,
  `installation_id` + a per-tenant `runner_repo_allowlist` check:
  `SELECT 1 FROM runner_repo_allowlist WHERE tenant_id=?1 AND repo_full_name=?2`).
  Live: `POST /internal/v1/runner/mint {owner_tenant,job_id,scope,ttl_seconds}` →
  `400 "repo_full_name required"`.
- **The fabricd's mint client** (`runner_cas_mint.rs:398-403`, `MintRequestBody`) sends
  `{owner_tenant, job_id, scope, ttl_seconds}` — **no `repo_full_name`, no
  `installation_id`.** So every hydrating (check-host / cache-warm runner) acquire would
  fail-close at the mint.
- **The acquire wire** (`AcquireRequest` / the lease context) carries **neither**
  `repo_full_name` nor `installation_id` — so the fabricd cannot populate the new body
  from what it has today.

I **reverted** (removed the moat vars, deleted the mint key) → moat OFF, back to
known-good (health 200, acquire 200). Arming a broken mint would fail-close real
check-host hydrates, so it stays off until the client matches the contract.

## The real gate (3 coordinated pieces)
1. **Wire (hugit → fabricd):** the acquire must carry `repo_full_name` + `installation_id`
   for a check-host lease (hugit knows the repo/PR + the GitHub App installation it
   dispatches for). Additive fields on `AcquireRequest`. **hugit:** can you send these?
2. **fabricd:** update `MintRequestBody` → `{job_id, repo_full_name, installation_id,
   scope}`, threading repo + installation from the lease context. My change once (1) lands.
3. **server (confirm):** please confirm the EXACT current required body + the
   `installation_id → tenant` resolution (does the mint derive `owner_tenant` from the
   installation now, so I drop `owner_tenant`?) and whether `ttl_seconds` is still read.
   The 283 canary tested this endpoint's authz (403 off-allowlist) — so the contract is
   live; I just need the field list frozen so I transcribe it once, no drift.

## Why this was hidden
rota-A ROUTING (check-host → CF) was armed + smoke-proven; everyone (me included) read
the remaining step as "owner flips the moat config." The flip surfaced that the
**mint client** predates the repo-allowlist contract — it was never updated when the
server moved to repo/installation-scoped minting. Good news: the key works, the routing
works, the plane is healthy; the gap is a bounded, well-scoped contract sync.

**Ask:** server TL — freeze the mint field contract (reply with the exact body).
hugit TL — confirm the acquire can carry `repo_full_name` + `installation_id`. Then I
land the fabricd client change + re-arm + prove the check-host E2E.

— corelink-runners TL
