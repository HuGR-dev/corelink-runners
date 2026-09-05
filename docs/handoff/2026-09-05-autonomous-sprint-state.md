# Autonomous sprint state — 2026-09-05

## Delivered and deployed

The preceding #538–#547 containment repair/hard-stop train merged to
`main@cda90940f74735c006693d9b9e3be85c37b26f1a`, tree-equal to its reviewed
and locally certified tip. Source census: **16/70 delivered, 54 remaining**;
source delivery is not operational/go-live acceptance.

Sprint 1 starts with #548 (`e89f9a7c`, re-drive pause), followed by #549's
version-bound evidence. Both remain open. Exact source e89f9a7c deployed at
19:12:20Z as Worker `95b39a6a-cf7e-4c5b-95fd-53dc290498f9`, 100% traffic.
The four bounded smoke probes passed. At 19:37:18Z, complete read-only provider
pagination found zero container instances in the exact runner, check-host and
fabricd applications, with no prior plaintext-var drift. Persistent DO records
remain intact; they are not running containers. See
[the evidence](../plan/evidence/T3-W18-containment-live.json).

A3.30/T3-W18 remains **RED**: authentic three-state 10/10, nonempty ordered
resume, measured side effects and unchanged-running-box evidence are not done.
No queued/in-progress runs existed in this repo during the access audit;
historical completed jobs are not pre-existing live redrive candidates.

## Active source preparation

These isolated branches earn no completion, deployment or merge credit yet:

- `prep/t8-auth-file`: Rust file-only auth, FIFO refusal and tests;
  `a08978b`, 12 focused tests plus doc-tests passed.
- `prep/t8-auth-boot@3723178`: independently approved 0400 bridge, clean
  re-exec, marker-injection refusal and shutdown cleanup. The actual defect was
  raw auth reaching outer dumb-init before the bridge; ENTRYPOINT now bridges
  first. The earlier root/coder ownership hypothesis was refuted by the real
  Dockerfile and did not require a privilege change. Composed boot tests pass.
- `prep/t8-auth-worker`: fixed Worker-owned auth file path and provider ingress;
  `6c164b9`, 49 focused tests and typecheck passed. These three lanes jointly
  implement T8-W4b, not three completed WPs.
- `prep/t6-tick@f53cf82`: independently approved default-off producer;
  46/46 focused tests and typecheck pass. Real Ed25519 fixtures, byte-identical
  concurrent reservation, stale/expired terminal CAS, hung stream/cancel/signer
  and actual Miniflare DO dispatch are proved. Deadline-observed paths persist
  TIMED_OUT directly; alarm remains crash recovery. No shared producer HMAC
  replaces role-separated ACK trust; production capabilities remain unbound.
- `prep/t6-alert-rules@b1f591b`: independently approved scoped T6-W9 subset;
  44 focused tests and typecheck pass. No complete monitor-rule matrix,
  external page delivery or acknowledgement proof is claimed.
- `prep/t4w4-boundaries@2fdcef8`: admission tests only; source behavior stays
  unchanged and R1 is unresolved. Shared Rust run passed 9 admission tests,
  5 existing infrastructure-capacity tests and 7 credential-route tests.
- `prep/t3w10-error-body@7ca6fb9`: canonical safe ErrorBody with original
  transport statuses; no widened production visibility. Four credential unit
  tests also pass after the entire library-test target compiles. The other
  T3-W10 half (durable
  teardown tombstone/cap retention) is undergoing a bounded design review.
- T6-W2 policy/workflow partial `9df0eaef6cbca789bebea37ab160094afd8add4c`:
  five focused fixtures pass. Optional behavior stays unchanged; required-hit
  refuses with78 and never runs a child. The actual authenticated atomic
  no-exec HIT/restore clw API is missing; this is not a completed required-HIT
  implementation. Do not substitute inferred HIT JSON or output text.

The published draft source stack follows #548/#549 with #550 Rust auth,
#551 boot bridge, #552 Worker binding, #553 tick producer, #554 alert subset,
#555 admission proof, #556 credential errors and #557 memoize policy. These
are reviewable layers, not separate sprints or completed WPs. The available
source snapshot is being prepared for the single complete bundle matrix;
it is not certified yet, and no operational/DAG acceptance is waived.
Root integration is `/private/tmp/corelink-sprint1-auth-stack`, current branch
`prep/sprint1-tail-signed`; old unsigned/obsolete simulation branches are not
publication sources. All source commits are retained through DCO merges.

Focused evidence (local logs; not full CI certificates):

| Scope | Result | Log SHA-256 |
|---|---|---|
| T6 tick + probe flags + actual DO runtime | 46/46 | `0cc387e294383921f47ba1d7c49153447850004771e3b7e66326254fd61ad8b2` |
| Shared Rust admission/capacity/credential routes | 21/21 | `667047e28c6e689ca432ce520b090b8dc90fd5a27c5df6a26af9c46061c50844` |
| Existing private credential unit cases | 4/4 | `19ede82e31fafc6c856983fc5aca8ceab101b4699c804c7de034aef120d2341d` |
| Memoize policy | 5/5 | `f97fb90cf11178cb7a46035f81ff5e08f9fd863a55f52b032a04d26aaa6cabc8` |

## Execution rules

The owner authorized autonomous deployment and maximum safe parallelism,
Luna-first (Terra for bounded harder work). Offline preparation can proceed
while operational gates are pending; actual integration, deployment and
acceptance dependencies remain enforced. Do not resurrect quiet-review loops
as a global authoring freeze. Do not fabricate provider contracts/credentials,
commercial decisions, authentic webhook payloads or live evidence.

Four scheduling trains cover the 54 remaining vertices: 12/14/15/13. They are
stacked merge trains, not four independent branches. Full CI runs only on
final stacked-sprint tips, locally or on CoreLink; focused checks run during
authoring. Zero Actions jobs never substitute for CI. Preserve commits and
merge top-down with exact-head/base and final-tree verification. Every commit,
including orchestration merge commits, needs DCO.

The size rule is **600 LOC per code file**, ~400 for new files. Documents,
whole WPs, PRs and diffs are not capped. Keep old godfiles from material growth.
The primary checkout is dirty/conflicted and is not an integration workspace.

## Recovery and local operations

Production containment recovery is forward-only with ContainmentDO/v7 retained.
Both pause switches exact `"1"` are the emergency posture. A direct rollback
across the new DO migration is unsupported. Disarming intake permits automatic
drain on scheduled/fresh traffic; the drain-request flag is not an interlock.
Durable PG remains intentionally disabled; do not infer re-arm from these probes.

Operational snapshots and sanitized logs are under
`/private/tmp/corelink-deploy-ops.V1KGSC/`. The new containment-admin recovery
copy is outside git in the owner's `.config/corelink-runners` directory with
0700 directory/0600 file modes. Never print or commit credentials.

Only a verified 710MiB regenerable Cargo registry cache was removed; its source
checkout was preserved. Four orphan test processes from a completed Runners
review were stopped; other projects' builds were untouched. Disk later rose
above30GiB, not solely from our cleanup. Rust focused builds use one shared
`/private/tmp/corelink-t8-focused-target`, debug0/incremental0; no concurrent full
workspace matrices. Keep the 12GiB stop guard.

The continuously maintained local execution anchor is
`/Users/gustavoschneiter/.codex/plans/corelink-deepseek-wp-acceleration.md`.
