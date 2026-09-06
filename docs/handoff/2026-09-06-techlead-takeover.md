# Techlead takeover — 2026-09-06

**Read the latest progress section first; older pending statuses are historical.**

Owner renewed the requirement to finish all backlog and prepare go-live, with
autonomous decisions, maximum useful parallelism, Luna by default, up to 15
agents, and Terra only when needed. Complete-sprint CI policy remains in force;
focused development checks are allowed. No debt or acceptance waiver was given.

## Working state

- Primary Runners checkout remains conflicted and untouched.
- Integration: `/private/tmp/corelink-techlead-takeover-20260906`, branch
  `delivery/techlead-takeover-20260906`, recovered from `fe591a1567b3f21b9fd392b6374a97209082919b`.
- The later September 6 rework contained product preparation absent from the
  September 5 handoff. Preserve it; do not repeat the initial census/reviews.
- Source baseline ledger still records 16/70 delivered, 54 outstanding, zero
  complete sprints. New local commits do not change delivered counts.
- Root selected shared per-tenant monthly compute budget under renewed owner
  autonomy. Implementation must use an actual common admission/reservation
  authority; the server's displayed consumed_vcpu_h=0 is a stub.
- Root authorized bounded dependency patches in isolated server/workspaces
  worktrees following the owner's response granting decision autonomy. Primary
  sibling workspaces remain untouched. Do not reopen the permission question.

## Active bounded work

- Credentials: `cb447235acb047e03d712488935487a9b7beab46` plus test repair
  `4362c782d3fc60646f26f3bd93cb58bc6d35ffa9`, integrated. Permanent job closure,
  late registration retained for revocation, terminal idempotency, restart and
  cursor tests. Suspension epoch/migration remains distinct unfinished scope.
- Worker control auth: `7cb2c8852f96a9c1c2e3a07bcf6fced84595dbaa` integrated,
  root wired routes. Three distinct tokens for spawn, exec, lifecycle; no shared
  fallback. Rust paired implementation and route-fixture updates still pending.
- Strict TS: `3be341dddff51fd04bebf8a76b608c9f9ffd7424` fetched from the author's
  isolated clone and integrated. Fixes actual runtime guards in billing/reconciler.
- Server relay: `/private/tmp/corelink-takeover-server-relay`, branch
  `fix/runners-devenv-relay-20260906`, `3ff1906be9c84d45a6013b65c75f58c3d135cfbc`.
  Prepared patch applied to actual server `c0a4466d...`; 45 focused helper tests
  and nine SQLite cases reported; root review and full sprint gates pending.
- clw: `/private/tmp/corelink-takeover-clw`, branch
  `fix/runners-required-hit-20260906`, author applying required-hit patch to
  exact Workspaces `57c2256e...`. Actual source repo is corelink-workspaces,
  not a directory named clw. Signed binary release and consumer pins pending.
- External monitor: default AWS CLI identity works; alternate oaas-dev does not.
  Read-only capabilities inventory in progress. Identity alone is not deployment
  capability or proof of independent accounts/schedulers/WORM journal.

## Immediate next integration

Finish paired control auth and tests, verify integrated credential/TS fixes;
close required mint/identity before-side-effect enforcement and durable webhook
acknowledgement. Existing claimSpawn KV RMW is not an atomic claim, but the
production canonical owner ledger also participates: assess both before replacing
the protocol. Run full CI only after complete sprint implementation composition.

T3-W18 genuine three-state/nonempty ordered-resume live evidence remains RED.
Read-only live preflight must use fresh actual provider output, never historical
JSON described as current. Preserve CONTAINMENT/v7 and plaintext bindings on deploy.
Host free disk was 18 GiB at takeover; avoid concurrent large Cargo targets.

## Progress at 01:50 BRT

Integration HEAD `03fdf87` now includes required mint fail-closed helper,
exact failed-attempt credential cleanup (root wired the three nonterminal
cleanup sites), paired Rust control credentials, durable retry epoch RPC wiring,
and transactional PostgreSQL suspension producer. No WP or sprint is marked
delivered: composed source and live gates remain outstanding.

Root verification: 57 mint/credential tests passed; 37 Cloudflare engine tests
passed; strict Worker TypeScript passed before retry integration. Focused retry,
real PostgreSQL and engine conformance checks are currently running. Root fixed
missing job-id preflight and the fabric-server scoped-token fixture. Credential
completion fencing must only run for actual job completion; failed attempts use
exact patId cleanup and leave later PAT registration permitted.

Review caught and repaired: Rust partial credentials accepted; mint extraction
left dead commented code; retry RPC method name mismatch; missing retry count
incorrectly reconstructed; cleanup inferred revoked from absent pending row;
Pg producer initially authored on stale `19eb2e1` instead of `ac13325`. Only the
repaired exact-baseline Pg commit `b143084122c648aacd1c3552f2eb55c2bce5b980` was
integrated. Pg author's unrelated dirty files are formatting-only and excluded.

User supplied account email and explicitly authorized autonomous decisions.
AWS organization `o-kfk50yjeiz` (ALL) created, management `046797548582`.
Member accounts accessible by actual STS smoke: monitor `975306274105`,
sensitivity `286590629898`, verifier `888348805607`. Gmail aliases used; no
long-lived member credentials emitted. Independent application roles and
services are not deployed. Account separation alone does not green O-MONITORHOST.
AWS primitives lack a documented native arbitrary-journal signed monotonic
time/checkpoint service; Azure Confidential Ledger helps with immutable ordering
but `az account show` requires login and trusted receipt time remains unresolved.
Do not fabricate capability or silently relax the normative gate.

Lima `corelink-ci-proof` is now running with existing disposable `corelink-pg16`,
loopback port55432. Root wrote protected task connection environment at
`/private/tmp/corelink-takeover-pg.env`; source it without printing credentials.
Host PostgreSQL5432 remains untouched. Free disk fell to14GiB before final builds.

Server authorize endpoint author is active in
`/private/tmp/corelink-takeover-server-authorize`, exact baseline
`3ff1906be9c84d45a6013b65c75f58c3d135cfbc`. Root froze POST
`/internal/v1/runner/authorize`: share mint identity/entitlement checks, return
tenant/caps only, zero PAT issuance. This enables capacity reservation before
mint, followed by validation that mint identity/caps match authorization.
Runner handler integration is still required; do not count helper-only work.

Route fixtures have unintegrated commits `5318b5c...` + `91ab62d...` in
`/private/tmp/corelink-takeover-route-fixtures`: 102/111 pass, remaining nine
contract/source failures require repair on composed source. Current preclaim
mint/normal webhook durable ACK, shared monthly budget, clw signed release,
monitor capability/runtime, live containment evidence and subsequent sprint
scope remain unfinished. Full CI remains prohibited until a complete original
sprint implementation is composed; focused checks continue.

## Progress at 02:38 BRT (supersedes earlier pending status)

Integration source HEAD `348f79846e3c857ee3a8b28c826c3b96a16f603d`.
Still 16/70 delivered, 54 outstanding, no complete sprint; no full CI, push,
release, deployment or new PR has been performed by this takeover.

- Required authorization HTTP client and capacity-before-mint are wired in the
  actual Worker. Root reproduced 14 authorization/capacity tests.
- Concurrency authority transactionally records ceiling refusal, exposes scoped
  authenticated job status, and renews active runner slots from keepalive. Root
  reproduced 28 slot/control tests. Refusal persistence errors now refuse without
  spending the bounded infrastructure fail-open budget; real-DO rollback test.
- Terra split prepareSpawn from provider drive in all four canonical callers.
  Required authorization/cap/mint failures precede claim/DRIVING. Root reproduced
  six real-DO public drain tests; pre-effect abandonment revokes only exact newly
  minted PAT, never a job fence or a potentially concurrent winner's slot.
- Cold review found a genuine same-job stash race: the old job-scoped stash could
  return another attempt's ticket. Stashes now use exact job/tenant/PAT lease IDs.
  Cleanup wipes the exact stash before marking revocation terminal; three focused
  tests include wipe-failure/cron recovery and preserve the winner's ticket.
- Root added a separate normal intake inbox in ContainmentDO, preserving the
  normal-empty exclusion from containment backlog. Durable enqueue precedes 202;
  limiter refusals enqueue with 60-second backoff or return503 on storage failure.
  Cron resumes normal canonical owner flow, uncertain effects never replay, and
  the active inbox is bounded500. Root fixed starvation beyond25 delayed heads,
  field/schema/index validation, and the normal caller's missing claim witness.
  The inherited isVitestLegacyFixtureContext production-source bypass is removed.
  Root11 inbox/public webhook tests and strict Worker TypeScript pass.
- Root39 earlier composed concurrency/capacity/route/slot lifecycle tests passed.
  Pg suspension producer: two REAL PostgreSQL tests and engine37+2 conformance
  tests passed. Lima corelink-ci-proof and its PostgreSQL container were stopped
  after those tests; host5432 untouched. Protected DB env remains available.

Dependencies are prepared in isolated sibling worktrees, not released:
- Server relay+authorize: `/private/tmp/corelink-takeover-server-authorize`,
  HEAD `7c58cc0febcedfeec844a641ce7bb52a3454cb41`. Shared read-only authorize
  gates, budget0 preservation, safe caps and whitespace identity rejection;
  reviewer reproduced73 tests, author78 after repair. Existing unrelated TS errors
  in durable_object/replication_coordinator are assigned for bounded repair.
- Workspaces/clw: `/private/tmp/corelink-takeover-clw`, HEAD
  `aca24f728ca44eac26118653a631033297eec51f`, required-hit implementation and
  version0.1.12 release preparation. Required release secret NAMES exist in GitHub;
  values were not read. Signed release/five targets and consumer pins remain.
  No renewed owner exception is needed: bounded sibling edits already authorized.

Route fixtures are being repaired by the existing Terra agent using real DOs,
external authorize/mint HTTP mocks, and actual composed source. Luna's44f6224
fixture commit is not integrated: it had33 failures. Do not accept legacy path
bypasses or remove contract assertions to green it. Root owns index.ts integration.

O-MONITORHOST remains unresolved. AWS accounts exist (above), but SNS/SQS FIFO
supports only5-minute deduplication and lacks historical exact operation receipt
lookup after deletion; CloudWatch logs lack the required exhaustive delivery
cursor proof. Rekor v2+Sigstore RFC3161 TSA can anchor blinded hash receipts, but
cannot retroactively prove transport delivery. Capability review notes live at
/private/tmp/corelink-monitor-capability-review.md; they are not a GREEN artifact.
No deploy/cost-monitor application exists. The later seven continuous observation
 days remain mandatory and have not started. Do not invent elapsed evidence.


## Progress at 03:52 BRT (supersedes earlier pending status)

Runners integration HEAD `fe4d824c9eb9b75982d9804f8f9a31ed6198902c` at `/private/tmp/corelink-techlead-takeover-20260906`.
Paired server integration `/private/tmp/corelink-server-typecheck` now `dc169f8c5`.
No full CI, new PR, push, release or application deployment. Canonical delivered
count remains 16/70 with 54 outstanding and no complete sprint.

Completed and root-reproduced source increments:
- Actual route fixture suite:109 passed, then restored two improperly removed
  cross-domain/rotation negatives and reproduced2/2. The new adoption fixture
  revision passed all111 tests on the root integration, including the root guard
  against unknown-operation/undefined-PAT acceptance.
- Root13 concurrency tests pass after exact preparation ownership and atomic
  expired-holder pruning, including rollback after partial deletion.
- Root6 credential stash tests pass; local wipe and remote revocation are
  independent, every job credential is attempted, and remote error bodies do
  not enter logs (`d612733`).
- Actual host shell auth bridge passed20.57s (`89bf884`); no simulated shell proof.
- Root found a DevEnv billing regression: shared runner slot-seconds helper
  was incorrectly used for DevEnv vCPU-seconds. Dedicated builder now preserves
  the tier multiplier. All63 DevEnv credential/lifecycle/usage tests pass
  (`4654f8b`), preserving existing assertions and adding four tier cases.
- ADR-0013 (`7a4a733`) records the implementation decision under explicit owner
  autonomy: a supplied installation is server-authoritative; an accompanying PAT
  must resolve to the same tenant. Missing configured PAT refuses before effects.
  Root31 Worker tests and85 paired server tests pass. Independent review approves.
  The ADR does not claim a human cryptographic signature or waive live gates.
- Server relay/authorize/TS dependency root99 focused tests and strictTS passed
  before owner conflict;85 authorize/mint tests passed again after it. Offline
  dependency installation repaired missing root node_modules; lockfile unchanged.

Credential issuer handoff source is composed; full WP/live acceptance remains open:
- Root Runners `14e21ce`: mint carries unique operation_id; local credential
  registration and immutable attribution precede server adoption204; adoption
  precedes claim/provider. Registration failure wipes local ticket and leaves
  issuer timeout ownership. Ambiguous adoption requests exact local cleanup and
  refuses provider effects. Root19 public preparation/client tests + strictTS pass.
- Server helper40d3dd3 repairs immutable deadline, full-tuple readback, missing
  metadataKV pending revocation and validated bounded drain. Root reviewed and
  integrated it with wiring117361b+3ec5309 into paired serverec426c505. A copied
  old helper in the author's wiring tree was not integrated. Root115 tests across
  issuer helper/routes/authorize/mint/DevEnv cleanup pass, plus strictTypeScript.
- SQL followup5be75a integrated asdc169f8c5. Root13 actual SQLite cases pass: moving
  clock at activation, real transaction rollback after PAT insertion, exact tuple,
  revoke-before-activate/adopt and adopted-before-revoke serialization.
- GroupB384d836+99fe0c8 were NOT integrated: incomplete fixture contract and
  an undeclared variable in capacity test. Split into independent Luna worktrees
  fixture-webhooks, fixture-lifecycle, fixture-capacity onrootfe4d824; previous
  groupB agent retains only SJ5. No business assertion weakening is authorized.
- Unit index fixturef126ec9 imported from the agent's isolated clone asfe4d824.
  Root rerun pending. Runner tests use npm-lock dependencies and direct node
  Vitest with minWorkers1/maxWorkers1. A pnpm invocation unexpectedly attempted
  dependency installation through the old shared symlink; root removed its own
  symlink/generated pnpm files and installed root-local npm dependencies via
  npm ci --offline, without lockfile changes. No test pass claimed from failures.

Pending owner decision:
- Async question submitted for the concrete proposal at
  `docs/plan/execution/2026-09-06-monitor-decision-proposal.md`: authorize AWS
  at-least-once delivery with explicit UNKNOWN outcomes instead of the two
  unsupported exhaustive provider-receipt guarantees, or retain the original.
  **No response yet; elapsed time is NOT authorization.** Do not implement the
  monitor or apply that proposal until approval. All remaining gates, trustworthy
  time qualification and seven actual days are preserved by the proposal.
- Google Chat API additionally evaluated: deterministic message lookup exists,
  but no documented monotonic receipt cursor/final watermark. It is not GREEN.
  Primary links are in `/private/tmp/corelink-google-chat-capability.md`.

Architecture/remaining acceptance:
- Shared tenant compute budget remains unimplemented. `/private/tmp/corelink-shared-budget-contract.md`
  is an UNAPPROVED proposal and contains stale baseline assumptions. Terra's
  subsequent DevEnv audit was rejected because it read the wrong worktree.
  Actual DevEnv already has authorized RPC, indirect tickets and billing outbox.
  Do not reimplement these or invent a provider kill deadline.
- F008 producer epoch is fixed, but no resume epoch reaches minted credentials;
  delayed suspension still can enumerate newer lifecycles. Migration/consumer
  ordering remains open. F005/F007 authoritative provider cancellation/recovery
  and active reservation expiry semantics remain open.
- Monitor capability/app, live T3-W18 proof, signed clw release/pins, shared budget,
  subsequent sprint acceptance and seven-day observation remain outstanding.
- Lima/PG stopped; primary conflicting checkout and original worktrees preserved.
  Disk last~8GiB free. Do not prune unknown/other agents' trees or dependencies.


## Progress at 04:05 BRT (latest)

Runners root b621893bf0589c6306835fa1b6bf630fc581cc8b is clean; paired server
root dc169f8c53196e242fff213bdb3c77be3a7ade00 remains clean. No new remote
publication, full CI, release or deployment. Delivered count still16/70.

- Root reproduced all111 route adoption tests and99 index unit tests. Rejected
  incomplete groupB package384d836/99fe0c8; its seven files were repaired in four
  bounded Luna lanes. Integrated8 capacity/normal-inbox tests,44 SJ5,10 lifecycle,
  and13 webhook tests after source allowlist metric repairb621893.
- Root strengthened SJ5 beyond the author's direct-enqueue seam: signed actual
  webhook202 -> durable capacity refusal with original tenant installation ->
  60-second retry -> exactly one JIT/container -> complete settlement. Restored
  no-spawn-failure metric assertion; no actual request is lost behind a fixture.
- Root repaired fixture undefined-PAT adoption matches and ensured dedup first
  delivery really spawned once before measuring the redelivery's no-op.
- Source27e9a96 requires the stash binding before confirming credential cleanup.
  Remote revoke remains independent. Recovery8079103 integrated61a4080 proves
  authority restart, restoring real CredStashDO, exact ticket wipe and terminal
  confirmation. Root66 tests passed before new recovery,20 webhook+stash tests
  passed after it; strict Worker TS passed. These counts overlap; do not sum them.
- The credential agent initially edited the root despite its assigned worktree;
  its bounded diff was reviewed, preserved and reproduced. Followup used the
  assigned isolated tree. No unrelated changes were included.
- Root server115 focused tests,13 real SQLite tests andstrictTS remain green for
  issuer handoff. No source claim of universal native-fabric issuer ownership:
  the new adoption protocol is wired in the Worker dispatcher only.
- Required-hit action integration now active in
  /private/tmp/corelink-takeover-memoize-required-hit. First5059a7d was rejected:
  known0.1.12 but absent credentials could fall through to cold execution, and
  failed version command could be accepted. Luna fixing explicit negatives.
- clw76c4cb fixes pre-dispatch required-hit errors to78; author focused actual
  process tests passed. Root review source complete; test-file split underway
  to avoid growing existing giant e2e. Root reproduction still pending.
- Primary checkout remains preserved; root-local npm dependencies are isolated.
  Disk4.4GiB free at04:02; do not create redundant Cargo targets or prune unknown
  worktrees. Lima/PG remain stopped.

Remaining source dependencies include external monitor (owner proposal still
unanswered), shared tenant compute authority, F008 resume/credential generations,
and F005/F007 authoritative provider cancellation/reconciliation. Read-only
Sprint1 census found most other original source slices already present; missing
source must not be confused with pending live evidence. T4-W4 historical A4.4
wording conflicts with exact fractional parser tests; strict IntrospectBody is a
conformance lens, NOT the runtime parser. Paired server producer still emits only
positiveu32 or omitted cap. Preserve actual code and report the inconsistency;
do not invent a fresh owner approval or silently change frozen billing vectors.
The shared-budget scratch proposal remains unapproved and partly stale.


## Progress at 04:09 BRT (latest)

Root Runners8260f79 includes repaired action5059a7d+181c8b9; paired clw now
91ab8175f458a1dfb946b83be25641d9071b2fc0 includes required-hit pre-dispatch fix
76c4cbb. Root reproduced16 shell fixture cases and2 actual CLI process tests
(required setup/alias failures and optional/required panic codes). Root then ran
three composed action -> actual locally built clw cases: required invalid config
78/nochild, required missing endpoint78/nochild, optional config failure0/child.
These are real negative process proofs, not live authenticated cache HIT evidence.
ADR0011 now records the supported no-exec interface and pending signed release.
No tag, release, push, image pin or deployment was fabricated. All agent work
packets for this wave are complete and integrated or explicitly rejected; no
remaining hidden fixture failures from the seven-file groupB packet.


## Progress at 04:34 BRT (latest)

Runners source tip c97ba5878e241dce73ce979c961cc927bc0d5330; paired server
and clw remain clean at dc169f8c53196e242fff213bdb3c77be3a7ade00 and
91ab8175f458a1dfb946b83be25641d9071b2fc0. Primary conflicted checkout is
preserved. Delivered count remains16/70,54 outstanding,0 complete sprints.

- Integrated Luna unsuspend a85e3c5 and root repair015edd4: persist first,
  preserve the suspension cache on failure, return503 rather than false success.
  Root7 enforcement tests passed, including actual acquire429 after failure,
  successful retry, confirmed/unconfirmed teardown and unsupported outbox.
  Root repaired old fixture assumptions; no production test bypass was added.
  The dependency fixture records outbox events but is not a Pg durability proof.
- Rootc97ba58 moves tenant-suspension to the lifecycle auth domain on both
  Worker and Rust producer.23 focused Worker auth tests and strictTS passed.
  Luna forwarding09f3ea7 adds missing exec/lifecycle bindings to fabricd's
  container environment. Root18 focused fabricd tests and strictTS passed.
  Root corrected docs: EACH control token must match client/server copies;
  domains rotate independently but do not use separate unmatched credentials.
- Removed obsolete billing comments claiming a vCPU multiplier in the current
  slot-seconds builder. Frozen event semantics and idempotency vectors unchanged.
- The first root Rust build failed for ENOSPC before tests. Removed only owned
  takeover build artifacts (Pg-r2 target, engine-auth target, clw incremental/
  deps/build), retained the actual clw binary. Rebuilt Pg-r2 target using
  CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2. Seven tests
  above passed. Disk recovered to5.8GiB at04:32. Do not prune unknown worktrees.
- All bounded packets in this wave are integrated/repaired. No full CI, new PR,
  push, release, migration, deployment, PG rearm or external message was sent.

The monitor proposal remains explicitly UNANSWERED and unapplied:
`docs/plan/execution/2026-09-06-monitor-decision-proposal.md`. No monitor app
implementation or owner waiver is implied by elapsed time. The existing seven
real observation days and trusted-time qualification are still mandatory.

Independent unfinished implementation remains: shared monthly compute authority;
F008 generation-bound consumer, ordered resume and legacy migration; and
F005/F007 authoritative provider cancellation/reconciliation. Do not attribute
all unfinished work to the pending monitor decision. The read-only budget audit
found June documentation for zero-disabled and allocated vCPU-ms, but September
A4.4 wording conflicts on absent/fractional values. The root handoff itself is
not a human ratification. Preserve current wire vectors; a stale scratch design
or code parser is not fresh product approval. The F008 audit's proposed dual
Pg/D1 generations is only a proposal: it has not established a single authority
or a safe cross-service resume protocol and must not be implemented as two
independent clocks. No additional user permission is needed for routine source
work already authorized in this session.
