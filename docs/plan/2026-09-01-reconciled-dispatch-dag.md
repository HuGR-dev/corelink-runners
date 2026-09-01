# Reconciled dispatch DAG — rev6 Round-10 repair draft

**Date:** 2026-09-01 · **Schema:** `dispatch-dag/v1` · **Status: NOT DISPATCHABLE**

This is the sole canonical dispatch registry. Every plan, delta, triage table, handoff and
dispatcher must reference this file and must not restate its DAG. The current Round-10 repair tree
has not received a cold review and is **NOT QUIET**, so the table is a schedule calculation and no
row is authorized yet. This staged
input must first receive two consecutive quiet reviews over byte-identical bytes. Promotion is a
normative new snapshot, resets quiet count to zero, and must itself receive two consecutive quiet
reviews over byte-identical promoted bytes before the one clean post-incident baseline can be
captured and the dispatch freeze discussed. After that sequence lifts the freeze, a row may run when
its own hard predecessors are satisfied: an unrelated decision/obstacle/relay token does not stop
the whole graph.

`FABRIC_PG_DISABLED=1` is a production durability and green-credit interlock, not an implementation
lock. It blocks T1-W6 durable-PG success, every dependent production proof, and the final live flip;
it does not block code/test packets whose own predecessors are ready. In particular, post-freeze
T3-W17 implementation and the T3-W18 safety-only containment deploy/probe may run while the switch
remains armed. Neither can claim durable recovery or production green.

## Registry contract

The vertex set is the WP rows in the table below. A predecessor cell may contain only another WP
id, a decision id (`D1`–`D13`), an obstacle id (`O1`, `O-DEVENV-PIN`, `O-BILLING`, `O-ALLOWLIST`,
`O-PIN`, `O-APP`, `O-CANARY`, `O-FLEETBUSY`, `O-MINTKEY`, `O-CHECKHOST`, `O-CFTOKEN`, `O-ROTATE`,
`O-PUBLISH`, `O-CFINVENTORY`, `O-CFCANCEL`, `O-CFRATE`, or `O-MONITORHOST`), or a relay id
(`R1`–`R6`).
`O-MONITORHOST` is a pre-implementation capability token satisfied only when the owner-approved
`docs/plan/evidence/O-MONITORHOST-external-monitor.json` names a viable runtime, scheduler, durable
incident store, alert-delivery transport and credential domain that are all outside Cloudflare and
outside every monitored component, with documented permissions/idempotency/SLO support. It does
the same for a sensitivity scheduler and an external receipt verifier that are distinct from the
monitor application and from each other: the artifact names their separate accounts, credential
ids, version/config digests, durable state, delivery-read permissions, control cadence of at most
six hours and independent failure/configuration/control domains. These are capability-only
properties; O-MONITORHOST does not claim application behavior, a deployed control, a receipt or a
delivery result. T6-W15 alone owns integration, deployed-version
binding and crash/timing/kill-path proof in `T6-W15-monitor-base.json`; absence of either the CAP
token or that later packet evidence is RED. Acceptance ids are
deliberately absent from predecessor cells; they map to WPs in the source registries but are not
graph vertices. `D13` is the staged owner-of-record decision for AU4.18 and its signed artifact is
`docs/adr/0013-runner-tenant-owner-precedence.md`.

`O-CFINVENTORY` is one atomic obstacle but requires three separately revocable, read-only
principals: worker reconciliation (`T3-W16`), PG-rearm inactivity proof (`T1-W6`) and external
monitoring (`T6-W12`). The single version-bound
`docs/plan/evidence/O-CFINVENTORY-provider-capabilities.json` must name all three credential ids,
prove that none can create, mutate or delete, prove isolated quotas and revocation, and record the
provider-issued freshness watermark. Reuse of a principal by any other consumer leaves the token
unresolved.

`O-CFCANCEL` is separately satisfied only by
`docs/plan/evidence/O-CFCANCEL-provider-cancellation.json`, version-bound to the exact deployed
provider API/SDK. That artifact must prove a linearizable exact-handle cancellation operation, or
prove that `await destroy()` after the original `start()` settles is a barrier after which that
start cannot materialize. A numeric latency or inventory absence cannot satisfy it. T3-W16 requires
both O-CFINVENTORY and O-CFCANCEL; without the cancellation token it remains
`RECONCILIATION_REFUSED` indefinitely.

`T7-W3` is the evidence-schema gate and `T7-W4b` is the evidence-freshness gate. Every probe or
test+probe row has `T7-W4b` as a hard predecessor (and therefore transitively has T7-W3), has
`T3-W18` ancestry (and therefore cannot probe before containment is live), and has a unique
artifact filename in its artifact column. No row owns the broad `docs/plan/evidence/**` tree.
`T1-W6` is the durable-PG live-success gate; only durability-dependent live probes wait for it.
`T6-W13` is the immediate canary-key lane (including current/stale key delivery and
acknowledgement) and necessarily follows T6-W4, T6-W9, O-CANARY and T7-W4b, but has no PG predecessor;
`T6-W14` is the later isolated no-wake re-enable trial. `T6-W9` implements and seals the alert
rules before `T6-W6` attempts their live proof. `T6-W4` implements only the default-off canary
scheduled-tick producer with its own scoped key. `T6-W15` then deploys the provider-neutral
external-monitor base, including authenticated lifecycle-sample and canary-heartbeat ingestion,
source-specific missing-sample detectors, durable incident state and an idempotent delivery
outbox. It may implement only after `O-MONITORHOST` names the capable non-Cloudflare runtime,
scheduler, state, delivery and credential domains; T6-W15 then binds, deploys and proves the base.
`T6-W12` is the serialized pre-rearm follow-on that adds the paginated provider adapter, cost
correlator and version-bound A6.20 live proof after `O-CFINVENTORY` proves the required cumulative
surfaces and their freshness. It reopens and integrates the base's index, scheduler, config, state,
incident and type seams, applies the provider-cursor migration, and performs the final external-
service deployment. With `FABRIC_PG_DISABLED=1`, T6-W12 must first execute every unchanged T6-W15
test against its candidate image before cutover. A candidate PASS permits an atomic cutover; T6-W12
must then rerun the entire unchanged T6-W15 suite against the active final deployed tuple before PG
rearm. The existing `docs/plan/evidence/T6-W12-independent-monitor.json` records both complete
executions and their candidate/final image digest, config digest, credential epochs, expected-source
registry digest and T6-W15 test-tree digest/results, plus the previous active version and atomic
cutover/rollback outcome. A candidate failure forbids cutover; any post-cutover failure rolls back,
keeps PG disabled and forbids T6-W12 completion. Only a post-cutover PASS against the active final
tuple completes T6-W12. This is an execution obligation only: T6-W15 retains exclusive ownership of
every base test file, and T6-W12 may not copy, weaken or rewrite the suite. The final post-cutover
PASS also seals the exact
`monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest)`
and its digest. Stale or frozen provider watermarks and any T6-W15-suite regression fail closed;
provider/correlator source files alone cannot seal this phase. Only this final green monitor/provider
version may precede `T1-W6`, which records the exact sealed `monitor_rearm_tuple` digest, verifies
all six fields unchanged and performs a fresh version-bound monitor/provider poll immediately
before rearming PG. Any tuple-field
drift after T6-W12's post-cutover PASS and before rearm invalidates both packets and keeps PG
disabled. After rearm, any proposed tuple-field drift must first atomically restore
`FABRIC_PG_DISABLED=1`, then repeat T6-W12's candidate/cutover/active-suite proof and T1-W6's final
poll; drift without that sequence is hard RED and fails closed.

**Nonce-bound rearm attestation and durable interlock.** T6-W12's
`deploy/cost-monitor/src/rearm_attestation.ts` derives the current effective `monitor_rearm_tuple`
from the running deployment. For each fresh caller nonce it returns a signature over that nonce,
the exact effective tuple digest and separately timestamped provider-poll and delivery-route
health. The challenge response age is at most 10 seconds, and provider-poll and delivery health
observations are at most 60 seconds old. T1-W6's
`crates/corelink-fabric-server/src/monitor_interlock.rs`
obtains a new response before every readiness answer, PG-backed mutation, and PG/exporter
socket/init/pool use, then verifies the bound tuple digest, nonce echo, signature and each applicable
freshness rule. A cached success, nonce replay or earlier valid response is never reusable.

Any tuple mismatch, unavailable attestation, stale provider-poll or delivery health, bad signature
or nonce failure first atomically arms a durable non-PG disable latch. Once latched, T1-W6 closes and
discards every PG pool/socket, returns typed 503 for readiness and mutation, and permits zero
further PG/exporter socket or mutation actions, including after restart. A planned
`monitor_rearm_tuple` change must arm the latch and drain/discard pools before the change begins.
`crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs` independently mutates all six tuple
fields and injects unavailable, missing, stale, bad-signature, wrong-nonce, replayed and
cached attestations; `deploy/cost-monitor/test/rearm-tuple-attestation.test.ts` proves the signed
effective tuple and the two correctly bounded health classes. Every negative case is green only
when the latch is durable, pools/sockets are gone, typed 503 is returned and zero action occurs.
The latch may clear only after complete T6-W12 candidate/cutover/active-final reproof, exact T1-W6
rebinding to the new tuple and an explicit manual reset; none alone restores PG.

T6-W12 also owns the A6.17 sensitivity-window implementation. It runs from an O-MONITORHOST
scheduler and credential distinct from the monitor application and validates delivery through an
external receipt verifier isolated from monitor application configuration, so disabling or
desensitizing the monitored detector/delivery path cannot green the seven-day window. T6-W10 owns
only the evidence-only consumption of those seven-day sensitivity results. The seven-day evidence
is bound to exactly
`A6.17_window_tuple=(monitor_rearm_tuple_digest,sensitivity_scheduler_config_key_digest,receipt_verifier_version_config_digest,on_call_escalation_schedule_digest)`.
Any constituent drift during the window invalidates all elapsed time and restarts a full seven-day
window. Drift of `monitor_rearm_tuple_digest` additionally invokes the broader PG-disable/reproof
rule above; drift confined to the other three A6.17 fields invalidates only the A6.17 window and
does not by itself invalidate the PG-rearm proof. `T6-W10` must record and consume one unchanged
`A6.17_window_tuple` digest for the complete window. Sensitivity receipt/health is excluded from the
rearm attestation and PG latch: a missing or overdue sensitivity receipt alerts and restarts only
the A6.17 window unless an independent tuple, provider-poll or core delivery failure separately
triggers the interlock. The sensitivity receipt is overdue only relative to its configured cadence
of at most six hours, never the rearm attestation's 60-second observation bound. `T6-W14` later
makes
canary read the passive fabricd outer-Worker lifecycle route
and deliver service-bound authenticated lifecycle samples into T6-W15's ingestion contract; the
outer Worker never self-heartbeats or posts to the monitor.

Before T6-W12 seals its final deployed tuple, it pre-registers the exact future T1-W6
`fabric-server` and `fabricd-proxy` source ids and issues both isolated write-only
key-id/credential-epoch pairs. Those registrations and credential epochs are inputs to both
complete T6-W15-suite executions. T1-W6 may only bind the already-issued pairs; it cannot mint,
rotate, substitute or register them. `T6-W4` durably enqueues each scheduled tick before
transmission through a Durable Object outbox. Its existing canonical
`deploy/cloudflare-canary/test/scheduled-tick-outbox-recovery.test.ts` and
`deploy/cloudflare-canary/test/scheduled-tick-order.test.ts` deterministic tests, run with
`FABRIC_PROBES_ENABLED=0`, prove durable capacity one and a total bound of at most 60 seconds from
enqueue to the external monitor's committed ACK or typed terminal; the Wrangler binding/migration
and crash/retry/order tests remain mandatory atoms, not implied by an envelope unit test.

`T3-W16` directly waits for T6-W15 so its attempt/binding producer can use only the deployed
external monitor. That lane has durable capacity one: it may hold at most one nonterminal head, and
the total interval from durable enqueue to the external monitor's committed ACK or typed terminal
is at most 60 seconds. The exact head must be externally ACKed before container start or before any
next immutable lifecycle/cost action. If the head is not terminal within 60 seconds, the packet is
hard RED and the start/action fails closed. `T1-W6` applies the same already-required action gate to
its two non-interchangeable write-only producer scopes (`fabric-server` and `fabricd-proxy`), each
with its own durable capacity-one ordered outbox, key id and credential epoch; neither may reuse any
read-only O-CFINVENTORY principal. T1-W6's provider inactivity proof therefore names
O-CFINVENTORY directly and uses only the rearm-probe principal. T6-W14's periodic lifecycle and
synthetic lanes are independently capacity one and cannot enqueue a successor or perform the
successor's immutable action until the current head receives its external ACK or typed terminal.
Credential rotation cannot reset the original enqueue clock or bypass a head: the producer must
either drain that exact head under its original credential or perform a signed epoch migration that
preserves its exact bytes, source/event/sequence identity, original timestamps and original
deadline. Migration never restarts the 60-second bound; exceeding it is hard RED and all dependent
actions remain fail closed.

The worker scope is a total order because these rows write `deploy/cloudflare/src/index.ts` or
`deploy/cloudflare/src/lib.ts`. The exact inherited Round-7 repair spine remains:

`T3-W17 → T3-W18 → T4-W1 → T4-W2 → T3-W3 → T3-W1 → T3-W2 → T8-W1 → T8-W3 → T3-W14 → T3-W9 → T8-W5 → T8-W2 → T3-W16 → T3-W15 → T3-W5`.

The Round-7 repair links include `T3-W17→T3-W18`, `T3-W18→T4-W1`,
`T4-W1→T4-W2`, `T4-W2→T3-W3`, `T3-W3→T3-W1`, `T8-W3→T3-W14`,
`T3-W14→T3-W9`, `T3-W9→T8-W5`, `T8-W5→T8-W2`, `T8-W2→T3-W16`,
`T3-W16→T3-W15`, and `T3-W15→T3-W5`. The complete total order above also retains the inherited
`T3-W1→T3-W2→T8-W1→T8-W3` links. `T3-W17`/`T3-W18` are the first post-freeze
containment lane; `T3-W16` (attempt-handle/inventory cross-check) precedes `T3-W15` (re-drive
liveness). This containment precedence applies to every worker-monolith mutation, forced deploy,
destructive or live Cloudflare operation and live proof. It does not serialize independent
documentation, local-test, runbook or CI work that neither writes the worker monolith nor touches
Cloudflare.
`T9-W1` is a separate Sol decision-gated lane because its exact scope is limited to the devenv DO
and its focused test; it owns neither worker-monolith file. It is nevertheless a hard predecessor
of both billing proof lanes: satisfying the external `O-BILLING` token cannot bypass the devenv
poison-pill repair. The registry has exactly 69 vertices:
48 principal WPs, 9 staged principal WPs and 12 AU WPs (T3-W5 is one of the 12 new AU WPs, not an
extension). T6-W15 is the principal A6.10 external-detector owner; T6-W4 owns only the canary tick
producer half. T6-W15 also owns A6.20's base half, but cannot green that staged item without
T6-W12's final pre-rearm provider/live half.

Because `D3` is an external token rather than a graph vertex, each D3 consumer also names `D7`
directly; the scheduler may not infer the owner-decision relation `D7 → D3`. Similarly, T4-W2 names
R2 directly before changing usage accounting, and the stranger proof T5-W4 waits for T5-W1's
onboarding surface. T8-W4 directly precedes the pin, deploy and image-ship packets so T8-W7 can
prove a deployed digest that actually contains the JIT-config fix.

T3-W16 uses a durable local attempt-to-DO-handle record committed before start. Handle RPC
`getState`/`destroy` is authoritative; provider inventory is a read-only cost/cross-check signal,
and incomplete or ambiguous inventory pauses replacement. The same packet implements its
paginated, authenticated, read-only provider adapter in `index.ts`/`lib.ts` with native `fetch`;
this introduces no new SDK/package dependency and never makes provider inventory authoritative.
`O-CFINVENTORY` supplies the three isolated read-only credentials and feed-capability artifact;
`O-CFCANCEL` independently supplies the exact-handle cancellation barrier. T3-W15 consumes the
adapter and inherits both gates through T3-W16 for its re-drive liveness decision.

For staged A3.30, `T3-W17` owns the deterministic repo tests and has no live evidence credit;
`T3-W18` exclusively owns the version-bound three-state live probe artifact. The two switches remain
independent, and no later deploy or worker mutation can bypass the live containment half.

For staged A6.22, the non-waking lifecycle endpoint is implemented in the fabricd **edge Worker**
and reads only Durable Object lifecycle state; it never calls container `fetch`. Its authoritative
response binds a monotonic sequence, transition id/state/time, deployed version, nonce and sample
time, uses `no-store`, and is rejected when stale or replayed. The route is passive: it never owns
a timer, monitor credential, delivery sequence or heartbeat. T6-W14's canary sampler reads it with
a fresh nonce, validates it, then durably emits a service-bound authenticated lifecycle envelope
with rotation/revocation, replay and monotonic-sequence refusal semantics into the T6-W15 monitor.
The T6-W4 canary scheduled tick uses an independently keyed envelope with the same anti-replay
discipline; the external monitor owns both missing-sample timers, incident state and page-delivery
outbox, not either Cloudflare producer. Canary tests must prove that this surface, rather than a
test-controlled or static response, drives both positive and negative transitions before
`FABRIC_PROBES_ENABLED` changes in canary config.

T6-W12 also integrates the external monitor's real C1–C5 rules and synthetic-ingest contract after
T6-W9 seals the rule semantics; T6-W6 cannot attempt its live outage proof until that external
implementation is deployed. T6-W14 implements and deploys the canary's synthetic
acquire→spawn→release transaction and its focused deterministic test, but deploys it default-off
and earns no AU6.17 probe credit. Its synthetic-result producer uses its own scoped credential,
source id and monotonic sequence/outbox lane, and binds one correlation id across acquire, spawn and
release; tick and lifecycle credentials or sequence spaces are never reused. The A6.22 evidence
runs with this synthetic lane off. T6-W10 follows T6-W6, T6-W12 and T6-W14, exclusively owns its
later arming plus the 20/20 AU6.17 execution, and writes the live C1–C5 failure-domain/AU6.17
artifacts. Evidence that precedes either real implementation is invalid.

## Canonical node table

| node | phase / wave | exact hard predecessors | exclusive path atoms; artifact filename | lane |
|---|---|---|---|---|
| T0-W1 | W0 unblock | — | `docs/plan/union-catalog-ledger.md`; — | Luna / mechanical |
| T1-W1 | W0 unblock | — | `scripts/ops/fabricd-preflight.sh`; `scripts/ops/fabricd-boot-rate.sh`; — | Luna / runbook |
| T2-W1a | W0 unblock | — | `.github/workflows/build-cf-container-images.yml`; — | Luna / CI |
| T2-W2a | W0 unblock | T2-W1a, T8-W4 | `scripts/ci/image-pin-freshness.sh`; `scripts/ci/image-pin-freshness.selftest.sh`; — | Luna / CI |
| T9-W0 | W1 parallel | — | `deploy/cloudflare/vitest.config.ts`; `deploy/cloudflare/test/devenv-do.test.ts`; — | Luna / CI |
| T3-W17 | W0 unblock | T0-W1 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/metrics.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/containment-intake.test.ts`; `deploy/cloudflare/test/containment-redrive.test.ts`; — | Sol / safety |
| T3-W4 | W1 serial | D1 | `crates/corelink-fabric-server/src/**`; `crates/corelink-fabric-server/tests/corelink_plans.rs`; `crates/corelink-fabric-server/tests/close_reaper_lock_split.rs`; — | Sol / architecture |
| T4-W4 | W1 serial | T3-W4, D1, R1 | `crates/corelink-fabric-server/src/**`; `crates/corelink-fabric-server/tests/corelink_admission_arms.rs`; `crates/corelink-fabric-server/tests/acceptance_infra_capacity.rs`; — | Sol / architecture |
| T3-W10 | W1 serial | T3-W4, T4-W4 | `crates/corelink-fabric-server/src/reaper.rs`; `crates/corelink-fabric-server/src/handlers/cas_cred.rs`; `crates/corelink-fabric-server/tests/reaper_teardown_retry.rs`; `crates/corelink-fabric-server/tests/cas_cred_error_body.rs`; — | Sol / architecture |
| T6-W1 | W1 parallel | — | `scripts/orphan-box-check.selftest.sh`; `scripts/pre-merge-gate-check.selftest.sh`; `scripts/pre-merge-gate-check.sh`; `.github/workflows/ci.yml`; `.github/workflows/selftests.yml`; — | Luna / CI |
| T6-W2 | W1 parallel | D11 | `.github/workflows/moat-benchmark.yml`; `.github/workflows/moat-action-test.yml`; `actions/corelink-memoize/action.yml`; — | Sol / contract |
| T6-W3 | W1 parallel | — | `.github/workflows/conformance.yml`; `.github/workflows/spawn-worker-ci.yml`; `sdk/**`; — | Luna / CI |
| T6-W8 | W1 parallel | — | `.github/workflows/pg-suite.yml`; `crates/corelink-fabric/tests/acceptance_cf0_fabric_core.rs`; — | Sol / live-risk |
| T6-W4 | W1 parallel | T3-W18, T7-W4b | `.github/workflows/secret-scan.yml`; `.github/workflows/corelink-stress.yml`; `deploy/cloudflare-canary/src/index.ts`; `deploy/cloudflare-canary/src/types.ts`; `deploy/cloudflare-canary/src/tick_outbox.ts`; `deploy/cloudflare-canary/wrangler.jsonc` (Durable Object binding and migration); `deploy/cloudflare-canary/package.json`; `deploy/cloudflare-canary/package-lock.json`; `deploy/cloudflare-canary/test/scheduled-tick-envelope.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-outbox-recovery.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-order.test.ts`; `docs/plan/evidence/T6-W4-stress-host.json` | Sol / live-risk |
| T6-W15 | W1 serial | T6-W4, O-MONITORHOST, T7-W4b | `deploy/cost-monitor/Containerfile`; `deploy/cost-monitor/config.schema.json`; `deploy/cost-monitor/src/index.ts`; `deploy/cost-monitor/src/ingest.ts`; `deploy/cost-monitor/src/incidents.ts`; `deploy/cost-monitor/src/lifecycle.ts`; `deploy/cost-monitor/src/scheduler.ts`; `deploy/cost-monitor/src/state.ts`; `deploy/cost-monitor/src/delivery.ts`; `deploy/cost-monitor/src/outbox.ts`; `deploy/cost-monitor/src/types.ts`; `deploy/cost-monitor/package.json`; `deploy/cost-monitor/package-lock.json`; `deploy/cost-monitor/tsconfig.json`; `deploy/cost-monitor/vitest.config.ts`; `deploy/cost-monitor/test/lifecycle-missing.test.ts`; `deploy/cost-monitor/test/canary-missing-tick.test.ts`; `deploy/cost-monitor/test/ingest-idempotency.test.ts`; `deploy/cost-monitor/test/incident-state.test.ts`; `deploy/cost-monitor/test/scheduler.test.ts`; `deploy/cost-monitor/test/state.test.ts`; `deploy/cost-monitor/test/delivery.test.ts`; `deploy/cost-monitor/test/outbox-recovery.test.ts`; `deploy/cost-monitor/test/outbox-transition-head.test.ts`; `deploy/cost-monitor/test/outbox-periodic-head.test.ts`; `deploy/cost-monitor/test/outbox-quarantine.test.ts`; `deploy/cost-monitor/test/delivery-dedupe.test.ts`; `deploy/cost-monitor/test/credential-isolation.test.ts`; `deploy/cost-monitor/test/independence.test.ts`; `docs/plan/evidence/T6-W15-monitor-base.json` | Sol / alerting |
| T5-W1 | W1 parallel | R6 | `docs/onboarding/**`; `actions/corelink-memoize/README.md`; — | Luna / documentation |
| T5-W2 | W1 parallel | D7, D3 | `integrations/**`; — | Sol / release |
| T5-W3 | W1 serial | T5-W2 | `integrations/github-actions/action.yml`; `integrations/github-actions/test/validate.sh`; — | Sol / security |
| T7-W1 | W1 parallel | T0-W1 | `docs/ROADMAP.md`; `CHANGELOG.md`; — | Luna / documentation |
| T7-W2 | W1 parallel | — | `docs/**` excluding `plan/`,`handoff/`,`review/`,`audits/`,`onboarding/`,`runbook/`,`product/`,`adr/`,`ROADMAP.md`; `deploy/**/README.md` excluding canary; — | Luna / documentation |
| T7-W3 | W1 parallel | — | `scripts/ci/claim-artifact-lint.sh`; `docs/plan/evidence/schema-v1.json`; `docs/plan/evidence/manifest-v1.json` | Luna / mechanical |
| T7-W4 | W1 parallel | — | `docs/runbook/secret-inventory.md`; `crates/corelink-fabric/src/plans.rs`; `scripts/ci/secret-inventory-drift.sh`; `scripts/ci/secret-inventory-drift.selftest.sh`; — | Luna / documentation |
| T7-W4b | W1 serial | T7-W3 | `scripts/ci/probe-freshness-check.sh`; `docs/plan/evidence/freshness-v1.json` | Luna / mechanical |
| T2-W3 | W1 closer | T2-W1a, T6-W1, T6-W2, T6-W3, T6-W4, T6-W8, T7-W4 | `.github/workflows/*.yml`; — | Luna / CI |
| T4-W1 | W2 worker | T3-W18, D13 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/webhook-installation-allowlist.test.ts`; — | Sol / money-path |
| T4-W2 | W2 worker | T4-W1, R2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/usage-ledger-backfill.test.ts`; — | Sol / money-path |
| T3-W3 | W2 worker | T4-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/orphan-box-reconcile.test.ts`; — | Sol / lifecycle |
| T3-W1 | W2 worker | T3-W3 | `deploy/cloudflare/src/index.ts`; `crates/corelink-cloud-engine/src/cloudflare.rs`; `crates/corelink-cloud-engine/src/http.rs`; `crates/corelink-cloud-engine/src/lib.rs`; `crates/corelink-cloud-engine/tests/acceptance_cloud.rs`; `crates/corelink-cloud-engine/tests/cloudflare_conformance.rs`; `deploy/cloudflare/test/spawn-conformance.test.ts`; — | Sol / wire |
| T3-W2 | W2 worker | T3-W1, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/src/metrics.ts`; `deploy/cloudflare/test/metrics-do.test.ts`; `docs/plan/evidence/T3-W2-lifecycle.json` | Sol / safety |
| T8-W1 | W2 worker | T3-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/admission-failopen-budget.test.ts`; `deploy/cloudflare/test/cold-reason-attribution.test.ts`; — | Sol / safety |
| T8-W3 | W2 worker | T8-W1, O-MINTKEY, T6-W9, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/option-c-pat-dispatch.test.ts`; `deploy/cloudflare/test/spawn-claim-atomic.test.ts`; `docs/plan/evidence/T8-W3-required-mint.json` | Sol / safety |
| T3-W14 | W2 worker | T8-W3 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/retry-epoch.test.ts`; — | Sol / safety |
| T3-W9 | W2 worker | T3-W14, O-APP | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/spawn-durability.test.ts`; — | Sol / lifecycle |
| T8-W5 | W2 worker | T3-W9 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/credential-revocation.test.ts`; — | Sol / credential lifecycle |
| T8-W2 | W2 worker | T8-W5 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/cred-cred-route.test.ts`; `deploy/cloudflare/test/cred-stash-do.test.ts`; — | Sol / credential scope |
| T3-W16 | W2 worker | T8-W2, T2-W2b, T6-W15, O-CFINVENTORY, O-CFCANCEL, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/attempt-handle-reconcile.test.ts`; `deploy/cloudflare/test/attempt-monitor-outbox.test.ts`; `deploy/cloudflare/test/inventory-crosscheck.test.ts`; `docs/plan/evidence/T3-W16-attempt-handle-crosscheck.json` | Sol / inventory |
| T3-W15 | W2 worker | T3-W16 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/redrive-state-machine.test.ts`; — | Sol / lifecycle |
| T8-W4 | W1 parallel | — | `deploy/runner/entrypoint.sh`; `deploy/runner/test/jitconfig-secret-surface.sh`; `crates/corelink-check-exec-server/src/**`; `crates/corelink-check-exec-server/tests/auth_token_file.rs`; — | Sol / security |
| T8-W6 | W3 live proof | T8-W5, T8-W2, T1-W6, T7-W4b | deployed worker version containing T8-W2; `docs/plan/evidence/au4.16b-suspension-pat-revocation.json`; `docs/plan/evidence/au3.23b-completion-pat-revocation.json` | Sol / live-risk |
| T8-W7 | W3 live proof | T8-W4, T2-W2a, T2-W2b, T2-W4, T1-W6, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` | Sol / live-risk |
| T3-W5 | W4 post-decision | D4, T3-W15 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/burst-above-ceiling.test.ts`; `deploy/cloudflare/test/orphan-retry.test.ts`; — | Sol / decision-gated |
| T9-W1 | W2 separate lane | D2, T9-W0 | `deploy/cloudflare/src/durable_objects/runner_dev_env.ts`; `deploy/cloudflare/test/devenv-do.test.ts`; — | Sol / decision-gated |
| T1-W5 | W1 serial | T3-W10, T3-W18, O-MINTKEY, T7-W4b | `crates/corelink-fabric-server/src/runner_cas_mint.rs`; `crates/corelink-fabric-server/src/server.rs`; `crates/corelink-fabric-server/tests/mint_selfcheck.rs`; `docs/plan/evidence/T1-W5-mint-selfcheck.json` | Sol / architecture |
| T1-W6 | W1 serial | D12, T1-W5, T6-W12, O-CFINVENTORY, T7-W4b | `crates/corelink-fabric/src/pg_ledger.rs`; `crates/corelink-fabric/src/billing_sink.rs`; `crates/corelink-fabric-server/src/main.rs`; `crates/corelink-fabric-server/src/server.rs`; `crates/corelink-fabric-server/src/billing_export.rs`; `crates/corelink-fabric-server/src/monitor_outbox.rs` (write-only `fabric-server` producer); `crates/corelink-fabric-server/src/monitor_interlock.rs`; `crates/corelink-fabric-server/tests/monitor_outbox.rs`; `crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs`; `crates/corelink-fabric/tests/pg_refusal_breaker.rs`; `deploy/cloudflare-fabricd/src/index.ts`; `deploy/cloudflare-fabricd/src/monitor_outbox.ts` (write-only `fabricd-proxy` producer); `deploy/cloudflare-fabricd/test/resilience.test.ts`; `deploy/cloudflare-fabricd/test/monitor-outbox.test.ts`; `deploy/cloudflare-fabricd/wrangler.jsonc`; `docs/plan/evidence/T1-W6-pg-durable-live.json` | Sol / live-risk |
| T1-W2 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T1-W2-control-plane.json` | Sol / live-risk |
| T1-W3 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T1-W3-resilience.json` | Sol / live-risk |
| T1-W4 | W3 live proof | O1, T1-W6, T6-W14, T7-W4b | `docs/plan/evidence/T1-W4-boot-rate.json` | Sol / live-risk |
| T2-W2b | W3 live proof | T2-W1a, T3-W18, T8-W4, O1, O-DEVENV-PIN, O-FLEETBUSY, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/T2-W2b-deploy.json` | Sol / live-risk |
| T2-W4 | W3 live proof | T2-W2b, T8-W4, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/T2-W4-image-ship.json` | Luna / release |
| T2-W5 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T2-W5-runbook-compat.json` | Luna / runbook |
| T2-W6 | W3 live proof | O1, T2-W2b, T7-W4, T7-W4b | `docs/runbook/secret-rotation.md`; `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` | Luna / runbook |
| T3-W7 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T3-W7-moat.json` | Sol / live-risk |
| T3-W8 | W3 live proof | O1, T3-W7, T1-W6, T7-W4b | `docs/plan/evidence/T3-W8-inventory-hitrate.json` | Sol / live-risk |
| T4-W7 | W3 live proof | T4-W2, T9-W1, O-BILLING, R1, R2, T1-W6, T7-W4b | `docs/plan/evidence/T4-W7-money.json` | Sol / money-path |
| T4-W8 | W3 live proof | T4-W2, T9-W1, O-BILLING, T1-W6, T7-W4b | `docs/plan/evidence/T4-W8-billing-reconcile.json` | Sol / money-path |
| T5-W4 | W3 live proof | D7, D3, D8, R3, T5-W1, T1-W6, T7-W4b | `docs/plan/evidence/T5-W4-stranger.json` | Sol / onboarding |
| T5-W5 | W3 live proof | D7, D3, D8, R3, T5-W4, T1-W6, T7-W4b | `docs/plan/evidence/T5-W5-stranger-adversarial.json` | Sol / onboarding |
| T5-W6 | W3 live proof | T3-W18, D7, D3, T5-W3, O-PUBLISH, T7-W4b | `docs/plan/evidence/T5-W6-release-artifacts.json` | Luna / release |
| T6-W5 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T6-W5-e2e.json` | Sol / live-risk |
| T6-W6 | W3 live proof | T6-W9, T6-W12, O-CANARY, T7-W4b | `docs/plan/evidence/T6-W6-canary.json` | Sol / live-risk |
| T6-W7 | W3 live proof | T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T6-W7-authz.json` | Sol / live-risk |
| T6-W9 | W3 live proof | T6-W4, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/rules.ts`; `deploy/cloudflare-canary/src/notify.ts`; `deploy/cloudflare-canary/test/rules.test.ts`; `docs/plan/evidence/T6-W9-alert-rules.json` | Sol / alerting |
| T6-W10 | W3 live proof | T6-W6, T6-W9, T6-W12, T6-W14, T1-W6, T7-W4b | later synthetic-slot arming and 20-of-20 execution; `docs/plan/evidence/T6-W10-alerting-depth.json`; `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` | Sol / alerting |
| T6-W11 | W3 live proof | T1-W6, T7-W4b | `docs/plan/evidence/T6-W11-runbook-execution.json` | Luna / runbook |
| T7-W5 | W3 live proof | O1, O-CFRATE, T3-W7, T1-W6, T7-W4b | `docs/product/pricing.md`; `docs/plan/evidence/au7.11-queued-running-latency.json`; `docs/plan/evidence/au4.19-cloudflare-container-rate.json`; `docs/plan/evidence/au7.12-memoization-hit-rate.json` | Luna / economics |
| T6-W12 | W1 pre-rearm live gate | T6-W15, T6-W9, T3-W16, O-CFINVENTORY, T7-W4b | `deploy/cost-monitor/Containerfile`; `deploy/cost-monitor/config.schema.json`; `deploy/cost-monitor/package.json`; `deploy/cost-monitor/package-lock.json`; `deploy/cost-monitor/src/index.ts`; `deploy/cost-monitor/src/scheduler.ts`; `deploy/cost-monitor/src/state.ts`; `deploy/cost-monitor/src/incidents.ts`; `deploy/cost-monitor/src/types.ts`; `deploy/cost-monitor/src/provider.ts`; `deploy/cost-monitor/src/correlator.ts`; `deploy/cost-monitor/src/capability_rules.ts`; `deploy/cost-monitor/src/synthetic_ingest.ts`; `deploy/cost-monitor/src/sensitivity.ts`; `deploy/cost-monitor/src/rearm_attestation.ts`; `deploy/cost-monitor/migrations/0002-provider-cursors.json`; `deploy/cost-monitor/test/provider.test.ts`; `deploy/cost-monitor/test/provider-stale-frozen.test.ts`; `deploy/cost-monitor/test/correlator.test.ts`; `deploy/cost-monitor/test/cursor-crash.test.ts`; `deploy/cost-monitor/test/provider-unavailable.test.ts`; `deploy/cost-monitor/test/incident-boundary.test.ts`; `deploy/cost-monitor/test/acked-incident-update.test.ts`; `deploy/cost-monitor/test/recovery-horizon.test.ts`; `deploy/cost-monitor/test/c1-c5-rules.test.ts`; `deploy/cost-monitor/test/c1-c5-synthetic-ingest.test.ts`; `deploy/cost-monitor/test/sensitivity-window.test.ts`; `deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`; `docs/plan/evidence/T6-W12-independent-monitor.json` | Sol / alerting |
| T3-W18 | W0 containment (post-freeze) | T3-W17, T7-W4b | `deploy/cloudflare/wrangler.jsonc` (re-drive-only arming and serialized worker deploy); `docs/plan/evidence/T3-W18-containment-live.json` | Sol / live-risk |
| T6-W13 | W3 live proof | T6-W4, T6-W9, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/**`; `deploy/cloudflare-canary/wrangler.jsonc`; `deploy/cloudflare-canary/test/metrics-key-lane.test.ts`; `docs/plan/evidence/T6-W13-canary-key-lane.json` | Sol / alerting |
| T6-W14 | W3 live proof | T6-W13, T6-W12, T1-W6, O-CANARY, O-MONITORHOST, T7-W4b | `deploy/cloudflare-fabricd/src/index.ts`; `deploy/cloudflare-fabricd/src/lifecycle.ts`; `deploy/cloudflare-fabricd/test/lifecycle-marker.test.ts`; `deploy/cloudflare-canary/src/index.ts`; `deploy/cloudflare-canary/src/lifecycle_outbox.ts`; `deploy/cloudflare-canary/src/synthetic_slot.ts`; `deploy/cloudflare-canary/src/synthetic_outbox.ts`; `deploy/cloudflare-canary/src/rules.ts`; `deploy/cloudflare-canary/src/types.ts`; `deploy/cloudflare-canary/wrangler.jsonc` (lifecycle and default-off synthetic bindings/migrations); `deploy/cloudflare-canary/test/no-wake-target.test.ts`; `deploy/cloudflare-canary/test/lifecycle-monitor-envelope.test.ts`; `deploy/cloudflare-canary/test/lifecycle-sampler-outbox.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-lifecycle.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-outbox.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-default-off.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-credential-isolation.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-correlation.test.ts`; `docs/plan/evidence/T6-W14-canary-no-wake.json` | Sol / live-risk |

## Deterministic ready sets and proof

For review purposes, the dispatcher runs Kahn's algorithm over the complete table, removes
satisfied decision/obstacle/relay tokens, sorts each ready set lexicographically by node id, and
emits at most eight nodes per batch. The rendered batches are a deterministic **full-plan replay**
with every external token assumed satisfied; they deliberately include T0-W1 even though that WP
is already complete in the current incident. A runtime dispatcher must subtract every WP with a
durable complete record before calculating its live ready set and must never redispatch that WP;
completion subtraction preserves its outgoing edges as satisfied. The batches are a schedule
calculation, not authorization:

```text
B00: T0-W1 T1-W1 T2-W1a T3-W4 T5-W1 T5-W2 T6-W1 T6-W2
B01: T3-W17 T4-W4 T5-W3 T6-W3 T6-W8 T7-W1 T7-W2 T7-W3
B02: T3-W10 T7-W4 T7-W4b T8-W4 T9-W0
B03: T2-W2a T3-W18 T9-W1
B04: T1-W5 T2-W2b T4-W1 T5-W6 T6-W4
B05: T2-W3 T2-W4 T2-W6 T4-W2 T6-W15 T6-W9
B06: T3-W3 T6-W13
B07: T3-W1
B08: T3-W2
B09: T8-W1
B10: T8-W3
B11: T3-W14
B12: T3-W9
B13: T8-W5
B14: T8-W2
B15: T3-W16
B16: T3-W15 T6-W12
B17: T1-W6 T3-W5 T6-W6
B18: T1-W2 T1-W3 T2-W5 T3-W7 T4-W7 T4-W8 T5-W4 T6-W11
B19: T3-W8 T5-W5 T6-W14 T6-W5 T6-W7 T7-W5 T8-W6 T8-W7
B20: T1-W4 T6-W10
```

The checker validates that every predecessor token is in the registry, every WP appears exactly
once in the ready-set output, no batch exceeds eight, and the final emitted count equals the table
vertex count. This rendering has 21 batches, 69 unique emissions and maximum width eight. Kahn's
algorithm consumed all vertices (no residual indegree), proving this version acyclic. A future
change must regenerate the batches and update `schema: dispatch-dag/v1`; hand-edited edges or
repeated DAG text in another document are invalid.

The table and batches remain **NOT DISPATCHABLE** until the staged snapshot has two quiet rounds,
the normative promotion has reset quiet count, the byte-identical promoted snapshot has two more
quiet rounds, and the clean post-incident baseline exists. After that freeze lifts, an unresolved
D/O/R token blocks only the rows that name it and their descendants. `FABRIC_PG_DISABLED=1` may
remain armed during implementation and safety containment; it blocks T1-W6 durable-PG green,
dependent production proof credit and the final production live flip, not unrelated code/test
dispatch.
