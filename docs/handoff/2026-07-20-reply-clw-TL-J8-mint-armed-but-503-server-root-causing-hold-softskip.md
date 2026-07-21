# Runners TL → clw TL: J8 test-mint — route ARMED + key works, but the mint currently 503s (server-TL root-causing). Keep soft-skip a bit longer.

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-20
**Re:** your J8 PING — status update (supersedes my earlier "armed + owner ferries" note)

## Where J8 stands right now (honest, end-to-end tested)
- ✅ **`POST /v1/test/mint-cred-ticket` is ARMED in prod.** Probed: `401` without the key, `400/503`
  (never `401`) with it → the key is accepted, no drift.
- ✅ **The 40-char `FABRIC_TEST_MINT_KEY` is verified-working** and the owner is ferrying it to you OOB
  (`fabric-test-mint-key-OOB.txt`). Auth via `X-Fabric-Test-Mint-Key: <key>` (or `Authorization: Bearer`).
- 🔴 **BUT the mint itself currently returns `503 {"error":"CAS PAT mint failed"}`** — so it does NOT yet
  hand back a usable `{ticket, lease_id, fabric_endpoint}` trio. **Do not wire J8 to expect a live trio
  yet** — it'll 503.

## Why it 503s (and why it's NOT what we first guessed)
My first theory was "f0005 lacks a mint entitlement." **That's wrong** — the server-TL checked prod D1:
**f0005 already HAS `runners_entitlement` (max_concurrency 25) + 28 pat rows.** So the `CAS PAT mint
failed` has a *different* root cause, somewhere in the server's `runner_mint.ts` — their candidates: the
per-tenant mint ceiling (`max(max_concurrency*K, FLOOR)` vs f0005's existing 28 pats), the acquiring-PAT
scope/marker the mint requires, or a null `max_vcpu_h`. I tested with the **real live f0005 acquiring
PAT** as `acquiring_pat` (`repo_full_name` required in the body) → still `503 CAS PAT mint failed`, so it's
not an input mistake on my side.

**The server-TL is root-causing it now** — the owner is OOB-ferrying them the live f0005 acquiring PAT so
they can reproduce the mint server-side and read the true rejection (the caller only gets the bare
`CAS PAT mint failed`; the reason is server-side). Note it may also have been perturbed by today's
introspect-container incident (now resolved) — they'll re-test clean first.

## What to do
- **Keep J8 soft-skipping** (as it does when the secret is absent) — nothing red, exactly as you have it.
- **Hold the key** the owner ferries; you're ready the instant the mint works.
- **I'll ping you** the moment the server-TL confirms `mint-cred-ticket` returns a real `200` trio — then
  J8 flips live against the real fabric with zero further changes on your side (the redeem contract is
  already proven both sides: your 2026-07-18 close, `POST .../cas-cred {ticket}→{cas_pat}`, tenant-scoped,
  410-on-reredeem).

## The exact contract (so you're wired correctly for when it's green)
```
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket
  header: X-Fabric-Test-Mint-Key: <the OOB key>
  body:   { "repo_full_name": "<any>", "acquiring_pat": "<an f0005-resolving PAT>" }   // tenant defaults to f0005, allowlist-locked
  200  →  { "ticket": "...", "lease_id": "...", "fabric_endpoint": "..." }   // single-use, 10-min TTL, 410 on re-mint of a consumed lease
```

Net: **armed + key good; the mint is a server-TL fix away.** Sit tight on soft-skip; I'll wave you in.

— runners TL
