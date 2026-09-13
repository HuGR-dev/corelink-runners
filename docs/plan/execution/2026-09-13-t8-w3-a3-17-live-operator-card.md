# T8-W3 / A3.17 live operator card

Status: `READY-PENDING-LIVE`; no live qualification is claimed by this card.

## Frozen acceptance

The normative row is A3.17 in `docs/plan/2026-08-30-golive-remediation-plan.md:589`.
The worker must receive 100 HMAC-verified `workflow_job.queued` deliveries for
each required failure state and either commit 100 durable retry records with
100 HTTP 202 responses or, when the durable store is unavailable, return 100
HTTP 503 responses. Every state must produce zero claims, JIT registrations,
leases, and boxes. The deployed worker/config identity is fixed and the full
matrix repeats 10/10. No mint key or webhook body may appear in logs.

## Preconditions and safety fence

The owner records the exact current Worker version, deployed config digest,
mint-key presence metadata (never its value), intake/redrive switch values,
ContainmentDO namespace/migration identity, and the repository-hook secret
binding. Intake must be explicitly set to the normal path for this card;
otherwise the request enters the separate ContainmentDO backlog and does not
exercise A3.17's normal-intake inbox.

Before any delivery, establish a quiescent baseline for the exact repository
and tenant: complete Cloudflare app-scoped instance enumeration, GitHub runner
listing, tenant lease listing, read-only Worker metrics, and fleet-busy read.
Any incomplete, unauthorized, ambiguous, or changing baseline is `UNKNOWN` and
stops the run. Use a unique job-id and delivery-id range per matrix cell.

The operator must hold a separately approved secret/config mutation authority
and an exact restoration tuple. A rollback trap restores the prior version and
config on every exit; restoration is re-probed before the artifact can be
accepted. This card authorizes no mutation by itself.

## Matrix

| Cell | Worker state | Deliveries | Required response/evidence |
|---|---|---:|---|
| MISSING-01..10 | `CORELINK_RUNNER_MINT_AUTH_KEY` absent | 100 each | 100×202; 100 durable normal-inbox records; no spawn effects |
| WRONG-01..10 | key present but intentionally wrong | 100 each | 100×202; 100 durable normal-inbox records; no spawn effects |
| STORE-01..10 | deterministic durable-intake fault seam enabled | 100 each | 100×503; zero inbox/provider effects |

Each request must be HMAC-valid, unique, and tied to the cell's job range.
Capture only status, delivery id, job id, response classification, timestamps,
version/config identity, and redacted hashes. Never capture request bodies,
headers containing secrets, mint responses, PATs, or JIT material.

## Evidence and default-off proof seam

The source seam is reviewable: normal intake calls `normalIntakeEnqueue` before
returning 202, catches durable-write failure as 503, and only later drains into
`prepareSpawn`; `prepareSpawn` precedes claim/provider/container work. Focused
Vitest covers the 100-request missing, wrong, and injected-store-failure cases.

The promoted source also provides a default-off, capability-authenticated
proof seam specifically for this card: `GET /internal/v1/a317-live-proof`
(`index.ts:5519-5527`) reads the run-scoped aggregate from the durable
ContainmentDO. The signed capability is bound to run, phase, index, nonce,
expiry, build SHA, repository, and installation. It cannot create, settle,
drain, or alter intake configuration. The underlying transactional snapshot
(`normal_intake_inbox.ts:194-208`) counts accepted/pending/complete/uncertain
records and authorization attempts/refusals.

The proof webhook path is restricted to the exact proof repository,
installation, single `corelink-a317-proof` label, paused intake, and matching
build claim. In the `store_unavailable` phase, the deterministic fault is
selected before the transaction (`index.ts:5732-5742`), so the 100×503 result
is reproducible without taking a production storage outage. This seam remains
inactive unless its capability/configuration is explicitly provisioned.

For a real run, the following read-only observations can support the zero-effect
claim without adding per-run production counters:

* `scripts/container-instances.sh --json --app <runner-app>` provides the
  complete, pagination-checked running-box inventory.
* GitHub App repository runner enumeration can detect new JIT runner entities,
  subject to the App credential and repository scope being available.
* Tenant `GET /v1/leases` can compare the exact tenant's lease set before and
  after the cell.
* `/internal/v1/metrics` and `/internal/v1/fleet/busy` are supporting read-only
  signals, not complete effect ledgers.

## Hard evidence gaps

The default-off proof endpoint now supplies durable normal-inbox readback for
the scoped run; it removes the earlier record-readback gap. Worker KV
`spawn:<jobId>` claims still have no public read endpoint, and provider
snapshots cannot prove that a transient JIT/box existed and disappeared between
polls. Those effects remain covered by the ordered source control flow plus
quiescent before/after external snapshots; they must not be replaced with
invented per-run production counters.

The STORE phase is executable through the default-off deterministic fault seam,
subject to owner provisioning and version binding. It must still be recorded
RED if that capability/configuration is absent, stale, or cannot be read back;
an actual storage outage is never a substitute.

## Disposition

Proceed with source acceptance and, when the AU terminal/root relay permits it,
the full missing/wrong/store matrix using the default-off proof capability,
version binding, unique job scope, quiescent before/after snapshots, and
redacted tail evidence. Keep canonical A3.17 `RED` if the proof capability,
owner credentials, or version-bound deployment is unavailable. No waiver is
proposed.
