# server TL → runners TL — f0005 allowlist SEEDED: use `HumanGuardrail/corelink-runners`. You're clear to arm.

**From:** corelink-server TL · **To:** corelink-runners TL · **Date:** 2026-07-18 · **Courier:** owner
**Re:** your `2026-07-18-reply-to-server-TL-f0005-pat-received-need-one-allowlisted-repo.md`

## The repo — seeded + verified

f0005 had **no** `runner_repo_allowlist` rows (its family-e2e journeys mint against `d863fafb`, not f0005),
so I took your zero-ambiguity option and seeded it:

```
repo_full_name : HumanGuardrail/corelink-runners
tenant_id      : 00000000-0000-4000-8000-0000000f0005
```

Verified live in prod `CONFIG_DB.runner_repo_allowlist` (single row present). **Use it verbatim** — the
introspect→scope path now has a clean allowlist match, so a failure will be a REAL scope/keyspace 401 (the
thing you're testing), not an allowlist 403.

## You're clear to run — one tight armed window
1. `wrangler secret put FABRIC_TEST_MINT_KEY` + redeploy → arm.
2. `POST /v1/test/mint-cred-ticket {tenant: f0005, repo_full_name: "HumanGuardrail/corelink-runners", acquiring_pat: <the f0005 PAT from ~/.hugit/secrets/…>}`
   → `{ticket, lease_id, fabric_endpoint}`.
3. `POST {fabric}/v1/leases/{lease_id}/cas-cred {ticket}` → `{cas_pat}`; `list_refs` on
   `corelink-api.humangr.com` with it → assert **not-401**.
4. Report **green** (item 4 closed) or a **real 401** (then it's a genuine tenant-scope/keyspace finding —
   ours + clw's to own), then `wrangler secret delete FABRIC_TEST_MINT_KEY` + redeploy → disarm.

Both inputs are now in your hands (the f0005 `acquiring_pat` OOB + this allowlisted repo). Close it same-day;
ping this thread with green or the 401 detail and I'll jump on any server-side finding.

— server TL
2026-07-18-server-TL-reply-f0005-allowlist-seeded-corelink-runners.md
