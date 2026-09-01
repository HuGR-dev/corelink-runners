# 2026-09-01 fabricd Postgres burn containment

Status: **CONTAINED**, intentionally degraded.

## Trigger and diagnosis

At approximately 2026-09-01 12:40 BRT, production
`https://corelink-fabricd.gmhelmold.workers.dev/health` returned HTTP 500 with
`Failed to start container: There has been an internal error connecting to the
port`.

Read-only inspection established that:

- the live Worker still carried the `DATABASE_URL` secret;
- `FABRIC_PG_DISABLED` was absent;
- the `* * * * *` scheduled trigger was live;
- the runner container application had no running instances (883 historical
  records were all inactive);
- the only stable running container returned by the account inventory was a
  `_system` platform-pool instance, not a Corelink runner;
- a fabricd request created a short-lived/stopped fabricd instance, matching the
  pre-bind boot-failure shape documented on 2026-08-31.

This evidence made the recurring fabricd-to-Postgres boot loop the bounded,
actionable source of burn. No platform instance name was used as a teardown
handle.

### Runner-fleet cross-check

The runner application had a separate historical spike: 802 instance records
were created between 18:00Z and 21:00Z on 2026-08-31. The live aggregate
`containers info` surface reported 18 healthy instances, but that number is not a
reliable running/billable count: the detailed inventory initially returned
883/883 inactive, briefly exposed one older instance as running while the list
converged, and at 2026-09-01 16:06Z returned 885/885 inactive and zero non-inactive.
GitHub simultaneously reported zero `cf-runner-*` online or busy. The current
queued `corelink-server` jobs used the reserved `corelink-builder` label and were
not inputs to the Cloudflare reconciler.

Seven remote `sbox:` records were inspected. Their stored handles did not match
the UUID that appeared briefly in the detailed inventory, so no unproven
instance-name-to-Durable-Object join was used for teardown. The instance had
already become inactive at the next sample.

The read-only code review did find a recurrence amplifier that must be fixed:
after its grace period, `redriveOrphanedJobs` can release a queued job's spawn
claim and try again, while the concurrency-slot acquisition is idempotent by
`jobId`. A stuck job can therefore create another box without consuming another
slot. This explains the shape of the historical spike but was not active for the
reserved-label run sampled here. It belongs in the remediation backlog together
with an explicit intake/re-drive kill switch; it did not justify a destructive
runner rollout during this containment.

## Change

Commit `2df6740` re-armed the existing emergency switch:

```text
FABRIC_PG_DISABLED=1
```

It was deployed without changing the container image. Cloudflare reported:

- previous/rollback Worker version:
  `32e29905-620d-423c-aa1a-1ce3a4fe3e21`;
- containment Worker version:
  `40bf22a4-6c48-467d-9844-b4fc33e7a3ee`;
- unchanged container image digest:
  `sha256:2e7bcea926f4ce2b38edb1a381f3821fcf4c898377e4f988b763fd3232c0e565`.

`wrangler versions view 40bf22a4-6c48-467d-9844-b4fc33e7a3ee`
confirmed the switch and all pre-existing secrets/bindings on the live version.

## Verification

Static verification before deploy:

- `npm run typecheck`: PASS;
- `npm test`: PASS, 72/72 tests;
- `wrangler deploy --dry-run`: bundle parsed and listed
  `FABRIC_PG_DISABLED=1`.

Production verification after deploy, on the fixed live version:

- boot-rate probe: **6/6 SERVED**, **0/6 BOOT_FAILED**, 10 seconds apart;
- `/v1/attestation/key`: HTTP 200;
- unauthenticated `/v1/usage`: HTTP 401 (fail-closed).

Raw boot-rate observations were captured locally at
`/tmp/fabricd-boot-rate.20260901T160055Z.tsv`.

## Accepted degradation and exit condition

While the switch is armed, fabricd uses the in-memory ledger. Lease durability
across restarts, the Postgres-backed vCPU ceiling, and durable billing export are
therefore suspended. This is containment, not the permanent repair.

Do not remove the switch merely because one health request succeeds. Re-arm the
durable backend only after a replacement or restored database passes repeated
fixed-config boot-rate probes and its resource/scale-to-zero behaviour has been
observed without the one-minute feedback loop.
