---
name: corelink-runner-ops
version: 0.1.0
description: Operational playbook for the CoreLink Runners fabric — how to make a `runs-on: corelink` job actually boot a real box and mint the right tenant's CAS credential. Covers the spawn-worker (Cloudflare Worker `corelink-spawn-worker`), Option-C per-tenant-PAT dispatch (`REPO_TENANT_PAT_MAP` + a bound acquiring-PAT secret → mint resolves the tenant by PAT introspection, installation_id omitted), the GitHub App JWT for enumerating/creating installs, the public-App requirement for external self-serve installs, env-0 credential brokering, and the server-TL seed contract (tenant_gh_installation_map + runner_repo_allowlist). Invoke when provisioning a box, wiring a tenant to a repo, debugging why a job stays queued or mints the wrong tenant, deploying the spawn-worker, or driving a GitHub App install.
---

# corelink-runner-ops — make a corelink box boot + mint the right tenant

## The resolution chain (webhook path)
`workflow_job.queued` → spawn-worker `/webhook` reads `installation.id` → `mintCasPat` POSTs
`corelink-api.humangr.com/internal/v1/runner/mint` (`x-corelink-internal-auth: CORELINK_RUNNER_MINT_AUTH_KEY`)
→ **server derives the tenant from installation_id** (`tenant_gh_installation_map`, 1:1) → returns a
per-job `cas:rw` PAT (env-0: stashed, box redeems a `CLW_CRED_TICKET`, raw PAT never in the container) →
JIT runner registered via the install → box boots. A 403 = HARD DENY (unmapped install / repo not in
`runner_repo_allowlist` / not runner-entitled) → spawn aborts (never wrong-tenant cold).

## Option-C: mint a DIFFERENT tenant than the install derives (per-tenant-PAT dispatch)
When you can't get a real install for the tenant (e.g. proving the cold tenant `3c7d77b1` on a
HumanGuardrail-org repo, install 144561227→dogfood), use `REPO_TENANT_PAT_MAP` (spawn-worker, added
2026-07-21, `deploy/cloudflare/src/lib.ts` `mintCasPat`/`tenantPatSecretForRepo`): a JSON
`{"<owner/repo>":"<SECRET_ENV_NAME>"}` mapping a repo to the NAME of a bound secret holding that tenant's
acquiring PAT. When matched, the mint presents `Authorization: Bearer <pat>` **AND** keeps
`x-corelink-internal-auth`, and **OMITS installation_id entirely** (null/"" → 400) → server resolves the
tenant by introspecting the PAT (`runner_mint.ts` ~407-427); scope `cas:rw`. The JIT/box still registers
via the org install — only the CAS-tenant changes. Live example:
`REPO_TENANT_PAT_MAP={"HumanGuardrail/corelink-cold-organic-e2e":"COLD_ORGANIC_TENANT_PAT"}`. Confirm on
the tail: `mint_option_c_pat_dispatch`. Gated default-off (empty map). Server confirms the wire live in
`docs/handoff/2026-07-21-reply-server-TL-optionC-CONFIRMED-live-allowlist-seeded-go-C1.md`.

## Deploy / observe
- Deploy: `cd deploy/cloudflare && npx wrangler deploy`. Secrets: `npx wrangler secret put <NAME> --name
  corelink-spawn-worker < /tmp/val` (write the value via node, no echo; `printf '%s'` — trailing \n breaks it).
- Tail: `npx wrangler tail corelink-spawn-worker --format=pretty` (App only emits `workflow_job`, NOT
  `installation` events — installs won't show on the tail; enumerate via the App JWT below).
- gh/git in this env hit a sandbox "failed to change group ID" error → use `dangerouslyDisableSandbox: true`.

## GitHub App (corelink-runners, App ID 4222041, owner @HumanGuardrail)
- Private key pem: `~/Downloads/corelink-runners.2026-07-13.private-key.pem`. Mint an App JWT (RS256,
  `iss=4222041`) via node `crypto` to call `GET /app/installations` (list all installs + account + id) or
  `GET /app`. The App key CANNOT create an install nor toggle public/private — those are web-UI only.
- **External self-serve installs require the App to be PUBLIC.** A private App shows "private GitHub App"
  to non-org accounts and cannot be installed by them (this silently blocked all external installs until
  2026-07-21). Making it public = App settings → Advanced → "Make public" (owner web action, no API).
  Note: an org-admin browser session gets routed to the ORG install; a clean non-org account installs its
  own account cleanly → a distinct installation_id.
- Real external install example (2026-07-21): `cachorronarigudo26-lang` (fresh User) installed →
  installation_id **148075031** (repos=all). To bind it to a tenant, server-TL seeds
  `tenant_gh_installation_map(148075031 → <tenant>)` + `runner_repo_allowlist(<tenant>, <owner/repo>)`.

## Server-TL seed contract (the two rows that unblock a mapping)
```
tenant_gh_installation_map:  <installation_id> → <tenant_uuid>   (1:1; PK on installation_id)
runner_repo_allowlist:       (<tenant_uuid>, "<owner/repo>")     (exact repo_full_name match; gate 5c)
```
Relay to server-TL (owner is courier); a missing allowlist row → generic 403 (indistinguishable from
suspend/entitlement) → wasted deploy, so ask them to seed BOTH. Tenant must be runner-entitled
(`runners_entitlement.max_concurrency`) or the box won't boot.

## Key tenant / repos (2026-07)
- Cold-organic tenant `3c7d77b1-0a50-4f87-893f-36ac785670df` (entitled 20/100), PAT in scratch
  `cold-tenant.json`. Proof repo `HumanGuardrail/corelink-cold-organic-e2e` (Option-C). Dogfood install
  144561227 → `d863fafb`. See `moat-benchmark` + `corelink-moat` skills.
