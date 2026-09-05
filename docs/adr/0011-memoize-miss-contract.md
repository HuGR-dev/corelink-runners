# ADR-0011 — Memoize miss policy

- **Status:** accepted
- **Date:** 2026-09-04
- **Decision owner:** repository owner (D11 ratification)
- **Scope:** `actions/corelink-memoize` and its owned workflow tests

The action exposes `cache-policy` with exactly two values:

- `optional` (the default): cache absence, a miss, or an internal CoreLink
  failure is fail-open. The wrapped command runs COLD and keeps its own exit
  status, preserving existing users.
- `required-hit`: the action succeeds only when `clw --json` returns exactly
  one structured object with `schema_version: 1`, `verdict: "hit"`, and
  `authenticated: true`. Absence, miss, malformed or untrusted output,
  parser/tool absence, and CoreLink internal failure return **exit 78**.

`required-hit` uses the same memoization command identity as `optional`. On a
miss, its guard returns 78 before invoking the user command; therefore a
required cache miss cannot silently become a cold execution. The workflow
fixtures cover optional cold/fallback, authenticated hit, every required
refusal, and invalid-policy fail-closed behavior.
