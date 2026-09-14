# → hugit techlead: §13.2 trajectory-ingest channel — fabric PROPOSAL (your agent emits)

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-14 · **Status:** PROPOSAL — the runner owns the mechanism per
§13.2, so this is "here is the channel your agent loop writes to", not a blocking
question. Confirm your in-box agent can emit this shape (or counter-propose).

---

## What §13.2 already gives us, and the one missing half

§13.2 obligation 1 says the runner provides *"a channel (e.g. a side-channel pipe,
a structured event stream, or **a callback endpoint**) through which the job's
agent loop can write raw transcript events"* — and explicitly **delegates the
mechanism to the runner**. The **subscribe** half (your `hugit-ledger::envelope`
producer pulling the bytes) is already live: `GET /v1/leases/{id}/envelope/events`
+ `/meta` (PULL, per your Q2a). The missing half is the **WRITE** path — how the
agent loop *running inside the box* streams events into the lease's `CaptureHook`.
Today nothing feeds it in production (the hook is registered at acquire but no
in-box producer writes to it).

## Fabric proposal — a lease-authenticated ingest endpoint

The runner exposes, and injects into the box:

- **Endpoint:** `POST /v1/leases/{id}/envelope/ingest` — authenticated by the
  lease's Bearer PAT (the same credential registered at acquire, contract §13.2
  "authenticated hook point"). The runner writes each posted event straight to the
  lease's `CaptureHook` (`hook.write`) — in-flight forward only, **never persisted**
  (§13.3). Bounded FIFO; over-capacity sets the existing `*_overflow` flag.
- **Box injection:** at provision the runner injects `CORELINK_ENVELOPE_INGEST_URL`
  + the lease credential into the box env, so your in-box agent loop knows where to
  stream without any out-of-band config.
- **Wire shape (one event per POST, or an NDJSON batch):** the `TranscriptEvent`
  variants the hook already accepts —
  ```jsonc
  { "kind": "model_turn", "bytes_b64": "<raw transcript bytes>", "usage": { … } | null, "busy_ms": <u64> }
  { "kind": "tool_call",   "bytes_b64": "…", "busy_ms": <u64> }
  { "kind": "tool_result", "bytes_b64": "…" }
  { "kind": "prompt",      "bytes_b64": "…" }
  ```
  Raw bytes are forwarded verbatim — the runner does **not** scrub (§13.3 redaction
  is your write-path job). `usage` carries the mandatory cache-split token counts
  when the model API reports them; `null` = unknown (never fabricated).
- **Close signal:** the existing close path already emits the §13.2-3 job-close
  signal (final `wall_ms`/`active_ms`, the ack window, `capture_incomplete` on
  timeout) — unchanged.

## What I need from you

1. **Confirm your in-box agent loop can emit to this endpoint** in this shape — or
   counter-propose the channel (a unix-domain socket mounted into the box is the
   main alternative if you'd rather not do HTTP from inside the box; say the word).
2. **The two-transcript imperative (§13.2):** the runner forwards the single raw
   stream + per-turn metadata; the *compaction* into `task_transcript_ref` is your
   summariser's job (§13.2 obligation 2). Confirm the per-turn `meta` we already
   surface (turn index, ts, tool name, token count) is sufficient for your
   compactor, or name the extra field.

## Status / non-blocking

I'm building the fabric side now (the ingest endpoint + box injection + the
hook-write wiring) behind the contract's delegated mechanism — it's additive and
needs no frozen-type change. This activates ADR-0004 **Phase 2b** (the per-turn
durable checkpoint, which writes on each ingested turn) and makes the §13 envelope
capture real in production instead of test-only. Your agent adopting the endpoint
is the last mile; the counter-proposal window is open until then.

— roteado via owner; nenhum `path`/`git`-dependency entre repos. Frozen contract
unchanged (§13.2 delegates the channel mechanism to the runner).
