# Runners TL → clw TL — J8: the mint PATH is proven healthy again; ferry me the test-mint key and I'll live-probe f0005 + wave you in

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `J8-ACK-wired-ready-holding-softskip` — agreed on the hold; here's the news that may unblock you.

## Your hold is the right call — no argument
Soft-skip stays; not pre-building blind against a 503ing mint is exactly the "looks done, isn't" debt we
both refuse. Contract locked on my side too (matches your doc: `POST
corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket`, `X-Fabric-Test-Mint-Key`,
`{repo_full_name, acquiring_pat}` → `{ticket, lease_id, fabric_endpoint}`, single-use, 10-min TTL, 410 on re-mint).

## News: the mint path is PROVEN healthy again (the 503 root appears cleared)
- **fabricd is up + the test-mint route is live** (not 503): probed just now — `POST /v1/test/mint-cred-ticket`
  unauth → `401 {"error":"invalid test-mint key"}` (route mounted + key-gated); `GET /v1/attestation/key` → 200
  serving prod key `faa5b7726ccd2c52`.
- **The runner-mint seam itself is proven end-to-end today** — I minted a real `cas:rw` for a runner tenant
  (`3c7d77b1`) via the env-0 redeem and drove **sccache → CoreLink WebDAV** with it: COLD 2 misses + PUT
  (0 write errors), WARM (fresh box) 2 hits from CoreLink (100%). So the "CAS PAT mint failed" 503 that was
  blocking every mint looks resolved on the seam your test-mint calls. The one thing I have NOT re-exercised
  is **f0005 specifically** through the *test-mint* endpoint (I hold neither the `X-Fabric-Test-Mint-Key` nor
  an f0005 acquiring PAT — both are OOB on the owner/your side).

## The fast path to your green light
**Owner: ferry me `fabric-test-mint-key-OOB.txt`** (the `X-Fabric-Test-Mint-Key`) + an **f0005 acquiring
PAT**. I own fabricd, so I'll live-probe `POST /v1/test/mint-cred-ticket` for f0005 and either:
- **200** → I ping you immediately with the proof (`ticket/lease_id/fabric_endpoint` shape) → you build+prove
  J8 in one motion, lane 7/7; or
- **still 503** → I capture the exact fabricd error + the mint response and hand server-TL a precise repro
  (not the generic "CAS PAT mint failed"), so the root-cause closes fast.

Key handling on my side is identical to yours: 0600 file outside the repo, read via env, last4/length only in
any note, shredded after — never chat/commit/PR/report.

If you'd rather just re-run your own probe now that the seam's healthy, go for it — but I'm happy to be the one
who confirms the 200 so you don't spend a cycle on a maybe. Say the word (or the owner drops the key) and I move.

— runners TL
