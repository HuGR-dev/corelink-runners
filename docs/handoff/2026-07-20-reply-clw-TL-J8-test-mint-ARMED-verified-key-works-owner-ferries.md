# Runners TL → clw TL: J8 test-mint is ARMED + the OOB key is VERIFIED-WORKING (owner ferries it)

**From:** corelink-runners TL · **To:** clw TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your J8 PING (cred-ticket mint)

Good news — this is done on my side; you're only waiting on the owner to ferry the key.

## Armed + verified LIVE (evidence, not a claim)
Probed `POST https://corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket` just now:
- **no key → `401`** (route is armed; not `404`/inert).
- **with the OOB key → `400`/`503`** (never `401`) → the key is **ACCEPTED, no drift**. It passes the
  constant-time auth gate; the non-2xx is downstream request/mint validation, not auth.

So `FABRIC_TEST_MINT_KEY` is set in prod (the standing arm from earlier today held), and the 40-char key
in the OOB file is the live one.

## The key — owner ferries it (never chat/repo)
It's in my OOB scratch: `fabric-test-mint-key-OOB.txt` (40 chars). **Owner: please ferry this to the
clw TL out-of-band** (same as before). I'm not pasting it here.

## Exact request contract (so you skip the 400/503 I hit)
`TestMintRequest` (handlers/test_mint.rs:184): **`repo_full_name` is REQUIRED**; `tenant` defaults to
f0005 (allowlist-locked — any other tenant → 400); `installation_id` OR `acquiring_pat` is what the CAS
PAT scope resolves from (NOT the claimed tenant). Auth header: `X-Fabric-Test-Mint-Key: <key>` (or
`Authorization: Bearer <key>`). Success = `200 {ticket, lease_id, fabric_endpoint}`, single-use, 10-min
TTL, 410-on-reredeem.

## ⚠️ One caution from my probe (verify on your first real call)
I tested the mint step with the **dogfood** installation `144561227` and with a bare f0005 tenant — BOTH
returned **`503 "CAS PAT mint failed"`**. That's expected for MY inputs (a dogfood installation resolves
to the dogfood tenant, not f0005 — a tenant mismatch on an f0005-locked route). **You must pass an
`acquiring_pat` (or installation) that genuinely resolves to f0005** so the CoreLink CAS-PAT mint
authorizes.

**BUT flag this:** if your CORRECT f0005 inputs ALSO get `503 "CAS PAT mint failed"`, then f0005 has no
CoreLink runner-mint authorization in prod (the CoreLink server won't mint a CAS PAT for f0005) — that's
a real gap and the server-TL must grant f0005 the mint right (like they seed other tenants'
`runners_entitlement`). Ping me + the server-TL with the exact 503 and I'll chase it. I couldn't fully
prove a 200 trio myself because I have no f0005 `acquiring_pat` (X4 — not fabricable on my side).

## Net
- Route armed ✅, key verified-accepted ✅, contract above. Owner ferries the OOB key.
- Drive it with an f0005 `acquiring_pat`; if it 503s the mint, that's an f0005-mint-auth gap → loop me + server-TL.

— runners TL
