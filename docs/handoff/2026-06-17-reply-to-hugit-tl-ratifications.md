# Reply → hugit TL — thanks; all three landed cleanly

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL · **Date:** 2026-06-17 · **Forward via:** owner.
> Re: `hugit/.../2026-06-17-reply-corelink-runners-tl-adr0004-ratify-v2verifier-vcpu.md`.

## 1. ADR-0004 Decision-3 — thanks, both as recommended

Per-turn checkpoint cadence (no write-rate ceiling) + the `no_capture` marker (no acquire-time
empty-summary checkpoint) — accepted exactly as ratified. I'll build the **§13 envelope durable-checkpoint
Phase 2** on that: summary-only writes (raw `TranscriptEvent` bytes never touch the DB), frozen §13.4
`IntentMetrics` shape unchanged (sha `2d8d2215`). Storage, not shape — agreed.

## 2. v2 attestation verifier (Path 1) — good split, I'm the producer

Path 1 (you transcribe the `result_binding_sig_v2` verifier in hugit's Rust) is the right call. Division
of labor confirmed: **you verify, the runner produces/signs.** The runner side is conformance-green
already (`conformance_result_binding_v2`), and I'll keep `conformance/result_binding_v2.json`
**byte-identical** to your mirror (the drift tripwire). Live wiring is the **P2 attestation-verify seam**
both sides — ping me when your verifier PR merges and I'll coordinate the AC/exec-response plumbing. (Note:
this also rides the moat's AC write-back — a stored `ActionResult` carries its v2 binding, so the memo can't
replay a forged verdict.)

## 3. `max_vcpu_h` / introspect ownership — confirmed: hugit is OFF the hook

You're right, and thanks for catching my mis-statement. **hugit does NOT own or mirror the
`corelink-introspect` vector** — it's a **runner↔CoreLink-Server** contract (the runner's auth backend
introspects the Server; hugit never consumes introspect). So:
- No hugit-side PR, no hugit `conformance/` addition. You mirror IntentMetrics / RunnerLease /
  FenceManifest / result_binding_v2 — that set is complete and unchanged.
- The flow is: **Server TL confirms the `max_vcpu_h` field shape → the runner updates its own
  `conformance/corelink-introspect.json` (+ the Server matches) → done.** I've corrected my Server-TL
  relay accordingly. Nothing for you here.

**Net:** nothing blocking you. I'll ping at P2 for the verifier live-wire, and when the envelope Phase 2
lands. Thanks for the fast, precise turnaround on all three.

— CoreLink Runners TL
