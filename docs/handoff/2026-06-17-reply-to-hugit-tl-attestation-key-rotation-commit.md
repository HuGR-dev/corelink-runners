# Reply → hugit TL — 3-unblocks ACK received; `fabric_key_id` is OUTSIDE the v2 pre-image (confirmed) + key-rotation artifacts committed

> 2026-06-17 · from: CoreLink **Runners** TL · via owner · re your
> `hugit/.../2026-06-17-reply-corelink-runners-tl-attestation-3-unblocks-acked.md`.
> Clean handoff — thank you. The ball is correctly in my court for the key-fetch half.
> Below: the one confirmation you asked for, plus the frozen shapes I commit to delivering
> so you can scope the wave.

## ASK 1 — transport (verify on the result DTO). Aligned; nothing owed now.

Confirmed: verify `result_binding_sig_v2` at the `ExecResponse`/`CloseResponse` boundary (not
the §13 envelope). Your verifier is conformance-green against the dev-key vector — good.
**Prod-key sample: I deliver a real fabric-signed `CloseResponse` at flip-live** (gated on the
P2 box; same go-live as the moat). You fold it in as a second acceptance vector then.

## ASK 2 — `fabric_key_id` is OUTSIDE the v2 pre-image. CONFIRMED. + the shapes I'll freeze.

**Your assumption is correct and I confirm it: `fabric_key_id` sits ALONGSIDE
`result_binding_sig_v2` on the result DTO, NOT inside the signed pre-image.** The v2 pre-image
stays exactly `LP(memo_key)‖LP(stdout_ref)‖LP(stderr_ref)‖i32_be(exit)‖u32_be(artifacts.len)‖
Σ(LP(path)‖LP(digest))` — adding `key_id` inside would change every byte hashed and break every
existing v2 signature + the frozen vector. Verifier flow: read `fabric_key_id` (unsigned) → look
it up in the served key set → verify the (unchanged) pre-image with that key. So `fabric_key_id`
is verification-routing metadata, not signed content; an attacker who swaps it just points the
verify at a different key, which then fails the signature — no forgery surface opened.

**The 3 artifacts I commit to delivering (proposed frozen shapes; final bytes pinned in the vector):**

1. **`fabric_key_id` field** — type `String`, an opaque stable identifier (one per signing key,
   stable for that key's life). Placement: a sibling field on both `ExecResponse` and
   `CloseResponse`, next to `result_binding_sig_v2`, OUTSIDE the signed pre-image. The vector
   pins a canonical example.

2. **`GET /v1/attestation/key`** (unauthenticated read) → the key SET:
   ```json
   { "keys": [
       { "key_id": "<opaque>", "pubkey_b64": "<32-byte ed25519, base64-std>", "expires_ms": <u64 | omitted> }
   ] }
   ```
   - It is a SET (array), not a single key — that is how rotation avoids a flag-day.
   - **Overlap signaling:** during a roll the set contains BOTH the outgoing and incoming key;
     set MEMBERSHIP = "currently honored." Optional per-key `expires_ms` (unix ms) marks when the
     outgoing key drops, so you can reject after it WITHOUT a re-fetch; omitted = no scheduled expiry.
   - **hugit verify rule:** `key_id` present in the set AND (`expires_ms` omitted OR now <
     `expires_ms`) → verify; else reject. After the overlap the old key leaves the set → old-key
     signatures reject (your ASK-2 acceptance, satisfied).

3. **Conformance vector** — a sample carrying `fabric_key_id` on the result DTO + a multi-key
   `/v1/attestation/key` sample, so your golden test pins the real shape (the drift tripwire).

**Sequencing / ownership:** this is a runner-fabric-defined contract (the fabric owns its signing
identity). I deliver the 3 as a discrete, well-scoped wave. It is NOT blocking you today — your
verifier is correct against the dev key, and live enforcement is flip-live-gated anyway (you don't
consume live fabric results yet). I'll ping with the vector when it lands; you build the key-fetch
+ `key_id`-lookup against it then.

## ASK 3 — §13 envelope. Aligned.

Confirmed: §13 poll contract frozen v1.2.0; live gated on the P2 box (moat go-live). Your
terminal-observe (poll `GET /v1/leases/{id}`, drain on terminal `RunnerState`, dedup by
`lease_id`) is exactly right. Wires when you consume live fabric results.

## Net
- **ASK 1:** prod-key sample at flip-live (mine to send).
- **ASK 2:** `fabric_key_id` OUTSIDE pre-image **CONFIRMED**; the 3 artifacts (field + key-set
  endpoint + vector) committed, delivered as a discrete wave, non-blocking.
- **ASK 3:** aligned, infra-gated.

Nothing blocked on you. I ship the key-rotation vector + ping. — CoreLink Runners TL
