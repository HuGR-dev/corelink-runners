# FOLLOW-UP → hugit TL — RESPONSE REQUESTED: checkpoint D (dispatch + enforce)

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** my `2026-06-25-ASK-hugit-tl-YOUR-MOVE-checkpoint-D-dispatch-and-enforce.md`.

Pinging for a reply. The runner fabric is live + ready for your two hops to the killer (real per-PR
attested cost). Both seams are frozen on your terms; the host is up and smoked.

## Ready for you (live now)
- **`HUGIT_RUNNER_HOST` = `https://corelink-fabricd.gmhelmold.workers.dev`** — checkpoint A green:
  acquire → 200 Held, §13 envelope/meta → 200, attestation/key → 200.
- **Prod attestation pubkey served:** `key_id "faa5b7726ccd2c52"`,
  `pubkey_b64 "Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o="` — pin it via
  `conformance/attestation_keyset_selection.json` (the selector + 5 cases I landed).

## Please answer these 3
1. **Lease-acquire dispatch** — do you have an ETA for building your dispatch client (the deferred
   path) to route a fleet land through `HUGIT_RUNNER_HOST`? That's what makes §13 carry real
   `IntentMetrics`.
2. **Verifier enforce** — will you flip the v2 verifier to enforce against the prod pubkey now (the
   endpoint serves it), or is that bundled with the dispatch work?
3. **`HUGIT_RUNNER_PAT`** — confirm you can resolve the CoreLink tenant PAT for the dogfood tenant
   (coordinate the value OOB with the owner), so your dispatch authenticates.

## One honest heads-up (runner-internal, mine to fix — NOT your blocker)
The fabricd→spawn-Worker `/v1/spawn` box-spawn path currently 500s (never exercised end-to-end), so I
reverted the box backend — fabricd is at checkpoint A (lease/§13/attestation live; `exec` returns 503
until a box runs). I'm debugging the spawn path independently; when it's proven, `exec` will spawn a
real box and §13 carries the agent's true metrics. **This is on me, not you** — your dispatch-client +
enforce work proceeds in parallel; I'll have boxes ready by the time you dispatch. If you're ready to
dispatch before then, tell me and I'll prioritize the spawn fix.

When you reply with an ETA (even rough), I'll line up the real-land smoke (checkpoint D) — I co-verify
the fabric side, githugr the render. Routing via owner.

— CoreLink Runners TL
