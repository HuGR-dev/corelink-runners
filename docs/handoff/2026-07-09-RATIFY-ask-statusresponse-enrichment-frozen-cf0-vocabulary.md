# RATIFY ask → owner / hugit-TL: enrich the frozen `StatusResponse` (GET /v1/leases/{id})

**From:** corelink-runners TL · **Date:** 2026-07-09 · **Type:** frozen-contract change, HELD for ratification (NOT shipped unilaterally)

## What this is
During the Stage-C console-read-API wave, an agent enriched `StatusResponse`
(the `GET /v1/leases/{id}` body) with 4 fields for the customer console:
`created_at_ms`, `updated_at_ms`, `deadline_ms`, `box_ref`. **I did NOT merge that
part** — `StatusResponse` is a **CF0-frozen API-vocabulary type** with
`#[serde(deny_unknown_fields)]` (`crates/corelink-fabric-api/src/dto.rs:191`,
pinned by `acceptance_cf0_api_vocabulary.rs`). Per CLAUDE.md ("the DTOs are the
frozen transcriptions… never edit hugit's expectations unilaterally") and the M1
precedent ("NO unilateral frozen-contract edit"), a frozen `deny_unknown_fields`
type must not change without ratification — the point of the freeze is that it does
not move unilaterally, even for an additive change.

## Why it's safe (the case FOR ratifying)
- **No cross-repo tripwire:** there is NO `StatusResponse` conformance vector (the
  byte-identical cross-repo drift tripwires are only for RunnerLease / AcquireRequest
  / Close* / AgentExec* / IntentMetrics / etc.). `StatusResponse` is server-side API
  vocabulary, not a conformance-pinned wire type.
- **Not in hugit's documented seam:** `docs/spec/hugit-integration-contract.md` +
  `docs/interop.md` do NOT list `GET /v1/leases/{id}` / `StatusResponse` as a
  hugit-consumed path (their seam is acquire / envelope / close / exec). grep = empty.
- **Additive + server-produced:** the fabric only ever SERIALIZES `StatusResponse`;
  the risk is solely a consumer that transcribes it with `deny_unknown_fields`.
- **Values are honest ledger mirrors** (from `LeaseRecord`), not invented.

## Why I held it (the case for CAUTION)
- `deny_unknown_fields` on a frozen vocabulary type exists precisely to CATCH drift;
  making the change pass required editing the frozen-vocab tripwire test — the
  "modify the guard to fit the change" shape, which I refuse to do unilaterally.
- If ANY consumer (hugit, now parked; a future SDK) transcribes `StatusResponse`
  with `deny_unknown_fields`, the new fields would 400 their parse.

## The ask
**Owner / hugit-TL: ratify (or reject) enriching `StatusResponse` with
`{created_at_ms, updated_at_ms, deadline_ms, box_ref}`.**
- **If ratified:** I merge the enrichment + update the frozen-vocab test additively,
  and (if hugit ever consumes it) note the additive change in the integration contract.
- **If rejected / deferred to a new type:** the console reads the same detail from
  `GET /v1/leases` (the `LeaseEntry` list — which I DID safely enrich with `box_ref`
  this wave, since it's a local server struct, not the frozen vocabulary), or via a
  NEW additive `/v1/leases/{id}/detail` type that leaves `StatusResponse` frozen.

The safe half shipped this wave; only the frozen-type enrichment waits on your word.
Branch with the held change is preserved: `feat/wp-b1b-lease-detail` (origin).

— corelink-runners TL
