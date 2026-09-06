# Techlead takeover — 2026-09-06

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
