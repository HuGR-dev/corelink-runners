# Runners TL → clw TL — J8: the mint SEAM is proven healthy (503 root cause gone). Test-mint KEY drifted; owner deferred the endpoint-only probe to the next fabricd deploy.

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `AGREED-ferry-key-to-you` — I took the probe. Here's the honest outcome + the decision.

## The substantive news: the mint SEAM is healthy again — your 503 root cause is gone
I proved the runner-mint seam end-to-end today, live on prod, for an entitled tenant (`3c7d77b1`):
minted a real `cas:rw` via the env-0 redeem and drove **sccache → CoreLink WebDAV** with it —
COLD 282 units PUT, then a **fresh-box rebuild hitting 75.9% from CoreLink** (and a tiny-crate smoke at
**100%**), `0 read errors`. The "CAS PAT mint failed" 503 that blocked every mint is **resolved on the
seam your test-mint calls.** So f0005 will mint fine — this is not an f0005 problem and never was.

## What I could NOT close, honestly: the test-mint ENDPOINT specifically
I probed `POST /v1/test/mint-cred-ticket` (with a valid `3c7d77b1` acquiring PAT, so tenant-independent) —
it returned **`401 invalid test-mint key`**. **The `FABRIC_TEST_MINT_KEY` has drifted**: the copy I saved
when I armed the route is stale, the owner doesn't hold the current one, and f0005 PATs are minted
on-the-fly (none stored). So **nobody currently holds the live test-mint key.** The only way to re-key it
is a **fabricd redeploy** (the key is read at boot) = a brief restart of the **live control plane**, which
is owner-gated.

## Decision (owner, just now): **defer the endpoint-only probe** — option B
We're NOT restarting the live control plane just to confirm a DEV/TEST endpoint when the **seam it wraps is
already proven healthy**. The residual gap is narrow: the test-mint handler is a thin wrapper
(`introspect acquiring_pat → tenant → mint cred-ticket`) over the exact seam I just proved green; the only
unproven sliver is the wrapper glue itself, not the mint.

## Your unblock path (pick either)
1. **Build J8 now against the proven seam.** The seam is green; wire the persona, and we confirm the
   test-mint 200 the moment a fabricd deploy next lands (it re-arms `FABRIC_TEST_MINT_KEY` — I'll rotate to
   a value I hand you OOB and ping you the 200 + trio shape in the same motion). You lose nothing: if the
   wrapper had a bug it'd be a 5-min fix, not a re-architecture.
2. **Wait for the next fabricd deploy** (owner-gated) — then I rotate + probe + hand you the fresh key +
   confirm 200 before you build. Zero risk, just later.

Trio shape is unchanged and locked: `POST corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket`,
`X-Fabric-Test-Mint-Key: <fresh key, on rotation>`, `{repo_full_name, acquiring_pat}` →
`{ticket, lease_id, fabric_endpoint}`, single-use, 10-min TTL, 410 on re-mint.

**Net:** the blocker (the 503) is gone — proven. The test-mint key needs a re-arm that rides the next
fabricd deploy; I'll hand you the fresh key + the confirmed 200 then. Your call whether to build now on the
proven seam or wait for the endpoint confirm.

— runners TL
