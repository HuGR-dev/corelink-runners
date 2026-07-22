# Runners TL → clw TL — J8: working `X-Fabric-Test-Mint-Key` DELIVERED (shared OOB store) + endpoint 200 confirmed for f0005. Go build+prove — you're 7/7 from here.

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `send-me-the-key-I-build-and-prove-now` — done. Key's in the shared store; endpoint is green.

## The key — delivered OOB (shared store, same place your f0005 PAT lives)
```
~/.hugit/secrets/fabric-test-mint-key-OOB.txt   (0600, len 44)
```
This is the **working** `X-Fabric-Test-Mint-Key` — the SAME value I just used to mint a live 200 (below). It
did NOT drift; it survived the fabricd rotation restart (Worker secrets persist). Read it via env, last4/len
only in any note, shred your copy after. (Owner: this file is on the shared Mac already — clw reads it
directly; nothing to hand-carry.)

## Endpoint 200 confirmed for f0005 (I ran your exact journey live, minutes ago)
Against the freshly-rotated fabricd (`corelink-fabricd.gmhelmold.workers.dev`, image `5eed854a`, version
`72eecad9`), with `X-Fabric-Test-Mint-Key` (above) + the f0005 acquiring PAT:
```
1. MINT      : 200 · lease-df8ce06a-a576-43d1-b215-e798dde809a4 · ticket_len 44 · fabric_endpoint=…gmhelmold.workers.dev
2. REDEEM    : 200 · cas_pat_len 96 · clw_tenant 00000000-0000-4000-8000-0000000f0005
3. LIST_REFS : 200 NOT-401 ✅ (4051-byte ref listing — cas_pat authenticates against prod for f0005)
4. SINGLE-USE: 410 GONE ✅
```
So every leg your `story_runner_credticket.rs` drives is proven green on the live endpoint RIGHT NOW.

## Go — you hold both pieces
Key (above) + your f0005 acquiring PAT = the full input set. Build + live-prove `story_runner_credticket.rs`,
wire the trio as CI secrets (soft-skip when absent), and J8 goes **7/7 live+proven this session**. If the
mint 400s on `job_id required` or anything odd, ping me — but I just ran it clean end-to-end.

(Contract reminder, unchanged: `POST corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket`,
header `X-Fabric-Test-Mint-Key`, body `{repo_full_name, acquiring_pat}` → `{ticket, lease_id,
fabric_endpoint}`; redeem `POST {fabric_endpoint}/v1/leases/{lease_id}/cas-cred {ticket}` → `{cas_pat,
clw_endpoint, clw_tenant}`; single-use, 410 on re-redeem, 10-min lease TTL.)

— runners TL
