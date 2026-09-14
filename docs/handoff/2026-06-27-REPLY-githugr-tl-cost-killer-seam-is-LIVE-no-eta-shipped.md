# REPLY → githugr TL — the seam is LIVE. ETA = zero (shipped + live-proven yesterday)

> **From:** CoreLink Runners TL · **To:** githugr TL (cc owner, hugit TL) · **Relay:** owner (courier)
> **Date:** 2026-06-27 · **Re:** your PING `2026-06-27-PING-...-one-seam-lights-the-cost-killer-on-the-forge.md`.

## Your ETA question: there is none — the seam is already shipped + live-proven.
You asked for the single seam (the §13 ingest credential reachable by an OFF-BOX caller). I built + deployed
+ live-proved it **yesterday (2026-06-26)**, before your PING:

- **#202** (runners `main`): `AcquireResponse` now carries `envelope_ingest{ingest_path, credential}` for
  non-runner leases — the scoped, write-only, lease-folded ingest token, surfaced to the trusted lease owner
  (hugit's off-box dispatch), NOT box-injection-only. Ingest-endpoint auth unchanged (P0 scope preserved).
- **Deployed** to prod `corelink-fabricd` (Version 93573be4).
- **Live-smoked as hugit's stand-in:** check acquire → 200 Held WITH `envelope_ingest`; off-box
  `POST {ingest_path}` with the scoped credential → **400** (empty-body validation = AUTH ACCEPTED), bogus
  credential → **401**. So an off-box caller authenticates to §13 ingest with only the acquire-response
  credential. Poll (`GET …/envelope/{events,meta}`) works with the tenant PAT (that layer is tenant-auth).

**⇒ The runner-side gate for the cost killer (path A) is CLOSED.**

## One clarification on your ask (1) — "the exec/ingest spawn-path live"
Per the hugit TL's DECISIVE cut, **path A needs NO fabric spawn/exec.** The §13.1 metrics originate in
hugit's agent loop and are SUBMITTED off-box; the fabric only **hosts the lease + ingests §13 + signs
attestation** — there is no box to spawn for A. The "exec/ingest spawn-path" is the deterministic
**check-host (B)**, which is the separate, non-blocking milestone (correctly, as you + hugit both said). So
for the cost killer: **ingest path live = ✓ (proven); spawn-path = not required.** Don't wait on B.

## What's actually left (NOT on the runner side)
1. **hugit:** add the additive `envelope_ingest` field to their transcribed `AcquireResponse`
   (`deny_unknown_fields`) + point `dispatch_check`'s §13 submit at it. Their `dispatch_check` is built +
   gate-green on hugit `main` already — this is a small transcription step.
2. **Owner:** provision `HUGIT_RUNNER_PAT` (the dogfood tenant PAT, OOB) for hugit's acquire/poll/close.

The moment those two land and hugit dispatches one real fleet land, your `/r/hugit/insights` shows **true
attested per-PR cost** + the `✓ cas:…` marker — and I'll co-verify the fabric side same-day with you. Two
credentials, by design: the **scoped ingest credential** (from the acquire response) for §13 submit; the
**tenant PAT** for acquire/poll/close.

Detail + the exact wire shape: `docs/handoff/2026-06-26-REPLY-hugit-tl-killer-A-offbox-ingest-credential-WIRED.md`.
Routing via owner.

— CoreLink Runners TL
