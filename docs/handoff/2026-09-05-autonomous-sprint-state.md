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

### Next prepared cohort and Linux validation

The earlier `c494639` bundle passed its Mac-executable matrix (Rust1296,
Worker694, canary101), but is **RED on real Linux G0**: GNU `stat -f`
emitted filesystem output before failing, contaminating the BSD-first fallback
and rejecting a valid sealed JIT file. The failure is retained; the Mac result
does not override it. Correction `8eef9fc9` uses isolated GNU-first assignments
and preserves strict ownership/mode checks. Real Linux tests on source
`82d049c3` pass every G0 cell, including literal PID1 and its old-shape negative
control, dev-shm and JIT `/proc` surface checks. Runner blobs are unchanged in
the later cleanup source `010906e9`; git-diff equality was checked.

The next isolated source preparation is
`/private/tmp/corelink-sprint1-cleanup-next`, branch `prep/sprint1-cleanup-signed`.
It adds atomic stale-Pending cleanup claims across memory, File and PostgreSQL,
confirmed provider teardown, and conditional final deletion. Claims retain
concurrency and compute reservation, fence Held/normal removal, and survive
File replay. File fsync precedes in-memory publication. Provider handles and
Hybrid routes survive a successful teardown followed by a failed ledger finish;
they are forgotten only after the conditional finish succeeds. Missing cloud
registry state is Unconfirmed, never inferred provider absence. Complete cloud
restart reclamation remains unproved because provider identities are still
process-local. This is partial T3-W10 preparation, not operational acceptance.

Central focused checks passed: 3 memory/File integration tests, 1 journal
append-failure unit test, 37 existing reaper regressions, 6 cleanup/provider
tests, and **3 real PostgreSQL16.15 tests** including claim/Held race and
reservation retention. The first PG run exposed a fixture that bound both
compute and concurrency but assumed concurrency precedence; it now tests each
limit independently without changing production precedence. An unavailable
short-lived SSH tunnel attempt also failed explicitly; the confirmed run uses
Lima's loopback-only port55432, not the live database.

Four test harnesses now provide authenticated close ACKs through existing
public fixture seams: 39/39 tests pass in0.35s of execution versus approximately
120s previously. No production timeout, public test-accessor API, assertions or
explicit timeout tests were removed. The offline T6-W13 key-lane test is also
independently reviewed; its simulated12 cycles do not prove a live hour/page ACK.

| Focused evidence | SHA-256 |
|---|---|
| Real PG16 cleanup3/3 | `f33d157792c3d043ae731638479c633d458d30f6d8b8305db4ea808ec5965576` |
| Existing reaper37/37 | `666c848383b47117b5a4c040bfa49ca81c1aeaffab6fc02e51eb8c47505ac072` |
| Cleanup/provider6/6 | `a3bd5d38a353c7bba1305a83ebfde63aa9b19ef1d222d802a0250782e1138c01` |
| File append-failure1/1 | `a6f65629258fc06b4679ddb8da20849cd018e2a7afe2f94b6f75089889db2a5e` |
| ACK fixtures39/39 | `43163e5416d404991d9e8ac212a5e3b8e4ff102fcb7a1dc84001986f01497aa0` |
| Actual Linux G0/JIT | `81a5fb9972c14cfe5639a7c88e9938ad631723bee7756a0bb63c2ce823852bc8` |

Full CI for this new source tip is still required. No new train merge or WP
completion is claimed. The hard integration predecessors remain unchanged:
offline preparation/default-off does not itself remove a DAG edge. T3-W18 live
acceptance and T4-W4/R1 still constrain the prepared descendants.

The subsequent integration review found and corrected two concrete rollback
interleavings before full CI. Acquisition rollback and a dropped queued waiter
now use a named Pending claim, not normal teardown followed by removal. The
four pre-provision call sites can finish because provisioning never began;
post-provision paths require explicit confirmation. Normal Held teardown is
unchanged. A capacity error returns the requeue outcome only after cleanup
finishes; otherwise it revokes the PAT, returns503 and leaves the claim for
the stale sweep, rather than inserting the same retained id again.

Five composed server suites pass42/42 (capacity5, lock split1, cloud21,
cleanup8, slots7), and the real DB-backed named-claim run passes7/7 across
memory/File/Pg (4 Pg cases). The additional private router/queue regression
passes: cleanupRetryable retains the cap, a second tick performs no second
provision, the PAT is revoked, and a later confirmed sweep finishes exactly
once. Private queue access stays inside a unit-test child module; no production
visibility was widened. Linux auth bridge also passes, with3 durable process
observations finding no raw token in environment or argv.

The preparation is published as DRAFT #558, base #557. Its next exact tip gets
the complete shared matrix; no per-author full suite or zero-job merge credit.
Final CI logs are allocated at `/private/tmp/corelink-sprint1-cleanup-ci.GRfK0o`.

### Standing rules

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
