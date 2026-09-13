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

## Evidence that exists today

The source seam is reviewable: normal intake calls `normalIntakeEnqueue` before
returning 202, catches durable-write failure as 503, and only later drains into
`prepareSpawn`; `prepareSpawn` precedes claim/provider/container work. Focused
Vitest covers the 100-request missing, wrong, and injected-store-failure cases.

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

Worker KV `spawn:<jobId>` claims and ContainmentDO normal-inbox records have no
external read endpoint. A 202 therefore proves the handler's enqueue branch,
but cannot independently prove all 100 durable records without a commit receipt
or read-only inbox seam. Provider snapshots also cannot prove that a transient
JIT/box existed and disappeared between polls.

Most decisively, production has no deterministic way to make the Durable Object
store unavailable. A real outage is unsafe and non-reproducible; inventing a
fault response or adding a production-only counter would not satisfy the
contract. The STORE phase is therefore `RED` until an isolated, owner-approved
fault seam exists.

## Disposition

Proceed with source acceptance and, when the AU terminal/root relay permits it,
the missing/wrong-key live cells using version binding, unique job scope,
quiescent before/after snapshots, and redacted tail evidence. Keep canonical
A3.17 `RED` overall until the durable inbox evidence and deterministic store
fault seam are independently available. No waiver is proposed.
