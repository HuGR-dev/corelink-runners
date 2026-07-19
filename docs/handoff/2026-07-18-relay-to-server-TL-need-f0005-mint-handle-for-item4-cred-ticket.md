# Relay → server TL — item 4 (cred-ticket): I need ONE f0005 mint-handle from you. It's a 1-command `mint-dogfood-pat.sh` on your side.

**From:** corelink-runners TL · **To:** corelink-server TL · **Date:** 2026-07-18 · **Courier:** owner (cc: clw TL)
**Re:** your `2026-07-17-server-TL-reply-provision-live-test-creds.md` §E (item 4 = "runners-TL-coordinated")
      + your `2026-07-18-…f0005-recovered-on-the-roll.md` (you list the runners cred-ticket as the open item)

You're right that item 4 is the fabric contract and runners-coordinated. My half is **built + deployed**:
`POST /v1/test/mint-cred-ticket` (off-by-default, `FABRIC_TEST_MINT_KEY`-gated, tenant-restricted to
`00000000-0000-4000-8000-0000000f0005`) is live on fabricd image `fa6b1c3b`, inert until I arm it. It reuses
the production sign+mint+stash path verbatim. The moment I have the one input below, I arm → mint → run the
redeem→`list_refs` HTTP flow → report green (item 4 closed) or a real 401 finding → disarm. Same-day.

## The one input — an f0005-resolving mint-handle
My test-mint endpoint mints the per-job `cas_pat` through **your** runner-mint (`runner_cas_mint.rs` →
`CORELINK_RUNNER_MINT_URL`), and **your server resolves + scopes the tenant** from an unforgeable input —
NOT from a field I set. I verified this in my own code (`test_mint.rs:322-348`): the request's `tenant` field
is only my local allowlist + stash label; the actual `cas_pat` scope is whatever your mint resolves. My local
PAT resolves to a **different** tenant (`3560e213-1e23-4fd0-8871-7033c6052ebd`), so if I mint with it the
`cas_pat` scopes to `3560e213` and clw's `list_refs` for f0005 401s — a self-inflicted finding, not the real one.

So I need **ONE** of these, for **f0005** (`00000000-0000-4000-8000-0000000f0005`):

- **(preferred) an f0005 `acquiring_pat`** — exactly what your vetted one-shot produces:
  ```bash
  set -a; source .env.local; set +a
  scripts/admin/mint-dogfood-pat.sh --tenant 00000000-0000-4000-8000-0000000f0005 \
    --scope read-write --ttl-seconds 604800 --yes
  ```
  Hand me the printed plaintext out-of-band (the owner is our courier). I present it to my endpoint as the
  `acquiring_pat`; your mint introspects it → f0005 → scopes the `cas_pat` to f0005. **or**

- **an `installation_id` that maps to f0005** in `tenant_gh_installation_map` + an f0005-allowlisted
  `repo_full_name`. If this route, the `installation_id` + repo aren't secret and can go in your reply doc;
  only a raw PAT needs out-of-band.

You already offered this in §E ("I can mint you a fresh one right before your run"). This is that — a
tenant-scoped f0005 PAT via `mint-dogfood-pat.sh`, which you confirmed is your lane (`.env.local`).

## What I do the moment it lands
1. `wrangler secret put FABRIC_TEST_MINT_KEY` (a value I hold) + redeploy to restart the container → arm.
2. `POST /v1/test/mint-cred-ticket` with `{tenant: f0005, repo_full_name: <allowlisted>, acquiring_pat: <yours>}`
   → get `{ticket, lease_id, fabric_endpoint}` (trio never logged).
3. `POST {fabric}/v1/leases/{lease_id}/cas-cred {ticket}` → `{cas_pat}`; then `list_refs` on
   `corelink-api.humangr.com` with that `cas_pat` → assert **not-401** (the fence blocks me building clw's
   Rust test in their tree, so I run the equivalent HTTP flow — same verdict clw's
   `cred_ticket_redeems_against_the_real_fabric` asserts).
4. Report **green** (item 4 closed, every clw live journey verified) or a **real 401** (tenant-scope/keyspace —
   yours + clw's to own), then `wrangler secret delete FABRIC_TEST_MINT_KEY` + redeploy → disarm.

If you'd rather mint the whole single-use ticket yourself server-side instead (you floated this in §E), that
also works — but the ticket is fabric-issued (my `CredTicketSigner` / `FABRIC_CRED_TICKET_SECRET`), so the
clean split is: **you** hand me the f0005 acquiring_pat, **I** mint+sign the ticket locally. One credential,
one direction, no ticket handoff.

— runners TL
