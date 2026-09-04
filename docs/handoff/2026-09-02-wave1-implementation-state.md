# SUPERSEDED — historical Wave 1 implementation state (2026-09-02)

> This handoff preserves the historical 14/70 source-delivery count and the
> deployment observations exactly as recorded on 2026-09-02. It is superseded
> by [`2026-09-04-backlog-resume.md`](2026-09-04-backlog-resume.md); neither
> record is current runtime, freeze, dispatch, or live evidence.

# Session state — Wave 1 implementation, 2026-09-02

**Scope:** resumable source-delivery state after the second repo-only remediation
bundle. This record grants no live, acceptance, freeze, dispatch, deploy, or
rearm credit.

## Delivered source packets

- Wave 0 is merged at `358d1a2` with nine implementation packets: `T0-W1`,
  `T1-W1`, `T2-W1a`, `T6-W1`, `T6-W3`, `T6-W8`, `T7-W3`, `T7-W4`, and
  `T8-W4a`.
- Wave 1 branch `integration/wp-wave1` delivers five more: `T2-W2a`, `T7-W1`,
  `T7-W2`, `T7-W4b`, and `T9-W0`. Review repairs bind image freshness to the
  real build-source closure, bind probe evidence to immutable source and
  deployment authority, and remove stale live claims from `docs/ROADMAP.md`.
- The canonical dispatch DAG contains 70 packets. The correct source-delivery
  count after this bundle is therefore **14/70**, leaving 56. The `wp-check`
  report of 48 WPs counts current principal-acceptance owners; it is not the
  total DAG packet count.
- `T2-W2a` remains report-only until `T2-W2b`; the actual image pins are RED and
  no rebuilt/deployed image is pre-credited. `T8-W4b` remains deferred behind
  `T3-W18`; `T8-W4a` does not close its secret-bridge scope.

## Central bundle evidence

- The shared Cloudflare Worker bundle ran once in the integration worktree:
  TypeScript typecheck PASS, 39 test files / 578 tests PASS, global coverage
  81.27%, and `RunnerDevEnvDO` coverage 60.55% lines/statements.
- Plan/WP/AU structure, exact actionlint baseline, the 166-corruption planning
  selftest, ShellCheck, diff-check, and DCO passed centrally.
- Image freshness selftest passes 10/10. Default mode reports current RED drift
  without blocking; strict mode rejects it. Probe freshness selftest passes 11
  negative cases and the committed RED baseline exits nonzero as intended.
- GitHub-hosted workflow creation remains billing-blocked. Local evidence is a
  merge input, not a claim that GitHub Actions executed.

## Live containment boundary

- Read-only Cloudflare inventory at `2026-09-02T04:56:17Z` found zero running
  runner, fabricd, or check-host containers. The sole running record was the
  unrelated `corelink-prod-corelinkserver-prod` `_system` instance.
- Current deployed versions remained `corelink-fabricd`
  `40bf22a4-6c48-467d-9844-b4fc33e7a3ee` with
  `FABRIC_PG_DISABLED=1`, and `corelink-canary`
  `852277c1-9778-459f-b1ff-9d56fbe7c32f` with
  `FABRIC_PROBES_ENABLED=0`.
- No deploy, restart, image push, delete, rearm, probe enablement, or other live
  mutation occurred.

## Next planning boundary

- The plan remains **NOT FROZEN / NOT DISPATCHABLE**, quiet count zero.
- `T3-W17` is the next apparent repo-only packet, but it is not ready to
  dispatch until the plan pins exact switch/config semantics, durable sequence
  and drain-lease authority, both scheduled re-drive paths, atomic boundaries,
  test evidence recording, and exact-scope enforcement. `T3-W18` remains the
  separate live deploy/probe packet.
- Mechanical implementation may be parallelized only after each packet has a
  frozen exact file scope, predecessors, acceptance matrix, forbidden actions,
  and focused validation command. Shared suites run once on the integration
  bundle.
