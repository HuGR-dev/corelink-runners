# Session state — Wave 0 implementation, 2026-09-02

**Scope:** resumable implementation state after the first go-live remediation
bundle. Read the older 2026-09-01 handoff for incident history and normative
planning history; this file supersedes its implementation-status notes only.

## Landed milestone

- PR #533 merged to `main` as
  `358d1a2faf77d4d63f3273d00fc027e412bcf301`. Its reviewed source head was
  `79da48534529cf2b969aeb6a88e1bfcd99a06ae3`.
- Completed implementation packets: the previously landed `T0-W1`, plus
  `T1-W1`, `T2-W1a`, `T6-W1`, `T6-W3`, `T6-W8`, `T7-W3`, `T7-W4`, and the
  split JIT packet `T8-W4a` — nine WPs total.
- `T8-W4` was corrected structurally: `T8-W4a` owns only runner JIT sealing;
  `T8-W4b` remains RED in the backlog and owns the check-exec/check-host/DevEnv
  auth-secret bridge. No AU1.9 completion credit was claimed.
- Review-driven repairs also fail closed on unresolved image digests, unknown
  container classes, skipped or lookalike mandatory checks, multiline/unbound
  claims, and tenant-PAT inventory drift. The PostgreSQL workflow now targets
  `runs-on: corelink`, not a hosted runner.

## Evidence boundary

- Local bundle validation passed: Plan/WP/AU structure, the 166-corruption
  planning mutation suite, exact actionlint baseline plus its four mutations,
  Rust acceptance 7/7, `corelink-fabric` lib 137/137, TypeScript 9/9, Python
  8/8, JIT fixture 10/10 repeated runs, signal/dev-shm tests, ShellCheck, Ruff,
  rustfmt, DCO, and the focused shell selftests.
- The JIT `/proc/<pid>/{cmdline,environ}` assertion remains unexecuted on Linux:
  the local Docker daemon was unavailable and GitHub could not schedule a
  CoreLink job. The macOS fixture passed but explicitly skipped that cell.
- The first real `services: postgres` run on the CoreLink fleet is capability
  evidence. It is not pre-credited by the local Rust suite.
- The evidence manifest intentionally remains `RED`; the normative plan remains
  **NOT FROZEN**, quiet count zero. Implementation delivery is not live proof,
  promotion, rearm, or go-live approval.

## Operational containment

- Latest read-only observation remains `2026-09-02T01:29:19Z`: fabricd
  `FABRIC_PG_DISABLED=1`, 3/3 inactive; canary
  `FABRIC_PROBES_ENABLED=0`; runner detail 426 inactive and zero live;
  check-host detail reported no instances.
- No deploy, image push, restart, delete, rearm, probe enablement, or other live
  Cloudflare mutation occurred in the Wave 0 implementation session.
- GitHub organization billing still prevented workflow creation before any
  hosted or self-hosted runner could receive work. PR #533 therefore reported
  zero checks and was admin-merged using the recorded local bundle evidence, as
  explicitly directed by the repository owner.

## Continuation plan

1. **Complete — establish Wave 1 base:** `integration/wp-wave1` starts exactly
   at merge `358d1a2` in `.claude/worktrees/wp-wave1-integration`.
2. **In progress — select the next batch:** choose at most eight repo-only WPs
   whose hard predecessors are satisfied; exclude live probes, deploy/rearm,
   unresolved obstacles, and human decisions.
3. **Pending — dispatch Luna/Terra:** give every agent an exact file scope and
   focused tests only. Shared suites run once in the Wave 1 integration bundle.
4. **Pending — independent bundle review:** repair every actionable false-green,
   then rerun only affected focused gates plus the shared bundle matrix.
5. **Pending — merge without hosted-CI deadlock:** prefer local/CoreLink evidence;
   if billing again prevents job creation, record zero-job status and do not
   treat it as a test result. Never let that alone strand a locally green merge.
