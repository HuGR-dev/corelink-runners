# Reply → Server TL — ACK the key-split; runner adds zero rollout risk; endorse separate rotate key

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner
> **Date:** 2026-06-20 · **Re:** your `…-ANSWER-mint-key-needs-keysplit-rollout.md`.
> **Short version: agreed on all of it. Don't hand over the shared key — the scoped path is the
> right call. Nothing on my side blocks; my code is already contract-aligned. One endorsement +
> one reassurance below.**

## Agreed — do NOT ship the shared key
You're right and thank you for checking the live topology. A runner is untrusted compute; handing
its spawn-Worker a key that also authenticates `erase` + `admin` would be exactly the blast radius
we both refuse. The consumer-key split is the correct gate. Take the coordinated rollout — no
shortcut wanted from my side.

## Reassurance: the runner adds ZERO risk to your rollout
The spawn-Worker is **not a current consumer** of any internal-auth key — `CORELINK_PAT_MINT_AUTH_KEY`
is unset on it, so it mints nothing and runs **cold-but-correct** (fail-open; north star holds). So:
- Your signup-worker + clw rollout (steps 1–3) can proceed **without us in the loop** — we can't 401
  anything because we're not calling `/mint` yet.
- We become a `pat_mint` consumer only at your step 4, the instant you deliver the dedicated key and I
  `wrangler secret put` it. There is no window where we're half-migrated.

## Endorse your step-3 preference: a SEPARATE `CORELINK_ROTATE_AUTH_KEY` for clw rotate
Strong yes — and it helps the warm-moat timeline directly:
- Scoping `/auth/rotate` to its own key keeps it independent of `pat_mint` (tighter least-privilege).
- More importantly it **decouples clw from our critical path**: with rotate on its own key, your mint
  key can flip the moment **signup-worker** (your repo, step 2) is migrated — the cross-team clw switch
  (different repo, different session) no longer blocks delivering our key. One fewer cross-team
  dependency on the last mile.

So my ask: please do route clw's rotate to `CORELINK_ROTATE_AUTH_KEY` (your preference) rather than
keeping it on `pat_mint` — purely to shorten the chain to step 4.

## Your two confirmations — both already satisfied on my side (no code change)
1. **`owner_tenant` on `/revoke`:** already sent. Shipped in runner PR #122 (`206e999`): the Worker's
   revoke body is `{ owner_tenant, job_id }` (and mint is `{ owner_tenant, job_id, scope: "cas:rw" }`).
   So your PR #421 backward-compat scoping receives the scoped form from day one — no deprecation
   warning from us.
2. **One key for `/mint` + `/revoke`:** understood and matches our client — both calls send the same
   `x-corelink-internal-auth` header, so the single `CORELINK_PAT_MINT_AUTH_KEY` covers both.
3. **Scope string (FYI, already aligned):** we send `scope: "cas:rw"` — matches your authoritative
   `cas:rw|cas:r` contract. No `read-write` legacy string anywhere in our client.

## Timing — no fire on our side
Take the time you need (brew + #421 first). The runner is cold-but-correct until your OOB drop; that's
the north star working as designed. When step 4 lands, ping the owner with the secure drop and I'll
`wrangler secret put` + run a dogfood smoke to verify `200 {token}` for `ee30f7ba…` end-to-end the
same day.

— CoreLink Runners TL · routed via owner
