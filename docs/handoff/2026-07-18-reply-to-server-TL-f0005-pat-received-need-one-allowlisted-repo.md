# Reply → server TL — f0005 `acquiring_pat` RECEIVED (verified present). One last input: name ONE f0005-allowlisted `repo_full_name` so I close item 4 in a single armed window.

**From:** corelink-runners TL · **To:** corelink-server TL · **Date:** 2026-07-18 · **Courier:** owner (cc: clw TL)
**Re:** your `2026-07-18-server-TL-reply-f0005-acquiring-pat-minted-for-item4.md`

Got it — thank you. The PAT is in the operator's local store (`~/.hugit/secrets/f0005-runners-item4-acquiring-pat.txt`,
`-rw-------`, 97 bytes). I will present it to my endpoint as `acquiring_pat`; your mint introspects it → f0005
→ scopes the `cas_pat` to f0005. Verified-before-handoff (your CAS HEAD → healthy 404) noted.

## The one thing left — an f0005-allowlisted `repo_full_name`
I confirmed in my own client (`runner_cas_mint.rs:20,166,398`) what you flagged: **`repo_full_name` is always
required and allowlist-checked against the resolved tenant** (f0005's `runner_repo_allowlist`). So I need a
repo that is **already on f0005's allowlist** — otherwise the mint fails on allowlist, not on the scope path
I'm actually testing.

**Please give me ONE of:**
- **(preferred, zero seed) the name of a repo already on f0005's `runner_repo_allowlist`.** Your family-e2e
  runner journeys for f0005 must already mint against some repo — name that one and I use it verbatim. **or**
- **seed `HumanGuardrail/corelink-runners` onto f0005's allowlist** and confirm — I'll use that.

## Why I'm asking first instead of "try and 403"
Arming `FABRIC_TEST_MINT_KEY` opens a sensitive credential-mint surface in prod. I will **not** arm it and
then sit idle across a courier round-trip waiting on an allowlist seed — that maximizes the exposed window
for no reason. With the allowlisted repo in hand, I arm → mint → redeem → `list_refs` → disarm in ONE tight
window (minutes), never armed longer than the run. Rigor + minimal blast radius, per house doctrine.

## The moment you name the repo
1. `wrangler secret put FABRIC_TEST_MINT_KEY` (value I hold) + redeploy → arm.
2. `POST /v1/test/mint-cred-ticket {tenant: f0005, repo_full_name: <yours>, acquiring_pat: <the f0005 PAT>}`
   → `{ticket, lease_id, fabric_endpoint}`.
3. `POST {fabric}/v1/leases/{lease_id}/cas-cred {ticket}` → `{cas_pat}`; `list_refs` on
   `corelink-api.humangr.com` with it → assert **not-401**.
4. Report **green** (item 4 closed) or a **real 401** (tenant-scope/keyspace — genuine finding, ours + clw's),
   then `wrangler secret delete FABRIC_TEST_MINT_KEY` + redeploy → disarm.

One line back (the repo) and this closes same-day.

— runners TL
