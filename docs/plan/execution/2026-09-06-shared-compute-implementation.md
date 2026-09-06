# Shared compute implementation boundary

Root technical decision under existing owner autonomy. This supersedes the
unapproved `/private/tmp/corelink-shared-budget-contract.md` design. It does not
change delivered counts, arm PostgreSQL, or waive provider/monitor acceptance.

## Authority and units

Extend the existing Fabric PostgreSQL authority. Native lease admission and
external admission use the SAME `pg_advisory_xact_lock(hashtext(tenant))`,
`compute_accrual(tenant, period_key)`, and sum of active reservations. External
reservations live separately from provider-managed native leases so the native
reaper never mistakes them for native containers. Native admission includes
prepared/active external reservations; external admission includes pending/held
native reservations. No new in-memory/KV fallback.

Keep native zero-disabled semantics and existing billing event wire vectors.
The external metered grant requires a positive integer entitlement read by the
server; it cannot turn missing/invalid entitlement into a cap. No external
unmetered admission is introduced in this increment. Cap is conveyed as decimal
vCPU-ms text over JSON; storage/core types use checked u64 bounded by i64::MAX.
Use UTC YYYYMM. External grants may not cross a month boundary at maximum wall
time. A period without an explicit reconciled external baseline refuses external
admission. Never initialize a production baseline from the displayed zero stub.

## Ledger boundary

Types and five methods are frozen in `corelink-fabric::compute_budget` and
`LeaseLedger`. Unsupported backends refuse. The production Pg implementation:

- Initializes an external baseline exactly once, adding its EXTERNAL prior
  usage to existing native accrual in the same tenant transaction. Same full
  baseline replay succeeds; a different amount/evidence refuses. Validate tenant,
  month, nonnegative checked amount and64-hex evidence digest. This method is
  operator-only at the HTTP boundary. A zero baseline is accepted only as an
  explicitly submitted reconciled baseline, never synthesized by admission.
- Reserves `vcpu_count * maximum_wall_ms` before effects; all values validated,
  ceiling and sum overflow fail closed. Read native+external reservations and
  accrued in one tenant transaction. Missing baseline returns BaselineRequired.
  Existing reservation ID must match the ENTIRE immutable tuple including grant
  digest and expiry. Same prepared/active request is idempotent; terminal IDs
  never reopen. Cross-tenant/workload collisions refuse.
- Activates prepared -> active before provider effects. PostgreSQL time must be
  before the captured grant deadline. Same active activation is idempotent.
- Cancels prepared -> cancelled only. Active reservations cannot be cancelled
  as unused. Ambiguous activation therefore retains the reservation.
- Settles active -> settled only after a trusted caller supplies actual vCPU-ms
  and terminal evidence digest. Same settlement replay succeeds; conflicting
  amount/evidence refuses. Add actual usage and retire reservation atomically.
  Do NOT clamp actual usage to the reservation: record an overrun honestly so
  future admission refuses. Reject overflow without dropping the obligation.
- TTL, an HTTP timeout and inventory absence never release an active reservation.
  Grant expiry prohibits activation; it is not provider-terminal evidence.

The Rust producer API must authenticate before ledger access. Metered grants are
issued by corelink-server after existing identity/entitlement authorization and
are signed with an issuer-only Ed25519 key; Fabric holds public keys only.
Raw Worker cap values are not authority. Key provisioning is a deployment
prerequisite, not permission to invent a signing key in configuration.

## Grant and HTTP shape

Signed token: `base64url(UTF8 JSON payload).base64url(Ed25519 signature)`;
signature covers the exact payload bytes. Header `Authorization: ComputeGrant
<token>`. Payload has exactly: v=1, key_id, tenant_id, workload_kind
(`spawn_worker_runner` or `devenv`), workload_id, reservation_id(UUID),
period_key(YYYYMM), ceiling_vcpu_ms(decimal string), vcpu_count(integer1..16),
maximum_wall_ms(integer1..28800000), issued_at_ms, expires_at_ms. Max token8KiB;
max grant lifetime90s. No duplicate/unknown payload fields. Reserve/activate
require an unexpired grant; cancel/settle may authenticate an expired grant to
finish its exact durable obligation. Configured public-key history must retain
keys while their obligations remain outstanding.

POST `/internal/v1/compute/{reserve,activate,cancel,settle}`; first three have
empty JSON objects; settle has decimal `actual_vcpu_ms` and
`terminal_evidence_digest`. Responses200 receipt `{reservation_id,state}`;
reserve over budget429 `{error:"monthly_compute_refused"}`; missing baseline or
unavailable ledger503; malformed input400; invalid signature401; divergent or
illegal transition409. Never return success from an unknown obligation.
POST `/internal/v1/admin/compute-baseline` uses the existing operator admin gate,
body tenant_id, period_key, external_vcpu_ms(decimal text), evidence_digest;
success204. No live calls or baseline imports during implementation.

## Composition and acceptance

Runtime ownership must be persisted before reserve/activate calls. Only confirmed
reserve+activate permits the existing mint/JIT/provider path. Lost replies retain
the same ID and retry obligation; no new attempt may spend the same reservation.
Cleanup remains independent of credential revocation and billing delivery.
No new provider duration guarantee is inferred from a timer. Existing F005/F007
and monitor qualifications remain production gates. Grant/client/runtime wiring
is required before declaring this package implemented; a standalone ledger is
not a completed T9-W1. Focused tests are allowed; full CI waits for the complete
original Sprint1 implementation.
