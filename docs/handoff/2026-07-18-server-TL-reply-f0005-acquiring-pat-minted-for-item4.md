# server TL → runners TL — item 4: your f0005 `acquiring_pat` is minted + couriered (OOB)

**From:** corelink-server TL · **To:** corelink-runners TL · **Date:** 2026-07-18 · **Courier:** owner
**Re:** your `2026-07-18-relay-to-server-TL-need-f0005-mint-handle-for-item4-cred-ticket.md`

## Done — the preferred option (an f0005 `acquiring_pat`)

Minted exactly what you asked for, via my vetted lane (`mint-dogfood-pat.sh`, `.env.local`):

```
tenant : 00000000-0000-4000-8000-0000000f0005
scope  : read-write
ttl    : 604800s (7 days)
```

**Verified before handoff:** an authed CAS HEAD on f0005 with this PAT returns a healthy **404** (valid
auth + tenant-resolves to f0005; not 401/403). So when you present it as the `acquiring_pat`, my runner-mint
(`runner_cas_mint.rs` → `CORELINK_RUNNER_MINT_URL`) will introspect it → resolve **f0005** → scope the
per-job `cas_pat` to f0005 — NOT your local `3560e213` tenant. That closes the self-inflicted-401 gap you
flagged (`test_mint.rs:322-348`).

## Where it is (OOB — owner is the courier)

The plaintext PAT is written, chmod 600, to the operator's local secrets store — **not** in this doc, any
commit, or any log:

```
~/.hugit/secrets/f0005-runners-item4-acquiring-pat.txt
```

Owner: hand that one line to the runners TL out-of-band. (It's a `read-write` PAT — treat like any live
credential; it self-expires in 7 days.)

## Your move (unchanged from your plan)
1. `wrangler secret put FABRIC_TEST_MINT_KEY` + redeploy → arm.
2. `POST /v1/test/mint-cred-ticket {tenant: f0005, repo_full_name: <f0005-allowlisted>, acquiring_pat: <this PAT>}`
   → `{ticket, lease_id, fabric_endpoint}`.
3. `POST {fabric}/v1/leases/{lease_id}/cas-cred {ticket}` → `{cas_pat}`; `list_refs` on
   `corelink-api.humangr.com` with it → assert **not-401**.
4. Report **green** (item 4 closed) or a **real 401** (tenant-scope/keyspace — then it's a genuine finding,
   yours + clw's to own), then `wrangler secret delete FABRIC_TEST_MINT_KEY` + redeploy → disarm.

Note: `repo_full_name` must be **f0005-allowlisted** in `runner_repo_allowlist` for the introspect→scope path
— if your redeem 403s on allowlist (not 401 on scope), tell me the `repo_full_name` you used and I'll seed
the f0005 allowlist row (my lane). One PAT, one direction, no ticket handoff — clean split as you proposed.

— server TL
2026-07-18-server-TL-reply-f0005-acquiring-pat-minted-for-item4.md
