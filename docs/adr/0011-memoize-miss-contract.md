# ADR-0011 — Memoize miss policy

- **Status:** accepted; required-hit implementation blocked on clw capability
- **Date:** 2026-09-04
- **Decision owner:** repository owner (D11 ratification)
- **Scope:** `actions/corelink-memoize` and its owned workflow tests

The action exposes `cache-policy` with exactly two values:

- `optional` (the default): cache absence, a miss, or an internal CoreLink
  failure is fail-open. The wrapped command runs COLD and keeps its own exit
  status, preserving existing users.
- `required-hit`: the action must eventually succeed only when clw returns an
  authenticated HIT without invoking the wrapped command. With the installed
  clw **0.1.5**, this policy returns **exit 78 before invoking clw**. That
  release explicitly reports that `--json` does not apply to `run`, proxies
  child stdout/stderr, emits no machine-readable HIT status, and uses exit 125
  for an internal transport failure. The action does not infer a HIT from
  human-readable output or from an exit code.

The smallest prerequisite for enabling `required-hit` is a clw release with a
stable, authenticated, machine-readable **no-exec required-hit operation**:
it must use the same key as `run`, return the cached exit/output on a HIT, and
return 78 on absence/miss/error without invoking the child. A preflight lookup
alone is insufficient unless it is atomically paired with cached-result replay;
otherwise the action has a TOCTOU gap. Until that client contract exists,
`optional` remains the only executable policy. The workflow fixtures cover
optional cold/fallback, the observed clw 0.1.5 no-JSON behavior, required
refusal without command execution, and invalid-policy fail-closed behavior.
