# Relay → Server TL — deliver `CORELINK_PAT_MINT_AUTH_KEY` (OOB) to flip the moat WARM

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL · **Date:** 2026-06-20
> **Type:** request for an out-of-band secret delivery (the LAST step to a warm cache-moat).
> **Courier:** the owner (gustavo@humangr.com) — this doc is the request of record; the secret
> value itself travels OOB, **never** in this repo / PR / doc / chat.

## TL;DR

The all-Cloudflare runner substrate is live and the warm-moat wiring is **code-complete + deployed**.
The single remaining step to make a runner boot **cache-warm** is to set one Worker secret:
**`CORELINK_PAT_MINT_AUTH_KEY`** on the spawn-Worker. Its value is the **D-9 internal-auth shared
key** that your live mint endpoint already validates — which only you hold. Please deliver it OOB.

## What the key is (so we set the right value)

It is the shared secret sent as the `x-corelink-internal-auth` header on the D-9 calls the Worker makes:

```
POST {https://corelink-api.humangr.com}/internal/v1/runner/mint
  x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>     ← the value we need
  body: { "owner_tenant": "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3", "job_id": "<gh workflow_job.id>", "scope": "cas:rw" }
  → { "token": "<per-job CAS PAT>" }
```

The value must be **exactly** the key your deployed mint Worker authenticates against (Option B / public
hostname, which you confirmed live). If a mint call with this header returns `200 {token}` for the dogfood
tenant `ee30f7ba…`, it's the right value.

## How to deliver (OOB — pick whatever you already use)

Same secure channel the dogfood **tenant PAT** came through (it landed in the owner's
`~/Downloads/corelink-dogfood-pat.txt`, chmod 600, never committed). A 1Password/secret-manager item, an
encrypted message to the owner, or a file dropped on the owner's machine — anything **except** committing it.

The owner then runs (or hands to me to run with wrangler already authed on the `gmhelmold` account
`6a1fc1c626fc2628823e60b9db01f5cd`):

```
cd deploy/cloudflare && npx wrangler secret put CORELINK_PAT_MINT_AUTH_KEY   # paste the value
```

The moment it's set, `/webhook` mints a per-job PAT + injects `CLW_*` → the runner hydrates in-network
(zero-egress R2). No redeploy. Until then the runner spawns **cold** (fail-open — north star intact).

## Two confirmations we'd like with it

1. **Same key for `/revoke`?** We just shipped revoke-on-completion (PR #122). The Worker calls
   `POST /internal/v1/runner/revoke {owner_tenant, job_id}` with the **same** `x-corelink-internal-auth`
   header. Confirm one shared key covers both `/mint` and `/revoke` (we assume yes). The revoke endpoint
   contract we built against is here for your freeze:
   `docs/handoff/2026-06-20-relay-to-server-tl-d9-revoke-contract.md`.
2. **Rotation expectation?** If this key is rotated server-side, we just re-`put` the Worker secret (no
   code change). Let us know your rotation cadence so we don't get surprised by a sudden 401 on mint.

## Status on our side (nothing else blocks)

- Mint client + `CLW_*` inject: live, fail-open, unit-tested (`deploy/cloudflare/src/lib.ts`).
- Revoke-on-completion: merged (`206e999`), fail-open, 20 vitest tests.
- Worker secrets rotated off the dogfood throwaways (`CLOUDFLARE_SPAWN_AUTH_TOKEN`, `GITHUB_WEBHOOK_SECRET`
  — verified by a webhook ping → 200). `CORELINK_PAT_MINT_AUTH_KEY` is the only unset secret.

— CoreLink Runners TL
