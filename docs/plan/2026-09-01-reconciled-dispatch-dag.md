# Reconciled dispatch DAG — rev6 Round-5 repair draft

**Date:** 2026-09-01 · **Schema:** `dispatch-dag/v1` · **Status: NOT DISPATCHABLE**

This is the sole canonical dispatch registry. Every plan, delta, triage table, handoff and
dispatcher must reference this file and must not restate its DAG. It is a post-freeze schedule
calculation only: the current Round-5 repair ledger is **NOT QUIET**, so no row is authorized to run.
Dispatch remains blocked until two consecutive quiet review rounds, the signed decisions and
obstacles below, and one clean post-incident baseline all exist.

## Registry contract

The vertex set is the WP rows in the table below. A predecessor cell may contain only another WP
id, a decision id (`D1`–`D13`), an obstacle id (`O1`, `O-DEVENV-PIN`, `O-BILLING`, `O-ALLOWLIST`,
`O-PIN`, `O-APP`, `O-CANARY`, `O-FLEETBUSY`, `O-MINTKEY`, `O-CHECKHOST`, `O-CFTOKEN`, `O-ROTATE`,
`O-PUBLISH`, `O-CFINVENTORY`, or `O-CFRATE`), or a relay id (`R1`–`R6`). Acceptance ids are
deliberately absent from predecessor cells; they map to WPs in the source registries but are not
graph vertices. `D13` is the staged owner-of-record decision for AU4.18 and its signed artifact is
`docs/adr/0013-runner-tenant-owner-precedence.md`.

`T7-W3` is the evidence-schema gate and `T7-W4b` is the evidence-freshness gate. Every probe or
test+probe row has both as hard predecessors (the latter transitively includes the former) and a
unique artifact filename in its artifact column. No row owns the broad `docs/plan/evidence/**`
tree. `T1-W6` is the durable-PG live-success gate; durability-dependent live probes explicitly
wait for it. `T6-W13` is the immediate canary-key lane (including current/stale key delivery and
acknowledgement); `T6-W14` is the later isolated no-wake re-enable trial.

The worker scope is a total order because these rows touch the worker monolith. The exact
Round-5 repair spine is:

`T3-W17 → T3-W18 → T4-W1 → T4-W2 → T3-W3 → T3-W1 → T3-W2 → T8-W1 → T8-W3 → T3-W14 → T3-W9 → T8-W5 → T8-W2 → T3-W16 → T3-W15`.

The eleven required Round-5 repair links are `T3-W17→T3-W18`, `T3-W18→T4-W1`,
`T4-W1→T4-W2`, `T4-W2→T3-W3`, `T3-W3→T3-W1`, `T8-W3→T3-W14`,
`T3-W14→T3-W9`, `T3-W9→T8-W5`, `T8-W5→T8-W2`, `T8-W2→T3-W16`, and
`T3-W16→T3-W15`. The complete total order above also retains the inherited
`T3-W1→T3-W2→T8-W1→T8-W3` links. `T3-W17`/`T3-W18` are the first post-freeze
containment lane; `T3-W16` (complete inventory join) precedes `T3-W15` (re-drive liveness).
`T9-W1` is a separate Sol decision-gated lane, not part of this chain.

## Canonical node table

| node | phase / wave | exact hard predecessors | exclusive path atoms; artifact filename | lane |
|---|---|---|---|---|
| T0-W1 | W0 unblock | — | `docs/plan/union-catalog-ledger.md`; — | Luna / mechanical |
| T1-W1 | W0 unblock | — | `scripts/ops/**`; — | Luna / runbook |
| T2-W1a | W0 unblock | — | `.github/workflows/build-cf-container-images.yml`; — | Luna / CI |
| T2-W2a | W0 unblock | T2-W1a | `scripts/ci/image-pin*`; — | Luna / CI |
| T9-W0 | W1 parallel | — | `deploy/cloudflare/vitest.config.ts`; `deploy/cloudflare/test/devenv-do.test.ts`; — | Luna / CI |
| T3-W17 | W0 unblock | T0-W1 | `deploy/cloudflare/src/index.ts` (intake/re-drive switch entrypoints); — | Sol / safety |
| T3-W4 | W1 serial | D1 | `crates/corelink-fabric-server/src/**`; — | Sol / architecture |
| T4-W4 | W1 serial | T3-W4, D1, R1 | `crates/corelink-fabric-server/src/**`; — | Sol / architecture |
| T3-W10 | W1 serial | T3-W4, T4-W4 | `crates/corelink-fabric-server/src/reaper.rs`; `crates/corelink-fabric-server/src/handlers/cas_cred.rs`; — | Sol / architecture |
| T6-W1 | W1 parallel | — | `scripts/*.selftest.sh`; `scripts/pre-merge-gate-check.sh`; `.github/workflows/ci.yml`; `.github/workflows/selftests.yml`; — | Luna / CI |
| T6-W2 | W1 parallel | D11 | `.github/workflows/moat-benchmark.yml`; `.github/workflows/moat-action-test.yml`; `actions/corelink-memoize/action.yml`; — | Sol / contract |
| T6-W3 | W1 parallel | — | `.github/workflows/conformance.yml`; `.github/workflows/spawn-worker-ci.yml`; `sdk/**`; — | Luna / CI |
| T6-W8 | W1 parallel | — | `.github/workflows/pg-suite.yml`; `crates/corelink-fabric/**/tests/**`; — | Sol / live-risk |
| T6-W4 | W1 parallel | — | `.github/workflows/secret-scan.yml`; `.github/workflows/corelink-stress.yml`; `deploy/cloudflare-canary/src/**`; — | Sol / live-risk |
| T5-W1 | W1 parallel | R6 | `docs/onboarding/**`; `actions/corelink-memoize/README.md`; — | Luna / documentation |
| T5-W2 | W1 parallel | D3 | `integrations/**`; — | Sol / release |
| T5-W3 | W1 serial | T5-W2 | `integrations/github-actions/action.yml`; — | Sol / security |
| T7-W1 | W1 parallel | T0-W1 | `docs/ROADMAP.md`; `CHANGELOG.md`; — | Luna / documentation |
| T7-W2 | W1 parallel | — | `docs/**` excluding `plan/`,`handoff/`,`review/`,`audits/`,`onboarding/`,`runbook/`,`ROADMAP.md`; `deploy/**/README.md` excluding canary; — | Luna / documentation |
| T7-W3 | W1 parallel | — | `scripts/ci/claim-artifact-lint.sh`; `docs/plan/evidence/schema-v1.json`; `docs/plan/evidence/manifest-v1.json` | Luna / mechanical |
| T7-W4 | W1 parallel | — | `docs/runbook/secret-inventory.md`; `crates/corelink-fabric/src/plans.rs`; `scripts/ci/secret-inventory-drift.sh`; — | Luna / documentation |
| T7-W4b | W1 serial | T7-W3 | `scripts/ci/probe-freshness-check.sh`; `docs/plan/evidence/freshness-v1.json` | Luna / mechanical |
| T2-W3 | W1 closer | T6-W1, T6-W2, T6-W3, T6-W4 | `.github/workflows/*.yml`; — | Luna / CI |
| T4-W1 | W2 worker | T3-W18, D13 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / money-path |
| T4-W2 | W2 worker | T4-W1 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / money-path |
| T3-W3 | W2 worker | T4-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / lifecycle |
| T3-W1 | W2 worker | T3-W3 | `deploy/cloudflare/src/index.ts`; `crates/corelink-cloud-engine/src/**`; — | Sol / wire |
| T3-W2 | W2 worker | T3-W1 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/src/metrics.ts`; — | Sol / safety |
| T8-W1 | W2 worker | T3-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / safety |
| T8-W3 | W2 worker | T8-W1 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / safety |
| T3-W14 | W2 worker | T8-W3 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / safety |
| T3-W9 | W2 worker | T3-W14 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / lifecycle |
| T8-W5 | W2 worker | T3-W9 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / credential lifecycle |
| T8-W2 | W2 worker | T8-W5 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / credential scope |
| T3-W16 | W2 worker | T8-W2, T2-W2b, O-CFINVENTORY | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `docs/plan/evidence/T3-W16-inventory-join.json` | Sol / inventory |
| T3-W15 | W2 worker | T3-W16 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / lifecycle |
| T8-W4 | W1 parallel | — | `deploy/runner/entrypoint.sh`; `crates/corelink-check-exec-server/src/**`; — | Sol / security |
| T8-W6 | W3 live proof | T8-W5, T1-W6, T7-W4b | `docs/plan/evidence/au4.16b-suspension-pat-revocation.json`; `docs/plan/evidence/au3.23b-completion-pat-revocation.json` | Sol / live-risk |
| T8-W7 | W3 live proof | T8-W4, T1-W6, T7-W4b | `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` | Sol / live-risk |
| T3-W5 | W4 post-decision | D4, T3-W15 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; — | Sol / decision-gated |
| T9-W1 | W2 separate lane | D2 | `deploy/dev-env/**`; `deploy/cloudflare/src/**` (quarantine gate only); — | Sol / decision-gated |
| T1-W5 | W1 serial | T3-W10, O-MINTKEY | `crates/corelink-fabric-server/src/**`; `docs/plan/evidence/T1-W5-mint-selfcheck.json` | Sol / architecture |
| T1-W6 | W1 serial | T1-W5, D12 | `crates/corelink-fabric-server/src/**`; `deploy/cloudflare/src/**`; `docs/plan/evidence/T1-W6-pg-durable-live.json` | Sol / live-risk |
| T1-W2 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T1-W2-control-plane.json` | Sol / live-risk |
| T1-W3 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T1-W3-resilience.json` | Sol / live-risk |
| T1-W4 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T1-W4-boot-rate.json` | Sol / live-risk |
| T2-W2b | W3 live proof | T2-W1a, O1, O-DEVENV-PIN, T7-W4b | `docs/plan/evidence/T2-W2b-deploy.json` | Sol / live-risk |
| T2-W4 | W3 live proof | T2-W2b, T7-W4b | `docs/plan/evidence/T2-W4-image-ship.json` | Luna / release |
| T2-W5 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T2-W5-runbook-compat.json` | Luna / runbook |
| T2-W6 | W3 live proof | O1, T2-W2b, T7-W4, T7-W4b | `docs/runbook/secret-rotation.md`; `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` | Luna / runbook |
| T3-W7 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T3-W7-moat.json` | Sol / live-risk |
| T3-W8 | W3 live proof | O1, T3-W7, T1-W6, T7-W4b | `docs/plan/evidence/T3-W8-inventory-hitrate.json` | Sol / live-risk |
| T4-W7 | W3 live proof | O-BILLING, R1, R2, T1-W6, T7-W4b | `docs/plan/evidence/T4-W7-money.json` | Sol / money-path |
| T4-W8 | W3 live proof | O-BILLING, T1-W6, T7-W4b | `docs/plan/evidence/T4-W8-billing-reconcile.json` | Sol / money-path |
| T5-W4 | W3 live proof | D3, D8, R3, T1-W6, T7-W4b | `docs/plan/evidence/T5-W4-stranger.json` | Sol / onboarding |
| T5-W5 | W3 live proof | D3, D8, R3, T5-W4, T1-W6, T7-W4b | `docs/plan/evidence/T5-W5-stranger-adversarial.json` | Sol / onboarding |
| T5-W6 | W3 live proof | D3, T5-W3, O-PUBLISH, T7-W4b | `docs/plan/evidence/T5-W6-release-artifacts.json` | Luna / release |
| T6-W5 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T6-W5-e2e.json` | Sol / live-risk |
| T6-W6 | W3 live proof | T6-W4, O-CANARY, T7-W4b | `docs/plan/evidence/T6-W6-canary.json` | Sol / live-risk |
| T6-W7 | W3 live proof | T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T6-W7-authz.json` | Sol / live-risk |
| T6-W9 | W3 live proof | T6-W6, T7-W4b | `docs/plan/evidence/T6-W9-alert-rules.json` | Sol / alerting |
| T6-W10 | W3 live proof | T6-W6, T6-W9, T1-W6, T7-W4b | `docs/plan/evidence/T6-W10-alerting-depth.json`; `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` | Sol / alerting |
| T6-W11 | W3 live proof | T1-W6, T7-W4b | `docs/plan/evidence/T6-W11-runbook-execution.json` | Luna / runbook |
| T7-W5 | W3 live proof | O1, O-CFRATE, T3-W7, T1-W6, T7-W4b | `docs/product/pricing.md`; `docs/plan/evidence/au7.11-queued-running-latency.json`; `docs/plan/evidence/au4.19-cloudflare-container-rate.json`; `docs/plan/evidence/au7.12-memoization-hit-rate.json` | Luna / economics |
| T6-W12 | W3 live proof | T6-W10, T3-W16, O-CFINVENTORY, T1-W6, T7-W4b | `docs/plan/evidence/T6-W12-independent-monitor.json` | Sol / alerting |
| T3-W18 | W0 containment (post-freeze) | T3-W17, T7-W4b | `docs/plan/evidence/T3-W18-containment-live.json` | Sol / live-risk |
| T6-W13 | W3 live proof | T6-W4, T6-W6, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/**`; `docs/plan/evidence/T6-W13-canary-key-lane.json` | Sol / alerting |
| T6-W14 | W3 live proof | T6-W13, T6-W12, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/**`; `docs/plan/evidence/T6-W14-canary-no-wake.json` | Sol / live-risk |

## Deterministic ready sets and proof

For review purposes, the dispatcher runs Kahn's algorithm over the table, removes satisfied
decision/obstacle/relay tokens, sorts each ready set lexicographically by node id, and emits at
most eight nodes per batch. The following batches are the deterministic result with every external
token assumed satisfied; they are a schedule calculation, not authorization:

```text
B00: T0-W1 T1-W1 T2-W1a T3-W4 T5-W1 T5-W2 T6-W1 T6-W2
B01: T2-W2a T3-W17 T4-W4 T5-W3 T6-W3 T6-W4 T6-W8 T7-W1
B02: T2-W3 T3-W10 T7-W2 T7-W3 T7-W4 T8-W4 T9-W0 T9-W1
B03: T1-W5 T7-W4b
B04: T1-W6 T2-W2b T3-W18 T5-W6 T6-W6
B05: T1-W2 T1-W3 T1-W4 T2-W4 T2-W5 T2-W6 T3-W7 T4-W1
B06: T3-W8 T4-W2 T4-W7 T4-W8 T5-W4 T6-W11 T6-W13 T6-W5
B07: T3-W3 T5-W5 T6-W7 T6-W9 T7-W5 T8-W7
B08: T3-W1 T6-W10
B09: T3-W2
B10: T8-W1
B11: T8-W3
B12: T3-W14
B13: T3-W9
B14: T8-W5
B15: T8-W2 T8-W6
B16: T3-W16
B17: T3-W15 T6-W12
B18: T3-W5 T6-W14
```

The checker validates that every predecessor token is in the registry, every WP appears exactly
once in the ready-set output, no batch exceeds eight, and the final emitted count equals the table
vertex count. Kahn's algorithm consumed all vertices (no residual indegree), proving this version
acyclic. A future change must regenerate the batches and update `schema: dispatch-dag/v1`; hand-edited
edges or repeated DAG text in another document are invalid.

The table and batches remain **NOT DISPATCHABLE** while the quiet count is below two, any D/O/R
token is unresolved, `FABRIC_PG_DISABLED=1` is armed, or the post-incident baseline is absent.
