# HANDOFF → hugit TL — the four lease-DTO conformance vectors are published on `main`; mirror them byte-identically (the drift tripwire)

> **TO:** hugit TL · **FROM:** CoreLink Runners TL · **cc:** owner · **Relay:** owner · **DATE:** 2026-06-30
> **RE:** the conformance-vector pass you asked for after the 3 lease-client wire-drifts (acquire-req, acquire-resp, close).

## Done on my side — published + golden-tested on `corelink-runners` `main`
Four byte-frozen vectors under `conformance/`, each with a golden byte-exact test (`crates/corelink-fabric-api/tests/conformance_lease_dtos.rs`) and a `manifest.sha256` entry:

| Vector | sha256 (first 12) | Exercises (additive fields) |
|---|---|---|
| `conformance/AcquireRequest.json`  | `56f8bba592ac` | `runner` (RunnerSpec, Repo target + labels) **+** `toolchain_digest` |
| `conformance/AcquireResponse.json` | `e408336d64d0` | `envelope_ingest` (§13.2 off-box ingest cred) |
| `conformance/CloseRequest.json`    | `4160dc1d853b` | `cost_usd_micros: 4200000` **+** `check_result` |
| `conformance/CloseResponse.json`   | `b02ad893e0e7` | non-zero `metrics`, `check_result`, `attestation` + both sigs + `fabric_key_id` |

## Your side (the lockstep)
1. **Copy the four `conformance/*.json` files verbatim** into the hugit repo's conformance dir (byte-identical — same law as `RunnerLease.json` / `result_binding_v2.json`).
2. **Add a golden test** on the hugit lease-client: deserialize each vector into your transcribed DTO, re-serialize, assert **byte-identical** to the committed vector. Any divergence on either side trips the wire (no silent drift).
3. **Ping me if any field doesn't round-trip on your side** — that's a real drift to reconcile NOW (better than the 4th production drift).

## Scope note (no overclaim)
These pin the **wire SHAPE** (field names, types, nesting, serde behavior) — NOT verifiable signatures. The `attestation.sig` / `result_binding_sig` / `result_binding_sig_v2` values are illustrative base64 placeholders; reproducible dev-key signature verification lives separately in `conformance/result_binding_v2.json`. (Documented in the test header.)

## After you mirror them
The four lease DTOs are frozen byte-identical in both repos — the 3-wire-drift history ends. This is a separate, ungated workstream; it does not block the cost killer (your provider-`/usage` loop is the remaining piece there).

— CoreLink Runners TL
