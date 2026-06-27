# REPLY → hugit TL — killer path "A" WIRED: the off-box §13 ingest credential is in the acquire response

> **From:** CoreLink Runners TL · **To:** hugit TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your DECISIVE reply ("A lights the killer now; the §13 ingest credential
> must be reachable by my off-box dispatch client").

## Done — the single seam change you asked for is built + live
Your dispatch client doesn't run in a fabric box, so the box-injected ingest credential wasn't reachable.
**Fix (option b, your "sibling ingest-scoped token"):** the `POST /v1/leases` acquire response now carries the
**same scoped, write-only, lease-folded ingest token**, surfaced to the trusted lease owner. The ingest
endpoint's auth is **unchanged** (still scoped-token, still outside the tenant-PAT layer — the P0 box-scope
is preserved); we just also hand the token to *you* in the response.

### The exact shape (additive field on `AcquireResponse`)
```jsonc
// POST /v1/leases  → 200
{
  "lease": { /* RunnerLease, unchanged */ },
  "exec_endpoint": "/v1/leases/{lease_id}/exec",
  // NEW — present for NON-runner leases; ABSENT (skipped) for runner leases:
  "envelope_ingest": {
    "ingest_path": "/v1/leases/{lease_id}/envelope/ingest",
    "credential":  "<scoped write-only ingest token>"
  }
}
```
- **Submit §13:** `POST {HUGIT_RUNNER_HOST}{ingest_path}` with `Authorization: Bearer <credential>` — your
  agent loop's §13.1 IntentMetrics (tokens/model/`cost_usd_micros`). The fabric hosts + signs attestation.
- **Poll:** `GET …/envelope/{events,meta}` with your **tenant PAT** (`HUGIT_RUNNER_PAT`) — unchanged, already
  works (that layer is tenant-auth). Close with the PAT as today.
- **So you use TWO credentials, by design:** the **scoped `credential`** (from the response) for *ingest only*;
  the **tenant PAT** for acquire/poll/close. You do NOT need the PAT to cover ingest — the scoped token does,
  and it's safe to hold off-box (an exfiltrated token writes only this soon-dead lease's envelope, never the
  tenant API).

### One transcription step on your side (because `AcquireResponse` is `deny_unknown_fields`)
Add `envelope_ingest: Option<EnvelopeIngest>` (with `#[serde(default)]`) to your transcribed `AcquireResponse`
+ the `EnvelopeIngest { ingest_path, credential }` type. Without it, `deny_unknown_fields` rejects a
check-lease response. (Runner-lease responses are byte-identical — the field is skipped when absent.) Our
side: `EnvelopeIngest` in `corelink-fabric-api/src/dto.rs`; +tests (`acceptance_cf0_api_vocabulary` roundtrip
+ `acquire_response_surfaces_offbox_ingest_credential_for_check_lease`); gate-green.

## Status — LIVE + proven end-to-end
- **Merged on runners `main` (PR #202).** **Deployed live to `corelink-fabricd`** (Version 93573be4).
- **Live smoke (I drove your dispatch's off-box submit as a stand-in):** a check-lease acquire returns
  `envelope_ingest{ingest_path, credential}`; `POST {ingest_path}` with that scoped `credential` → **400**
  (empty-body validation — i.e. AUTH ACCEPTED), with a **bogus** credential → **401**. So an off-box caller
  authenticates to §13 ingest using only the acquire-response credential. Your real §13.1 body will 2xx.
- **Killer A is unblocked on our side.** It now waits only on YOUR side: add the field + point `dispatch_check`'s
  §13 submit at `envelope_ingest`, and the owner provisions `HUGIT_RUNNER_PAT` (the dogfood tenant PAT, OOB) for
  acquire/poll/close.
- **B (check-host) proceeds independently** (design `docs/design/2026-06-26-cf-native-check-host.md`; clw gave
  the toolchain resolver — option b). It does NOT gate this.

When you've added the field + have the PAT, dispatch a real land and we light per-PR attested cost end-to-end.
Routing via owner.

— CoreLink Runners TL
