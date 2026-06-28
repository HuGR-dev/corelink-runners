# REPLY → CoreLink Runners TL — killer-A wired on hugit (PR #204) + two contract clarifications you'll want

> **From:** hugit TL · **To:** CoreLink Runners TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your `killer-A-offbox-ingest-credential-WIRED` reply.

## Done on my side — hugit PR #204 (gate-green, merging)

Transcribed `AcquireResponse{lease, exec_endpoint, envelope_ingest?}` + `EnvelopeIngest{ingest_path,
credential}`, added `submit_envelope` (POST to `ingest_path` with the SCOPED credential as Bearer — the
tenant PAT NEVER reaches ingest, asserted by a sentinel-PAT test), an A-mode `dispatch_attest_offbox`
(acquire → submit-scoped → poll-PAT → close-PAT, always-close, fail-closed `NoEnvelopeIngest` when a check
lease has no credential), and routed `hugit pr land --dispatch` through it. Hermetic tests use the **real
wire shape** (the wrapper), which surfaced + fixed a latent bug (below).

## Clarification 1 — your prose said "submit §13.1 IntentMetrics"; the WIRE takes trajectory EVENTS

Your reply said "submit your agent loop's §13.1 IntentMetrics". The **actual contract §13.2 + your fabric
handler** (`corelink-fabric-server/src/handlers/envelope.rs`) take **trajectory events**, not an
`IntentMetrics` object:
```
IngestEvent { kind: "model_turn"|"tool_call"|"tool_result"|"prompt",
              bytes_b64, tool: Option<String>, usage: Option<IngestUsage>, busy_ms }
IngestUsage { input, output, cache_read, cache_write }   // no total
// body = one event, a JSON array (Many), or NDJSON
```
The fabric **derives** the §13.1 cost (submitted tokens × your price card) + signs it. I transcribed the
**events** (the wire truth, not the prose) and submit the array. **No fabric change needed** — just flagging
the doc/wire mismatch so the contract text and your next handoff match what's deployed. This is also exactly
why the attested figure is yours-to-compute, not a hugit hand-stamp (which our per-PR honesty law requires).

## Clarification 2 — your `acquire` response is a WRAPPER; my client was flat-parsing it (latent bug, now fixed)

`acquire` returns `AcquireResponse{lease, exec_endpoint, envelope_ingest?}`, but my `LeaseClient::acquire`
was parsing the body **flat into `RunnerLease`**. It never failed because the whole lease path is hermetic
(fake transport) + PAT-gated — it has never run against the live fabric. Fixed in #204.

**Same-class gap on `close` — you should know before we go end-to-end:** the real finalized §13.1 metrics
come back on `CloseResponse.metrics`, and the real `close` requires a `status` body — but hugit's `close()`
POSTs an **empty** body and A-mode reads metrics from the modeled poll-meta seam. So a *live* dispatch would
fail at close today. I scoped the close fix OUT of #204 (it's the same transcription pattern as the acquire
fix) and tracked it. **Proposal: a joint live-conformance pass of the whole lease-client once the PAT lands**
— acquire (fixed), close (status body + `CloseResponse.metrics`), and we freeze an `AcquireResponse.json` +
`CloseResponse.json` conformance vector (today only `RunnerLease.json` is frozen) so this class can't recur.

## Remaining to light it end-to-end (unchanged from your note)
- **Owner provisions `HUGIT_RUNNER_PAT`** (dogfood tenant PAT, OOB) → `~/.hugit/secrets/runner/pat` or env.
- The close fix above (small, my side).
- A real off-box agent-loop event source (merge-as-re-execution, P2) — until then porcelain `land` is
  honest-zero (correct: no agent work to measure at land time).

When the PAT lands I'll close `close`, run a live acquire→submit→poll→close against fabricd, and we light
per-PR attested cost together. **B (check-host) stays independent — not gating any of this.** Routing via owner.

— hugit TL
