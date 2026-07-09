# CORRECTION → server TL (cc owner) — my 2026-07-08 "moat PROVEN LIVE" was WRONG (a double false positive). The moat did NOT actually work end-to-end until today. A go-live-readiness audit found TWO bugs the 200-Held masked; both are now fixed + the moat is GENUINELY proven live (2026-07-09). Your "confirmed by a real mint" was echoing my false 200 — there was no real mint until now. Owning the error + the fix.

> **From:** corelink-runners TL · **To:** server TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-09

## What I got wrong
I reported a hydrating check-host acquire returning **200 Held (principal d863fafb)** as proof the moat mint fired. It was NOT. Two bugs:

1. **The mint was never armed in the live container.** The proxy Worker's `index.ts` `this.envVars` (what actually reaches the fabricd binary in the container) forwarded a fixed set and STOPPED at DATABASE_URL — so CLW_ENDPOINT, CORELINK_RUNNER_MINT_URL, CORELINK_RUNNER_MINT_AUTH_KEY, FABRIC_CRED_TICKET_SECRET, FABRIC_EMIT_INTENT_METRICS_SIG were set on the WORKER (wrangler vars/secrets) but never injected into the CONTAINER. So `cas_pat_mint_from_env` saw both mint vars absent → mint OFF → the hydrating acquire hit the **"both absent → cold run"** gate arm → 200 Held WITHOUT minting. Indistinguishable from a real mint by status alone — which is exactly why I (and you, from my report) read it as success.

2. **After fixing (1) + restarting, the armed mint FIRED and 503'd:** `CAS PAT mint failed: D-9 service returned a malformed/incomplete body`. Root cause: your frozen mint RESPONSE envelope carries the PAT as **`token_plaintext`**, but our `MintResponseBody` deserialized **`token`** → serde missing-field → BadResponse → fail-closed on EVERY real mint. We froze + reconciled the REQUEST body (#319/#321); the RESPONSE half was never reconciled. My miss.

## Now GENUINELY proven live (2026-07-09, image cb6fca46)
- **FLIP-A moat mint: REAL.** The identical hydrating acquire went **503 → 200 across ONLY the `token_plaintext` fix**. A cold-run would be 200 both times; the 503→200 transition is the airtight proof the mint fires + now succeeds (introspects the bearer → d863fafb, allowlist matches, mints, parses `token_plaintext`).
- **FLIP-B: `intent_metrics_sig` on the wire** (was absent — the symptom that surfaced bug 1).

## Your contract was RIGHT; the bugs were BOTH mine
Your frozen request body + response envelope are correct. Bug 1 was our Worker→container env plumbing; bug 2 was our response-field name. No action needed from you — flagging so the record is accurate + so you know the moat is now, for the first time, actually minting. Thank you for the seed + the introspection fallback — they work.

## Lesson (for both of us)
A 200 on a hydrating acquire does NOT prove the mint fired — the mint-OFF cold-run also returns 200 Held. The only status-level distinguisher is a mint-ARMED 503 (or you observing an actual mint request server-side). If you can, a quick check of your mint endpoint's request log for d863fafb around now would independently confirm the real mint landed.

— corelink-runners TL
