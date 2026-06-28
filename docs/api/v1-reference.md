# CoreLink Runners — `/v1` HTTP API Reference

**Schema version:** 1.2.0 (envelope metrics, `cost_usd_micros`).
**Path constants:** frozen in `crates/corelink-fabric-api/src/paths.rs` (CF0 freeze item 3).
**Wire types:** transcribed in `crates/corelink-runners-contracts/src/` — no git dep; conformance via `conformance/` vectors.

---

## Table of contents

1. [Authentication](#authentication)
2. [Error vocabulary](#error-vocabulary)
3. [Lease lifecycle](#lease-lifecycle)
   - [POST /v1/leases](#post-v1leases)
   - [GET /v1/leases/{lease_id}](#get-v1leaseslease_id)
   - [POST /v1/leases/{lease_id}/cancel](#post-v1leaseslease_idcancel)
4. [Execution](#execution)
   - [POST /v1/leases/{lease_id}/exec](#post-v1leaseslease_idexec)
5. [Queue trigger](#queue-trigger)
   - [POST /v1/queue/trigger](#post-v1queuetrigger)
6. [Envelope (§13 turn-feed)](#envelope-13-turn-feed)
   - [GET /v1/leases/{lease_id}/envelope/events](#get-v1leaseslease_idenvelopeevents)
   - [GET /v1/leases/{lease_id}/envelope/meta](#get-v1leaseslease_idenvelopemeta)
   - [POST /v1/leases/{lease_id}/envelope/ingest](#post-v1leaseslease_idenvelopeingest)
   - [POST /v1/leases/{lease_id}/close](#post-v1leaseslease_idclose)
7. [Metrics](#metrics)
   - [GET /v1/metrics/tenant](#get-v1metricstenant)
8. [Attestation](#attestation)
   - [GET /v1/attestation/key](#get-v1attestationkey)
9. [Health](#health)
   - [GET /v1/health](#get-v1health)
10. [Verifying attestations](#verifying-attestations)
11. [Wire type reference](#wire-type-reference)

---

## Authentication

All routes except `GET /v1/health` and `POST /v1/leases/{lease_id}/envelope/ingest` require a **Bearer PAT**:

```
Authorization: Bearer <tenant-PAT>
```

The PAT resolves to a `TenantId` through the token-store seam. Every failure maps to the frozen error vocabulary — never to anonymous admission:

| Condition | Status |
|-----------|--------|
| Header absent or malformed | 401 `unauthorized` |
| Token unknown | 401 `unauthorized` |
| Token store unreachable | 503 `fail_closed` |

The `POST /v1/leases/{lease_id}/envelope/ingest` route uses a **per-lease scoped ingest token** instead of the tenant PAT (see [Envelope ingest](#post-v1leaseslease_idenvelopeingest)).

### Tenant isolation

A valid PAT touching another tenant's resource is `404 not_found`, **never** `403 forbidden`. A `403` would confirm the resource exists and leak tenancy. There is no existence oracle.

---

## Error vocabulary

All non-2xx responses share a single wire shape (`deny_unknown_fields`):

```json
{ "code": "<machine-code>", "message": "<human-readable, no client contract>" }
```

The `code` field is the frozen machine code. The `message` field is informational only — clients must switch on `code`, never on `message`.

| HTTP status | `code` | When |
|-------------|--------|------|
| 400 | `invalid` | Semantically rejected before any box/VM contact (unpinned image, bad status vocab, memo-key mismatch, etc.) |
| 401 | `unauthorized` | Missing or unknown Bearer PAT; missing or wrong ingest token |
| 404 | `not_found` | Resource does not exist for this tenant; cross-tenant access; `Pending` pre-wire state |
| 429 | `over_cap` | Concurrency cap or per-minute rate ceiling reached (preventive — no box spawned) |
| 503 | `fail_closed` | Control-plane dependency unreachable (token store, lease ledger, plan source, etc.); execution backend refused |

No `403 Forbidden` exists in this vocabulary by design.

---

## Lease lifecycle

A lease represents one isolated job slot. Lifecycle: `Pending → Held → (Released | Expired | Crashed)`. `Pending` is a pre-wire admission state, never visible on the wire. Acquire returns `Held` atomically.

### `POST /v1/leases`

Acquire a lease (contract §1 "Acquire"). The concurrency cap and rate ceiling are enforced here, **before** any box/VM is spawned.

**Auth:** Bearer PAT  
**Request body:** `AcquireRequest` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `image_digest` | `String` | Pinned image reference — **must** start with `sha256:`. Unpinned images are rejected 400 before any box contact. |
| `net_policy` | `String` | Network policy name governing the runner's outbound access. |
| `tmp_root` | `String` | Temporary root directory requested for this runner. |
| `expiry_ms` | `u64` | Requested lease TTL in milliseconds. The fabric converts this to an absolute Unix epoch-ms deadline (`RunnerLease.expiry`). Expiry is fail-closed. |

**Response body (200):** `AcquireResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `lease` | `RunnerLease` | The granted lease, wire-conformant to the frozen `RunnerLease` type (conformance vector `conformance/RunnerLease.json` is the oracle). |
| `exec_endpoint` | `String` | The exec path for this lease (`/v1/leases/{lease_id}/exec`). |

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Lease granted; state is `Held`. |
| 400 `invalid` | Unpinned image digest, invalid `net_policy`, or unsafe `tmp_root`. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 429 `over_cap` | Tenant's concurrency cap or 60-second rate ceiling reached. |
| 503 `fail_closed` | Plan source unreachable, ledger unavailable, or box provisioning failed. |

---

### `GET /v1/leases/{lease_id}`

Fetch the current lifecycle state of a lease. Mirrors the CP1 ledger exactly — no invented intermediate states.

**Auth:** Bearer PAT  
**Path parameters:** `lease_id` — the opaque lease id returned by acquire.  
**Request body:** none

**Response body (200):** `StatusResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `lease_id` | `String` | The lease id. |
| `state` | `RunnerState` | Current lifecycle state. One of: `"held"`, `"released"`, `"expired"`, `"crashed"`. |

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Lease found and owned by the authenticated tenant. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease does not exist for this tenant (also returned for another tenant's lease and for `Pending` records). |
| 503 `fail_closed` | Ledger unavailable. |

---

### `POST /v1/leases/{lease_id}/cancel`

Release a held lease and initiate forensic teardown. Idempotent on an already-`Released` lease (returns `released: true` again, no second transition). Terminal states (`Expired`, `Crashed`) cannot be cancelled — the legal matrix forbids it.

**Auth:** Bearer PAT  
**Path parameters:** `lease_id`  
**Request body:** none (empty body)

**Response body (200):** `CancelResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `lease_id` | `String` | The lease id. |
| `released` | `bool` | `true` if the lease is now `Released`. |
| `forensic_clean` | `bool` | `true` if teardown left the box forensically clean. Honest placeholder: real forensic oracle is attached at API3; never fabricated `true`. |

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Cancel accepted; `released: true`. |
| 400 `invalid` | Lease is terminal (`Expired` or `Crashed`); the legal matrix forbids release. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease does not exist for this tenant. |
| 503 `fail_closed` | Ledger unavailable. |

---

## Execution

### `POST /v1/leases/{lease_id}/exec`

Execute a `CheckDef` inside the leased box/VM and return the frozen `CheckResult` (contract §3).

Gate order (none skippable):
1. Tenant scope — unknown and cross-tenant leases are the same 404.
2. Held only — terminal states → 400.
3. Expired-at-exec-time — if `now >= deadline`, 400 even if the ledger still reads `Held`; an expired job performs zero work and stores nothing.
4. Execution — via the `LeasedExec` port. Any backend refusal → 503; results are never fabricated.
5. Attestation — every result carries a signed `AttestationChain` plus result-binding signatures. A result without an attestation is unrepresentable on the wire.

**Auth:** Bearer PAT  
**Path parameters:** `lease_id`  
**Request body:** `ExecRequest` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `check_def` | `CheckDef` | The check to execute. See [CheckDef](#checkdef). |
| `tree_hash` | `String` | Merkle root hash of the workspace snapshot (lowercase hex). First memo axis of `CheckResult.memo_key`. |

**Response body (200):** `ExecResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `result` | `CheckResult` | The execution result. Same `CheckDef` over the same inputs must be byte-identical (contract §3). See [CheckResult](#checkresult). |
| `attestation` | `AttestationChain` | Signed provenance chain: `tree` = workspace snapshot ref, `def` = check-definition digest, `runner` = executor identity, signed with the published fabric key. See [AttestationChain](#attestationchain). |
| `result_binding_sig` | `String` | **v1** result-binding signature. Detached base64 ed25519 signature over `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)` of `result`. Does **not** cover `exit` or `artifacts`. |
| `result_binding_sig_v2` | `String` | **v2** full-outcome result-binding signature. Covers `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref) ‖ i32_be(exit) ‖ u32_be(artifacts.len) ‖ ∀ artifact: LP(path) ‖ LP(digest)`. Closes the forgeable-verdict gap. Additive alongside v1; `serde(default)` — older payloads deserialize to an empty string. |

Both `attestation` and `result_binding_sig` are **required** fields (contract §7 `no_attestation_no_result_fail_closed`).

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Check executed and attested. |
| 400 `invalid` | Lease not held (released/expired/crashed), or lease deadline has passed. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease does not exist for this tenant. |
| 503 `fail_closed` | Ledger unavailable; execution backend refused; no image digest on file; lease terminalized concurrently during execution. |

---

## Queue trigger

### `POST /v1/queue/trigger`

hugit's landing queue triggers execution of an uncached check on demand (contract §9, `QueueApi` seam). Uses the same execution engine as the exec path with the same gate order and attestation obligations.

The concurrency cap was enforced at acquire. The trigger is lease-scoped — no cap re-check here.

**Idempotency:** The fabric dedups on `(tenant, entry.item_id, tree_hash)`. A duplicate delivery returns the same attested `TriggerResponse` byte-identically without re-executing. Dedup is bounded in-memory (cap: 4096 entries); at the cap, new results are served but not memoized — later duplicates re-execute (correct under at-least-once semantics, merely wasteful).

**Auth:** Bearer PAT  
**Request body:** `TriggerRequest` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `entry` | `LandableEntry` | The landable queue entry. See [LandableEntry](#landableentry). |
| `check_def` | `CheckDef` | The check to execute. |
| `tree_hash` | `String` | Merkle root hash of the workspace snapshot (lowercase hex). First memo axis. |
| `lease_id` | `String` | The lease (already acquired via `POST /v1/leases`) whose box executes the check. |

**Response body (200):** `TriggerResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `item_id` | `String` | The queue item id echoed from `entry.item_id` (for queue correlation under at-least-once delivery). |
| `result` | `CheckResult` | The execution result. Byte-identical on duplicate delivery. |
| `attestation` | `AttestationChain` | Signed provenance chain — same coverage map, same fabric key as `ExecResponse`. Byte-identical on duplicate delivery. |
| `result_binding_sig` | `String` | **v1** result-binding signature — same pre-image and verification as `ExecResponse.result_binding_sig`. |
| `result_binding_sig_v2` | `String` | **v2** full-outcome result-binding signature — same pre-image and verification as `ExecResponse.result_binding_sig_v2`. Additive; `serde(default)`. |

`attestation` and `result_binding_sig` are **required** fields (ATT parity amendment: one execution engine, one §7 obligation).

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Trigger executed and attested (or returned from dedup map). |
| 400 `invalid` | Lease not held; lease deadline passed. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease does not exist for this tenant. |
| 503 `fail_closed` | Ledger unavailable; execution backend refused; no image digest on file. |

---

## Envelope (§13 turn-feed)

The §13 envelope surfaces expose the in-box agent loop's raw transcript. All surfaces are **bounded in-flight only** — nothing is persisted (§13.3). Overflow is honest: the mechanism sets an overflow flag rather than silently dropping.

There are two trust boundaries with two distinct credentials:
- **Poll (GET events/meta):** hugit's trusted subscriber authenticates with the same Bearer PAT that acquired the lease.
- **Ingest (POST ingest):** the untrusted in-box agent loop authenticates with a per-lease, write-only, ingest-scoped capability token (never the tenant PAT).

### `GET /v1/leases/{lease_id}/envelope/events`

Drain-and-release the raw transcript events surface (§13.2 surface 1). Returns the batch currently in-flight; each poll releases those entries from the mechanism's bounded surface.

**Auth:** Bearer PAT (same tenant that acquired the lease)  
**Path parameters:** `lease_id`  
**Request body:** none

**Response body (200):**

```json
{ "events": ["<base64>", "<base64>", ...] }
```

| Field | Type | Description |
|-------|------|-------------|
| `events` | `Vec<String>` | Drained raw transcript event bytes, oldest first, each standard-alphabet base64-encoded. |

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Poll succeeded (may be empty if no events in flight). |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease not found for this tenant. |
| 503 `fail_closed` | Hook refused the registered credential (internal inconsistency). |

---

### `GET /v1/leases/{lease_id}/envelope/meta`

Drain-and-release the per-turn metadata surface (§13.2 surface 2).

**Auth:** Bearer PAT  
**Path parameters:** `lease_id`  
**Request body:** none

**Response body (200):**

```json
{
  "meta": [
    {
      "turn_index": 0,
      "timestamp_ms": 1234,
      "tool": "Edit",       // omitted if not a tool event
      "tokens": 512         // omitted if no usage reported
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `meta` | `Vec<TurnMetaDto>` | Drained `TurnMeta` entries, oldest first. |

**`TurnMetaDto` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `turn_index` | `u64` | Monotone per-hook turn index (0-based). |
| `timestamp_ms` | `u64` | Milliseconds since the hook opened. |
| `tool` | `String?` | Tool name — present only for a tool-call event. |
| `tokens` | `u64?` | Derived token total — present only when the event carried usage. |

**Status codes:** same as events poll above.

---

### `POST /v1/leases/{lease_id}/envelope/ingest`

The §13.2 trajectory turn-feed WRITE side (ENV3). The in-box agent loop forwards its transcript events here. Events are written into the lease's capture hook in-flight only — never persisted.

**Auth:** Per-lease scoped ingest token — `Authorization: Bearer <ingest-token>`. This is NOT the tenant PAT. The token is computed by the fabric at acquire time as `HMAC(ingest-secret, lease_id)` and injected into the box env as `CORELINK_ENVELOPE_INGEST_CREDENTIAL`. An exfiltrated token can only authorize ingest to that one (soon-dead) lease — no tenant takeover.

The handler recomputes and constant-time compares the expected token for the request's `{lease_id}`. A missing, wrong, or another-lease's token is `401 unauthorized`. A wrong token for a real lease and any token for a non-existent lease are byte-identical 401s (no existence oracle).

**Path parameters:** `lease_id`  
**Request body:** one of:
- A single `IngestEvent` JSON object
- A JSON array of `IngestEvent` objects
- NDJSON (one `IngestEvent` JSON object per non-blank line)

**`IngestEvent` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `kind` | `String` | Event discriminator: `"model_turn"` \| `"tool_call"` \| `"tool_result"` \| `"prompt"`. |
| `bytes_b64` | `String` | Raw transcript bytes, standard-alphabet base64. |
| `tool` | `String?` | Tool name — required for `tool_call` and `tool_result`. |
| `usage` | `IngestUsage?` | Per-turn token usage. `null` or absent = no usage reported. Totals are derived at finalize, never supplied. |
| `busy_ms` | `u64` | Model/tool busy span in ms (default `0`). |

**`IngestUsage` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `input` | `u64` | Input (non-cached) tokens. |
| `output` | `u64` | Output tokens. |
| `cache_read` | `u64` | Tokens read from prompt cache. |
| `cache_write` | `u64` | Tokens written to prompt cache. |

**Response body (200):** empty body (HTTP 200 status only).

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | All events written into the capture hook. |
| 400 `invalid` | Empty event batch; unknown `kind`; malformed base64; `tool_call`/`tool_result` missing `tool`; unparseable body. |
| 401 `unauthorized` | Missing, wrong, or forged ingest token. |
| 404 `not_found` | Lease has no live hook (closed, reaped, or never registered). |
| 503 `fail_closed` | Capture hook refused the event (closed or not-yet-open). |

---

### `POST /v1/leases/{lease_id}/close`

Drive the §13.2 item-3 job-close machinery and release the lease (ENV2). This is the **only** correct way to release an agent-job lease; it delivers metrics and result atomically.

Gate order (none skippable):
1. Tenant scope — unknown and cross-tenant leases are the same 404.
2. Held only — terminal states → 400 (a double-close lands here because the first close released the lease).
3. Status vocabulary — only `"succeeded"` or `"failed"` accepted. `"killed"` is the fabric's own abnormal-path verdict.
4. `check_result.memo_key` integrity — if a result is supplied, its `memo_key` must equal `lower_hex(SHA-256(LP(tree_hash) ‖ LP(def_digest) ‖ LP(toolchain_digest)))`. The fabric will attest this result; a lying `memo_key` is rejected before any side effect.
5. Teardown first — the box is torn down before the lease is terminalized. A failed teardown returns 503 with the lease still `Held` so a retry (reaper sweep or re-close) can recover cleanly.
6. Close machinery — runs after teardown. The §13.2 exactly-once close signal fires; ack window honored.
7. `Held → Released` — only after the outcome, never before (`lease_not_released_before_close_signal_published`).

**Auth:** Bearer PAT  
**Path parameters:** `lease_id`  
**Request body:** `CloseRequest` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `status` | `String` | Terminal job status: `"succeeded"` or `"failed"`. |
| `check_result` | `CheckResult?` | The job's `CheckResult`, if the close delivers one. Echoed back in the response in the same atomic step as the metrics (§13.1 delivery rule). |
| `cost_usd_micros` | `u64?` | The **provider-billed** total cost of the lease's work in micro-USD (`1 USD = 1_000_000`), read from the provider's `/usage` by the caller. Additive; `serde(default)`. The fabric **records it verbatim** into `metrics.cost_usd_micros` — it never recomputes or price-cards it (owner 2026-06-27 provider-billed re-decision, #64). Absent ⇒ the honest-zero derived floor stands. Rides the same atomic close payload as the §13.1 token metrics. |

**Response body (200):** `CloseResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `lease_id` | `String` | The lease id. |
| `released` | `bool` | `true` — the lease reached `Released` as a result of this call. The transition happens only after the close machinery produced its outcome. |
| `capture_incomplete` | `bool` | `true` iff transcript capture was lossy or unconfirmed (overflow, undrained residue, or a missed ack window). Honest — never silent. |
| `metrics` | `IntentMetrics` | Finalized §13.1 per-job metrics. **Required** — never `null` (schema 1.2.0). See [IntentMetrics](#intentmetrics). |
| `check_result` | `CheckResult?` | The `CheckResult` echoed from the request. |
| `attestation` | `AttestationChain` | **Required.** When `check_result` is present, the chain links are that result's `tree_hash`/`def_digest`/`runner_ref`. When absent, all links are empty strings — the honest "no result claimed" attestation, still signed. |
| `result_binding_sig` | `String` | **Required.** v1 result-binding signature over the echoed result's `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)` (empty frames when no result). |
| `result_binding_sig_v2` | `String` | v2 full-outcome result-binding signature (covers `exit` + ordered `artifacts`; empty-outcome frames when no result). Additive; `serde(default)`. |

`metrics`, `attestation`, and `result_binding_sig` are **required** fields.

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Close succeeded; lease is `Released`. |
| 400 `invalid` | Lease not held; invalid status vocab; `check_result.memo_key` does not match its input axes. |
| 401 `unauthorized` | Missing or unknown PAT. |
| 404 `not_found` | Lease does not exist for this tenant. |
| 503 `fail_closed` | Ledger unavailable; teardown failed (lease stays `Held`, retry is safe); close machinery panicked; concurrent cancel/reaper won the ledger race (no double-free). |

---

## Metrics

### `GET /v1/metrics/tenant`

The authenticated tenant's own per-tenant wait statistics (contract §6 non-interference surface). No parameter to ask for another tenant's metrics — cross-tenant reads are unrepresentable.

**Auth:** Bearer PAT  
**Request body:** none

**Response body (200):**

| Field | Type | Description |
|-------|------|-------------|
| `tenant` | `String` | The authenticated tenant id (the PAT's resolved tenant). |
| `p50_ms` | `u64` | Nearest-rank p50 wait time in ms (`0` when `count == 0`). |
| `p95_ms` | `u64` | Nearest-rank p95 wait time in ms (`0` when `count == 0`). |
| `histogram` | `[u64; 6]` | Bucket counts: `[<10ms, <50ms, <250ms, <1s, <5s, >=5s]`. |
| `count` | `u64` | Number of samples in the bounded window. |

Wait samples are populated by the queued-admission loop (`FABRIC_ADMISSION_MODE=queue`). Under the default `reject` mode, no admission loop runs and `count` is honestly `0`.

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Snapshot returned (may have `count: 0` if no samples). |
| 401 `unauthorized` | Missing or unknown PAT. |
| 503 `fail_closed` | Wait-stats lock poisoned. |

---

## Attestation

### `GET /v1/attestation/key`

Retrieve the fabric's published well-known ed25519 public key (ATT2). Every `AttestationChain.sig`, `result_binding_sig`, and `result_binding_sig_v2` emitted by this fabric verifies against this key.

Key custody per ratified decision #2: one ed25519 fabric signing key per region (M1: single region).

**Auth:** Bearer PAT  
**Request body:** none

**Response body (200):** `AttestationKeyResponse` (`deny_unknown_fields`)

| Field | Type | Description |
|-------|------|-------------|
| `ed25519_pubkey_b64` | `String` | The fabric's 32-byte ed25519 public key, standard-base64 encoded (RFC 4648 §4, padded). |

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Key returned. |
| 401 `unauthorized` | Missing or unknown PAT. |

---

## Health

### `GET /v1/health`

Liveness / readiness probe. **Unauthenticated.** Reports nothing tenant-scoped. Mounted outside the global in-flight concurrency limiter so it answers even under saturation — an LB or orchestrator can always probe liveness without being shed.

**Auth:** none  
**Request body:** none  
**Response body (200):** `"ok"` (plain text or minimal JSON — implementation detail, not a contract)

**Status codes:**

| Status | Condition |
|--------|-----------|
| 200 | Fabric process is alive. |

---

## Verifying attestations

### Verification workflow

1. Fetch the fabric's public key from `GET /v1/attestation/key`. Cache it; rotate on key-change events.
2. Decode `ed25519_pubkey_b64` (standard base64, 32 bytes).
3. For each `ExecResponse`, `TriggerResponse`, or `CloseResponse`:
   - Verify the `AttestationChain.sig` over the frozen pre-image:
     ```
     LP(tree) ‖ LP(def) ‖ LP(runner) ‖ LP(model) ‖ VEC(principal)
     where LP(s)  = u32_be(byte_len(s)) ‖ utf8_bytes(s)
           VEC(v) = u32_be(elem_count(v)) ‖ LP(v[0]) ‖ LP(v[1]) ‖ …
     ```
   - Verify `result_binding_sig` (**v1**) over:
     ```
     LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
     ```
   - Verify `result_binding_sig_v2` (**v2**) over:
     ```
     LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
       ‖ i32_be(exit)
       ‖ u32_be(artifacts.len)
       ‖ ∀ artifact in order: LP(path) ‖ LP(digest)
     ```
     v2 covers the pass/fail verdict (`exit`) and all output digests (`artifacts`). Use v2 when the verdict matters (i.e., always for production).

### `corelink verify`

The CLI command `corelink verify` automates steps 1–3. It fetches the key, validates the chain, and checks both binding signatures against the result payload. See the CLI reference (`docs/cli.md`) for usage.

All three signatures verify against the same ed25519 key published at `GET /v1/attestation/key`.

---

## Wire type reference

### `RunnerLease`

| Field | Type | Description |
|-------|------|-------------|
| `lease_id` | `String` | Unique lease identifier. |
| `principal_chain` | `Vec<String>` | Ordered chain of principals (agent ids / user ids) that own this lease. |
| `path_set` | `Vec<String>` | Set of filesystem paths this lease grants access to. |
| `expiry` | `u64` | Unix epoch milliseconds at which this lease expires. |
| `net_policy` | `String` | Network policy name governing this runner's outbound access. |
| `tmp_root` | `String` | Temporary root directory allocated to this runner. |
| `state` | `RunnerState` | Current lifecycle state. |

### `RunnerState`

Serialized as a snake_case string. One of: `"held"`, `"expired"`, `"crashed"`, `"released"`.

### `CheckDef`

| Field | Type | Description |
|-------|------|-------------|
| `def_digest` | `String` | SHA-256 hex digest of the definition body. Second axis of the memo key. |
| `command` | `String` | The command to execute (argv[0] + args). |
| `inputs` | `Vec<String>` | Declared input paths / globs that affect the check. |
| `toolchain_ref` | `String` | Reference to the toolchain (content-addressed digest or version string). |
| `env_manifest` | `String` | Reference to an environment manifest (content-addressed blob ref). |
| `glob_set` | `Vec<String>` | File glob patterns scoping materialization for this check. |

### `CheckResult`

| Field | Type | Description |
|-------|------|-------------|
| `memo_key` | `String` | Memoisation key (64-char lowercase hex). Frozen formula: `lower_hex(SHA-256(LP(tree_hash) ‖ LP(def_digest) ‖ LP(toolchain_digest)))`. |
| `tree_hash` | `String` | Merkle root hash of the workspace snapshot (lowercase hex). First memo axis. |
| `def_digest` | `String` | SHA-256 hex digest of the definition body. Second memo axis. |
| `toolchain_digest` | `String` | Content-addressed digest of the toolchain. Third memo axis. |
| `exit` | `i32` | Process exit code (0 = success). |
| `artifacts` | `Vec<Artifact>` | Output artifacts: ordered `(path, digest)` pairs. |
| `stdout_ref` | `String` | Content-addressed ref to captured stdout blob. |
| `stderr_ref` | `String` | Content-addressed ref to captured stderr blob. |
| `duration_ms` | `u64` | Wall-clock duration of the check in milliseconds. |
| `runner_ref` | `String` | Reference to the runner that executed this check. |
| `produced_at` | `u64` | Unix epoch milliseconds when this result was produced. |

### `Artifact`

| Field | Type | Description |
|-------|------|-------------|
| `path` | `String` | Relative output path of the artifact. |
| `digest` | `String` | SHA-256 hex digest of the artifact content. |

### `AttestationChain`

| Field | Type | Description |
|-------|------|-------------|
| `tree` | `String` | Content-addressed ref to the workspace tree link of the chain. |
| `def` | `String` | Content-addressed ref to the check-definition link of the chain. |
| `runner` | `String` | Content-addressed ref to the runner link of the chain. |
| `model` | `String` | Content-addressed ref to the model link of the chain. |
| `principal` | `Vec<String>` | Ordered chain of principals in the provenance chain. |
| `sig` | `String` | Detached base64-encoded ed25519 signature over the frozen pre-image (see [Verifying attestations](#verifying-attestations)). |

### `IntentMetrics`

Schema 1.2.0 (transcribed from hugit-contracts @ 443ff1b).

| Field | Type | Description |
|-------|------|-------------|
| `tokens` | `TokenCounts` | Token spend with the cache split. |
| `wall_ms` | `u64` | Born → die wall-clock, ms. |
| `active_ms` | `u64` | Model + tool busy time, ms (excludes idle). |
| `tool_calls` | `u64` | Total tool calls. |
| `tool_breakdown` | `Vec<ToolCount>` | Per-tool call breakdown. |
| `model_turns` | `u64` | Number of model turns. |
| `cost_usd_micros` | `u64` | The **provider-billed** cost of the job in integer micro-USD (`1 USD = 1_000_000`), as submitted on `CloseRequest.cost_usd_micros` and recorded verbatim (owner 2026-06-27 provider-billed re-decision, #64) — or the honest-zero floor when none was submitted. NOT a fabric price-card multiply; NOT necessarily what the customer is billed. Bit-exact integer minor units (schema 1.2.0). |

### `TokenCounts`

| Field | Type | Description |
|-------|------|-------------|
| `input` | `u64` | Input tokens (non-cached). |
| `output` | `u64` | Output tokens. |
| `cache_read` | `u64` | Tokens read from prompt cache. |
| `cache_write` | `u64` | Tokens written to prompt cache. |
| `total` | `u64` | Total tokens. |

### `ToolCount`

| Field | Type | Description |
|-------|------|-------------|
| `tool` | `String` | Tool name (e.g. `"Edit"`, `"Bash"`). |
| `count` | `u64` | Number of calls to this tool. |

### `LandableEntry`

| Field | Type | Description |
|-------|------|-------------|
| `item_id` | `String` | Unique identifier of the queue item. |
| `intent_id` | `String` | Identifier of the intent that produced this item. |
| `tree_hash` | `String` | Merkle root hash of the item's workspace snapshot (lowercase hex). |
| `order_index` | `u64` | Position of this item in the landing order. |

---

*Source of truth: `crates/corelink-fabric-api/src/paths.rs`, `crates/corelink-fabric-api/src/dto.rs`, `crates/corelink-runners-contracts/src/`, `crates/corelink-fabric-server/src/handlers/`. Conformance vectors: `conformance/RunnerLease.json`, `conformance/FenceManifest.json`, `conformance/manifest.sha256`.*
