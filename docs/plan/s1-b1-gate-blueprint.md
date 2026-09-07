# Sprint 1 / B1 closing-gate blueprint

Status: prepared; no gate, CI job, deployment, or remote action was run.
Baseline inspected: `bb165bad54125460ebae213e405df3a75baa6566`.
Sources: `docs/plan/2026-09-01-reconciled-dispatch-dag.md`,
`docs/plan/2026-08-30-golive-remediation-plan.md`, the current workflow files,
and the Sprint-1 ledger in the acceleration plan.

## Gate invariant

B1 is promotable only from one immutable, clean, composed Sprint-1 tip. The
bucket contains twelve labels (`T3-W18`, `T8-W4b`, `T6-W4`, `T6-W9`,
`T6-W13`, `T6-W15`, `T4-W4`, `T3-W10`, `T1-W5`, `T6-W2`, `T2-W3`, `T9-W1`),
but the canonical DAG places some of them in later waves and requires external
tokens (`R1`, `D1`, `D2`, `O-CANARY`, `O-MONITORHOST`, `T7-W4b`, etc.). A label
count or a green partial stack is never a B1 PASS. Any unmet hard edge or
owner/provider artifact is `BLOCKED`, with the exact edge recorded.

The gate packet must contain: final tip SHA and tree SHA; WP-to-commit and
write-scope manifest; DAG predecessor/owner checks; exact command, exit code,
duration and runner for every check; raw logs retained outside ephemeral
workers; and a single verdict `PROMOTE`, `FIX`, or `BLOCKED`.

## Order for fastest useful signal

Execute these against the frozen tip, in parallel where scopes remain disjoint. Stop
before heavy CI if a cheap structural check establishes an incomplete tip.

1. **Freeze and structural scan (minutes).** `git status --porcelain=v1` must
   be empty; verify the expected HEAD/tree; `git diff --check`; inspect the DAG
   predecessor closure, owner artifacts, and exact final write scopes. Check
   `git ls-files` discovery for every tracked `*.selftest.sh` and workflow path.
2. **Cheap gates (seconds to a few minutes).** Execute
   `cargo fmt --all --check`, `node --test scripts/e2e/completeness.test.mjs`,
   all discovered selftests (`git ls-files -z -- ':(glob)scripts/**/*.selftest.sh'`
   piped through a `bash` loop), the plan checks
   (`python3 docs/plan/actionlint-check.py`, `plan-check.py`, `wp-check.py`,
   `au-check.py`, and `gates-selftest.py` with the workflow-declared arguments),
   and DCO/workflow linters required by the final tip. Execute each once;
   these checks do not substitute for the later suites.
3. **Focused scope signals (parallel).**
   - Rust fabric-server: the named T4-W4/T3-W10/T1-W5 integration tests and
     their package tests (`cargo test -p corelink-fabric-server --locked
     --test <target>`), followed by `cargo clippy -p corelink-fabric-server
     --all-targets --locked -- -D warnings`.
   - Auth bridge/T8-W4b: the check-host shell test and the named Cloudflare
     Vitest files for auth-file, check-host, DevEnv and process-surface
     behavior; include Rust check-exec-server package tests.
   - Canary/alerts/T6-W4/T6-W9/T6-W13: `npm ci` once in
     `deploy/cloudflare-canary`, then `npx vitest execute` with the scheduled-tick,
     outbox, flag, rule and key-lane files. Do not rerun overlapping files in
     separate WP jobs.
   - Worker/DevEnv/T9-W1: `npm ci`, `npm run typecheck`, then one focused
     `npx vitest execute <files>` in `deploy/cloudflare`; apply coverage only in the
     required worker gate, not in every focused invocation.
   - T6-W2/moat: execute the two workflow-level planted-miss checks and
     `bash actions/corelink-memoize/test/policy.sh` once each.
4. **Complete B1 matrix, only after all twelve scopes and DAG edges are
   present.** Dispatch the required workflow jobs once from the exact tip:
   `CI` (`cargo fmt`, workspace clippy, workspace tests, deny, audit),
   PostgreSQL suite (isolated PostgreSQL 16 contract plus serial DB units),
   spawn-worker CI (typecheck + `npm run test:coverage`), Conformance/SDKs,
   Script selftests, moat benchmark/action test, and the T6-W4 stress/secret
   scan lanes. Execute applicable image/build checks only when their changed paths
   require them. GitHub `startup_failure`/zero jobs yields `BLOCKED`, not evidence.

## Required resources and expected wall time

The complete matrix needs parallel `corelink` Linux/KVM runners with Rust
1.96, Node 22/24, pinned `cargo-deny` 0.19.8 and `cargo-audit` 0.22.2; one
isolated PostgreSQL 16 service; and Docker/container capability for stress or
moat jobs. The workflow timeouts total 10–30 minutes per job, but cold Rust
compilation and runner acquisition dominate. With warm caches and enough
parallel runners, plan for roughly 2–5 hours wall time; cold or capacity-starved
execution can consume 6–10 hours. This remains an estimate, not a passed SLA.

## Deduplication rules

`cargo test --workspace --locked` remains the Rust correctness source of truth;
package tests must not be repeated as separate WP-wide suites after completion.
The PostgreSQL workflow owns DB-backed contract/unit coverage and is the only
DB proof; do not credit early-return tests or point them at a host database.
The worker workflow owns the single coverage run; focused Vitest invocations remain
development signals only. Canary Vitest files, script selftests, moat policy,
SDK/conformance, and Linux process tests each have one owner. A check already
executed on the identical final SHA reuses manifest hash, not a rerun.

## Failure triage: N failures by cause and write scope

Every failure becomes a row containing job/check, exact SHA, exit code, first
stable error, environment/runner, and touched write scope. Normalize stack
traces, assertion names, missing-resource errors, and timeout signatures.

- Group failures with the same normalized signature and overlapping write scope
  into one root-cause incident, even when N jobs fail. Assign one repair owner
  for that scope; parallelize only groups with disjoint scopes.
- If one scope has multiple competing fixes, serialize them by the canonical
  DAG and keep the smallest reproducible focused command as the loop.
- After a fix, execute its focused check, then the affected workflow job. Reopen
  the complete matrix only on the new composed final tip; never mask N failures
  with N speculative reruns.
- A true infrastructure failure (runner loss, service unavailable, zero jobs)
  is recorded as `UNKNOWN`/`BLOCKED`, with its raw evidence preserved; retry at
  most once under the same frozen SHA. A reproduced product failure is `FIX`.
- `PROMOTE` requires zero unresolved failures, zero skipped required checks,
  exact SHA/tree binding, and all hard predecessors/owner artifacts satisfied.
