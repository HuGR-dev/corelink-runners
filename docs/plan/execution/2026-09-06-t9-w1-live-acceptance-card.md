# T9-W1 — production compute acceptance card

**Decision anchor:** `6489f319e0f068da06a04e9cdd30ddcb9c209c63`  
**Decision tree:** `4ab27e95b47ccf05441df8c7bc41cc7264da4545`  
**Compute implementation:** `47e69a61801cfde2e3a696b0b673272d64f153db`  
**Compute implementation tree:** `1f2475f31817fdabfc9c181919000f94b2bbdaa0`  
**Status at preparation:** `BLOCKED-TARGET`

This card defines the version-bound execution record for the five T9-W1 provider receipts. It contains
no provider credentials and authorizes no production mutation by itself. A receipt qualifies only when
linked to the deployed authority version, the exact grant key id, the reservation id and the
same generation on every record.

## Binding discovered in source

The Worker-side authority is `FABRIC_COMPUTE_URL`, interpreted as an HTTPS origin only. The client
does not accept a path, query, fragment, username or password. It calls:

```text
POST {FABRIC_COMPUTE_URL}/internal/v1/compute/reserve
POST {FABRIC_COMPUTE_URL}/internal/v1/compute/activate
POST {FABRIC_COMPUTE_URL}/internal/v1/compute/cancel
POST {FABRIC_COMPUTE_URL}/internal/v1/compute/settle
```

The authentication mechanism is the short-lived signed grant in
`Authorization: ComputeGrant <token>`. The server verifies an Ed25519 signature against
`FABRIC_COMPUTE_GRANT_PUBLIC_KEYS`; the issuer and private signing key belong to CoreLink Server.
The grant payload binds `tenant_id`, `workload_kind`, `workload_id`, `reservation_id`, `period_key`,
`ceiling_vcpu_ms`, `vcpu_count`, `maximum_wall_ms`, `issued_at_ms` and `expires_at_ms`. The first
three requests carry `{}`; settlement carries `actual_vcpu_ms` and a 64-hex
`terminal_evidence_digest`.

The server returns only `{reservation_id,state}` at this HTTP boundary. `reserve` returns
`prepared|active`; `activate` returns `active`; `cancel` returns `cancelled`; and `settle` returns
`settled`. A `429` is the typed pre-materialization `monthly_compute_refused` result. `401`, `409`,
`503` and schema failures are refusal or ambiguity and must remain durable obligations.

## Binding audit and safe probes

The repository and checked-in deployment declarations contain no `FABRIC_COMPUTE_URL`, no
`FABRIC_COMPUTE_GRANT_PUBLIC_KEYS`, no non-production origin and no acceptance grant. The
Cloudflare fabric declaration currently keeps `FABRIC_PG_DISABLED="1"`; its own comments state that
the Postgres ledger, vCPU ceiling and durable billing export remain inert under that containment.
The source decision explicitly says that no value is provisioned by this repository.

The only documented public host is `https://corelink-api.humangr.com`. Read-only probes on
2026-09-07T00:41Z (America/Sao_Paulo session) established:

| Probe | Result | Meaning |
|---|---:|---|
| `GET https://corelink-api.humangr.com/` | `404` | Host returned 404; root was not an identity/readiness contract. |
| `GET https://corelink-api.humangr.com/v1/health` | `401` JSON | Host returned 401; this route required authentication. |
| `GET` and `OPTIONS` on each compute path | `403` HTML | Edge access policy answered before the compute application; compute API deployment or readiness remained unproven. |

No POST was sent, no grant was presented, and no provider-side state or spend was created. These
probes do not turn the public production host into an acceptance target. The required missing
inputs are:

1. a dedicated non-production HTTPS `FABRIC_COMPUTE_URL` origin for the four compute routes;
2. the authority version/digest and its readiness or identity evidence;
3. the matching public grant key configuration and key id, with the issuer-side signing authority;
4. a bounded entitled test tenant and five fresh grants with recorded ceiling and expiry; and
5. a non-production ledger baseline, with Postgres enabled and durable receipts retained outside the
   service logs.

Bindings were still pending independent verification; the correct result remained
`BLOCKED-TARGET`; issuing a grant against the documented production host would violate the D2
decision and could create uncontrolled compute or billing effects.

## Receipt envelope

Every evidence file must be JSON with secrets removed and contain this envelope. Do not record the
grant token, Authorization header, private key, tenant PAT, admin key, database URL or raw provider
handle.

```json
{
  "receipt_version": "t9-w1-live-v1",
  "source_commit": "6489f319e0f068da06a04e9cdd30ddcb9c209c63",
  "authority_origin": "https://<dedicated-non-production-origin>",
  "authority_version": "<deployment-version-or-image-digest>",
  "grant_key_id": "<public-key-id>",
  "tenant_id": "<test-tenant-uuid>",
  "workload_kind": "devenv",
  "workload_id": "<bounded-test-workload>",
  "reservation_id": "<uuid>",
  "generation": "<generation-or-session-id>",
  "operation": "cancel-before-start|start-stop|settlement|hard-stop|restart-retry-concurrency",
  "provider_state": "prepared|active|cancelled|settled|refused",
  "materialized": false,
  "actual_vcpu_ms": "0",
  "terminal_evidence_digest": "<64-hex-digest-or-null>",
  "idem_key": "<redacted-safe-derived-id-or-null>",
  "observed_at": "<RFC3339>",
  "result": "PASS|FAIL|BLOCKED-TARGET",
  "notes": "<bounded observation>"
}
```

For a start-stop receipt, `materialized` is `true` and `actual_vcpu_ms` must be copied from the
authority's trusted settlement evidence. For a refusal, `materialized` is `false` and no provider
handle may be retained in the evidence.

## Five bounded acceptance receipts

### R1 — cancel before materialization / late start fence

Use a fresh grant and reservation. Confirm `reserve`/`activate` only if the test intentionally
starts; for the cancel path, call `cancel` while the reservation is `prepared` or otherwise before
provider dispatch. Record `cancelled`, `materialized=false`, and `actual_vcpu_ms="0"`. After the
cancel receipt is durable, retry `activate`/start with the same reservation and with a recreated
Worker/DO owner. The authority must refuse it and no handle, claim or usage event may appear.

Acceptance: one cancelled terminal receipt, one late-start refusal, zero materialization and zero
usage. A local `destroy()` result, timer expiry or missing inventory is not evidence.

### R2 — start-stop terminality

With a fresh grant, reserve and activate, then perform exactly one bounded start and one stop. The
stop must be acknowledged by the external authority with a terminal receipt tied to the same
reservation and generation. Preserve the provider terminal evidence digest. A timeout or generic
404 keeps the obligation pending and is a failure of this receipt, never a successful stop.

Acceptance: `active` followed by provider-confirmed terminality, one `settled` receipt, and no
second reservation created by cleanup.

### R3 — actual usage settlement

After R2's provider terminal evidence, call `settle` with the authority-produced decimal
`actual_vcpu_ms` and the exact terminal evidence digest. Verify the ingested usage event carries the
same reservation/generation and derived idempotency key. Replay settlement once to prove idempotency;
the second replay must not add usage or alter the amount. The value must differ from at least one
wall-clock estimate in the fixture, proving that local duration is not the source of truth.

Acceptance: `settled`, exact authority usage, accepted ingest, same `idem_key` on replay, one usage
effect. Monthly aggregate or local timer alone cannot close this receipt.

### R4 — hard stop before materialization

Use a fresh test grant whose requested reservation exceeds the entitled remaining ceiling. Submit
only the normal `reserve` request. The authority must return `429` with the typed refusal before any
provider call. Verify no container/VM, claim, reservation activation or usage event exists. Repeat
with the authority unavailable; the Worker must fail closed and retain the obligation rather than
falling back to local admission.

Acceptance: typed `over_compute`/`monthly_compute_refused`, no materialization and no billable event.
Do not use a local cap, monthly display, fake timer or in-memory ledger as the proof.

### R5 — restart, retry and bounded concurrency

Execute two or three fresh reservations under the same bounded test tenant. Interrupt after a remote
request is sent but before its response is observed, then restart/recreate the Worker/DO. Retry the
same reservation id and generation; it must converge idempotently. Execute the bounded concurrency set
within the test tenant's explicit cap and verify no duplicate provider materialization and no
cross-reservation receipt mix-up. An ambiguous result must retain its obligation and must not mint a
new attempt against the same reservation.

Acceptance: same reservation/generation across restart and retry, at most one provider effect per
reservation, cap-consistent concurrent admission, and durable pending state for any unresolved
operation.

## Execution stop conditions

Stop immediately and mark `BLOCKED-TARGET` if the origin is absent, not dedicated non-production,
the grant key id cannot be matched, the ledger baseline is not explicit and durable, the authority
version is unknown, or any response is ambiguous. Never probe these gates by sending a real grant
to production. Never report `PASS` from local unit tests, a fake authority, a timer, a successful
`destroy()`, a monthly aggregate, or the current Durable Object state.
