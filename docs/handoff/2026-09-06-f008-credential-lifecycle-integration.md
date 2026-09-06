# F008 credential lifecycle — live integration handoff

## Operating ownership

Root is the orchestrator, product owner, and stakeholder representative. Agents
implement, test, independently review, and integrate their assigned packets.
Root retains priority, contract, risk, and acceptance decisions. Prefer Luna
for bounded work; use Terra for integration complexity, with no more than 15
simultaneous agents.
This handoff records verified source state; it does not claim completion of F008
or replace the frozen implementation contract.

## Integrated and verified

The integration tree is `/private/tmp/corelink-techlead-takeover-20260906` at
`cabe443fdaa1826e1a35e10c4bec411258634e28` before this handoff update.

## Current Runner composition repair

`ab4669d2c3a2f325be05852b3e13e4fcc9c990c3` restores the existing
`/internal/v1/runner/authorize` response contract: it consumes only tenant,
concurrency, and optional compute grant data. The lifecycle generation is
validated only in the canonical `/runner/mint` result, then recorded in the
credential registration and carried through each prepared/failed spawn cleanup
identity. No authorize request or response protocol field was added.

The focused Runner checks passed on that commit:

- `npm run typecheck`
- `npx vitest run --pool=forks --minWorkers=1 --maxWorkers=1` over the eleven
  authorization, mint, preparation, credential cleanup, and tenant-suspension
  suites: 11 files / 116 tests passed.

This is source-level integration evidence only. F008 remains at 16/70 and is
not production qualified; the active legacy PAT inventory, one invalid data
row, live 75-second CAS gate, and seven-day monitor remain human/data or
operational gates.

## Accepted F008/T8-W5 source criteria

Two independent rounds, `lifecycle_suite_critic` and `credential_cold_review`,
accepted the paired source criteria at server `6bf459541` and Runner
`10355b5f9ebca2cf50f0111298f0a3f352c1aa9d`. This records implementation
acceptance only and does not deliver the work package or change F008's 16/70
count.

The remaining production gates are exactly three live 75-second CAS runs,
private inventory qualification (137 active unknown credentials and one invalid
metadata row), and production migration/key qualification. The private
inventory remains outside Git; this handoff records counts only.

## Unsupported lifecycle acceptance

`cabe443fdaa1826e1a35e10c4bec411258634e28` integrates independently approved
test-only packet `2059a429d6d7f0b35b3a50f32d50fa3b4ac2c0a0`. Its in-memory
ledger acceptance verifies that unsupported lifecycle reads repeatedly refuse
and do not fabricate lifecycle or event state.

With the required shared target controls and a sourced, non-empty
`TEST_DATABASE_URL`, the focused command passed:

- `CARGO_TARGET_DIR=/private/tmp/corelink-takeover-suspension-pg-r2/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test -p corelink-fabric --lib lifecycle_unsupported`

## Compute settlement containment

Independently approved commit `241950d` refuses `/internal/v1/compute/settle`
with 503 until qualified provider terminal-artifact authority exists. It keeps
an active reservation active and does not call ledger settlement when a valid
grant presents zero usage and an arbitrary digest.

The added regression was RED against parent `10355b5` in a disposable worktree:
the valid bearer received 200. It is GREEN after the repair:

- `CARGO_TARGET_DIR=/private/tmp/corelink-takeover-suspension-pg-r2/target CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test -p corelink-fabric-server --lib compute_budget_api` — 5 passed.

This is a mocked API-boundary regression, not a real PostgreSQL HTTP-capacity
proof. Existing real-PG external-compute conservation evidence remains separate;
no cheap HTTP-plus-PG harness was available in the focused server suite.

## T4-W1 source acceptance

The T4-W1 source criteria are accepted without delivery credit. The Worker
handler packet `b861037a` passed 11 focused tests and verifies installation plus
PAT identity reaches authorization and mint, while denial leaves no effects.
The AU4.18 guard stack ending at `212d0ee` prevents a nonempty live
`REPO_TENANT_PAT_MAP` from being depleted by a candidate deploy. It reads exactly
one 100% active deployment version, parses its JSONC bindings with pinned
`jsonc-parser@3.2.0`, and refuses redirects before a bearer can be forwarded.

Independent review reproduced the guard self-test with `npm ci --ignore-scripts`
in an isolated worktree using the committed lockfile; the shared checkout's
unrelated dependency symlink was not used. Actual compatible server/Worker
deployment, production map and secret bindings, and live guard qualification
remain separate gates.

## T8-W2/A3.13 source acceptance

T8-W2 source criteria are accepted without delivery credit. Server
`68221dff` was independently reproduced at 141/141: cross-tenant runner PAT
use refuses before storage, trusted own-tenant scope is forwarded, forged
elevation is ignored, and expired PATs refuse. Runner `1e3d663` adds the real
broker and `CredStashDO` lease-isolation regression; its focused route test
passed 9/9.

The broker remains multi-use while a lease is live and refuses after expiry.
Three live 75-second refusal proofs remain separate T8-W6 gates.

The remaining legacy KV spawn-claim race is not waived by this acceptance. It
belongs to T8-W3/A3.18's atomic spawn-claim work, with its F005/F007
implications, rather than T8-W2's credential-scope contract.

## R2/T4-W2 billing composition acceptance

`r2_pair_acceptance` accepted the source-only paired billing composition. The
Runner is at `b367aeaaf5334e9953f092534876898d1133f36f`, including bounded
1024-event batches, serialized flushes, exact-byte retry, and prefix-identity
retirement that preserves events appended during an in-flight post. The paired
Server is `412c9bb03f4a171ceda5102b873e0c3bda10f015`.

The Runner shared-target gate, after sourcing the PostgreSQL environment and
asserting `TEST_DATABASE_URL`, passed `cargo test -p corelink-fabric-server
--lib corelink_billing`: 21 passed, 0 failed, 456 filtered, exit 0. The
corrected concurrent append/flush gate passed 1/1. The Worker wire conformance
gate passed 5/5, and the Server `billing_ingest` gate passed 25/25. The Runner
and Worker logs are retained outside Git at
`/private/tmp/corelink-r2-evidence/runner-corelink-billing.log` and
`/private/tmp/corelink-r2-evidence/worker-usage-event-conformance.log`.

This accepts T4-W2 implementation criteria only. It remains prepared rather
than delivered; F008's recorded delivery count stays 16/70, and no production
or live qualification is claimed.

## Earlier integrated packets

The prior packets in this tree include:

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

## Remaining production gates

The accepted source criteria do not qualify deployment or production delivery.
The three live 75-second CAS runs, private inventory qualification, and
production migration/key qualification remain required before F008 can move
beyond its current source-only status.
