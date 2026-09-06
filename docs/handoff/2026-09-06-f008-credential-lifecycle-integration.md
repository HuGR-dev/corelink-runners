# F008 credential lifecycle — live integration handoff

## Operating ownership

Root is the orchestrator, product owner, and stakeholder representative. Agents
implement, test, independently review, and integrate their assigned packets.
This handoff records verified source state; it does not claim completion of F008
or replace the frozen implementation contract.

## Integrated and verified

The integration tree is `/private/tmp/corelink-techlead-takeover-20260906` at
`70ee2d14425a43e6ec989cf5864d6015a226b4d8`:

- `e42a691e484661d6a512573a3c6cdc10314e293a` wires the Fabric dedicated issuer
  key through fabricd, exposes the ContainmentDO tenant-close RPC, validates a
  supplied tenant generation, and fixes the Axum 0.7 route parameter syntax.
  Cold review approved it. `cargo test -p corelink-fabric-server
  credential_lifecycle_api --lib -- --nocapture` passed 3 tests using the shared
  target with `CARGO_INCREMENTAL=0` and `CARGO_BUILD_JOBS=2`.
- `8e07ba8` adds approved Worker floor acceptance coverage. `npx vitest run
  test/credential-generation-floor.test.ts --pool=forks --minWorkers=1
  --maxWorkers=1` passed 6 tests.
- `70ee2d1` adds approved immutable PG event generation to the reaper envelope.
  `cargo test -p corelink-fabric-server reaper::tests --lib` passed 40 tests
  with the same shared target controls.

The paired server composition tree is
`/private/tmp/corelink-server-budget-20260906` at
`a4f2f3d4109054344eeb5b28132db6b35f23ca5f`:

- D1 lifecycle-floor commits `6f8d493a8`, `f33b6a71b`, and `4668c0df9` are
  integrated after independent approval.
- Lifecycle-client commits `f702dd717` and `a4f2f3d41` are integrated after
  independent approval.
- `pnpm --dir worker exec vitest run tests/credential_generation.test.ts
  tests/credential_lifecycle_client.test.ts --pool=forks --maxWorkers=1` passed
  14 tests.

## Frozen remaining seam

The authenticated chain is Fabric reaper exact-PG-event lookup → Worker
lifecycle bearer → paired server `POST
/internal/v1/runner/credentials/close-generation` using the existing runner
mint internal key. The payload is exactly
`{action:"suspended",event_id,tenant_id,lifecycle_generation}` at the Worker
boundary; no tenant or generation aliases are accepted.

A successful completion response is exactly
`{event_id,tenant_id,lifecycle_generation,complete:true}` with HTTP 200. HTTP
202 means a durable pending receipt and must not ACK the Fabric outbox. A full
triple conflict is 409; malformed input is 400; dependency failure is 503.
Legacy inventory unknown or empty remains pending/fail-closed and cannot be
classified as completion.

## Pending work

The server receipt/close-generation route and Worker receipt/consumer are not
integrated. Modern runner and DevEnv mint call sites still need the approved
floor-aware transaction packets. Suspension ACK ordering, exact KV
invalidation completion, terminal job fences across generations, legacy
reconciliation, and the existing live 75-second CAS refusal gate remain open.
