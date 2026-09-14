# Session state — 2026-09-05

Snapshot checked at approximately **22:40 UTC / 19:40 America/Sao_Paulo**.
This document records facts, not a new implementation plan or sprint certificate.
Read [the session handoff](2026-09-05-session-handoff.md) for resumption instructions.

## Explicit acknowledgment of orchestration failure

I completely lost control of the situation and was not able to orchestrate
this work. The owner is ending this session for that reason. This was my
failure as the orchestrator, not a failure by the owner to provide direction.
The underlying cause has not been established; do not invent an explanation
or attribute it to tooling, billing or technical blockers without evidence.
The observable failures include running full CI on incomplete sprint scope,
failing to turn the four-sprint plan into a delivered sprint, and reporting
preparation and test activity without corresponding delivery. The next session
must independently reconcile actual work and evidence, rather than trust this
session's previous progress assessments.

## 1. Actual delivery and the owner's latest correction

- The current four-sprint effort has delivered **0 of 4 complete sprints**.
- The last recorded source-delivery census is **16/70 WP vertices delivered,
  54 unfinished**. No additional WP/main merge was completed in the latest work.
- The combined registry contains48 principal +9 staged-principal +13 AU WPs.
  These70 WPs are not70 audit findings: the underlying audit has247 findings,
  and the principal acceptance catalog has94 rows,92 live rows. Do not conflate
  findings, acceptance criteria, WPs, PR layers and delivered sprints.
- The earlier containment/hard-stop train, PRs#538–#547, had already merged.
  Remote `main` was freshly verified as
  `cda90940f74735c006693d9b9e3be85c37b26f1a`.
- **Full CI is allowed only on a COMPLETE SPRINT**, not on a WP, partial cohort
  or arbitrary bundle. The entire agreed implementation scope must be composed
  into that sprint's stacked-PR tip before full CI starts. Necessary targeted
  development checks remain distinct from full CI. Do not rename a subset a
  sprint or silently shrink its scope to justify a run.
- The full matrices run against partial Sprint1/#558 were **premature and
  contrary to that instruction**. Their logs are real historical evidence for
  those source snapshots, not evidence that a sprint was completed.
- No full CI, deployment or implementation work is being launched for this
  handoff. The dedicated proof VM was freshly verified **Stopped**.

## 2. Checkout safety and current sources

Repository: `HuGR-Labs/corelink-runners`.
Main workspace: `/Users/gustavoschneiter/Documents/HuGR/corelink-runners`.

**The main workspace is NOT the current development baseline.** Its local HEAD
is `19eb2e1ddb037ce3e8cbbc1940ae21cd83bd6252`, and its index has these conflicts:

```text
UU deploy/cloudflare/src/index.ts
UU deploy/cloudflare/src/metrics.ts
DU deploy/cloudflare/test/containment-intake.test.ts
?? .atlas/
?? .vite/
?? node_modules/
?? plan-integrity.yml
```

Do not reset, abort, clean, switch or resolve this checkout without understanding
ownership. Its `CLAUDE.md` contains older July production assertions; the current
dated planning files were not present there during this snapshot. The two new
session documents are intentional untracked additions, not conflict resolution.

Current clean, isolated sources:

| Purpose | Worktree / local branch | Remote branch |
| --- | --- | --- |
| Partial Sprint1, PR#558 | `/private/tmp/corelink-sprint1-cleanup-next` / `prep/sprint1-cleanup-signed` | `wp/sprint1-cleanup-preparation` |
| Later Action preparation | `/private/tmp/corelink-sprint4-action-bundle.O2xzb3` / `prep/sprint4-action-bundle` | `wp/sprint4-action-preparation` |

PR#558 source, freshly verified locally and remotely:

```text
commit bb165bad54125460ebae213e405df3a75baa6566
tree   37e28a39c368ae891361bbcde334aedc30a96c3b
```

Later Action preparation, freshly verified locally and remotely:

```text
commit 8505fda9245ac02665ddac264c7e5f3414022de2
tree   1f73c7231aa008bc3cb8d4387e77486d0085cba5
```

The latter contains signed-off composition `d362742d7d5aa7e5071269ca18beb319e5633018`
plus its handoff document. It is not a completed Sprint4, has no new PR, and has
not received a full Sprint4 CI run. Both remote branches preserve the authored
history; temporary worktrees are not the only copies of the code.

## 3. Planning sources and inherited sprint grouping

Read these from a current clean worktree, not the old conflicted checkout:

- `CLAUDE.md` and any applicable `AGENTS.md`.
- `docs/plan/2026-08-30-golive-remediation-plan.md`.
- `docs/plan/2026-09-01-reconciled-dispatch-dag.md` — canonical WP/dependency registry.
- `docs/plan/union-triage-remaining.md` — AU/finding placement.
- `docs/plan/audit-2026-08-30-finding-ids.txt`.
- Local memory: `/Users/gustavoschneiter/.codex/plans/corelink-deepseek-wp-acceleration.md`.

The local memory contains the latest owner correction under
`Four-sprint stacked delivery policy`. It also contains many superseded progress
entries. Older references to a certified partial bundle do not override the
latest rule. Older global quiet-review/freeze prose must not be reinstated as
an indefinite process gate; actual product dependencies still matter.

The inherited grouping below covers the54 remaining WP vertices exactly once.
It is a recorded grouping, **not a claim that its execution plan is adequate or
that any sprint has been completed**. The owner explicitly challenged that gap.

| Sprint | Count | WP scope |
| --- | --- | --- |
| 1 | 12 | T3-W18, T8-W4b, T6-W4, T6-W9, T6-W13, T6-W15, T4-W4, T3-W10, T1-W5, T6-W2, T2-W3, T9-W1 |
| 2 | 14 | T2-W2b, T2-W4, T2-W6, T4-W1, T4-W2, T3-W3, T3-W1, T3-W2, T8-W1, T8-W3, T3-W14, T3-W9, T8-W5, T8-W2 |
| 3 | 15 | T3-W16, T3-W15, T6-W12, T1-W6, T6-W6, T1-W2, T1-W3, T2-W5, T3-W7, T3-W8, T4-W7, T4-W8, T6-W5, T6-W7, T8-W6 |
| 4 | 13 | T6-W14, T6-W10, T1-W4, T3-W5, T5-W1, T5-W2, T5-W3, T5-W4, T5-W5, T5-W6, T6-W11, T7-W5, T8-W7 |

## 4. Open PR chain

Fresh GitHub read found11 open PRs. Each row after#548 is based on the previous
row's branch. These are existing preparation layers, not11 sprints.

| PR | Head branch | Scope | Draft |
| --- | --- | --- | --- |
| #548 | `wp/t3-w18-config` | Re-drive pause configuration; base `main` | No |
| #549 | `wp/t3-w18-live-evidence` | Deployed smoke and explicitly incomplete live matrix | No |
| #550 | `wp/t8-w4b-rust` | Protected auth-file server | Yes |
| #551 | `wp/t8-w4b-boot` | Boot/auth environment cleanup | Yes |
| #552 | `wp/t8-w4b-worker` | Worker auth-file consumers | Yes |
| #553 | `wp/t6-w4-tick` | Default-off durable tick preparation | Yes |
| #554 | `wp/t6-w9-alerts` | Alert/delivery subset | Yes |
| #555 | `wp/t4-w4-admission-proof` | Admission boundary tests | Yes |
| #556 | `wp/t3-w10-error-body` | Credential error-body normalization | Yes |
| #557 | `wp/t6-w2-policy` | Memoize policy/refusal subset | Yes |
| #558 | `wp/sprint1-cleanup-preparation` | Pending cleanup, Linux fix, fixtures | Yes |

PR#558: https://github.com/HuGR-Labs/corelink-runners/pull/558.
Its existing wording about a passing local bundle must be read with the owner's
subsequent correction: **it is not a completed or certified sprint**.

## 5. Implemented preparation worth preserving

### Partial Sprint1

- Pending cleanup claims across memory, File and PostgreSQL: claim before
  provider I/O, fence Held/ordinary removal, retain cap/compute reservation until
  confirmed teardown and conditional finish. File claims fsync before publication.
- Provider handles/Hybrid routes survive successful teardown followed by ledger
  failure. Unknown process-local identity is not proof of provider absence.
- Pending acquisition and dropped-waiter rollback share the claim protocol.
  Capacity errors cannot requeue an id before cleanup finishes; otherwise503,
  PAT revocation and retained claim. Held lifecycle behavior remains separate.
- Linux GNU/BSD `stat` fallback corrected without relaxing JIT ownership/mode.
- Auth-file Rust/boot/Worker work, default-off canary outbox, scoped alerts,
  admission tests, credential error bodies and memoize policy preparation.
- Test fixtures use authenticated ACKs through existing interfaces; production
  ACK deadlines were not shortened. These improvements do not close whole WPs.

Engineering contract: `/private/tmp/corelink-t3w10-cleanup-contract.md`.
It includes the explicit BeforeProvision/AfterProvision distinction. A generic
provider500 after provisioning began retains the reservation without an actual
absence/teardown confirmation. Full restart reclamation remains unproved because
provider identity is still process-local.

### Later T5-W3 / AU5.11 and AU5.12 preparation

- Environment-only input passage, quoted argv, no input expressions in Bash or
  github-script, control-file env stripping from the CLI, bounded always-cleanup.
- Python parser replaced by the existing pinned github-script action, typed JSON
  and i32/boolean validation, generic errors, preserved public outputs.
- Independent review approved author commit
  `840ff9cd40bfff2cfd13ca011663d17e8cb5b1f7`, tree
  `32fb19756a7dba27f44de3e654bf87ba863be62e`.
- Eleven focused validator groups include14 malicious-input cells, cleanup
  sentinels, malformed responses and combined raw CLI exit1/check exit137.
- A complete composite execution also passed with actual pinned github-script
  in local act/Node24/Bash without Python. It is not a hosted GitHub job, live
  release, customer onboarding or a full Sprint4 CI certificate.

Author worktrees: `/private/tmp/corelink-t5-w3-au511` and
`/private/tmp/corelink-t5-w3-au512`. Parser contract:
`/private/tmp/corelink-t5-au512-parser-contract.md`.
The next-preparation branch contains
`docs/handoff/2026-09-05-action-sprint-preparation.md`.

## 6. Evidence — historical, not permission to rerun partial CI

At PR#558 source `bb165bad`,37 recorded local checks passed: Rust1,314;
Worker694; canary103; real PostgreSQL contract7/unit138/cleanup4; four Linux
jobs without skips; fmt/clippy/deny/audit, shell and repository checks.
Worker coverage was85.06/83.21/86.70/85.06 percent. Production npm audits had
zero advisories; development dependency advisories were not claimed resolved.
Workspace DB-gated early returns are not counted as actual PostgreSQL proof.

That execution was real but **dispatched too early for the owner's workflow**.
Do not rerun it on the same partial scope or claim that it certifies Sprint1.

- Host directory: `/private/tmp/corelink-sprint1-bundle-proof.kuK8DM/`.
- Manifest: `local-bundle-manifest.json`.
- Manifest SHA256: `49c547c055e7e873795fff8f1abae4ac121d667b5f779b272169e1473990eaf3`.
- Full manifest is also preserved in PR comment:
  https://github.com/HuGR-Labs/corelink-runners/pull/558#issuecomment-5555151786.
- Older failed candidates/logs remain in
  `/private/tmp/corelink-sprint1-ci.yHVO1a/`,
  `/private/tmp/corelink-sprint1-cleanup-ci.GRfK0o/` and
  `/private/tmp/corelink-sprint1-cleanup-final-ci.QkvyZm/`.
  They are not passing certificates for the latest source.

Auditable Action runtime evidence:

- Host directory: `/private/tmp/corelink-act-proof.x0IfKC/`.
- `final-proof.log` SHA256:
  `501522aa8f3299c03629c658d0cf19cefab7afa8843a5a019dd581773e6eb10b`.
- `executed-proof.yml` SHA256:
  `54bf818387c34cba978cb29f9b594b147832d7bc37fb50d64b85bd2544f2badc`.
- `executed-action.yml` SHA256:
  `2ce6bcfc6d330049a572deb7a6ea926b8048c2dcb60374c099f8f81a75d93026`.
- The directory also holds source checksums, image inspection, Dockerfile and
  provenance. Root independently checked the host files before VM shutdown.
- An earlier observed runtime success lost its guest `/tmp` log on shutdown.
  The old `ad728a...` hash is not auditable evidence. Only the persisted rerun
  above is used. Never reconstruct missing raw logs from a summary.

Host `/private/tmp` is also temporary storage: source commits and selected
evidence summaries are pushed, but do not assume all raw logs survive OS cleanup.

## 7. Operational state and unresolved dependencies

No new provider read was performed while writing this snapshot. Last recorded
read-only provider observation was **2026-09-05T21:41:58Z**:

- Worker `corelink-spawn-worker`, version
  `95b39a6a-cf7e-4c5b-95fd-53dc290498f9`, deployment
  `f091a0a1-9ba1-407c-9f61-b2425c2eafae`,100% traffic.
- Runner426 and fabricd3 persistent records were all inactive; check-host0.
  This is point-in-time containment, not invoice or continuous-cost proof.
- Deployed source was `e89f9a7c289bb8069ac266d4c94b8649f291a73c`.
  Its recorded config had `AUTOSCALER_REDRIVE_PAUSED="1"`; intake pause absent.
  The later CLI read did not freshly expose those binding flags.
- CONTAINMENT/v7 migration must be retained. Recovery is forward-only, not a
  rollback across that Durable Object migration. Deploys replace plaintext vars:
  compare candidate versus live bindings before any future deployment.
- Production durability remains intentionally degraded under
  `FABRIC_PG_DISABLED=1`; no durable-PG/go-live recovery was delivered here.

Last known unresolved dependencies, to verify concretely rather than recite:

- T3-W18/A3.30: genuine three-state10/10 matrix, nonempty ordered resume and
  unchanged-running-box/side-effect evidence. Empty-drain smoke is insufficient.
- T4-W4/R1: capacity/relay prerequisite; constrains downstream T3-W10 integration.
- T6-W2: actual authenticated atomic no-exec HIT/restore protocol missing;
  required-hit refusal78 is not a successful cache HIT.
- O-MONITORHOST: external non-Cloudflare monitoring/control/journal capabilities
  not established. Do not invent owner accounts, credentials or authority.
- T5-W2/D7/D3 constrain the later Action preparation's integration. D13 owner
  precedence and other owner/provider decisions also remain in the registry.

These were not all resolved in this session. They do not justify endlessly
running CI on partial work, fabricating evidence, or claiming every other
independent implementation task is blocked.

## 8. Resources and secrets

- Dedicated Lima VM `corelink-ci-proof`: **Stopped**, freshly verified.
- Its guest `/tmp` is ephemeral. Docker storage/images survive, guest temporary
  source and Cargo caches must not be assumed to survive shutdown.
- Disposable proof PostgreSQL is container `corelink-pg16`; the unrelated host
  PostgreSQL listens on5432 and must not be touched. The required PG proof ran
  inside the disposable container network namespace with exact loopback5432.
- Last host disk check: approximately39GiB available. No disk deletion performed
  for this handoff. Preserve unrelated/sibling sessions and their processes.
- No known session-owned agents or full test processes remain active.
- Never copy the user-pasted API key, operational admin values, PATs, private
  keys or complete secret files into handoff documents, commits or logs.
  Protected local credential recovery exists; use existing authorized tools
  when needed, not secret dumps. No secret value is included here.
