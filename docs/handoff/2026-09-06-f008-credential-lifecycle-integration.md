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

## T3-W3 and A3.15 source composition acceptance

T3-W3 is accepted as prepared source at Runner
`ec10581e36c139c058587d10262cf0963313c72c`; it is not delivered. The composed
path is registry candidate discovery, positive GitHub installation membership,
then the existing scan and modern containment reservation path. A configured
registry that is unreadable, incomplete, or fails membership produces no scan
and no static-list fallback. Candidate repository spelling is preserved through
authorize and mint, while containment keys and provider receipt identity are
derived independently from the canonical repo/job identity. The identity guard
may run after mint preparation in a prepared redrive, but always runs before a
provider effect.

The T3 component map is deliberately small: the registry consumer is
`reconciler.ts`, membership confirmation is `reconciler_membership.ts`, index
wiring is `index.ts`, and the paired Server registry route is accepted at
`32f0ae4819c95b93e78dd89ff6b06c937dcd6389` (155/155 plus typecheck). The
Runner issuer acceptance has three cases and reaches real authorize, mint, JIT,
container-start mock, and canonical receipt paths. The final focused Runner
selection passed 14 files / 143 tests and typecheck; retained logs are
`/private/tmp/corelink-t3-final-evidence/vitest-final.log`
(`21c54597e8343ffacadc35572bf6decdaa5f3e72c6ba4f9f49f6abb0ddeda583`)
and `typecheck-final.log`
(`82643ccc44e5719477952a1167d376e5025923b1ebef38ba9f6705d147262265`).

A3.15's Engine/Worker source pairing is accepted for that criterion only. Its
Rust fixture stack ends at `5abfe9f2cd10e137c211abe23dfdb5a6696598d9` and its
Worker companion is `c463d4ed3ddf46efca6850952115c30cfc513b65`; the prior
controlled Rust gate passed 1/1 and the final Worker conformance tests passed
2/2 within the 143-test selection. A subsequent independent review at
`ec10581` also passed 29/29 across spawn preparation, capacity-before-mint,
and admission-budget authority tests: A3.14 orders authorization, mint, and
adoption before claim, then JIT/provider work; failed cleanup has no provider
effect. A3.16 limits 100 concurrent failures to five exceptional starts and
refuses missing, unreadable, or write-failed bindings before a start. These are
local Worker mocks with a real ContainmentDO harness, not provider-real proof.
T8-W1 remains partial because T3-W2 and F007-dependent lifecycle authority are
still open.

These are source-level acceptance records. T3-W3, T4-W2, and T8-W1 remain
prepared rather than delivered; the recorded delivery count remains 16/70.
Sprint 1 is still incomplete because T6-W15 and T9-W1 remain pending, so no
full CI was run.

## Open T9 and operational authority gates

T9-W1 remains partial. The original D2 quarantine calls its deferral a waiver
requirement, not a repair; its DoD requires that waiver before merge. The frozen
shared-compute implementation scope also still requires F005/F007 producers and
wiring for package completion. The current 503 containment response is therefore
not a deployment-only block and does not complete T9-W1.

The read-only legacy action card at
`/private/tmp/corelink-f008-legacy-action-ready.md` records a private,
hash-qualified inventory of 138 records and the canonical revoke route. Customer
impact review and human approval remain pending; no revocation was performed.
## T6-W15 limited monitor waiver and versioned decision

On 2026-09-06 the owner gave the literal authorization, “Aprovo pdoe seguir
autonomo,” for Option B in
`/private/tmp/corelink-monitor-owner-decision-ready.md`. This is the recorded
owner waiver for T6-W15 implementation only: use Lambda, EventBridge Scheduler,
DynamoDB, SNS, and S3 Object Lock in three independent AWS accounts; keep the
operation identity and receipt cursor in the monitor's own transactional record;
and have an independent verifier reconcile it. The signed chain, WORM storage,
and independent witness establish integrity of observed records only. They do
not claim completeness of internal SNS operations; delivery remains at-least-
once and duplicates remain possible.

The waiver's justification is the provider limitation documented in the Option B
proposal: SNS does not expose a provider-issued exhaustive cursor or historical
per-operation receipt query. Its tracking item is T6-W15. It does not waive or
alter T9 quarantine, F005/F007 work, PAT revocation, PostgreSQL rearm, or any
production gate.

Unknown outcomes remain fail-closed: ambiguity, missing reconciliation, stale or
future time, or a record gap cannot report healthy or rearm PG. Trusted time and
qualified TSA, WORM retention, three-account and verifier independence,
crash/rotation/isolation tests, and seven real continuous observation days remain
explicit qualification gates. None has started or is proven by this decision.
The waiver authorizes implementation; it is not proof of, or a waiver for, the
remaining production and PostgreSQL gates. No deployment is in scope during this
contract and qualification work.

## T6-W15 foundation-wave composition

The monitor foundation is composed through disjoint module ownership: state
backends (`src/state.ts`), SNS delivery (`src/delivery.ts`), and immutable
journal records (`src/journal.ts`) are independent integration packets. The
canonical integration owner composes approved packets; their contracts remain
bounded to durable operation records, at-least-once delivery, and observed-record
integrity. They do not claim an SNS-internal operation-completeness proof.

The limited auxiliary extension path for this work is `src/journal.ts` and the
future `src/trusted_time.ts`, with matching focused tests. It is necessary for
T6-W15's implementation but does not alter any acceptance gate, sprint, or the
qualification requirements recorded above.

## T6-W15 wave 2 contract and auxiliary paths

The frozen wave 2 contract is copied byte-for-byte to
`docs/plan/execution/2026-09-06-monitor-wave2-contract.md`. It assigns
types/config to `admission`, incidents to `devenv`, lifecycle to `engine`,
evidence log to `mint`, witness to `containment`, producer wire to `canary`,
and infrastructure to `external_monitor`. The integration owner remains the
sole owner of shared package, lockfile, runtime index, and canonical composition.

The bounded T6-W15 extensions are `src/evidence_log.ts`, `src/witness.ts`, and
`infra/**`; they are support paths, not a change to acceptance gates or sprint
scope. The package now pins the real AWS Lambda and Secrets Manager clients at
`3.1127.0` for a later qualified-version `InvokeFunction` and `GetSecretValue`
runtime. No runtime implementation, cloud mutation, or placeholder was added.

## T6-W15 outbox, witness, and terminal source acceptance

The outbox packet is accepted at `a0f13d37c2ace4b30f2d6f42d6a1b9fa4a49b6fe`
after the final cold composition review, including real type composition with
trusted-time and evidence-log APIs (8 focused tests and `tsc --noEmit`). The
witness source stack `5f64ca68e6410eec32fbc4480b790d0b9853c6dc` through
`baf5b9caef1f31fc393009c5dc9d5bdf6a6ad06a` is also accepted after independent
review (6 focused tests and `tsc --noEmit`); its canonical integration commits
are `fc1ec79` and `59b1a15`.

The paired terminal source stack
`eb2fed47b2156c28ffaaca794ea8ae672fce99f7` through
`d0ef2e9849ada9539e0c433637ceff9686f019d7` is accepted after independent
cross-runtime review. Its integration commits are `bba2815` and `f0b5987`;
the latter adds monitor/Canary RSA bridge coverage for both public-outbox kinds,
invalid configuration, and old-terminal drain. It makes no trust-registry
runtime claim.

The types stack `230f7e7` through `229de986` and lifecycle commits
`2404d93` and `8873950` are accepted after independent review (8 and 14
focused tests respectively, each with `tsc --noEmit`). Their integration is
`c04f550` through `1d06a04` and `20eb33f` through `528f032`. The durable time
floor source `6feaeecb3f907ac38992d82fd8c2ad880c9cca13` is independently
accepted; its integration `252c30c` passed its 4 focused tests and
`npm run typecheck`.

The journal extension source
`1cc9bb64ac03274d1b9ec75fa29d0f5b8cd01257` and
`b734531fd8c45de7521cc511b3698ce1b6bb9cd9` is accepted after cold review
(10 focused tests and `tsc --noEmit`); its integration is `f580023` through
`72c483b`, where the journal suite passed 10/10 and the typecheck completed.
Its signed checkpoint has exactly nine nominal fields before `signature`:
`version`, `logId`, `sequence`, `previousRoot`, `recordDigest`, `operationId`,
`trustedAtMs`, `signerKeyId`, and `signerEpoch`. The signature is separate.

The fresh independent witness-head source commits
`66037ec83235b5a248ee56b400c52e58f3b1304e` and
`a991435fda3de5d4351490df3a451c3e1a74583f` are accepted after composition
with journal `b734531f` (witness 7/7, journal 10/10, and typecheck). Their
integration is `c8e7a5e` through `95d51f3`. The exact wave-2 contract records
`SignedWitnessHead` with nine nominal fields before a separate signature:
`version`, `logId`, `nonce`, `sequence`, `checkpointRoot`, `witnessRoot`,
`trustedAtMs`, `signerKeyId`, and `signerEpoch`. These focused results overlap
earlier module evidence and are not added into a delivery or aggregate count.

The missing-source scheduler source stack
`521bda0386a26213878e7e957acc6b5c5ee1ec36` through
`4b8b32f6d8a9bc4ed55e076a05f324b33681ce09` is accepted after independent
review (10 focused tests and `tsc --noEmit`). Its integration is `0e50daf`
through `5576b1d`, where `scheduler.test.ts` and `canary-missing-tick.test.ts`
passed 10/10 and the typecheck passed. This source packet is not delivery
credit. The token contract is recorded at
`docs/plan/execution/2026-09-06-monitor-token-contract.md`; its disjoint codec,
manifest, recovery, and page paths remain under their assigned authors.

The AWS source-secret and Lambda-witness adapter stack
`ebaf2177744ab0b2d5cd3137b29a1875f1610e55` through
`b2404a948cb3d732942062dd5dcf00a58ad1f06a` is accepted after cold review
(5 focused tests and `tsc --noEmit`). Its integration is `e68d343` through
`7ddfdcb`, where `aws-adapters.test.ts` passed 5/5 and the typecheck passed.
It supplies bounded, pinned adapter paths only; it is not a runtime deployment
or a proof of IAM isolation, three-account independence, or live capability.

For the resulting fourteen monitor suites (89 tests), every suite has a
completed exit-0 run at `528f032`: trusted-time and evidence-log completed
before an aggregate 120-second process limit; witness, the ten non-heavy
suites, and terminal then completed in bounded isolated runs. The aggregate
timeout is not recorded as an aggregate pass. The complete Canary suite passed
13 files, 117 tests, and `npm run typecheck` at `f0b5987`.

This is source composition only. T6-W15 remains prepared and partial for
delivery. The remaining source components are terminal ingest, durable signer
registry, ACK recovery, durable human page acknowledgement, and concrete main
runtime composition (including its separately contracted witness runtime).
They remain out of the integration tree pending approval, and all qualification
gates remain open. The frozen runtime contract is recorded at
`docs/plan/execution/2026-09-06-monitor-runtime-contract.md`. No work-package
completion, deployment, production qualification, or change to the recorded
16/70 delivered count is implied.

## T6-W15 single-executor completion plan

The user assigned one T6-W15 executor through acceptance:
`r2_pair_acceptance` (Luna), using
`/private/tmp/corelink-t6-w15-owner-20260906` from
`e016ca9df43b8e5b9d005c2e62f454c9d49890f5`. Root remains the orchestrator and
architect; the global integrator composes approved work; and
`spawn_preparation_integration` performs Terra cold review. Component authors
have finished their assigned work and no new component dispatch or work package
is created by this ownership decision.

The three token codecs and terminal-ingest source through `aaa68154` are now
integrated. The ingest acceptance suite passed 25 distinct tests (integrity 6,
transition 4, periodic 4, quarantine 2, idempotency 7, credential isolation 2)
and `tsc --noEmit`; the `4a73` proof was already present in source as `e27276f`
and was not counted twice. The remaining composition is witness runtime, durable
signer registry, ACK recovery, durable human page acknowledgement, and concrete
main runtime under `r2_pair_acceptance`. This records execution ownership and
acceptance flow only. It preserves the T6-W15 DoD, `prepared`/partial status,
qualification gates, 16/70 delivery count, and all worktrees and data.

The ledger's direct item summary separates 16 historical
`recorded_delivered` items from 16 nonhistorical implementation-complete items,
6 partial items, and 32 unknown-backlog items. These categories total 70
items. Only the historical 16 are delivery credit; the other counts do not add
deliveries or change the T6-W15 DoD.

## Canonical closeout execution plan

`docs/plan/execution/2026-09-06-closeout-three-bundles.md` is the sole
canonical closeout plan, byte-identical to
`$CODEX_HOME/plans/corelink-wp-closeout-20260906.md` at SHA-256
`bf6839181736a67fc3c086a10456cffcd169ffce5712caadc502cc845d7cfbe1`.
It fixes exactly three Runner merges: B1 has 12 Sprint 1 WPs, B2 has 14 Sprint
2 WPs, and B3 has 28 Sprint 3/4 WPs. It supersedes prior closeout variants.

The plan's mandatory method governs entry, review, correction, and exit for
each WP: the same executor receives partial-review follow-up; a second
submission rejected by review goes to root for causal diagnosis before another
attempt; lack of progress requires a checkpoint within 60 minutes; and no WP
is abandoned or moved to a new plan after compaction. These rules preserve the
original DoD and forbid new policy or test suppression.

For T6-W15 base acceptance, trusted time/TSA, WORM, three-account independence,
crash/rotation/isolation, and A6.10 probe 3/3 remain required. The A6.17
seven-day observation is a later final qualification performed by T6-W12 and
collected by T6-W10; it is not a base-only T6-W15 blocker or a waiver.

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
