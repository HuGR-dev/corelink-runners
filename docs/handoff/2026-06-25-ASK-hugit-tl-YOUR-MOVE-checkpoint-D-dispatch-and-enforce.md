# ASK → hugit TL — your move: the fabric is fully live + box-wired. Checkpoint D is yours.

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** gap-#1 (b) DONE on the runner side. Handing the ball back.

Everything on the runner side for the killer is **live, smoked, and box-wired**. The remaining hops to
real per-PR attested cost are **yours** (engine dispatch + verifier enforce). Here's exactly what you
need and what's owed.

## What's live (use it now)
- **`HUGIT_RUNNER_HOST` = `https://corelink-fabricd.gmhelmold.workers.dev`** — the prod control plane
  (singleton CF Container, 24/7). Smoked live: acquire → 200 Held, §13 envelope/meta → 200,
  attestation/key → 200.
- **`HUGIT_RUNNER_PAT`** = the CoreLink **tenant PAT** (ADR-0002 machine principal) — the same
  credential class the §13 poll path already assumes. (For the dogfood tenant, the PAT the owner has
  provisioned; coordinate the exact value out-of-band.)
- **Box backend wired** — `CLOUDFLARE_SPAWN_*` is set, so a lease's `exec` spawns a real runner box
  via the spawn-Worker and runs the job (substrate verified "wired"). Per-job `IntentMetrics` land in
  the §13 envelope on close.
- **Prod attestation pubkey served** — `GET /v1/attestation/key` → `key_id "faa5b7726ccd2c52"`,
  `pubkey_b64 "Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o="`.

## Your move (the 2 hops that light the killer)
1. **Build the lease-acquire client (your deferred dispatch path) and route a fleet land through
   `HUGIT_RUNNER_HOST`.** Acquire → exec (your CheckDef + tree_hash) → close. fabricd spawns the box,
   runs the agent, and the §13 terminal envelope carries the real `IntentMetrics` (`cost_usd_micros`,
   `tokens`, model-turns) — which your xray/ledger projection maps to the githugr VM fields (the
   mapping githugr froze: `cost_usd_micros`→`cost_total_micros`, `tokens.total`→`tokens_count`, the
   §13 envelope CAS ref → `spend_proof`; you derive `waste`/`cache_saved` and source the display
   `model`).
2. **Flip the v2 verifier to ENFORCE.** Pin the prod pubkey via the selection vector
   `conformance/attestation_keyset_selection.json` (transcribe `select_attestation_key` + its 5 cases —
   key_id-match→accept, unknown→reject, expired→reject) ABOVE your existing `verify_result_binding_v2`.
   The endpoint serves the prod key now, so enforcement closes the P0 verdict-forgery window.

## What I'll do when you're ready
Ping me when your dispatch path is wired and I'll **co-verify the real-land smoke (checkpoint D)** end
to end: a fleet land → fabricd lease → box exec → §13 metrics → `/r/hugit/insights` renders true
attested per-PR cost + the `✓ cas:…` `spend_proof` marker. githugr verifies the render; I verify the
fabric side.

## Owed by me (runner side): nothing blocking
The control plane, §13, attestation, box backend, and the conformance vectors are all landed + live.
The only open runner-internal item is a cosmetic stale log line (`main.rs` "execs will 503" predates
the CF substrate) — non-functional, not on your path. If you hit ANY fabric-side issue during
dispatch, that's mine — send it over.

Ball's in your court. Routing via owner.

— CoreLink Runners TL
