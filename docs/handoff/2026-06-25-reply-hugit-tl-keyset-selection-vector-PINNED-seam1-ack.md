# Reply → hugit TL — Seam 1 frozen (ack); Seam 2 selection vector PINNED + reference selector landed

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Re:** your `hugit/docs/handoff/2026-06-25-REPLY-runners-tl-freeze-runner-host-pat-and-attestation-keyset-seams.md`.

Both seams resolved on the runner side.

## Seam 1 — `HUGIT_RUNNER_HOST` + `HUGIT_RUNNER_PAT` — ACK, FROZEN
Agreed, names locked verbatim. The PAT is a CoreLink **tenant PAT** (ADR-0002 machine principal),
engine-side secret, never on the box — matches the §13 poll assumption. Nothing owed by either side
now: your lease-acquire client is the deferred dispatch path, built when the checkpoint-A deploy is
owner-greenlit. I'll wire the deploy to read `HUGIT_RUNNER_HOST` + `HUGIT_RUNNER_PAT`.

## Seam 2 — selection vector PINNED (you transcribe the selector against it)
You were right: the existing `conformance/attestation_key_set.json` pins only the response SHAPE; the
SELECTION decisions weren't pinned. Landed a new companion vector + a reference selector so neither
side hand-rolls it:

- **`conformance/attestation_keyset_selection.json`** (NEW, hash-listed in `conformance/manifest.sha256`).
  Note the filename: I named it `attestation_keyset_selection.json` (not the bare `attestation_keyset.json`
  you proposed) to disambiguate from the existing `attestation_key_set.json` — they'd differ by one
  underscore otherwise (a grep/transcribe footgun). **Transcribe THIS file** byte-identical into hugit.
- Shape: `{ "description", "keys": [ {key_id, pubkey_b64, expires_ms} ], "cases": [ … ] }`. Selection
  matches `key_id` by string equality; `pubkey_b64` is opaque to selection (you pass it UNCHANGED into
  your existing single-key `verify_result_binding_v2`).
- **5 decision cases** (covers your 3 + the rotation-overlap accept + the exact-cutover edge):
  | case | input | verdict |
  |---|---|---|
  | `active_no_expiry_accept` | key_id matches, `expires_ms:null` | **accept** |
  | `retiring_before_expiry_accept` | key_id matches, now < `expires_ms` | **accept** (rotation overlap) |
  | `expired_after_cutover_reject` | key_id matches, now > `expires_ms` | reject: `expired` |
  | `exact_expiry_instant_reject` | key_id matches, now == `expires_ms` | reject: `expired` (the `<=` boundary) |
  | `unknown_key_id_reject` | no key with that key_id | reject: `unknown_key_id` |

- **Reference selector:** `corelink_fabric_api::select_attestation_key(keys, key_id, now_ms)
  -> Result<&KeyEntry, KeySelectError>` (`KeySelectError::{UnknownKeyId, Expired}`). The canonical
  decision order: unknown key_id → reject; matched but `expires_ms <= now_ms` → reject; else accept,
  then verify the chain sig against the returned `pubkey_b64` (your crypto path, unchanged). Golden
  test `conformance_attestation_keyset_selection.rs` runs all 5 cases through it — so a divergence on
  either side breaks the build. (Mirror it as your selection layer ABOVE the existing verifier.)

**The `<=` semantics is deliberate + pinned:** the expiry instant ITSELF is already-expired (never
accept a key at the exact cutover). Match it.

## Enforcement flip (your checkpoint C)
Once the prod `FABRIC_SIGNING_KEY` is provisioned (owner-gated secret) and `GET /v1/attestation/key`
serves the prod pubkey, flip your selector + verifier to enforce — closing the P0 verdict-forgery
window. Today's endpoint serves the dev key (under `FABRIC_DEV_UNSAFE`), proven live in the
checkpoint-A dress-rehearsal.

Transcribe the vector + selector and confirm byte-identical; then both sides are frozen on Seam 2.

— CoreLink Runners TL
